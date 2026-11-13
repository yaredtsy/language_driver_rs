// src/interp/call.rs
use crate::interp::Mark;
use crate::interp::Mode;
use crate::interp::bind::{Bound, EvaluatedArgs};
use crate::interp::defer::DeferredOp;
use crate::interp::env::Env;
use crate::interp::solver::{Got, Solver};
use crate::interp::summary::{CallEdge, CallSite, SiteId, Target};
use crate::interp::tree::Node;
use crate::interp::value::dedup;
use crate::interp::value::{AbstractValue, Values};
use crate::interp::value::{ActivationId, Made, OriginKey};
use crate::interp::walk::{Walk, contains_yield};
use crate::sem::Definition;
use crate::sem::Shape;
use crate::sem::Type;
use analyzer_core::graph::Basis;
use ruff_python_ast::{Expr, ExprCall, ExprRef, StmtFunctionDef};
use ruff_text_size::{Ranged, TextRange};
use smallvec::{SmallVec, smallvec};

/// How a call site knows what it calls: the basis of every edge it has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum How {
    /// The callee expression names a def or a class (`Basis::Name`).
    Named,
    /// Through the walk's values or the engine's exact type (`Basis::Type`, or `Basis::Path` for what the walk built).
    Typed,
    /// The engine found the def past a base it could not see (D28, `Basis::Likely`).
    Likely,
}

pub(crate) struct InlineRun<'db> {
    pub children: Vec<Node>,
    pub returns: Values<'db>,
    #[allow(dead_code)] // never read: inline mode's prologue used it
    pub act: Option<ActivationId>, // the run, for the prologue
}

impl<'db> Walk<'db> {
    pub(crate) fn eval_call(
        &mut self,
        sol: &mut Solver<'db>,
        call: &ExprCall,
        env: &mut Env<'db>,
    ) -> Values<'db> {
        let callees = self.eval(sol, &call.func, env);
        let (callees, likely) = self.or_engine_callee(sol, &call.func, callees);
        let args = self.eval_arguments(sol, &call.arguments, env);
        let site = SiteId::new(call.range(), self.frame.def_start);
        let how = if likely {
            How::Likely
        } else if sol.opts.mode == Mode::Summary && self.names_a_def(sol, &call.func) {
            How::Named
        } else {
            How::Typed
        };
        if sol.opts.mode == Mode::Summary && self.needs_defer(sol, &callees, &args, env) {
            // It can't be keyed yet: record it, and find its value in the fixpoint.
            let ty = sol.sem.type_of(&self.frame.model, ExprRef::from(call));
            let fallback = self.lift(sol, ty);
            let edge = self.frame.calls.len();
            self.frame.calls.push(CallEdge {
                site,
                targets: SmallVec::new(), // refilled by every replay, so the edge keeps its place
            });
            let id = self.defer(
                DeferredOp::Call {
                    site,
                    callee: callees,
                    args,
                    fallback,
                    edge,
                    how,
                },
                env,
            );
            return smallvec![AbstractValue::Pending(id)];
        }
        self.call_values(
            sol,
            site,
            &callees,
            &args,
            Some(call),
            &Values::new(),
            None,
            how,
            env,
        )
    }

    /// Where the walk holds a callee value it cannot call, the engine's own answer for the callee expression (D27). A value cannot be called
    /// when it is `Unknown`, or a `Ty` with no def (`callee_definition` is `None`): the two arms of `call_values` that set `unresolved`. This
    /// is what a call through `passthrough(f)(x)` holds, where `passthrough` returns its type variable `F`: the walk carries `F` out of the
    /// generic body, and ty has solved it for this call.
    ///
    /// The engine's type is taken only when it is exact: a function, a bound method or a class (`Function`, `BoundMethod`, `Class`), or a
    /// union of only those. Anything else (a `Callable`, a type variable, `Unknown` or a union with it) leaves `walked` as it is, so the site
    /// stays `Unresolved`. Values the walk can call are kept, and the engine is not asked when there are none to replace.
    ///
    /// One more answer is taken (D28): a def the engine found past a base it could not see (`Sem::likely_def`), alone or as a member of
    /// such a union. Its def is lifted, and the `bool` says the site is likely. A union with plain `Unknown` still leaves `walked` as it is.
    fn or_engine_callee(
        &mut self,
        sol: &mut Solver<'db>,
        callee: &Expr,
        walked: Values<'db>,
    ) -> (Values<'db>, bool) {
        let file = self.frame.file;
        let cannot_call = |v: &AbstractValue<'db>| match *v {
            AbstractValue::Unknown => true,
            AbstractValue::Ty(t) => sol.sem.callee_definition(t, file).is_none(),
            _ => false,
        };
        if !walked.iter().any(cannot_call) {
            return (walked, false);
        }
        let t = sol.sem.type_of(&self.frame.model, ExprRef::from(callee));
        let exact = |t| {
            matches!(
                sol.sem.shape(t, file),
                Shape::Function(_) | Shape::BoundMethod { .. } | Shape::Class(_)
            )
        };
        let members = match sol.sem.shape(t, file) {
            Shape::Union(members) => members,
            _ => vec![t],
        };
        // Each member is exact, or a def the engine found past a base it could not see (D28): then the site is likely.
        let mut likely: Vec<Type<'db>> = Vec::new();
        for &m in &members {
            if exact(m) {
                continue;
            }
            match sol.sem.likely_def(m, file) {
                Some(part) => likely.push(part),
                None => return (walked, false),
            }
        }
        if members.is_empty() {
            return (walked, false);
        }
        let is_likely = !likely.is_empty();
        // all exact: the type as it is; else the exact members and the likely parts, lifted one by one
        let lifted: Vec<Type<'db>> = if is_likely {
            members
                .into_iter()
                .filter(|&m| exact(m))
                .chain(likely)
                .collect()
        } else {
            vec![t]
        };
        // `cannot_call` and `exact` read `sol`; `lift` needs it mutably, so they are finished with before it.
        let mut out: Values<'db> = walked.into_iter().filter(|v| !cannot_call(v)).collect();
        for m in lifted {
            out.extend(self.lift(sol, m));
        }
        dedup(&mut out);
        (out, is_likely)
    }

    /// Does the code name what this callee expression calls? ty says: its type is a function or a class (the `def` or `class` itself), or a
    /// union of only those (`Function`, `Class`). A method through a receiver (`BoundMethod`), a `type[X]`, a `Callable` and anything with an
    /// `Unknown` member do not name one. `or_engine_callee` also accepts a `BoundMethod`: it needs an exact def, not a name.
    fn names_a_def(&self, sol: &Solver<'db>, callee: &Expr) -> bool {
        let file = self.frame.file;
        let literal = |t| matches!(sol.sem.shape(t, file), Shape::Function(_) | Shape::Class(_));
        let t = sol.sem.type_of(&self.frame.model, ExprRef::from(callee));
        match sol.sem.shape(t, file) {
            Shape::Union(members) => !members.is_empty() && members.into_iter().all(literal),
            _ => literal(t),
        }
    }

    /// Would keying these values meet a `Pending`? Looks exactly as far as `fingerprint`.
    fn reaches_pending(
        &self,
        sol: &Solver<'db>,
        vals: &Values<'db>,
        env: &Env<'db>,
        depth: u8,
    ) -> bool {
        let limit = sol.opts.limits.fp_depth;
        vals.iter().any(|v| match *v {
            AbstractValue::Pending(_) => true,
            AbstractValue::Instance { origin, .. } if origin.holds_state() && depth < limit => env
                .attrs_of(origin)
                .iter()
                .any(|(_, a)| self.reaches_pending(sol, a, env, depth + 1)),
            AbstractValue::BoundMethod { receiver, .. } => {
                self.reaches_pending(sol, &smallvec![self.values[receiver]], env, depth + 1)
            }
            AbstractValue::Closure { func, activation } if depth < limit => {
                self.closure_reaches_pending(sol, func, activation, env, depth + 1, depth + 1)
            }
            _ => false,
        })
    }

    /// The free values of a closure, read at `free_depth`, and its defaults, at `default_depth`: as `key` reads them. A closure that is a
    /// value reads both one below it; a closure that is being called reads its own free values at 0, as its parameters (`key`).
    fn closure_reaches_pending(
        &self,
        sol: &Solver<'db>,
        func: Definition<'db>,
        activation: ActivationId,
        env: &Env<'db>,
        free_depth: u8,
        default_depth: u8,
    ) -> bool {
        sol.sem.free_variables(func).iter().any(|n| {
            self.reaches_pending(
                sol,
                &self.lookup_in(sol, func, activation, n),
                env,
                free_depth,
            )
        }) || self
            .defaults
            .get(&(func, activation))
            .into_iter()
            .flatten()
            .any(|(_, d)| self.reaches_pending(sol, d, env, default_depth))
    }

    /// A callee that is pending, or a project callee whose key would read a
    /// pending value: an argument, a receiver, a closure's free value, an attribute.
    fn needs_defer(
        &self,
        sol: &Solver<'db>,
        callees: &Values<'db>,
        args: &EvaluatedArgs<'db>,
        env: &Env<'db>,
    ) -> bool {
        if self.deferred.is_empty() {
            return false; // a Pending is an index into `deferred`: none exist yet
        }
        if callees
            .iter()
            .any(|c| matches!(c, AbstractValue::Pending(_)))
        {
            return true;
        }
        let project: SmallVec<[AbstractValue<'db>; 2]> = callees
            .iter()
            .copied()
            .filter(|c| match *c {
                AbstractValue::Function(d) | AbstractValue::Class(d) => sol.sem.is_project(d),
                AbstractValue::Closure { func, .. } | AbstractValue::BoundMethod { func, .. } => {
                    sol.sem.is_project(func)
                }
                _ => false,
            })
            .collect();
        if project.is_empty() {
            return false;
        }
        let arg = args
            .positional
            .iter()
            .chain(args.keywords.iter().map(|(_, v)| v))
            .any(|v| self.reaches_pending(sol, v, env, 0));
        arg || project.iter().any(|c| match *c {
            // its own free values sit at depth 0 and its defaults at depth 1, as `key` reads them
            AbstractValue::Closure { func, activation } => {
                self.closure_reaches_pending(sol, func, activation, env, 0, 1)
            }
            // the receiver is slot 0
            AbstractValue::BoundMethod { receiver, .. } => {
                self.reaches_pending(sol, &smallvec![self.values[receiver]], env, 0)
            }
            _ => false,
        })
    }

    /// The second half of a call: the callee values and the arguments are known.
    /// A deferred `Call` runs this again in the fixpoint, with `edge` set to the
    /// slot its `CallEdge` already holds, and no expression to ask ty about.
    // moved from pylspt unchanged: the plan moves the solver, it does not rewrite it (a rewrite is its own change)
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn call_values(
        &mut self,
        sol: &mut Solver<'db>,
        site: SiteId,
        callees: &Values<'db>,
        args: &EvaluatedArgs<'db>,
        call: Option<&ExprCall>,
        fallback: &Values<'db>,
        edge: Option<usize>,
        how: How, // the basis of what it calls: `Named` when the callee expression names a def or a class
        env: &mut Env<'db>,
    ) -> Values<'db> {
        // One node per function, in definition order: the join order is an accident.
        let mut targets: Vec<(Definition<'db>, Option<ActivationId>)> = Vec::new();
        let mut methods: Vec<(Definition<'db>, AbstractValue<'db>)> = Vec::new();
        let mut classes: Vec<Definition<'db>> = Vec::new();
        let mut ask_ty = false;
        let mut externals: Vec<Definition<'db>> = Vec::new();
        let mut named_externals: Vec<Definition<'db>> = Vec::new();
        let mut unresolved = callees.is_empty();
        for c in callees {
            match *c {
                AbstractValue::Function(def) if sol.sem.is_project(def) => {
                    targets.push((def, None))
                }
                AbstractValue::Closure { func, activation } if sol.sem.is_project(func) => {
                    targets.push((func, Some(activation)))
                }
                AbstractValue::BoundMethod { func, receiver } if sol.sem.is_project(func) => {
                    methods.push((func, self.values[receiver]))
                }
                AbstractValue::Class(class) if sol.sem.is_project(class) => classes.push(class),
                // `dict`, and the classes and methods that aren't project code
                AbstractValue::Function(d) | AbstractValue::Class(d) => {
                    ask_ty = true;
                    externals.push(d);
                    if how == How::Named {
                        named_externals.push(d);
                    }
                }
                AbstractValue::Closure { func, .. } | AbstractValue::BoundMethod { func, .. } => {
                    ask_ty = true;
                    externals.push(func);
                }
                AbstractValue::Ty(t) => {
                    ask_ty = true;
                    match sol.sem.callee_definition(t, self.frame.file) {
                        Some(d) => externals.push(d),
                        None => unresolved = true,
                    }
                }
                _ => {
                    ask_ty = true;
                    unresolved = true;
                }
            }
        }
        targets.sort_by_key(|(d, _)| sol.sem.def_position(*d));
        methods.sort_by_key(|(d, _)| sol.sem.def_position(*d));
        classes.sort_by_key(|&c| sol.sem.def_position(c));
        externals.sort_by_key(|&d| sol.sem.def_position(d));
        externals.dedup();
        // The callee is one def and nothing else: a write it made on every path is a write Python made. Known here, before the first
        // callee's effects are applied: the callees are all collected (and sorted) above.
        let strong_ok = targets.len() + methods.len() + classes.len() == 1 && !ask_ty;
        let mut edge_targets: SmallVec<[Target<'db>; 2]> = SmallVec::new();
        let mut out = Values::new();
        // Callees at one site are alternatives: each starts from the state before the call.
        let before = env.clone();
        let mut after: Vec<Option<Env<'db>>> = Vec::new();
        // The project callees at this site, numbered as they are walked: one origin per callee for the objects its summary builds.
        let mut callee = 0usize;
        let mut next_t = || {
            let t = u8::try_from(callee).unwrap_or(u8::MAX);
            callee += 1;
            t
        };
        for (def, enclosing) in targets {
            let v = match sol.opts.mode {
                Mode::Inline => {
                    let range = self.site_range(site);
                    let mut e = before.clone();
                    let v = self.call_function(sol, def, enclosing, None, range, args, &mut e);
                    after.push(Some(e));
                    v
                }
                Mode::Summary => self.call_project(
                    sol,
                    def,
                    enclosing,
                    None,
                    args,
                    how,
                    self.site_range(site),
                    next_t(),
                    &mut edge_targets,
                    env,
                    &before,
                    strong_ok,
                ),
            };
            out.extend(v);
        }
        for (func, recv) in methods {
            match sol.opts.mode {
                Mode::Inline => {
                    let range = self.site_range(site);
                    let mut e = before.clone();
                    out.extend(self.call_function(
                        sol,
                        func,
                        None,
                        Some(recv),
                        range,
                        args,
                        &mut e,
                    ));
                    after.push(Some(e));
                }
                Mode::Summary => out.extend(self.call_project(
                    sol,
                    func,
                    None,
                    Some(recv),
                    args,
                    // a method is reached through a receiver: never named
                    if how == How::Likely {
                        How::Likely
                    } else {
                        How::Typed
                    },
                    self.site_range(site),
                    next_t(),
                    &mut edge_targets,
                    env,
                    &before,
                    strong_ok,
                )),
            }
        }
        for class in classes {
            let range = self.site_range(site);
            match sol.opts.mode {
                Mode::Inline => {
                    let mut e = before.clone();
                    out.extend(self.construct(sol, class, range, args, &mut e));
                    after.push(Some(e));
                }
                Mode::Summary => out.extend(self.construct_summary(
                    sol,
                    class,
                    range,
                    args,
                    match how {
                        How::Named => Basis::Name,
                        How::Typed => Basis::Type,
                        How::Likely => Basis::Likely,
                    },
                    &mut edge_targets,
                    env,
                    strong_ok,
                )),
            }
        }
        if sol.opts.mode == Mode::Summary {
            // every call site gets an edge, even when nothing is walked
            edge_targets.extend(externals.into_iter().map(|d| {
                let basis = if how == How::Likely {
                    Basis::Likely
                } else if named_externals.contains(&d) {
                    Basis::Name
                } else {
                    Basis::Type
                };
                Target::External(d, basis)
            }));
            if unresolved {
                edge_targets.push(Target::Unresolved);
            }
        }
        match edge {
            Some(i) => self.frame.calls[i].targets = edge_targets,
            None if !edge_targets.is_empty() => self.frame.calls.push(CallEdge {
                site,
                targets: edge_targets,
            }),
            None => {}
        }
        if ask_ty {
            if sol.opts.mode == Mode::Summary {
                // Summary mode applied the project callees' writes into `env` as it went (inline mode kept each callee's env in `after`):
                // they are one alternative, ty's callee the other.
                after.push(Some(env.clone()));
            }
            after.push(Some(before.clone())); // ty's callee: no state change you can see
            match call {
                Some(call) => {
                    let ty = sol.sem.type_of(&self.frame.model, ExprRef::from(call));
                    out.extend(self.lift(sol, ty));
                }
                None => out.extend(fallback.iter().copied()),
            }
        }
        if let Some(joined) = Env::join(after) {
            *env = joined;
        }
        dedup(&mut out);
        out
    }

    /// Inline mode: a node named `name` at `site`, with the calls `def` makes under it.
    /// `name` already on the path: a `[recursive]` leaf, nothing walked, `None`.
    // moved from pylspt unchanged: the plan moves the solver, it does not rewrite it (a rewrite is its own change)
    #[allow(clippy::too_many_arguments)]
    fn inline_node(
        &mut self,
        sol: &mut Solver<'db>,
        name: String,
        shown: Definition<'db>, // what the node is named by: hidden with its subtree when not shown
        site: TextRange,
        def: Definition<'db>,
        enclosing: Option<ActivationId>,
        receiver: Option<AbstractValue<'db>>,
        args: &EvaluatedArgs<'db>,
        env: &mut Env<'db>,
    ) -> Option<Values<'db>> {
        if sol.path.contains(&name) {
            let mut node = Node::call(name, site);
            node.marks.push(Mark::Recursive);
            if sol.shown(shown) {
                self.push_leaf(sol, node);
            }
            return None;
        }
        sol.path.push(name.clone());
        let run = self.walk_inline(sol, def, enclosing, receiver, args, env);
        sol.path.pop();
        if sol.shown(shown) {
            let mut node = Node::call(name, site);
            node.children = run.children;
            self.push_leaf(sol, node);
        } // else: values flowed through it, the view drops it and what is under it
        Some(run.returns)
    }

    /// Inline mode: a call to a function.
    // moved from pylspt unchanged: the plan moves the solver, it does not rewrite it (a rewrite is its own change)
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn call_function(
        &mut self,
        sol: &mut Solver<'db>,
        def: Definition<'db>,
        enclosing: Option<ActivationId>,
        receiver: Option<AbstractValue<'db>>,
        site: TextRange,
        args: &EvaluatedArgs<'db>,
        env: &mut Env<'db>,
    ) -> Values<'db> {
        let name = sol.sem.qname(def);
        match self.inline_node(sol, name, def, site, def, enclosing, receiver, args, env) {
            None => smallvec![AbstractValue::Unknown], // recursive: not walked (step 01.10)
            Some(_) if args.has_empty() => Values::new(), // Python never makes this call
            Some(returns) => returns,
        }
    }

    /// Inline mode: a new object. The node is the class; `__init__`'s calls are under it.
    fn construct(
        &mut self,
        sol: &mut Solver<'db>,
        class: Definition<'db>,
        site: TextRange,
        args: &EvaluatedArgs<'db>,
        env: &mut Env<'db>,
    ) -> Values<'db> {
        let origin = self.origins.intern(OriginKey::New {
            site,
            act: self.frame.act,
            t: 0,
            made: Made::Here,
        });
        let obj = AbstractValue::Instance { class, origin };
        let name = sol.sem.qname(class);
        match sol
            .sem
            .init_of(class)
            .filter(|&init| sol.sem.is_project(init))
        {
            // `self` is the object. The guard is keyed on the class's name.
            Some(init) => {
                self.inline_node(sol, name, class, site, init, None, Some(obj), args, env);
            }
            // no body to walk, so nothing can recurse: a plain leaf
            None if sol.shown(class) => self.push_leaf(sol, Node::call(name, site)),
            None => {}
        }
        if args.has_empty() {
            Values::new()
        } else {
            smallvec![obj]
        }
    }

    /// Inline mode: walk `def`'s body now. No node and no guard: that is `inline_node`'s job.
    pub(crate) fn walk_inline(
        &mut self,
        sol: &mut Solver<'db>,
        def: Definition<'db>,
        enclosing: Option<ActivationId>,
        receiver: Option<AbstractValue<'db>>,
        args: &EvaluatedArgs<'db>,
        env: &mut Env<'db>,
    ) -> InlineRun<'db> {
        let parsed = sol.sem.parsed(def);
        let Some(node) = sol.sem.function_node(def, &parsed) else {
            return InlineRun {
                children: Vec::new(),
                returns: smallvec![AbstractValue::Unknown],
                act: None,
            };
        };
        sol.stats.walks += 1;
        let new = self.new_frame(sol, def);
        let act = new.act;
        let caller = std::mem::replace(&mut self.frame, new);
        let bound = self.bind_parameters(sol, def, node, receiver, args, enclosing);
        sol.key(self, def, &bound, enclosing, env);
        let mut start = self.env_from_bound(node, &bound);
        start.scope = Some(sol.sem.body_scope(def, node));
        start.enclosing = enclosing;
        start.attrs = env.attrs.clone(); // the callee sees every object as it is now
        if let Some(end) = self.walk_stmts(sol, &node.body, start) {
            let none = self.none_value(sol);
            self.frame.returns.extend(none); // fell off the end
            self.frame.exits.push(end); // and that is an exit
        }
        let callee = std::mem::replace(&mut self.frame, caller);
        if let Some(exit) = Env::join(callee.exits.into_iter().map(Some).collect()) {
            self.activations[act] = exit.clone(); // the run as it ended: closures read this
            env.attrs = exit.attrs; // the caller continues with what the callee left
        }
        let returns = if contains_yield(&node.body) {
            smallvec![AbstractValue::Unknown] // a generator object
        } else {
            callee.returns
        };
        InlineRun {
            children: callee.children,
            returns,
            act: Some(act),
        }
    }

    fn push_leaf(&mut self, sol: &mut Solver<'db>, node: Node) {
        self.frame.children.push(node);
        sol.stats.nodes += 1;
    }

    /// Bind in the callee's frame (defaults and decorators are typed with its
    /// model), then drop that frame: only the slots leave.
    fn bind_for_callee(
        &mut self,
        sol: &mut Solver<'db>,
        def: Definition<'db>,
        node: &StmtFunctionDef,
        receiver: Option<AbstractValue<'db>>,
        args: &EvaluatedArgs<'db>,
        enclosing: Option<ActivationId>,
    ) -> Bound<'db> {
        let new = self.new_frame(sol, def);
        let caller = std::mem::replace(&mut self.frame, new);
        let bound = self.bind_parameters(sol, def, node, receiver, args, enclosing);
        self.frame = caller;
        bound
    }

    /// Summary mode: key the call, record the edge, ask the solver. The callee's
    /// body is walked at most once per key; the tree is rendered from the graph.
    // moved from pylspt unchanged: the plan moves the solver, it does not rewrite it (a rewrite is its own change)
    #[allow(clippy::too_many_arguments)]
    fn call_project(
        &mut self,
        sol: &mut Solver<'db>,
        def: Definition<'db>,
        enclosing: Option<ActivationId>,
        receiver: Option<AbstractValue<'db>>,
        args: &EvaluatedArgs<'db>,
        how: How, // `Named`: a def or a closure the callee expression names (no receiver)
        site: TextRange, // the call expression, in the file
        t: u8,    // the callee's index at the site
        targets: &mut SmallVec<[Target<'db>; 2]>,
        env: &mut Env<'db>,
        before: &Env<'db>, // the env when the call began: what a `Ref` reads
        strong_ok: bool,   // the site has one callee, so a write it made on every path replaces
    ) -> Values<'db> {
        let parsed = sol.sem.parsed(def);
        let Some(node) = sol.sem.function_node(def, &parsed) else {
            return smallvec![AbstractValue::Unknown];
        };
        let runs = !args.has_empty();
        let bound = self.bind_for_callee(sol, def, node, receiver, args, enclosing);
        let ctx = sol.key(self, def, &bound, enclosing, env);
        // Likely when the engine found the def past a base it could not see (D28): first, whatever the receiver. Name when the callee
        // expression names the def or the closure. Path when the walk itself holds what the call rests on: an object it
        // built here, or a closure it made and called through a variable. Everything else is the engine's answer through a type: a method on
        // an object ty typed, and a method on an input object of this context (`self` of a walked method, an argument), whose class the key
        // gave.
        let basis = match receiver {
            _ if how == How::Likely => Basis::Likely,
            None if how == How::Named => Basis::Name,
            Some(AbstractValue::Instance { origin, .. })
                if self.origins[origin].is_built_here() =>
            {
                Basis::Path
            }
            _ if enclosing.is_some() => Basis::Path,
            _ => Basis::Type,
        };
        targets.push(Target::Call(ctx, basis)); // recorded now, whatever `get` answers
        match sol.get(ctx) {
            _ if !runs => Values::new(), // Python never makes this call, whatever `get` answered
            Got::Ready(res) => {
                let call = CallSite {
                    ctx,
                    site,
                    t,
                    bound: &bound,
                    enclosing,
                    strong_ok,
                };
                let plan = self.resolve(sol, &res, &call, before);
                self.apply(sol, plan, env)
            }
            Got::Pending if self.replaying => Values::new(), // the solver flagged the reader
            Got::Pending => {
                let op = DeferredOp::Apply {
                    ctx,
                    site,
                    t,
                    bound,
                    enclosing,
                };
                let id = self.defer(op, env);
                smallvec![AbstractValue::Pending(id)]
            }
        }
    }

    /// Summary mode's counterpart of `call_function` for a property's getter or setter: a call at `range` (the attribute expression), with
    /// `recv` as its receiver. It is a call like any other: the edge is recorded, the receiver is keyed, and a receiver that is not known
    /// yet defers the call to the fixpoint. A getter and a setter only: inline mode runs no deleter either.
    pub(crate) fn call_accessor(
        &mut self,
        sol: &mut Solver<'db>,
        func: Definition<'db>,
        recv: AbstractValue<'db>,
        args: EvaluatedArgs<'db>,
        range: TextRange,
        env: &mut Env<'db>,
    ) -> Values<'db> {
        let site = SiteId::new(range, self.frame.def_start);
        let callee: Values<'db> = smallvec![AbstractValue::BoundMethod {
            func,
            receiver: self.values.intern(recv),
        }];
        if self.needs_defer(sol, &callee, &args, env) {
            let edge = self.frame.calls.len();
            self.frame.calls.push(CallEdge {
                site,
                targets: SmallVec::new(), // refilled by every replay, so the edge keeps its place
            });
            let id = self.defer(
                DeferredOp::Call {
                    site,
                    callee,
                    args,
                    fallback: Values::new(), // a project callee never asks ty
                    edge,
                    how: How::Typed, // a getter or setter is reached through a receiver
                },
                env,
            );
            return smallvec![AbstractValue::Pending(id)];
        }
        self.call_values(
            sol,
            site,
            &callee,
            &args,
            None,
            &Values::new(),
            None,
            How::Typed,
            env,
        )
    }

    /// Summary mode: a new object here, and `__init__` keyed on it. With `Options::effects`, what `__init__` wrote is applied like the
    /// effects of any callee: `self` is slot 0, so `self.h = h` writes onto the new object, and an object `__init__` built is described
    /// and written too.
    // the construct site's inputs: the class and its site, the arguments, what the class expression rests on, and the walk's state
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn construct_summary(
        &mut self,
        sol: &mut Solver<'db>,
        class: Definition<'db>,
        site: TextRange,
        args: &EvaluatedArgs<'db>,
        basis: Basis, // `Name` when the class expression names the class
        targets: &mut SmallVec<[Target<'db>; 2]>,
        env: &mut Env<'db>,
        strong_ok: bool, // the site has one callee
    ) -> Values<'db> {
        let origin = self.origins.intern(OriginKey::New {
            site,
            act: self.frame.act,
            t: 0,
            made: Made::Here,
        });
        let obj = AbstractValue::Instance { class, origin };
        let mut init = None;
        if let Some(init_def) = sol.sem.init_of(class).filter(|&i| sol.sem.is_project(i)) {
            let parsed = sol.sem.parsed(init_def);
            if let Some(node) = sol.sem.function_node(init_def, &parsed) {
                let bound = self.bind_for_callee(sol, init_def, node, Some(obj), args, None);
                let ctx = sol.key(self, init_def, &bound, None, env);
                // its value is never used; its writes are effects. Asked either way: this walks `__init__`, and its context needs a slot.
                match sol.get(ctx) {
                    Got::Ready(res) if sol.opts.effects => {
                        let call = CallSite {
                            ctx,
                            site,
                            t: 0,
                            bound: &bound,
                            enclosing: None,
                            strong_ok,
                        };
                        let before = env.clone();
                        let plan = self.resolve(sol, &res, &call, &before);
                        self.apply(sol, plan, env);
                    }
                    // running: nothing waits on a constructor, so its effects are not carried (CS8)
                    Got::Pending if sol.opts.effects => self.note_effects_in_cycle(sol, (site, 0)),
                    _ => {}
                }
                init = Some(ctx);
            }
        }
        targets.push(Target::Construct { class, init, basis });
        if args.has_empty() {
            Values::new()
        } else {
            smallvec![obj]
        }
    }
}
