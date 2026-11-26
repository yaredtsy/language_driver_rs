//! What can be at a place, as far as the layer knows (`docs/03-typescript/12-context-layer/03-design.md` §3). Plain data: it imports nothing from `sem`
//! (§15.4), so the same laws hold for any language. A `Value` exists only in its normal form: the functions below are the only way to make one.

use smallvec::SmallVec;

use crate::flow::DefId;

/// The most members a value lists before it is wide (`KeyPolicy::max_set`, §12).
pub(crate) const MAX_SET: usize = 8;

/// The key of the counter in `Stats.language` for a value set that passes `MAX_SET`: counted at the join that makes it wide, when neither side was.
pub(crate) const VALUE_WIDENED_SET: &str = "value_widened_set";

/// A list of captured variables as a number: its place in the request's table of lists (`nested::Nested`), so that a value that holds other values
/// stays `Copy` (`KeyId` does the same for keys). Never 0-for-empty here: a def that captures nothing is `One::Fn`, not a closure with no list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Captures(u32);

impl Captures {
    pub(crate) fn new(index: u32) -> Captures {
        Captures(index)
    }

    pub(crate) fn index(self) -> u32 {
        self.0
    }
}

/// An object as a number: its place in the request's table of objects (`nested::Nested`), as `Captures` is for a list, so that a value that holds an object
/// stays `Copy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct ObjectId(u32);

impl ObjectId {
    pub(crate) fn new(index: u32) -> ObjectId {
        ObjectId(index)
    }

    pub(crate) fn index(self) -> u32 {
        self.0
    }
}

/// One thing a value can hold: a function, a nested function with what it captured, an anonymous function, an anonymous function of which one is not said (how
/// a key holds one), "nothing was passed", or an object. TC4 adds classes, TC5 instances. The derived order is `Fn` by def, then `Closure` by def and list,
/// then `Anon` by def and number, then `AnonAny`, then `Undefined`, then `Object` by number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum One {
    /// A def with a body.
    Fn(DefId),
    /// A nested def with the values of the variables around it it captured (`07-tc3` §4); never with an empty list (that is `Fn`).
    Closure(DefId, Captures),
    /// An anonymous function: the def it is written in, and its number among that body's anonymous functions (`Flow::anons`).
    Anon(DefId, u32),
    /// An anonymous function, which one not said: how a key holds one (`KeyData::normal` writes every `Anon` of a slot as this), so callers that pass
    /// different arrows share a context. Never made by `eval` from an expression.
    AnonAny,
    /// Nothing was passed: a parameter with this value starts from its default.
    Undefined,
    /// An object: a number into the request's table (`nested::Nested`).
    Object(ObjectId),
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
    pub(crate) fn data() -> Value {
        Value(Repr::Set {
            known: SmallVec::new(),
            open: false,
        })
    }

    /// Too much to list.
    pub(crate) fn wide() -> Value {
        Value(Repr::Wide)
    }

    /// `known` sorted, duplicates dropped; wide when more than `MAX_SET` remain.
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
    pub(crate) fn is_open(&self) -> bool {
        match &self.0 {
            Repr::Set { open, .. } => *open,
            Repr::Wide => true,
        }
    }

    pub(crate) fn is_wide(&self) -> bool {
        matches!(self.0, Repr::Wide)
    }

    /// Whether `one` is a member.
    pub(crate) fn contains(&self, one: One) -> bool {
        // `known` is sorted
        self.known().binary_search(&one).is_ok()
    }

    /// The members less `one`, open as it was; a wide value stays wide.
    pub(crate) fn without(&self, one: One) -> Value {
        if self.is_wide() {
            return Value::wide();
        }
        of_capped(
            self.known().iter().copied().filter(|member| *member != one),
            self.is_open(),
            MAX_SET,
        )
    }

    /// The members `drop` does not take, and open: what is left of a value when some of what it held is cut away, so something unknown can be there where it
    /// was. A wide value stays wide.
    pub(crate) fn opened_without(&self, drop: impl Fn(One) -> bool) -> Value {
        if self.is_wide() {
            return Value::wide();
        }
        of_capped(
            self.known().iter().copied().filter(|one| !drop(*one)),
            true,
            MAX_SET,
        )
    }

    /// Each member through `f`, in the normal form again (sorted, none twice); open as it was; a wide value stays wide.
    pub(crate) fn map(&self, f: impl FnMut(One) -> One) -> Value {
        if self.is_wide() {
            return Value::wide();
        }
        of_capped(self.known().iter().copied().map(f), self.is_open(), MAX_SET)
    }

    /// The least value that holds both: wide if either is; else the union of the members, open if either is, wide past the cap. Pure: whoever joins counts
    /// `value_widened_set` when the result is wide and neither side was.
    pub(crate) fn join(&self, other: &Value) -> Value {
        join_capped(self, other, MAX_SET)
    }
}

/// `of` with another cap: the laws are tested with a small one.
fn of_capped(known: impl IntoIterator<Item = One>, open: bool, cap: usize) -> Value {
    let mut known: SmallVec<[One; 2]> = known.into_iter().collect();
    known.sort_unstable();
    known.dedup();
    if known.len() > cap {
        return Value::wide();
    }
    Value(Repr::Set { known, open })
}

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

    /// The five members the laws are tested over: three functions, one anonymous function and `Undefined`.
    fn members() -> [One; 5] {
        [
            f(0),
            f(1),
            f(2),
            One::Anon(DefId::new(0, 0, Some(0)), 0),
            One::Undefined,
        ]
    }

    /// Every value over `members()` with the cap at 3: each subset of them, open and closed, and `wide`, once each.
    fn universe() -> Vec<Value> {
        let mut values: Vec<Value> = Vec::new();
        for subset in 0u32..32 {
            let members: Vec<One> = members()
                .into_iter()
                .enumerate()
                .filter(|(i, _)| subset >> i & 1 == 1)
                .map(|(_, member)| member)
                .collect();
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
    fn without_removes_one_member() {
        for v in &universe() {
            for x in members() {
                let less = v.without(x);
                assert!(!less.contains(x), "{v:?} without {x:?}");
                assert_eq!(less.without(x), less, "twice: {v:?} {x:?}");
                assert_eq!(less.is_open(), v.is_open(), "open as it was: {v:?} {x:?}");
                if v.is_wide() {
                    assert!(less.is_wide(), "wide stays wide: {x:?}");
                } else {
                    let only = Value::of([x], false);
                    assert_eq!(join(&less, &only), join(v, &only), "put back: {v:?} {x:?}");
                }
            }
        }
    }

    #[test]
    fn opened_without_keeps_what_is_not_dropped_and_opens() {
        let is_f1 = |one| one == f(1);
        let value = Value::of([f(0), f(1), One::Undefined], false);
        let cut = value.opened_without(is_f1);
        assert_eq!(cut, Value::of([f(0), One::Undefined], true));
        assert!(cut.is_open());
        // open stays open, and a value that held only what is dropped is unknown
        assert_eq!(
            Value::of([f(1)], false).opened_without(is_f1),
            Value::unknown()
        );
        assert_eq!(
            Value::of([f(0), f(1)], true).opened_without(is_f1),
            Value::of([f(0)], true)
        );
        // a wide value stays wide
        assert!(Value::wide().opened_without(is_f1).is_wide());
        // data held nothing: it is open after the call all the same
        assert_eq!(Value::data().opened_without(is_f1), Value::unknown());
    }

    #[test]
    fn map_keeps_the_normal_form() {
        let anon = |index| One::Anon(DefId::new(0, 0, Some(0)), index);
        let to_any = |one| match one {
            One::Anon(..) => One::AnonAny,
            other => other,
        };
        let value = Value::of([anon(0), anon(1), f(1)], false);
        let mapped = value.map(to_any);
        assert_eq!(mapped, Value::of([f(1), One::AnonAny], false));
        assert_eq!(mapped.known(), [f(1), One::AnonAny]);
        assert!(!mapped.is_open());
        assert!(
            Value::of([anon(0), anon(1), f(1)], true)
                .map(to_any)
                .is_open()
        );
        assert!(Value::wide().map(to_any).is_wide());
        assert_eq!(Value::unknown().map(to_any), Value::unknown());
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
