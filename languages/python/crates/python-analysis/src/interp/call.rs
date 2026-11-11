// src/interp/call.rs
use crate::interp::Mark;
use crate::interp::Mode;
use crate::interp::bind::{Bound, EvaluatedArgs};
use crate::interp::defer::DeferredOp;
use crate::interp::env::Env;
use crate::interp::solver::{Got, Solver};
use crate::interp::summary::{CallEdge, SiteId, Target};
use crate::interp::tree::Node;
use crate::interp::value::dedup;
use crate::interp::value::{AbstractValue, Values};
use crate::interp::value::{ActivationId, OriginKey};
use crate::interp::walk::{Walk, contains_yield};
use crate::sem::Definition;
use ruff_python_ast::{ExprCall, ExprRef, StmtFunctionDef};
use ruff_text_size::{Ranged, TextRange};
use smallvec::{SmallVec, smallvec};

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
        let args = self.eval_arguments(sol, &call.arguments, env);
        let site = SiteId::new(call.range(), self.frame.def_start);
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
            env,
        )
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
                sol.sem.free_variables(func).iter().any(|n| {
                    self.reaches_pending(
                        sol,
                        &self.lookup_in(sol, func, activation, n),
                        env,
                        depth + 1,
                    )
                }) || self
                    .defaults
                    .get(&(func, activation))
                    .into_iter()
                    .flatten()
                    .any(|(_, d)| self.reaches_pending(sol, d, env, depth + 1))
            }
            _ => false,
        })
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
            // its own free values and defaults sit at depth 1, as `key` reads them
            AbstractValue::Closure { .. } => self.reaches_pending(sol, &smallvec![*c], env, 0),
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
        env: &mut Env<'db>,
    ) -> Values<'db> {
        // One node per function, in definition order: the join order is an accident.
        let mut targets: Vec<(Definition<'db>, Option<ActivationId>)> = Vec::new();
        let mut methods: Vec<(Definition<'db>, AbstractValue<'db>)> = Vec::new();
        let mut classes: Vec<Definition<'db>> = Vec::new();
        let mut ask_ty = false;
        let mut externals: Vec<Definition<'db>> = Vec::new();
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
        let mut edge_targets: SmallVec<[Target<'db>; 2]> = SmallVec::new();
        let mut out = Values::new();
        // Callees at one site are alternatives: each starts from the state before the call.
        let before = env.clone();
        let mut after: Vec<Option<Env<'db>>> = Vec::new();
        for (def, enclosing) in targets {
            let v = match sol.opts.mode {
                Mode::Inline => {
                    let range = self.site_range(site);
                    let mut e = before.clone();
                    let v = self.call_function(sol, def, enclosing, None, range, args, &mut e);
                    after.push(Some(e));
                    v
                }
                Mode::Summary => {
                    self.call_project(sol, def, enclosing, None, args, &mut edge_targets, &*env)
                }
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
                    &mut edge_targets,
                    &*env,
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
                    &mut edge_targets,
                    &*env,
                )),
            }
        }
        if sol.opts.mode == Mode::Summary {
            // every call site gets an edge, even when nothing is walked
            edge_targets.extend(externals.into_iter().map(Target::External));
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
        targets: &mut SmallVec<[Target<'db>; 2]>,
        env: &Env<'db>,
    ) -> Values<'db> {
        let parsed = sol.sem.parsed(def);
        let Some(node) = sol.sem.function_node(def, &parsed) else {
            return smallvec![AbstractValue::Unknown];
        };
        let runs = !args.has_empty();
        let bound = self.bind_for_callee(sol, def, node, receiver, args, enclosing);
        let ctx = sol.key(self, def, &bound, enclosing, env);
        targets.push(Target::Call(ctx)); // recorded now, whatever `get` answers
        match sol.get(ctx) {
            _ if !runs => Values::new(), // Python never makes this call, whatever `get` answered
            Got::Ready(res) => self.resolve(&res.returns),
            Got::Pending if self.replaying => Values::new(), // the solver flagged the reader
            Got::Pending => {
                let id = self.defer(DeferredOp::Apply { ctx }, env);
                smallvec![AbstractValue::Pending(id)]
            }
        }
    }

    /// Summary mode: a new object here, and `__init__` keyed on it.
    fn construct_summary(
        &mut self,
        sol: &mut Solver<'db>,
        class: Definition<'db>,
        site: TextRange,
        args: &EvaluatedArgs<'db>,
        targets: &mut SmallVec<[Target<'db>; 2]>,
        env: &Env<'db>,
    ) -> Values<'db> {
        let origin = self.origins.intern(OriginKey::New {
            site,
            act: self.frame.act,
        });
        let obj = AbstractValue::Instance { class, origin };
        let init = sol
            .sem
            .init_of(class)
            .filter(|&i| sol.sem.is_project(i))
            .and_then(|init| {
                let parsed = sol.sem.parsed(init);
                let node = sol.sem.function_node(init, &parsed)?;
                let bound = self.bind_for_callee(sol, init, node, Some(obj), args, None);
                let ctx = sol.key(self, init, &bound, None, env);
                sol.get(ctx); // its value is never used; its writes are effects (step 03)
                Some(ctx)
            });
        targets.push(Target::Construct { class, init });
        if args.has_empty() {
            Values::new()
        } else {
            smallvec![obj]
        }
    }
}
