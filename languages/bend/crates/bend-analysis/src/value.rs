//! What a place can hold (`docs/08-bend/07-value-layer-design.md` §3, BV-D1): a closed set of the things that can be there, and whether the set is complete.
//!
//! This step has two members, the two BC1+BC2 knows: a def of the book with code, and a lambda that stays with the def that writes it. The closure, the partial
//! application and the constructor are added with the steps that make them (V4, V7, V8). A key's slot is a `Value`.

use crate::book::DeclId;

/// The most members a set may have (BV-D8): a larger one is unknown.
pub(crate) const MAX_SET: usize = 8;

/// How deep a value may hold values (BV-D8): a value nested deeper inside a value is unknown there, and the outer partial application or constructor is kept.
pub(crate) const MAX_DEPTH: usize = 3;

/// One thing a place can hold.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum One {
    /// A def of the book with code, in the root.
    Def(DeclId),
    /// A lambda that stays with its writer (06 §6): the `nth` one the walk of `writer`'s body enters. Known, never entered.
    Lambda { writer: DeclId, nth: u32 },
    /// A def applied to fewer arguments than it has written parameters (07 §7): what it was given, by position from the first. A def only: a partial application of a
    /// Base def or of a lambda is unknown.
    Partial { def: DeclId, args: Box<[Value]> },
    /// A constructor of a project type and what it was given, by field from the first (07 §9). Nothing can write a field afterwards, so the value is the object.
    Ctor {
        ty: DeclId,
        name: Box<str>,
        fields: Box<[Value]>,
    },
}

/// What can be at a place: the members, sorted and without duplicates, and, when `open`, something the layer does not know too.
///
/// 🦀 `#[derive(PartialOrd, Ord)]` on `One` is what lets the members be kept sorted: two values with the same members are then equal as bytes, which is what a
/// later key needs ("the same input, same output"). The derive orders by variant first, then by the fields in the order they are written.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Value {
    known: Box<[One]>,
    open: bool,
    /// Too much for the caps (a set of more than [`MAX_SET`], a value nested deeper than [`MAX_DEPTH`]): the top, which every join with it is. It has no member and is
    /// open, as `unknown` is, and it is not `unknown`: something that is merely not known still adds its members to a join, and a cap that did the same would give a
    /// different answer to whoever reached it first (07 §7).
    wide: bool,
}

impl Value {
    /// Nothing is known: ask the book, as layer 1 does.
    pub(crate) fn unknown() -> Value {
        Value {
            known: Box::new([]),
            open: true,
            wide: false,
        }
    }

    /// What a cap makes of a value that is too much: it is as unknown as `unknown`, and a join with it is itself.
    pub(crate) fn top() -> Value {
        Value {
            known: Box::new([]),
            open: true,
            wide: true,
        }
    }

    /// Nothing callable is there, and nothing is lost by saying so: a number, a string, a type. The empty closed set.
    pub(crate) fn data() -> Value {
        Value {
            known: Box::new([]),
            open: false,
            wide: false,
        }
    }

    /// Exactly this, and nothing else.
    pub(crate) fn one(one: One) -> Value {
        Value {
            known: Box::new([one]),
            open: false,
            wide: false,
        }
    }

    /// The one thing this is, when it is exactly one and nothing else can be there: what a key's slot can hold.
    pub(crate) fn single(&self) -> Option<&One> {
        match (&*self.known, self.open) {
            ([one], false) => Some(one),
            _ => None,
        }
    }

    /// Whether a def, a lambda or a partial application is anywhere in it, however deep: the only things a call can come to.
    pub(crate) fn holds_function(&self) -> bool {
        self.known.iter().any(|one| match one {
            One::Def(_) | One::Lambda { .. } | One::Partial { .. } => true,
            One::Ctor { fields, .. } => fields.iter().any(Value::holds_function),
        })
    }

    /// The same value with what is nested deeper than [`MAX_DEPTH`] unknown, and whether anything was.
    pub(crate) fn capped(&self) -> (Value, bool) {
        let mut hit = false;
        let value = self.cap(0, &mut hit);
        (value, hit)
    }

    /// What a partial application or a constructor holds, one level down: a value that would be deeper than the cap is unknown.
    fn cap_all(held: &[Value], depth: usize, hit: &mut bool) -> Box<[Value]> {
        held.iter()
            .map(|value| {
                if depth + 1 > MAX_DEPTH {
                    *hit = true;
                    Value::top()
                } else {
                    value.cap(depth + 1, hit)
                }
            })
            .collect()
    }

    fn cap(&self, depth: usize, hit: &mut bool) -> Value {
        let mut known: Vec<One> = self
            .known
            .iter()
            .map(|one| match one {
                One::Partial { def, args } => One::Partial {
                    def: *def,
                    args: Self::cap_all(args, depth, hit),
                },
                One::Ctor { ty, name, fields } => One::Ctor {
                    ty: *ty,
                    name: name.clone(),
                    fields: Self::cap_all(fields, depth, hit),
                },
                other => other.clone(),
            })
            .collect();
        // a value that was made unknown may now equal another member, and the order may have changed
        known.sort_unstable();
        known.dedup();
        Value {
            known: known.into(),
            open: self.open,
            wide: self.wide,
        }
    }

    /// The members, sorted.
    pub(crate) fn members(&self) -> &[One] {
        &self.known
    }

    /// Whether it is what the caps make of what is too much.
    pub(crate) fn is_top(&self) -> bool {
        self.wide
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// What can be at a place that holds this in one case and `other` in another: the members of both, and open when either is. No case is ruled out (no condition is
    /// evaluated), so the join of the cases of a `match` is the value of the `match`. `None` when the set would hold more than [`MAX_SET`] members: the caller says
    /// it is the top, and counts it (`value_widened_set`).
    pub(crate) fn join(&self, other: &Value) -> Option<Value> {
        if self.wide || other.wide {
            return Some(Value::top());
        }
        let mut known: Vec<One> = self
            .known
            .iter()
            .chain(other.known.iter())
            .cloned()
            .collect();
        known.sort_unstable();
        known.dedup();
        if known.len() > MAX_SET {
            return None;
        }
        Some(Value {
            known: known.into(),
            open: self.open || other.open,
            wide: false,
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
        assert_eq!(Value::one(def(3)).single(), Some(&def(3)));
        let lambda = One::Lambda {
            writer: DeclId::new(0, 1),
            nth: 2,
        };
        assert_eq!(Value::one(lambda.clone()).single(), Some(&lambda));
        // two members, or one member that something else may be beside: not a slot
        assert_eq!(
            Value::one(def(1))
                .join(&Value::one(def(2)))
                .unwrap()
                .single(),
            None
        );
        assert_eq!(
            Value::one(def(1)).join(&Value::unknown()).unwrap().single(),
            None
        );
    }

    #[test]
    fn a_join_is_sorted_has_no_duplicates_and_is_open_when_either_is() {
        let a = Value::one(def(2)).join(&Value::one(def(1))).unwrap();
        let b = Value::one(def(1)).join(&Value::one(def(2))).unwrap();
        // the order the cases came in does not matter
        assert_eq!(a, b);
        assert_eq!(a.members(), [def(1), def(2)]);
        assert_eq!(a.join(&Value::one(def(2))), Some(a.clone()));
        assert!(!a.is_open());
        assert!(a.join(&Value::unknown()).unwrap().is_open());
        // data adds nothing
        assert_eq!(a.join(&Value::data()), Some(a.clone()));
        assert_eq!(Value::data().join(&Value::data()), Some(Value::data()));
    }

    #[test]
    fn the_top_is_open_has_no_member_and_is_what_every_join_with_it_is() {
        let top = Value::top();
        assert!(top.is_open() && top.members().is_empty());
        assert_ne!(top, Value::unknown());
        for other in [Value::data(), Value::unknown(), Value::one(def(1))] {
            assert_eq!(top.join(&other), Some(top.clone()));
            assert_eq!(other.join(&top), Some(top.clone()));
        }
        // unknown is not: it adds its members to a join
        assert_eq!(
            Value::unknown()
                .join(&Value::one(def(1)))
                .unwrap()
                .members(),
            [def(1)]
        );
    }

    #[test]
    fn a_set_of_more_than_eight_is_not_a_set() {
        let mut set = Value::data();
        for n in 0..MAX_SET {
            set = set.join(&Value::one(def(n))).expect("up to the cap");
        }
        assert_eq!(set.members().len(), MAX_SET);
        // one more member of the same set, or none, is no change; a ninth def is over
        assert_eq!(set.join(&Value::one(def(3))), Some(set.clone()));
        assert_eq!(set.join(&Value::one(def(MAX_SET))), None);
    }

    #[test]
    fn a_value_nested_deeper_than_three_is_unknown_there_and_the_outer_partial_is_kept() {
        // a partial application of a def that holds a partial application that holds ... the numbers are the depth of what each holds
        let nest = |inner: Value| {
            Value::one(One::Partial {
                def: DeclId::new(0, 1),
                args: vec![inner].into(),
            })
        };
        let three = nest(nest(nest(Value::one(def(7)))));
        // a partial at depth 0 holds a partial at 1, which holds a partial at 2, which holds `def(7)` at 3: all of it fits
        assert_eq!(three.capped(), (three.clone(), false));
        let four = nest(three.clone());
        let (capped, hit) = four.capped();
        assert!(hit);
        // the def at depth 4 is gone; each partial around it is kept
        assert_eq!(capped, nest(nest(nest(nest(Value::top())))));
    }
}
