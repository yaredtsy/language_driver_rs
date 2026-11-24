//! What can be at a place, as far as the layer knows (`docs/03-typescript/12-context-layer/03-design.md` §3). Plain data: it imports nothing from `sem`
//! (§15.4), so the same laws hold for any language. A `Value` exists only in its normal form: the functions below are the only way to make one.

use smallvec::SmallVec;

use crate::flow::DefId;

/// The most members a value lists before it is wide (`KeyPolicy::max_set`, §12).
#[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
pub(crate) const MAX_SET: usize = 8;

/// One thing a value can hold. TC2 adds the anonymous functions and `Undefined`, TC4 classes and objects, TC5 instances.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum One {
    /// A def with a body.
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    Fn(DefId),
}

/// What can be at a place. `unknown` (nothing known: ask the engine), `data` (nothing callable, and the layer knows it), `wide` (too much to list: no
/// member, open, and every join with it is it), or a set of members, open when something unknown can be there too.
///
/// 🦀 The field is private and its type is private too, so no code outside this module can build a `Value` that is not in its normal form (members sorted,
/// none twice, at most the cap): two values that mean the same are then `==`, which is what lets a key that holds them be compared and hashed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Value(Repr);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Repr {
    /// Sorted, no duplicates, at most the cap. A `SmallVec<[One; 2]>` keeps up to two members inside the value itself and goes to the heap only past that.
    Set {
        known: SmallVec<[One; 2]>,
        open: bool,
    },
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    Wide,
}

impl Value {
    /// Nothing known: ask the engine.
    pub(crate) fn unknown() -> Value {
        Value(Repr::Set {
            known: SmallVec::new(),
            open: true,
        })
    }

    /// Nothing callable, and the layer knows it.
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    pub(crate) fn data() -> Value {
        Value(Repr::Set {
            known: SmallVec::new(),
            open: false,
        })
    }

    /// Too much to list.
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    pub(crate) fn wide() -> Value {
        Value(Repr::Wide)
    }

    /// `known` sorted, duplicates dropped; wide when more than `MAX_SET` remain.
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    pub(crate) fn of(known: impl IntoIterator<Item = One>, open: bool) -> Value {
        of_capped(known, open, MAX_SET)
    }

    /// The members; none for a wide value.
    pub(crate) fn known(&self) -> &[One] {
        match &self.0 {
            Repr::Set { known, .. } => known,
            Repr::Wide => &[],
        }
    }

    /// Whether something unknown can be there too: true for a wide value.
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    pub(crate) fn is_open(&self) -> bool {
        match &self.0 {
            Repr::Set { open, .. } => *open,
            Repr::Wide => true,
        }
    }

    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    pub(crate) fn is_wide(&self) -> bool {
        matches!(self.0, Repr::Wide)
    }

    /// The least value that holds both: wide if either is; else the union of the members, open if either is, wide past the cap. Pure: whoever joins counts
    /// `value_widened_set` when the result is wide and neither side was.
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    pub(crate) fn join(&self, other: &Value) -> Value {
        join_capped(self, other, MAX_SET)
    }
}

/// `of` with another cap: the laws are tested with a small one.
#[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
fn of_capped(known: impl IntoIterator<Item = One>, open: bool, cap: usize) -> Value {
    let mut known: SmallVec<[One; 2]> = known.into_iter().collect();
    known.sort_unstable();
    known.dedup();
    if known.len() > cap {
        return Value::wide();
    }
    Value(Repr::Set { known, open })
}

#[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
fn join_capped(a: &Value, b: &Value, cap: usize) -> Value {
    if a.is_wide() || b.is_wide() {
        return Value::wide();
    }
    of_capped(
        a.known().iter().chain(b.known()).copied(),
        a.is_open() || b.is_open(),
        cap,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: usize = 3;

    fn f(index: u32) -> One {
        One::Fn(DefId::new(0, 0, Some(index)))
    }

    /// Every value over five functions with the cap at 3: each subset of them, open and closed, and `wide`, once each.
    fn universe() -> Vec<Value> {
        let mut values: Vec<Value> = Vec::new();
        for subset in 0u32..32 {
            let members: Vec<One> = (0..5).filter(|i| subset >> i & 1 == 1).map(f).collect();
            for open in [false, true] {
                let value = of_capped(members.iter().copied(), open, CAP);
                if !values.contains(&value) {
                    values.push(value);
                }
            }
        }
        if !values.contains(&Value::wide()) {
            values.push(Value::wide());
        }
        values
    }

    fn join(a: &Value, b: &Value) -> Value {
        join_capped(a, b, CAP)
    }

    #[test]
    fn a_value_is_in_its_normal_form() {
        assert_eq!(
            Value::of([f(2), f(0), f(2)], false),
            Value::of([f(0), f(2)], false)
        );
        assert_eq!(Value::of([f(2), f(0), f(2)], false).known(), [f(0), f(2)]);
        assert!(of_capped([f(0), f(1), f(2), f(3)], false, 3).is_wide());
        assert!(Value::wide().known().is_empty());
        assert!(Value::wide().is_open());
        assert!(!Value::data().is_open() && Value::data().known().is_empty());
        assert!(Value::unknown().is_open() && Value::unknown().known().is_empty());
        assert_ne!(Value::data(), Value::unknown());
    }

    #[test]
    fn join_is_commutative_associative_and_idempotent() {
        let all = universe();
        eprintln!("the universe has {} values", all.len());
        let (mut pairs, mut triples) = (0, 0);
        for a in &all {
            assert_eq!(&join(a, a), a, "idempotent: {a:?}");
            for b in &all {
                pairs += 1;
                assert_eq!(join(a, b), join(b, a), "commutative: {a:?} {b:?}");
                for c in &all {
                    triples += 1;
                    assert_eq!(
                        join(&join(a, b), c),
                        join(a, &join(b, c)),
                        "associative: {a:?} {b:?} {c:?}"
                    );
                }
            }
        }
        eprintln!("{pairs} pairs and {triples} triples");
    }

    #[test]
    fn wide_absorbs_and_data_changes_nothing() {
        for a in &universe() {
            assert_eq!(join(a, &Value::wide()), Value::wide(), "{a:?}");
            assert_eq!(join(&Value::wide(), a), Value::wide(), "{a:?}");
            assert_eq!(&join(a, &Value::data()), a, "{a:?}");
            assert_eq!(&join(&Value::data(), a), a, "{a:?}");
        }
    }

    #[test]
    fn unknown_opens_and_keeps_the_members() {
        for a in universe().iter().filter(|a| !a.is_wide()) {
            let joined = join(a, &Value::unknown());
            assert!(joined.is_open(), "{a:?}");
            assert_eq!(joined.known(), a.known(), "{a:?}");
        }
    }

    #[test]
    fn a_join_widens_only_past_the_cap() {
        let all = universe();
        let mut pairs = 0;
        for a in &all {
            for b in &all {
                pairs += 1;
                let mut union: Vec<One> = a.known().iter().chain(b.known()).copied().collect();
                union.sort_unstable();
                union.dedup();
                let expected = a.is_wide() || b.is_wide() || union.len() > CAP;
                assert_eq!(join(a, b).is_wide(), expected, "{a:?} {b:?}");
            }
        }
        eprintln!("{pairs} pairs");
    }

    #[test]
    fn the_real_cap_is_eight() {
        assert!(!Value::of((0..8).map(f), false).is_wide());
        assert!(Value::of((0..9).map(f), false).is_wide());
        // and `join` uses it
        let low = Value::of((0..5).map(f), false);
        assert!(!low.join(&Value::of((5..8).map(f), false)).is_wide());
        assert!(low.join(&Value::of((5..9).map(f), false)).is_wide());
    }
}
