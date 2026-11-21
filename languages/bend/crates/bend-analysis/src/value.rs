//! What a place can hold (`docs/08-bend/07-value-layer-design.md` §3, BV-D1): a closed set of the things that can be there, and whether the set is complete.
//!
//! This step has two members, the two BC1+BC2 knows: a def of the book with code, and a lambda that stays with the def that writes it. The closure, the partial
//! application and the constructor are added with the steps that make them (V4, V7, V8), and `key.rs`'s `Known` becomes a `Value` with them.

use crate::book::DeclId;
use crate::key::Known;

/// One thing a place can hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum One {
    /// A def of the book with code, in the root.
    Def(DeclId),
    /// A lambda that stays with its writer (06 §6): the `nth` one the walk of `writer`'s body enters. Known, never entered.
    Lambda { writer: DeclId, nth: u32 },
}

/// What can be at a place: the members, sorted and without duplicates, and, when `open`, something the layer does not know too.
///
/// 🦀 `#[derive(PartialOrd, Ord)]` on `One` is what lets the members be kept sorted: two values with the same members are then equal as bytes, which is what a
/// later key needs ("the same input, same output"). The derive orders by variant first, then by the fields in the order they are written.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Value {
    known: Box<[One]>,
    open: bool,
}

impl Value {
    /// Nothing is known: ask the book, as layer 1 does.
    pub(crate) fn unknown() -> Value {
        Value {
            known: Box::new([]),
            open: true,
        }
    }

    /// Nothing callable is there, and nothing is lost by saying so: a number, a string, a type. The empty closed set.
    pub(crate) fn data() -> Value {
        Value {
            known: Box::new([]),
            open: false,
        }
    }

    /// Exactly this, and nothing else.
    pub(crate) fn one(one: One) -> Value {
        Value {
            known: Box::new([one]),
            open: false,
        }
    }

    /// What a key's slot holds.
    pub(crate) fn from_known(known: &Known) -> Value {
        Value::one(match *known {
            Known::Def(def) => One::Def(def),
            Known::Lambda { writer, nth } => One::Lambda { writer, nth },
        })
    }

    /// The one thing this is, when it is exactly one and nothing else can be there: what a key's slot can hold.
    pub(crate) fn single(&self) -> Option<One> {
        match (&*self.known, self.open) {
            ([one], false) => Some(*one),
            _ => None,
        }
    }

    /// The members, sorted.
    #[cfg(test)]
    pub(crate) fn members(&self) -> &[One] {
        &self.known
    }

    #[cfg(test)]
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// What can be at a place that holds this in one case and `other` in another: the members of both, and open when either is. No case is ruled out (no condition is
    /// evaluated), so the join of the cases of a `match` is the value of the `match`. The cap on the size of a set, and its counter, come with the first value that
    /// grows (V4).
    pub(crate) fn join(&self, other: &Value) -> Value {
        let mut known: Vec<One> = self
            .known
            .iter()
            .chain(other.known.iter())
            .copied()
            .collect();
        known.sort_unstable();
        known.dedup();
        Value {
            known: known.into(),
            open: self.open || other.open,
        }
    }
}

impl Known {
    /// A slot is a value that is exactly one def or lambda; `None` for any other.
    pub(crate) fn of(value: &Value) -> Option<Known> {
        value.single().map(|one| match one {
            One::Def(def) => Known::Def(def),
            One::Lambda { writer, nth } => Known::Lambda { writer, nth },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(n: usize) -> One {
        One::Def(DeclId::new(0, n))
    }

    #[test]
    fn unknown_is_open_with_no_member_and_data_is_closed_with_none() {
        assert!(Value::unknown().is_open());
        assert!(Value::unknown().members().is_empty());
        assert!(!Value::data().is_open());
        assert!(Value::data().members().is_empty());
        assert_ne!(Value::unknown(), Value::data());
        assert_eq!(Value::unknown().single(), None);
        assert_eq!(Value::data().single(), None);
    }

    #[test]
    fn a_single_is_one_closed_member() {
        assert_eq!(Value::one(def(3)).single(), Some(def(3)));
        let lambda = One::Lambda {
            writer: DeclId::new(0, 1),
            nth: 2,
        };
        assert_eq!(Value::one(lambda).single(), Some(lambda));
        // two members, or one member that something else may be beside: not a slot
        assert_eq!(Value::one(def(1)).join(&Value::one(def(2))).single(), None);
        assert_eq!(Value::one(def(1)).join(&Value::unknown()).single(), None);
    }

    #[test]
    fn a_join_is_sorted_has_no_duplicates_and_is_open_when_either_is() {
        let a = Value::one(def(2)).join(&Value::one(def(1)));
        let b = Value::one(def(1)).join(&Value::one(def(2)));
        // the order the cases came in does not matter
        assert_eq!(a, b);
        assert_eq!(a.members(), [def(1), def(2)]);
        assert_eq!(a.join(&Value::one(def(2))), a);
        assert!(!a.is_open());
        assert!(a.join(&Value::unknown()).is_open());
        // data adds nothing
        assert_eq!(a.join(&Value::data()), a);
        assert_eq!(Value::data().join(&Value::data()), Value::data());
    }

    #[test]
    fn a_key_slot_and_a_value_are_the_same_thing() {
        let known = Known::Def(DeclId::new(0, 5));
        assert_eq!(Known::of(&Value::from_known(&known)), Some(known));
        let lambda = Known::Lambda {
            writer: DeclId::new(0, 5),
            nth: 1,
        };
        assert_eq!(Known::of(&Value::from_known(&lambda)), Some(lambda));
        assert_eq!(Known::of(&Value::unknown()), None);
    }
}
