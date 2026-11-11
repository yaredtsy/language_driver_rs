// src/interp/summary.rs
use analyzer_core::graph::Basis;
use ruff_text_size::{TextRange, TextSize};
use smallvec::{SmallVec, smallvec};

use crate::interp::env::Env;
use crate::interp::key::ClosureStack;
use crate::interp::key::ContextId;
use crate::interp::solver::Solver;
use crate::interp::value::ActivationId;
use crate::interp::value::{AbstractValue, DeferId, Values};
use crate::interp::walk::Walk;
use crate::sem::{Definition, Name, Type};
use ruff_index::IndexVec;

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct FunctionResult<'db> {
    pub calls: Vec<CallEdge<'db>>, // one per call site reached, in evaluation order
    pub returns: SVals<'db>,       // the union of every return
    pub status: Status,
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
}

impl<'db> FunctionResult<'db> {
    /// What a call returns when a cost limit stopped it: something, we stopped looking.
    pub(crate) fn cut(cut: Cut) -> Self {
        FunctionResult {
            calls: Vec::new(),
            returns: smallvec![SVal::Plain(PlainVal::Unknown)],
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
    Closure {
        func: Definition<'db>,
        free: Box<[(Name, SVals<'db>)]>,     // captured variables
        defaults: Box<[(Name, SVals<'db>)]>, // the defaults computed when its `def` ran
    },
    Back(u8), // the closure k levels up
              // Ref, Obj, Bound: part 04
}

/// Values with no identity. No variant can hold an ActivationId, and later
/// no OriginId, ValueId or DeferId, so a leak does not compile.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum PlainVal<'db> {
    Function(Definition<'db>),
    Class(Definition<'db>),
    Module(Type<'db>),
    Ty(Type<'db>),
    Unknown,
}

impl<'db> Walk<'db> {
    /// The summary so far. Every pending value is replaced by what it resolved to.
    pub(crate) fn finish_round(
        &self,
        s: &Solver<'db>,
        resolved: &IndexVec<DeferId, Values<'db>>,
    ) -> FunctionResult<'db> {
        let returns = if self.generator {
            smallvec![SVal::Plain(PlainVal::Unknown)]
        } else {
            self.to_svals(
                s,
                &self.frame.returns,
                resolved,
                0,
                &mut ClosureStack::default(),
            )
        };
        FunctionResult {
            calls: self.frame.calls.clone(),
            returns,
            status: Status::default(),
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
    ) -> SVals<'db> {
        let mut out = SVals::new();
        for &v in vals {
            match v {
                AbstractValue::Pending(id) => {
                    out.extend(self.to_svals(s, &resolved[id], resolved, depth, cl))
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
                                        (n.clone(), self.to_svals(s, &v, resolved, depth + 1, cl))
                                    })
                                    .collect(),
                                defaults: self
                                    .defaults
                                    .get(&(func, activation))
                                    .into_iter()
                                    .flatten()
                                    .map(|(n, v)| {
                                        (n.clone(), self.to_svals(s, v, resolved, depth + 1, cl))
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

    /// The caller's own values for a callee's returns.
    pub(crate) fn resolve(&mut self, returns: &SVals<'db>) -> Values<'db> {
        self.resolve_in(returns, &mut Vec::new())
    }

    /// `cl`: the closures being rebuilt, innermost last. `Back(k)` is the k-th from the top.
    fn resolve_in(
        &mut self,
        vals: &SVals<'db>,
        cl: &mut Vec<(Definition<'db>, ActivationId)>,
    ) -> Values<'db> {
        let mut out = Values::new();
        for sv in vals {
            out.push(match sv {
                SVal::Plain(PlainVal::Function(d)) => AbstractValue::Function(*d),
                SVal::Plain(PlainVal::Class(d)) => AbstractValue::Class(*d),
                SVal::Plain(PlainVal::Module(t)) => AbstractValue::Module(*t),
                SVal::Plain(PlainVal::Ty(t)) => AbstractValue::Ty(*t),
                SVal::Plain(PlainVal::Unknown) => AbstractValue::Unknown,
                SVal::Back(k) => {
                    let (func, activation) = cl[cl.len() - 1 - *k as usize];
                    AbstractValue::Closure { func, activation }
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
                        let v = self.resolve_in(vals, cl);
                        self.activations[activation].bind(name, v);
                    }
                    let d = defaults
                        .iter()
                        .map(|(n, v)| (n.clone(), self.resolve_in(v, cl)))
                        .collect();
                    self.defaults.insert((*func, activation), d);
                    cl.pop(); // 4.
                    AbstractValue::Closure {
                        func: *func,
                        activation,
                    }
                }
            });
        }
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
