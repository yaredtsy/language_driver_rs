// src/interp/summary.rs
use analyzer_core::graph::Basis;
use ruff_text_size::{TextRange, TextSize};
use smallvec::{SmallVec, smallvec};

use crate::interp::env::Env;
use crate::interp::key::ClosureStack;
use crate::interp::key::ContextId;
use crate::interp::solver::Solver;
use crate::interp::value::ActivationId;
use crate::interp::value::{AbstractValue, DeferId, OriginId, OriginKey, Path, Values, dedup};
use crate::interp::walk::Walk;
use crate::sem::{Definition, Name, Type};
use ruff_index::IndexVec;

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct FunctionResult<'db> {
    pub calls: Vec<CallEdge<'db>>, // one per call site reached, in evaluation order
    pub returns: SVals<'db>,       // the union of every return
    /// The objects this call built that `returns` can reach, numbered in the order `finish` reaches them: `Obj(j)` is `objects[j]`.
    pub objects: Vec<NewObject<'db>>,
    /// What an `__init__` wrote onto `self`, relative to its inputs. Empty unless `Options::constructor_effects` is on.
    pub effects: Vec<Effect<'db>>,
    /// Attribute values of `self` an `__init__` wrote that no `EffectValue` can say (an object it built, a closure, a derived value).
    pub effects_dropped: u32,
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

/// `self.<attr> = <value>` in an `__init__`, written so that a caller with other arguments can replay it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Effect<'db> {
    pub attr: Name,
    pub value: EffectValue<'db>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum EffectValue<'db> {
    /// Whatever the caller passed for parameter slot `k` (the slot order of `Bound`: `self` is 0), defaults filled.
    Param(usize),
    /// A value with no identity: a class, a function, a module, a type, or nothing known.
    Plain(PlainVal<'db>),
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
            effects_dropped: 0,
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
        let objects = self.describe_objects(s, resolved, &mut numbering);
        let (effects, effects_dropped) = if s.opts.constructor_effects {
            self.init_effects(s, resolved)
        } else {
            (Vec::new(), 0)
        };
        FunctionResult {
            calls: self.frame.calls.clone(),
            returns,
            objects,
            effects,
            effects_dropped,
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
        let end = &self.activations[self.frame.act];
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

    /// What this walk wrote onto `self`, if it is an `__init__` run on an object that holds state: one `Effect` per attribute and
    /// value, sorted by attribute name, and how many values could not be said. A value is `Param(k)` when it is exactly what slot `k`
    /// held at the start (the first such `k`: callers that share this context have equal fingerprints there, so any of them says the same),
    /// else a plain value, else dropped. Nothing the walk did not change is an effect.
    fn init_effects(
        &self,
        s: &Solver<'db>,
        resolved: &IndexVec<DeferId, Values<'db>>,
    ) -> (Vec<Effect<'db>>, u32) {
        let none = (Vec::new(), 0);
        if s.sem.function_name(self.frame.func) != "__init__" {
            return none;
        }
        let Some([AbstractValue::Instance { origin, .. }]) =
            self.inputs.first().map(|v| v.as_slice())
        else {
            return none;
        };
        let origin = *origin;
        if !origin.holds_state() {
            return none;
        }
        let end = &self.activations[self.frame.act];
        let (mut effects, mut dropped) = (Vec::new(), 0);
        for (attr, vals) in end.attrs_of(origin) {
            if self.start_attrs.get(&(origin, attr.clone())) == Some(&vals) {
                continue; // it came in this way
            }
            let mut settled = Values::new();
            self.settle(&vals, resolved, &mut settled);
            for v in settled {
                let value = match v {
                    AbstractValue::Unknown => EffectValue::Plain(PlainVal::Unknown),
                    AbstractValue::Instance { origin: o, .. } if o == origin => {
                        dropped += 1; // `self.me = self`
                        continue;
                    }
                    _ => match self
                        .inputs
                        .iter()
                        .skip(1)
                        .position(|slot| slot.contains(&v))
                    {
                        Some(k) => EffectValue::Param(k + 1),
                        None => match v {
                            AbstractValue::Function(_)
                            | AbstractValue::Class(_)
                            | AbstractValue::Module(_)
                            | AbstractValue::Ty(_) => EffectValue::Plain(plain(v)),
                            _ => {
                                dropped += 1; // an object built here, a closure, a bound method, a derived value
                                continue;
                            }
                        },
                    },
                };
                let effect = Effect {
                    attr: attr.clone(),
                    value,
                };
                if !effects.contains(&effect) {
                    effects.push(effect);
                }
            }
        }
        (effects, dropped)
    }

    /// `vals` with every pending value replaced by what it resolved to.
    fn settle(
        &self,
        vals: &Values<'db>,
        resolved: &IndexVec<DeferId, Values<'db>>,
        out: &mut Values<'db>,
    ) {
        for &v in vals {
            match v {
                AbstractValue::Pending(id) => self.settle(&resolved[id], resolved, out),
                other => out.push(other),
            }
        }
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

    /// The caller's own values for a callee's returns. `Ref` and `Obj` are not resolved yet (CS2b): each is `Unknown`, as every object was
    /// before a summary could name one, and is counted `ref_unresolved`. An empty answer would say "Python never makes this call".
    pub(crate) fn resolve(&mut self, sol: &mut Solver<'db>, returns: &SVals<'db>) -> Values<'db> {
        self.resolve_in(sol, returns, &mut Vec::new())
    }

    /// `cl`: the closures being rebuilt, innermost last. `Back(k)` is the k-th from the top.
    fn resolve_in(
        &mut self,
        sol: &mut Solver<'db>,
        vals: &SVals<'db>,
        cl: &mut Vec<(Definition<'db>, ActivationId)>,
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
                SVal::Ref(_) | SVal::Obj(_) => {
                    sol.stats.ref_unresolved += 1;
                    out.push(AbstractValue::Unknown);
                }
                SVal::Bound { func, recv } => {
                    let recvs = self.resolve_in(sol, &smallvec![(**recv).clone()], cl);
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
                        let v = self.resolve_in(sol, vals, cl);
                        self.activations[activation].bind(name, v);
                    }
                    let d = defaults
                        .iter()
                        .map(|(n, v)| (n.clone(), self.resolve_in(sol, v, cl)))
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
        dedup(&mut out); // two objects that cannot be resolved yet are one `Unknown`, as they were
        out
    }
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
}
