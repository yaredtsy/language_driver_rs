use crate::interp::key::Interner;
use crate::sem::Name;
use crate::{
    interp::{solver::Solver, walk::Walk},
    sem::Shape,
};
use ruff_index::newtype_index;
use ruff_text_size::TextRange;
use smallvec::SmallVec;
use smallvec::smallvec;
use ty_python_core::definition::Definition;
use ty_python_semantic::types::Type;
// src/interp/value.rs
/// One run of a function body inside the walk. Walk-local: never leaves a summary.
#[newtype_index]
pub struct ActivationId;

/// An interned `AbstractValue`, so a value can hold another and stay `Copy`.
#[newtype_index]
pub struct ValueId;

/// Per walk. The same value is interned once.
pub type ValueArena<'db> = Interner<AbstractValue<'db>, ValueId>;

/// One object, named by where it was built. Walk-local, like `ActivationId`.
#[newtype_index]
#[derive(Ord, PartialOrd)] // `cut_order` sorts by origin
pub struct OriginId;

impl OriginId {
    /// Every object ty hands you. Interned first in every walk; it holds no state.
    pub const OPAQUE: OriginId = OriginId::from_u32(0);

    pub fn holds_state(self) -> bool {
        self != OriginId::OPAQUE
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum OriginKey {
    /// An object ty gave us, not built in this walk: `OriginId::OPAQUE`.
    Opaque,
    /// An input object, named by where it sits among the inputs. One per path per run.
    Entry { act: ActivationId, path: Path },
    /// An object built in this walk, at this site, in this run of its body.
    New { site: TextRange, act: ActivationId },
}

/// Where an input object sits: a root, then steps from it.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Path {
    pub root: Root,
    pub steps: SmallVec<[Step; 2]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Root {
    Param(u8),
    /// Free value `i` of the walked closure itself.
    Free(u8),
    /// Default `i` of the walked closure itself.
    Default(u8),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Step {
    Attr(Name),
    /// Free value `n` of the closure at the path so far.
    Free(u8),
    Default(u8),
    /// A bound method's receiver.
    Recv,
}

impl Path {
    pub fn root(root: Root) -> Self {
        Path {
            root,
            steps: SmallVec::new(),
        }
    }
    fn then(&self, step: Step) -> Self {
        let mut p = self.clone();
        p.steps.push(step);
        p
    }
    pub fn attr(&self, name: &Name) -> Self {
        self.then(Step::Attr(name.clone()))
    }
    pub fn free(&self, i: u8) -> Self {
        self.then(Step::Free(i))
    }
    pub fn default(&self, i: u8) -> Self {
        self.then(Step::Default(i))
    }
    pub fn recv(&self) -> Self {
        self.then(Step::Recv)
    }
}

/// A deferred operation of one walk: a recursive call, or a use of its value.
#[newtype_index]
pub struct DeferId;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AbstractValue<'db> {
    Function(Definition<'db>), // a def: something you can walk
    Closure {
        func: Definition<'db>,
        activation: ActivationId, // the run whose `def` statement made it
    },
    Class(Definition<'db>),
    Instance {
        class: Definition<'db>,
        origin: OriginId,
    },
    BoundMethod {
        func: Definition<'db>,
        receiver: ValueId, // our own object (or class), not ty's
    },
    Ty(Type<'db>), // ty's answer, for everything you don't model
    Module(Type<'db>),
    Unknown,          // nothing known
    Pending(DeferId), // the value of an operation waiting on a recursive call
}

/// Several possibilities, kept apart. Never collapsed into one ty union.
pub type Values<'db> = SmallVec<[AbstractValue<'db>; 2]>;

pub fn dedup(vals: &mut Values<'_>) {
    let mut seen = Vec::with_capacity(vals.len());
    vals.retain(|v| {
        if seen.contains(v) {
            false
        } else {
            seen.push(*v);
            true
        }
    });
}
impl<'db> Walk<'db> {
    pub fn lift(&mut self, sol: &mut Solver<'db>, ty: Type<'db>) -> Values<'db> {
        match sol.sem.shape(ty, self.frame.file) {
            Shape::Module => smallvec![AbstractValue::Module(ty)],
            Shape::Function(d) => smallvec![AbstractValue::Function(d)],
            // An object of a project class that nobody built in this walk. Other classes
            // (`int`, `str`) stay `Ty`: they hold no project state, and keys tell them apart.
            Shape::Instance(class) if sol.sem.is_project(class) => {
                smallvec![AbstractValue::Instance {
                    class,
                    origin: OriginId::OPAQUE
                }]
            }
            Shape::Instance(_) => smallvec![AbstractValue::Ty(ty)],
            // A project method: keep the function, give it a receiver. Others (`s.upper`)
            // stay `Ty`: the call goes to ty anyway, and keys tell them apart.
            Shape::BoundMethod { func, receiver } if sol.sem.is_project(func) => {
                let recvs = match receiver {
                    Some(r) => self.lift(sol, r),
                    None => smallvec![AbstractValue::Unknown],
                };
                recvs
                    .into_iter()
                    .map(|r| AbstractValue::BoundMethod {
                        func,
                        receiver: self.values.intern(r),
                    })
                    .collect()
            }
            Shape::BoundMethod { .. } => smallvec![AbstractValue::Ty(ty)],
            Shape::SubclassOf(Some(class)) => {
                sol.stats.lifted_subclass_of += 1;
                smallvec![AbstractValue::Class(class)]
            }
            Shape::SubclassOf(None) => smallvec![AbstractValue::Unknown], // type[Any], type[T]
            // A fallback: ty expands an alias in a declared global or attribute type
            // before you see it (checked against `isolation.py`'s `via_alias`). It
            // survives only in rarer spots, such as recursive aliases.
            Shape::Alias(value) => self.lift(sol, value),
            Shape::Class(d) => smallvec![AbstractValue::Class(d)],
            Shape::Union(members) => members
                .into_iter()
                .flat_map(|t| self.lift(sol, t))
                .collect(),
            Shape::Unknown => smallvec![AbstractValue::Unknown],
            Shape::Other => smallvec![AbstractValue::Ty(ty)],
        }
    }
}
