// src/interp/summary.rs
use analyzer_core::graph::Basis;
use ruff_text_size::{TextRange, TextSize};
use smallvec::{SmallVec, smallvec};

use crate::interp::bind::Bound;
use crate::interp::env::{Env, Write};
use crate::interp::key::ClosureStack;
use crate::interp::key::ContextId;
use crate::interp::solver::Solver;
use crate::interp::value::ActivationId;
use crate::interp::value::{
    AbstractValue, DeferId, Made, OriginId, OriginKey, Path, Root, Step, Values, dedup,
};
use crate::interp::walk::Walk;
use crate::sem::{Definition, Name, Type};
use ruff_index::IndexVec;

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct FunctionResult<'db> {
    pub calls: Vec<CallEdge<'db>>, // one per call site reached, in evaluation order
    pub returns: SVals<'db>,       // the union of every return
    /// The objects this call built that `returns` can reach, numbered in the order `finish` reaches them: `Obj(j)` is `objects[j]`.
    pub objects: Vec<NewObject<'db>>,
    /// What the call wrote onto the objects its caller can see, relative to its inputs, sorted by (target, attr). Empty unless
    /// `Options::effects` is on.
    pub effects: Vec<Effect<'db>>,
    pub status: Status,
}

/// An object a call built, with the state it ends with. Reachable from the call's returns, or it would not be here.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct NewObject<'db> {
    pub class: Definition<'db>,
    /// Where it was built, inside this function.
    pub site: SiteId,
    /// Its attributes at the end of the call, sorted by name.
    pub attrs: Vec<(Name, SVals<'db>)>,
}

/// A write onto an object the caller can see: `<target>.<attr> = <value>`, written so that a caller with other arguments can apply it.
/// A write onto an object the call built is not one: the object is described whole in `objects`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Effect<'db> {
    /// The input object written to, as it was when the call started.
    pub target: Path,
    pub attr: Name,
    pub value: SVals<'db>,
    /// Written on every path that exits normally.
    pub strong: bool,
}

/// What a caller must know about how far to trust a summary .
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize)]
pub(crate) struct Status {
    pub gaps: Gaps,
    /// A cost limit fired somewhere this summary was computed from.
    pub cut: Option<Cut>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
pub(crate) enum Cut {
    Stack,
    Contexts,
    ContextsPerFunc,
    Deadline,
    /// The caller's `Cancel` fired (`Options::cancel`).
    Cancelled,
}

impl<'db> FunctionResult<'db> {
    /// What a call returns when a cost limit stopped it: something, we stopped looking.
    pub(crate) fn cut(cut: Cut) -> Self {
        FunctionResult {
            calls: Vec::new(),
            returns: smallvec![SVal::Plain(PlainVal::Unknown)],
            objects: Vec::new(),
            effects: Vec::new(),
            status: Status {
                gaps: Gaps::default(),
                cut: Some(cut),
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize)]
pub(crate) struct Gaps(u8);

impl Gaps {
    /// The group's fixpoint hit `max_rounds`: later rounds might have added more.
    pub(crate) const CYCLE: Gaps = Gaps(1);
    pub(crate) fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct CallEdge<'db> {
    pub site: SiteId,
    pub targets: SmallVec<[Target<'db>; 2]>, // one per project callee at this site
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Target<'db> {
    Call(ContextId, Basis),
    /// A new object. `init`: the `__init__` context, if the MRO has a body.
    Construct {
        class: Definition<'db>,
        init: Option<ContextId>,
        basis: Basis,
    },
    /// Code outside the project: nothing is walked. The graph shows it, the tree does not.
    External(Definition<'db>, Basis),
    /// A call nothing could be said about: the graph shows it, the tree does not.
    Unresolved,
}

impl<'db> Target<'db> {
    /// The contexts this target leads to: what the graph walks next.
    pub(crate) fn contexts(&self) -> SmallVec<[ContextId; 1]> {
        match *self {
            Target::Call(ctx, _) => smallvec![ctx],
            Target::Construct { init, .. } => init.into_iter().collect(),
            Target::External(..) | Target::Unresolved => SmallVec::new(),
        }
    }
}

/// The call expression's range, relative to the start of the enclosing `def`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) struct SiteId {
    pub start: TextSize,
    pub end: TextSize,
}

impl serde::Serialize for SiteId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        (u32::from(self.start), u32::from(self.end)).serialize(s)
    }
}

impl SiteId {
    pub(crate) fn new(range: TextRange, def_start: TextSize) -> Self {
        SiteId {
            start: range.start() - def_start,
            end: range.end() - def_start,
        }
    }
}

pub(crate) type SVals<'db> = SmallVec<[SVal<'db>; 2]>;

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum SVal<'db> {
    Plain(PlainVal<'db>),
    /// The caller's own object at this path, as it stood when the call started.
    Ref(Path),
    /// `objects[j]` of the summary: an object the call built.
    Obj(u16),
    /// A method of a project class and the object it is bound to.
    Bound {
        func: Definition<'db>,
        recv: Box<SVal<'db>>,
    },
    Closure {
        func: Definition<'db>,
        free: Box<[(Name, SVals<'db>)]>,     // captured variables
        defaults: Box<[(Name, SVals<'db>)]>, // the defaults computed when its `def` ran
    },
    Back(u8), // the closure k levels up
}

/// Values with no identity. No variant can hold an ActivationId, OriginId, ValueId or DeferId, so a leak does not compile.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum PlainVal<'db> {
    Function(Definition<'db>),
    Class(Definition<'db>),
    Module(Type<'db>),
    Ty(Type<'db>),
    /// An object of a project class that holds no state: what `Walk::lift` makes from the engine's answer.
    Opaque(Definition<'db>),
    Unknown,
}

/// The objects a summary names, in the order `finish` reaches them. One walk gives one order, so equal walks give equal summaries.
/// Keyed by the object and its class: a site that builds one of two classes is two objects here, sharing the state the walk kept.
#[derive(Default)]
struct ObjectNumbering<'db> {
    reached: Vec<(OriginId, Definition<'db>, SiteId)>,
}

impl<'db> ObjectNumbering<'db> {
    fn number(&mut self, origin: OriginId, class: Definition<'db>, site: SiteId) -> SVal<'db> {
        let j = match self
            .reached
            .iter()
            .position(|&(o, c, _)| o == origin && c == class)
        {
            Some(j) => j,
            None => {
                self.reached.push((origin, class, site));
                self.reached.len() - 1
            }
        };
        match u16::try_from(j) {
            Ok(j) => SVal::Obj(j),
            Err(_) => {
                // One object per construct expression of one function body: a body this large is not a real program.
                debug_assert!(false, "a summary names more than {} objects", u16::MAX);
                SVal::Plain(PlainVal::Unknown)
            }
        }
    }
}

impl<'db> Walk<'db> {
    /// The summary so far. Every pending value is replaced by what it resolved to.
    pub(crate) fn finish_round(
        &self,
        s: &Solver<'db>,
        resolved: &IndexVec<DeferId, Values<'db>>,
    ) -> FunctionResult<'db> {
        let mut numbering = ObjectNumbering::default();
        let returns = if self.generator {
            smallvec![SVal::Plain(PlainVal::Unknown)]
        } else {
            self.to_svals(
                s,
                &self.frame.returns,
                resolved,
                0,
                &mut ClosureStack::default(),
                &mut numbering,
            )
        };
        // after the returns, before the objects are described: an object only an effect reaches is numbered after the returns' objects
        let effects = if s.opts.effects {
            self.effects(s, resolved, &mut numbering)
        } else {
            Vec::new()
        };
        let objects = self.describe_objects(s, resolved, &mut numbering);
        FunctionResult {
            calls: self.frame.calls.clone(),
            returns,
            objects,
            effects,
            status: Status::default(),
        }
    }

    /// The state of every object `numbering` holds, read from the joined exit env (`walk_context` leaves it at `activations[frame.act]`).
    /// A queue: describing object `j` can reach object `j + 1`, and the numbering grows while this walks it.
    fn describe_objects(
        &self,
        s: &Solver<'db>,
        resolved: &IndexVec<DeferId, Values<'db>>,
        numbering: &mut ObjectNumbering<'db>,
    ) -> Vec<NewObject<'db>> {
        let mut end = self.activations[self.frame.act].clone();
        for (key, vals) in self.replay_attrs.iter() {
            end.attrs.insert_mut(key.clone(), vals.clone()); // the objects deferred calls applied
        }
        let mut objects = Vec::new();
        while let Some(&(origin, class, site)) = numbering.reached.get(objects.len()) {
            let attrs = end
                .attrs_of(origin)
                .into_iter()
                .map(|(name, vals)| {
                    let vals = self.to_svals(
                        s,
                        &vals,
                        resolved,
                        0,
                        &mut ClosureStack::default(),
                        numbering,
                    );
                    (name, vals)
                })
                .collect();
            objects.push(NewObject { class, site, attrs });
        }
        objects
    }

    /// What this walk wrote onto its input objects, from the end env's `written`: one effect per (object, attribute), `strong` when the
    /// walk wrote it on every path that reaches the end. An attribute that is written and absent was forgotten (`Env::forget_attr`): its
    /// effect is `[Unknown]`, strong, so the next caller up forgets it too and none keeps a stale value. An empty value is not an effect.
    fn effects(
        &self,
        s: &Solver<'db>,
        resolved: &IndexVec<DeferId, Values<'db>>,
        numbering: &mut ObjectNumbering<'db>,
    ) -> Vec<Effect<'db>> {
        let end = &self.activations[self.frame.act];
        // Sorted before the values are converted: converting numbers the objects it reaches, and a hash map's order is no order (rule 9).
        let mut written: Vec<(&Path, &Name, OriginId, Write)> = end
            .written
            .iter()
            .filter_map(|((origin, attr), how)| match &self.origins[*origin] {
                OriginKey::Entry { path, .. } => Some((path, attr, *origin, *how)),
                _ => None,
            })
            .collect();
        written.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        let mut out = Vec::new();
        for (path, attr, origin, how) in written {
            let value = match end.attrs.get(&(origin, attr.clone())) {
                Some(vals) => self.to_svals(
                    s,
                    vals,
                    resolved,
                    0,
                    &mut ClosureStack::default(),
                    numbering,
                ),
                None => smallvec![SVal::Plain(PlainVal::Unknown)],
            };
            if value.is_empty() {
                continue;
            }
            out.push(Effect {
                target: path.clone(),
                attr: attr.clone(),
                value,
                strong: how == Write::Must,
            });
        }
        out
    }

    /// Walk-local values as summary data. `depth` counts the closures these
    /// values sit inside, and `cl` holds them, so a closure that captures
    /// itself is `Back(k)`.
    fn to_svals(
        &self,
        s: &Solver<'db>,
        vals: &Values<'db>,
        resolved: &IndexVec<DeferId, Values<'db>>,
        depth: u8,
        cl: &mut ClosureStack<'db>,
        numbering: &mut ObjectNumbering<'db>,
    ) -> SVals<'db> {
        let mut out = SVals::new();
        for &v in vals {
            match v {
                AbstractValue::Pending(id) => {
                    out.extend(self.to_svals(s, &resolved[id], resolved, depth, cl, numbering))
                }
                // An object the caller gave is its own, at its path; one built here is numbered and described; ty's has no identity.
                AbstractValue::Instance { class, origin } => {
                    out.push(match &self.origins[origin] {
                        OriginKey::Opaque => SVal::Plain(PlainVal::Opaque(class)),
                        OriginKey::Entry { path, .. } => SVal::Ref(path.clone()),
                        OriginKey::New { site, .. } => numbering.number(
                            origin,
                            class,
                            SiteId::new(*site, self.frame.def_start),
                        ),
                    })
                }
                AbstractValue::BoundMethod { func, receiver } => {
                    let recv: Values<'db> = smallvec![self.values[receiver]];
                    for r in self.to_svals(s, &recv, resolved, depth, cl, numbering) {
                        out.push(SVal::Bound {
                            func,
                            recv: Box::new(r),
                        });
                    }
                }
                AbstractValue::Closure { func, activation } => {
                    match cl.position(func, activation) {
                        Some(k) => out.push(SVal::Back(k)),
                        None if depth == s.opts.limits.fp_depth => {
                            out.push(self.closure_at_limit(s, func, activation))
                        }
                        None => out.push(cl.with(func, activation, |cl| {
                            SVal::Closure {
                                func,
                                free: s
                                    .sem
                                    .free_variables(func)
                                    .iter()
                                    .map(|n| {
                                        let v = self.lookup_in(s, func, activation, n);
                                        (
                                            n.clone(),
                                            self.to_svals(
                                                s,
                                                &v,
                                                resolved,
                                                depth + 1,
                                                cl,
                                                numbering,
                                            ),
                                        )
                                    })
                                    .collect(),
                                defaults: self
                                    .defaults
                                    .get(&(func, activation))
                                    .into_iter()
                                    .flatten()
                                    .map(|(n, v)| {
                                        (
                                            n.clone(),
                                            self.to_svals(s, v, resolved, depth + 1, cl, numbering),
                                        )
                                    })
                                    .collect(),
                            }
                        })),
                    }
                }
                other => out.push(SVal::Plain(plain(other))),
            }
        }
        // Two activations can give equal summary values: keep one.
        let mut unique = SVals::new();
        for x in out {
            if !unique.iter().any(|y| same_sval(&x, y)) {
                unique.push(x)
            }
        }
        unique
    }

    /// At the depth limit a closure keeps its function; every captured value
    /// and default is `Unknown`.
    fn closure_at_limit(
        &self,
        s: &Solver<'db>,
        func: Definition<'db>,
        act: ActivationId,
    ) -> SVal<'db> {
        let unknown = |n: &Name| (n.clone(), smallvec![SVal::Plain(PlainVal::Unknown)]);
        SVal::Closure {
            func,
            free: s.sem.free_variables(func).iter().map(unknown).collect(),
            defaults: self
                .defaults
                .get(&(func, act))
                .into_iter()
                .flatten()
                .map(|(n, _)| unknown(n))
                .collect(),
        }
    }

    /// The file range of a site recorded relative to this frame's `def`.
    pub(crate) fn site_range(&self, site: SiteId) -> TextRange {
        TextRange::new(
            self.frame.def_start + site.start,
            self.frame.def_start + site.end,
        )
    }

    /// Phase 1 of applying a summary: the caller's own values for what a callee returns, for the new objects it built, and for the
    /// writes it made onto objects the caller holds. Reads `before`, the env as it was when the call began, and writes nothing in it (it
    /// is a shared borrow); `apply` writes.
    pub(crate) fn resolve(
        &mut self,
        sol: &mut Solver<'db>,
        res: &FunctionResult<'db>,
        call: &CallSite<'_, 'db>,
        before: &Env<'db>,
    ) -> Plan<'db> {
        let objects: Vec<(OriginId, Definition<'db>)> = res
            .objects
            .iter()
            .enumerate()
            .map(|(j, o)| {
                let made = Made::Obj(u16::try_from(j).unwrap_or(u16::MAX));
                let origin = self.origins.intern(OriginKey::New {
                    site: call.site,
                    act: self.frame.act,
                    t: call.t,
                    made,
                });
                (origin, o.class)
            })
            .collect();
        let cx = Resolving {
            call,
            before,
            objects: &objects,
        };
        let returns = self.resolve_in(sol, &res.returns, &mut Vec::new(), &cx);
        let attrs = res
            .objects
            .iter()
            .zip(&objects)
            .map(|(o, &(origin, _))| {
                let attrs = o
                    .attrs
                    .iter()
                    .map(|(name, vals)| {
                        (
                            name.clone(),
                            self.resolve_in(sol, vals, &mut Vec::new(), &cx),
                        )
                    })
                    .collect();
                (origin, attrs)
            })
            .collect();
        let effects = res
            .effects
            .iter()
            .map(|e| {
                // The objects the target path leads to, as `before` held them: state only, so nothing runs.
                let found = self.follow(sol, &e.target, &cx);
                if found.is_empty() {
                    sol.stats.ref_unresolved += 1; // the effect has nowhere to go: counted, like a `Ref` nothing resolved
                }
                let mut targets: SmallVec<[OriginId; 2]> = SmallVec::new();
                for v in found {
                    if let AbstractValue::Instance { origin, .. } = v
                        && origin.holds_state()
                        && !targets.contains(&origin)
                    {
                        targets.push(origin);
                    }
                }
                let value = self.resolve_in(sol, &e.value, &mut Vec::new(), &cx);
                (targets, e.attr.clone(), value, e.strong)
            })
            .collect();
        Plan {
            returns,
            objects: attrs,
            effects,
            site: (call.site, call.t),
            strong_ok: call.strong_ok,
        }
    }

    /// Phase 2: write what `resolve` found into the caller's env, and give the call's value. A new object has nothing to keep, so its
    /// attributes are written strong. An effect replaces the old value only when the callee wrote it on every path, the target is exactly
    /// one object, and the site has one callee (`strong_ok`); otherwise it is joined. A strong effect of nothing but `Unknown` removes the
    /// attribute: a read of it asks the engine's declared type, where keeping the old value would be a stale one.
    ///
    /// In a replay the env is dropped after the operation: the new objects' writes are also kept in `replay_attrs`, and no effect is
    /// applied. Until CS8 carries them, each site that dropped one is counted, once.
    pub(crate) fn apply(
        &mut self,
        sol: &mut Solver<'db>,
        plan: Plan<'db>,
        env: &mut Env<'db>,
    ) -> Values<'db> {
        for (origin, attrs) in plan.objects {
            for (name, vals) in attrs {
                if self.replaying {
                    self.replay_attrs
                        .insert_mut((origin, name.clone()), vals.clone());
                }
                env.bind_attr(origin, &name, vals, true);
            }
        }
        if self.replaying {
            if !plan.effects.is_empty() {
                self.note_effects_in_cycle(sol, plan.site);
            }
            return plan.returns;
        }
        for (targets, attr, vals, strong) in plan.effects {
            let strong = strong && targets.len() == 1 && plan.strong_ok;
            let forgets = vals.iter().all(|v| matches!(v, AbstractValue::Unknown));
            for target in targets {
                if strong && forgets {
                    env.forget_attr(target, &attr);
                } else {
                    env.bind_attr(target, &attr, vals.clone(), strong);
                }
                sol.stats.effects_applied += 1;
            }
        }
        plan.returns
    }

    /// A call whose effects are not carried (a callee still running, or a replay): counted once per site, however many rounds ask.
    pub(crate) fn note_effects_in_cycle(&mut self, sol: &mut Solver<'db>, site: (TextRange, u8)) {
        if self.effects_in_cycle.insert(site) {
            sol.stats.effects_in_cycle += 1;
        }
    }

    /// `before` as an operation of a replay sees it: the env at that point of the walk, and the new objects earlier operations applied.
    pub(crate) fn replay_env(&self, before: &Env<'db>) -> Env<'db> {
        let mut env = before.clone();
        for (key, vals) in self.replay_attrs.iter() {
            env.attrs.insert_mut(key.clone(), vals.clone());
        }
        env
    }

    /// The caller's objects at the end of `path`, as `before` held them: from the call's own arguments (or the closure being called),
    /// then step by step. State only: an attribute is what `before.attrs` holds, never a property or the class side, so nothing runs.
    fn follow(&self, sol: &Solver<'db>, path: &Path, cx: &Resolving<'_, 'db>) -> Values<'db> {
        let key = &sol.contexts[cx.call.ctx];
        let mut cur: Values<'db> = match (path.root, cx.call.enclosing) {
            (Root::Param(i), _) => cx
                .call
                .bound
                .slots
                .get(i as usize)
                .cloned()
                .unwrap_or_default(),
            (Root::Free(i), Some(act)) => key
                .free
                .get(i as usize)
                .map(|(name, _)| self.lookup_in(sol, key.func, act, name))
                .unwrap_or_default(),
            (Root::Default(i), Some(act)) => self
                .defaults
                .get(&(key.func, act))
                .and_then(|d| d.get(i as usize))
                .map(|(_, v)| v.clone())
                .unwrap_or_default(),
            (Root::Free(_) | Root::Default(_), None) => Values::new(),
        };
        for step in &path.steps {
            let mut next = Values::new();
            for v in cur {
                match (step, v) {
                    (Step::Attr(name), AbstractValue::Instance { origin, .. })
                        if origin.holds_state() =>
                    {
                        if let Some(vals) = cx.before.attrs.get(&(origin, name.clone())) {
                            next.extend(vals.iter().copied());
                        }
                    }
                    (Step::Free(n), AbstractValue::Closure { func, activation }) => {
                        if let Some(name) = sol.sem.free_variables(func).get(*n as usize) {
                            next.extend(self.lookup_in(sol, func, activation, name));
                        }
                    }
                    (Step::Default(n), AbstractValue::Closure { func, activation }) => {
                        if let Some((_, vals)) = self
                            .defaults
                            .get(&(func, activation))
                            .and_then(|d| d.get(*n as usize))
                        {
                            next.extend(vals.iter().copied());
                        }
                    }
                    (Step::Recv, AbstractValue::BoundMethod { receiver, .. }) => {
                        next.push(self.values[receiver]);
                    }
                    _ => {}
                }
            }
            dedup(&mut next);
            cur = next;
        }
        cur
    }

    /// `cl`: the closures being rebuilt, innermost last. `Back(k)` is the k-th from the top.
    fn resolve_in(
        &mut self,
        sol: &mut Solver<'db>,
        vals: &SVals<'db>,
        cl: &mut Vec<(Definition<'db>, ActivationId)>,
        cx: &Resolving<'_, 'db>,
    ) -> Values<'db> {
        let mut out = Values::new();
        for sv in vals {
            match sv {
                SVal::Plain(PlainVal::Function(d)) => out.push(AbstractValue::Function(*d)),
                SVal::Plain(PlainVal::Class(d)) => out.push(AbstractValue::Class(*d)),
                SVal::Plain(PlainVal::Module(t)) => out.push(AbstractValue::Module(*t)),
                SVal::Plain(PlainVal::Ty(t)) => out.push(AbstractValue::Ty(*t)),
                SVal::Plain(PlainVal::Opaque(class)) => out.push(AbstractValue::Instance {
                    class: *class,
                    origin: OriginId::OPAQUE,
                }),
                SVal::Plain(PlainVal::Unknown) => out.push(AbstractValue::Unknown),
                SVal::Ref(path) => {
                    let found = self.follow(sol, path, cx);
                    if found.is_empty() {
                        // Never an empty answer: it would say "Python never makes this call" and drop what the engine finds.
                        sol.stats.ref_unresolved += 1;
                        out.push(AbstractValue::Unknown);
                    }
                    out.extend(found);
                }
                SVal::Obj(j) => match cx.objects.get(*j as usize) {
                    Some(&(origin, class)) => out.push(AbstractValue::Instance { class, origin }),
                    None => {
                        sol.stats.ref_unresolved += 1;
                        out.push(AbstractValue::Unknown);
                    }
                },
                SVal::Bound { func, recv } => {
                    let recvs = self.resolve_in(sol, &smallvec![(**recv).clone()], cl, cx);
                    for r in recvs {
                        out.push(AbstractValue::BoundMethod {
                            func: *func,
                            receiver: self.values.intern(r),
                        });
                    }
                }
                SVal::Back(k) => {
                    let (func, activation) = cl[cl.len() - 1 - *k as usize];
                    out.push(AbstractValue::Closure { func, activation });
                }
                SVal::Closure {
                    func,
                    free,
                    defaults,
                } => {
                    let activation = self.activations.push(Env::flat()); // 1. empty
                    cl.push((*func, activation)); // 2. so a Back inside can reach it
                    for (name, vals) in free.iter() {
                        // 3. its values
                        let v = self.resolve_in(sol, vals, cl, cx);
                        self.activations[activation].bind(name, v);
                    }
                    let d = defaults
                        .iter()
                        .map(|(n, v)| (n.clone(), self.resolve_in(sol, v, cl, cx)))
                        .collect();
                    self.defaults.insert((*func, activation), d);
                    cl.pop(); // 4.
                    out.push(AbstractValue::Closure {
                        func: *func,
                        activation,
                    });
                }
            }
        }
        dedup(&mut out);
        out
    }
}

/// What resolving a callee's summary needs from the call.
pub(crate) struct CallSite<'a, 'db> {
    /// The callee's context: its closure's free and default names.
    pub ctx: ContextId,
    /// The call expression, in the file: new objects are named by it.
    pub site: TextRange,
    /// The callee's index at the site (the project callees, in the order `call_values` walks them).
    pub t: u8,
    /// The slots: `Param(i)` is `bound.slots[i]`.
    pub bound: &'a Bound<'db>,
    /// The closure's own run, for `Free(i)` and `Default(i)`.
    pub enclosing: Option<ActivationId>,
    /// The site has one project callee and nothing else it could be (no engine callee, no unknown one): a write the callee made on every
    /// path is a write Python made.
    pub strong_ok: bool,
}

/// What `resolve` found: the call's value, the state of each new object, and each effect, to be written by `apply`.
pub(crate) struct Plan<'db> {
    pub returns: Values<'db>,
    pub objects: Vec<(OriginId, Vec<(Name, Values<'db>)>)>,
    /// Per effect: the objects it targets (state-holding ones, as many as the path leads to), the attribute, the value, and whether the
    /// callee wrote it on every path.
    pub effects: Vec<(SmallVec<[OriginId; 2]>, Name, Values<'db>, bool)>,
    /// The call site and the callee's index at it.
    pub site: (TextRange, u8),
    pub strong_ok: bool,
}

struct Resolving<'a, 'db> {
    call: &'a CallSite<'a, 'db>,
    before: &'a Env<'db>,
    /// `objects[j]` of the summary: its origin here and its class.
    objects: &'a [(OriginId, Definition<'db>)],
}

fn plain(v: AbstractValue<'_>) -> PlainVal<'_> {
    match v {
        AbstractValue::Function(d) => PlainVal::Function(d),
        AbstractValue::Class(d) => PlainVal::Class(d),
        AbstractValue::Module(t) => PlainVal::Module(t),
        AbstractValue::Ty(t) => PlainVal::Ty(t),
        _ => PlainVal::Unknown, // Closure and Pending are handled by `to_svals`
    }
}

pub(crate) fn same_sval<'db>(x: &SVal<'db>, y: &SVal<'db>) -> bool {
    match (x, y) {
        (
            SVal::Closure {
                func: f,
                free: a,
                defaults: da,
            },
            SVal::Closure {
                func: g,
                free: b,
                defaults: db,
            },
        ) => f == g && same_named(a, b) && same_named(da, db),
        _ => x == y,
    }
}

/// The objects of two rounds of one walk: numbered in the order `finish` reaches them, so they are compared by number.
pub(crate) fn same_objects<'db>(a: &[NewObject<'db>], b: &[NewObject<'db>]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.class == y.class && x.site == y.site && same_named(&x.attrs, &y.attrs))
}

/// The effects of two rounds of one walk: sorted by (target, attr), so compared in step; their objects are numbered by `finish_round`.
pub(crate) fn same_effects<'db>(a: &[Effect<'db>], b: &[Effect<'db>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.target == y.target
                && x.attr == y.attr
                && x.strong == y.strong
                && same_svals(&x.value, &y.value)
        })
}

/// Both lists come in a fixed order: free variables sorted, defaults in parameter order.
fn same_named<'db>(a: &[(Name, SVals<'db>)], b: &[(Name, SVals<'db>)]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|((n, x), (m, y))| n == m && same_svals(x, y))
}

/// As sets: the order of alternatives does not matter.
pub(crate) fn same_svals<'db>(a: &[SVal<'db>], b: &[SVal<'db>]) -> bool {
    a.iter().all(|x| b.iter().any(|y| same_sval(x, y)))
        && b.iter().all(|y| a.iter().any(|x| same_sval(x, y)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interp::defer::no_values;
    use crate::interp::fixture::with_solved;
    use crate::interp::solver::walk_context;
    use crate::interp::value::{Path, Root};

    const COMING_BACK: &str = "04-class-summaries/python/coming_back.py";
    const CLOSURE: &str = "04-class-summaries/python/closure_holds_object.py";
    const TWO_EXITS: &str = "04-class-summaries/python/two_exits.py";
    const EFFECTS: &str = "04-class-summaries/python/effects.py";
    const FORGET: &str = "04-class-summaries/python/forget.py";
    const CLOSURE_EFFECT: &str = "04-class-summaries/python/closure_effect.py";

    /// The one context of `func` (`module.name`) the solve made, and its summary.
    fn summary_of<'db>(s: &Solver<'db>, func: &str) -> FunctionResult<'db> {
        let mut found = s
            .contexts
            .iter()
            .filter(|(_, c)| s.sem.qname(c.func) == func)
            .map(|(ctx, _)| ctx);
        let ctx = found
            .next()
            .unwrap_or_else(|| panic!("no context of {func}"));
        assert!(found.next().is_none(), "{func} has more than one context");
        (**s.slots[ctx].result.as_ref().expect("a finished summary")).clone()
    }

    fn name(n: &str) -> Name {
        Name::new(n)
    }

    #[test]
    fn a_function_that_returns_its_argument_returns_a_ref_to_it() {
        with_solved(COMING_BACK, "identity", |s| {
            let res = summary_of(s, "coming_back.ident");
            assert_eq!(
                res.returns.as_slice(),
                &[SVal::Ref(Path::root(Root::Param(0)))]
            );
            assert!(res.objects.is_empty());
        });
    }

    #[test]
    fn an_object_built_and_returned_is_described_with_what_it_holds() {
        with_solved(COMING_BACK, "built_inside", |s| {
            let res = summary_of(s, "coming_back.make_json");
            assert_eq!(res.returns.as_slice(), &[SVal::Obj(0)]);
            assert_eq!(res.objects.len(), 2);
            let (boxed, handler) = (&res.objects[0], &res.objects[1]);
            assert_eq!(s.sem.qname(boxed.class), "coming_back.Box");
            assert_eq!(
                boxed.attrs.as_slice(),
                &[(name("handler"), smallvec![SVal::Obj(1)])]
            );
            assert_eq!(s.sem.qname(handler.class), "coming_back.JsonHandler");
            assert!(handler.attrs.is_empty());
        });
    }

    #[test]
    fn an_object_built_from_an_argument_holds_a_ref_to_the_argument() {
        with_solved(COMING_BACK, "built_with_arg", |s| {
            let res = summary_of(s, "coming_back.make_box");
            assert_eq!(res.returns.as_slice(), &[SVal::Obj(0)]);
            assert_eq!(
                res.objects[0].attrs.as_slice(),
                &[(
                    name("handler"),
                    smallvec![SVal::Ref(Path::root(Root::Param(0)))]
                )]
            );
        });
    }

    #[test]
    fn two_runs_of_one_walk_give_equal_summaries() {
        with_solved(COMING_BACK, "built_inside", |s| {
            let ctx = s
                .contexts
                .iter()
                .find(|(_, c)| s.sem.qname(c.func) == "coming_back.make_json")
                .map(|(ctx, _)| ctx)
                .unwrap();
            fn run<'db>(s: &mut Solver<'db>, ctx: ContextId) -> FunctionResult<'db> {
                let w = walk_context(s, ctx);
                w.finish_round(s, &no_values(&w))
            }
            let (first, second) = (run(s, ctx), run(s, ctx));
            assert!(!first.objects.is_empty());
            assert_eq!(first, second);
        });
    }

    #[test]
    fn an_object_only_a_returned_closure_holds_is_numbered_and_described() {
        with_solved(CLOSURE, "main", |s| {
            let res = summary_of(s, "closure_holds_object.make_getter");
            let [SVal::Closure { free, .. }] = res.returns.as_slice() else {
                panic!("expected one closure, got {:?}", res.returns);
            };
            assert_eq!(&free[..], &[(name("b"), smallvec![SVal::Obj(0)])]);
            assert_eq!(res.objects.len(), 2);
            assert_eq!(
                s.sem.qname(res.objects[0].class),
                "closure_holds_object.Box"
            );
            assert_eq!(
                res.objects[0].attrs.as_slice(),
                &[(name("handler"), smallvec![SVal::Obj(1)])]
            );
            assert_eq!(
                s.sem.qname(res.objects[1].class),
                "closure_holds_object.Handler"
            );
        });
    }

    #[test]
    fn the_state_a_summary_reads_is_every_exit_joined() {
        with_solved(TWO_EXITS, "main", |s| {
            let res = summary_of(s, "two_exits.pick");
            assert_eq!(res.returns.as_slice(), &[SVal::Obj(0)]);
            // the handler is each exit's own, numbered in the order `finish` reaches them
            let [(attr, handlers)] = res.objects[0].attrs.as_slice() else {
                panic!("expected one attribute, got {:?}", res.objects[0].attrs);
            };
            assert_eq!(attr, &name("handler"));
            let mut classes: Vec<String> = handlers
                .iter()
                .map(|h| match h {
                    SVal::Obj(j) => s.sem.qname(res.objects[*j as usize].class),
                    other => panic!("expected an object, got {other:?}"),
                })
                .collect();
            classes.sort();
            assert_eq!(classes, ["two_exits.CsvHandler", "two_exits.JsonHandler"]);
        });
    }

    #[test]
    fn a_write_onto_an_argument_is_a_strong_effect_that_refers_to_another_argument() {
        with_solved(EFFECTS, "installed", |s| {
            let res = summary_of(s, "effects.install");
            let [e] = res.effects.as_slice() else {
                panic!("expected one effect, got {:?}", res.effects);
            };
            assert_eq!(e.target, Path::root(Root::Param(0)));
            assert_eq!(e.attr, name("handler"));
            assert_eq!(e.value.as_slice(), &[SVal::Ref(Path::root(Root::Param(1)))]);
            assert!(e.strong);
        });
    }

    #[test]
    fn a_write_on_one_path_is_a_weak_effect_that_also_holds_what_was_there() {
        with_solved(EFFECTS, "maybe_installed", |s| {
            let res = summary_of(s, "effects.maybe");
            let [e] = res.effects.as_slice() else {
                panic!("expected one effect, got {:?}", res.effects);
            };
            assert_eq!(e.target, Path::root(Root::Param(0)));
            assert!(!e.strong);
            let before = Path::root(Root::Param(0)).attr(&name("handler"));
            assert!(
                e.value.contains(&SVal::Ref(before)),
                "the old value is one of the alternatives: {:?}",
                e.value
            );
            assert!(e.value.contains(&SVal::Ref(Path::root(Root::Param(1)))));
        });
    }

    #[test]
    fn a_constructor_s_write_onto_self_is_an_effect_like_any_other() {
        with_solved(EFFECTS, "ctor", |s| {
            let res = summary_of(s, "effects.Pipeline.__init__");
            let [e] = res.effects.as_slice() else {
                panic!("expected one effect, got {:?}", res.effects);
            };
            assert_eq!(e.target, Path::root(Root::Param(0)));
            assert_eq!(e.attr, name("handler"));
            assert_eq!(e.value.as_slice(), &[SVal::Ref(Path::root(Root::Param(1)))]);
            assert!(e.strong);
        });
    }

    #[test]
    fn an_object_a_callee_builds_and_stores_is_an_effect_value_and_is_described() {
        with_solved(EFFECTS, "fresh_installed", |s| {
            let res = summary_of(s, "effects.fresh");
            let [e] = res.effects.as_slice() else {
                panic!("expected one effect, got {:?}", res.effects);
            };
            assert_eq!(e.value.as_slice(), &[SVal::Obj(0)]);
            assert_eq!(s.sem.qname(res.objects[0].class), "effects.JsonHandler");
        });
    }

    #[test]
    fn a_write_a_callee_makes_is_the_callers_own_effect() {
        with_solved(EFFECTS, "through_two", |s| {
            let res = summary_of(s, "effects.outer_install");
            let [e] = res.effects.as_slice() else {
                panic!("expected one effect, got {:?}", res.effects);
            };
            assert_eq!(e.target, Path::root(Root::Param(0)));
            assert_eq!(e.value.as_slice(), &[SVal::Ref(Path::root(Root::Param(1)))]);
            assert!(e.strong);
        });
    }

    #[test]
    fn a_value_the_engine_cannot_resolve_is_unknown_and_a_strong_write_of_it_travels_up() {
        with_solved(FORGET, "main2", |s| {
            let forgotten = [SVal::Plain(PlainVal::Unknown)];
            for func in ["forget.wipe", "forget.wipe_twice"] {
                let res = summary_of(s, func);
                let [e] = res.effects.as_slice() else {
                    panic!("{func}: expected one effect, got {:?}", res.effects);
                };
                assert_eq!(e.target, Path::root(Root::Param(0)), "{func}");
                assert_eq!(e.attr, name("h"), "{func}");
                assert_eq!(e.value.as_slice(), &forgotten, "{func}");
                assert!(e.strong, "{func}");
            }
        });
    }

    #[test]
    fn a_write_onto_a_closure_s_free_object_is_reported_on_its_free_root() {
        with_solved(CLOSURE_EFFECT, "main", |s| {
            let res = summary_of(s, "closure_effect.main.swap");
            let [e] = res.effects.as_slice() else {
                panic!("expected one effect, got {:?}", res.effects);
            };
            assert_eq!(e.target, Path::root(Root::Free(0)));
            assert_eq!(e.attr, name("h"));
            assert_eq!(e.value.as_slice(), &[SVal::Obj(0)]);
            assert!(e.strong);
        });
    }

    #[test]
    fn effects_sort_by_target_then_attribute() {
        let (a, b) = (Path::root(Root::Param(0)), Path::root(Root::Param(1)));
        assert!(a < b);
        assert!(a < a.attr(&name("x")));
        assert!(a.attr(&name("x")) < a.attr(&name("y")));
        assert!(Path::root(Root::Param(9)) < Path::root(Root::Free(0)));
    }
}
