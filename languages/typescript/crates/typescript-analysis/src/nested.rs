//! The request's table of the values that hold values (`docs/03-typescript/12-context-layer/07-tc3-variables-and-captures.md` §4, `08-tc4-objects-and-props.md`
//! §3 and §10): a closure's list of captured variables and an object, each interned once in its normal form. A `One::Closure(def, captures)` is a number into the
//! lists and a `One::Object(id)` a number into the objects, as a key is a number into `Keys`, so a value that holds other values stays `Copy`.
//!
//! Neutral: it imports nothing from `sem`. It keeps **one depth** for both (`08-tc4` §10 row 3: Python's `fp_depth`): a list or an object nested deeper than
//! `MAX_DEPTH` is cut where it is interned, which is also what ends a def that captures a variable that holds itself, and the keys of a recursion that
//! builds an object from its own key (`walk(node) { walk({ parent: node }) }`).

use indexmap::IndexSet;

use crate::flow::VarId;
use crate::value::{Captures, ObjectId, One, Value};

/// How deep values may nest in values: a closure's list that holds a closure whose list holds a closure is one too many, and so is an object in an object in
/// an object (Python's `fp_depth`). `{ actions: { save } }` is depth 2 and kept.
pub(crate) const MAX_DEPTH: u8 = 2;

/// The most members an object lists: a larger literal is no object, its value is unknown (`KeyPolicy::max_members`).
pub(crate) const MAX_MEMBERS: usize = 64;

/// The key of the counter in `Stats.language` for a value written less exactly because it nested deeper than `MAX_DEPTH`: a closure as its bare function, an
/// object dropped from the value that held it.
pub(crate) const VALUE_WIDENED_DEPTH: &str = "value_widened_depth";

/// The key of the counter in `Stats.language` for an object that would list more than `MAX_MEMBERS` members, which makes it unknown.
pub(crate) const VALUE_WIDENED_MEMBERS: &str = "value_widened_members";

/// What a closure captured: each variable and its value, sorted by `VarId`.
pub(crate) type CaptureList = Box<[(VarId, Value)]>;

/// An object as the table keeps it: its members sorted by name, none twice, and whether a member not listed may be there.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ObjectData {
    pub(crate) members: Box<[(Box<str>, Value)]>,
    pub(crate) complete: bool,
}

/// Every list and every object of one request, each once: a number is its place in its table, in the order they are first made (the solver's order, so the
/// same on every run, rule 9). `depth` and `object_depth` are at the same places.
#[derive(Clone, Default)]
pub(crate) struct Nested {
    table: IndexSet<CaptureList>,
    depth: Vec<u8>,
    objects: IndexSet<ObjectData>,
    object_depth: Vec<u8>,
}

impl Nested {
    pub(crate) fn new() -> Nested {
        Nested::default()
    }

    /// `list` in its normal form, interned: every inner `Closure` whose list would make this one deeper than `MAX_DEPTH` written as its `Fn`, every inner
    /// `Object` that would is removed from its value (which is then open), every `Anon` written `AnonAny` (callers that pass different arrows share a list),
    /// unknown entries dropped (an entry the cut leaves unknown too), sorted by `VarId`. `None` when nothing is left. Also the number of cuts: the caller counts them (this table counts nothing, so
    /// a quiet world stays quiet).
    pub(crate) fn intern(&mut self, list: Vec<(VarId, Value)>) -> (Option<Captures>, u32) {
        let unknown = Value::unknown();
        let mut cut = 0;
        let mut kept: Vec<(VarId, Value)> = Vec::new();
        for (var, value) in list {
            if value == unknown {
                continue;
            }
            // an object cut away can leave nothing known: that entry is unknown, and a list lists no unknown entry
            let value = self.normal_value(value, &mut cut);
            if value != unknown {
                kept.push((var, value));
            }
        }
        if kept.is_empty() {
            return (None, cut);
        }
        kept.sort_by_key(|(var, _)| *var);
        let depth = 1 + self.deepest(kept.iter().map(|(_, value)| value));
        let (index, new) = self.table.insert_full(kept.into());
        if new {
            self.depth.push(depth);
        }
        let number = u32::try_from(index).expect("a request has fewer than u32::MAX capture lists");
        (Some(Captures::new(number)), cut)
    }

    /// An object with these members, interned: the members sorted by name (the caller gives no name twice), each value in its normal form as a list's is (an
    /// `Anon` is `AnonAny`, a closure or an object that would make this one deeper than `MAX_DEPTH` is cut), unknown members **kept** (an unknown member is not a
    /// missing one: a missing one is `undefined` where the object is complete). `None`, and no cut, when there are more than `MAX_MEMBERS`: the caller makes
    /// the value unknown and counts `VALUE_WIDENED_MEMBERS`. Also the number of cuts.
    pub(crate) fn intern_object(
        &mut self,
        members: Vec<(Box<str>, Value)>,
        complete: bool,
    ) -> (Option<ObjectId>, u32) {
        if members.len() > MAX_MEMBERS {
            return (None, 0);
        }
        let mut cut = 0;
        let mut kept: Vec<(Box<str>, Value)> = members
            .into_iter()
            .map(|(name, value)| (name, self.normal_value(value, &mut cut)))
            .collect();
        kept.sort_by(|(a, _), (b, _)| a.cmp(b));
        let depth = 1 + self.deepest(kept.iter().map(|(_, value)| value));
        let (index, new) = self.objects.insert_full(ObjectData {
            members: kept.into(),
            complete,
        });
        if new {
            self.object_depth.push(depth);
        }
        let number = u32::try_from(index).expect("a request has fewer than u32::MAX objects");
        (Some(ObjectId::new(number)), cut)
    }

    /// The list `captures` stands for, a number of this table.
    pub(crate) fn get(&self, captures: Captures) -> &[(VarId, Value)] {
        &self.table[captures.index() as usize]
    }

    /// The object `id` stands for, a number of this table.
    pub(crate) fn object(&self, id: ObjectId) -> &ObjectData {
        &self.objects[id.index() as usize]
    }

    /// `value` as a member of a list or an object: an inner `Object` that would make the holder deeper than `MAX_DEPTH` is removed (the value is open), an
    /// inner `Closure` that would is its `Fn`, every `Anon` is `AnonAny`. Each cut adds one to `cut`.
    fn normal_value(&self, value: Value, cut: &mut u32) -> Value {
        let too_deep = |one: One| {
            matches!(one, One::Closure(..) | One::Object(_))
                && 1 + self.depth_of_one(one) > MAX_DEPTH
        };
        let mut value = value;
        let objects = value
            .known()
            .iter()
            .filter(|one| matches!(one, One::Object(_)) && too_deep(**one))
            .count();
        if objects > 0 {
            *cut += u32::try_from(objects).unwrap_or(u32::MAX);
            value = value.opened_without(|one| matches!(one, One::Object(_)) && too_deep(one));
        }
        value.map(|one| match one {
            One::Anon(..) => One::AnonAny,
            One::Closure(def, _) if too_deep(one) => {
                *cut += 1;
                One::Fn(def)
            }
            other => other,
        })
    }

    /// The deepest list or object among the members of these values; 0 when there is none.
    fn deepest<'v>(&self, values: impl Iterator<Item = &'v Value>) -> u8 {
        values
            .flat_map(|value| value.known())
            .map(|one| self.depth_of_one(*one))
            .max()
            .unwrap_or(0)
    }

    /// The depth of what `one` holds: of a closure's list or of an object; 0 for anything else.
    fn depth_of_one(&self, one: One) -> u8 {
        match one {
            One::Closure(_, captures) => self.depth_of(captures),
            One::Object(id) => self.object_depth[id.index() as usize],
            One::Fn(_) | One::Anon(..) | One::AnonAny | One::Undefined => 0,
        }
    }

    /// The depth of the list `captures` stands for: 1 plus the deepest list or object among its values.
    fn depth_of(&self, captures: Captures) -> u8 {
        self.depth[captures.index() as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::DefId;

    fn d(index: u32) -> DefId {
        DefId::new(0, 0, Some(index))
    }

    fn v(at: u32) -> VarId {
        VarId { def: d(0), at }
    }

    fn f(index: u32) -> Value {
        Value::of([One::Fn(d(index))], false)
    }

    fn closure(def: u32, captures: Captures) -> Value {
        Value::of([One::Closure(d(def), captures)], false)
    }

    #[test]
    fn a_list_is_sorted_without_unknowns_and_with_arrows_as_one() {
        let mut sets = Nested::new();
        let (made, cut) = sets.intern(vec![
            (v(9), f(1)),
            (v(3), Value::unknown()),
            (v(5), Value::of([One::Anon(d(4), 2), One::Fn(d(1))], false)),
            (v(2), Value::data()),
        ]);
        assert_eq!(cut, 0);
        let list = sets.get(made.unwrap());
        let at: Vec<u32> = list.iter().map(|(var, _)| var.at).collect();
        assert_eq!(at, [2, 5, 9]);
        assert_eq!(list[0].1, Value::data());
        assert_eq!(list[1].1, Value::of([One::Fn(d(1)), One::AnonAny], false));
    }

    #[test]
    fn nothing_left_is_no_list() {
        let mut sets = Nested::new();
        assert_eq!(sets.intern(vec![]), (None, 0));
        assert_eq!(sets.intern(vec![(v(1), Value::unknown())]), (None, 0));
    }

    #[test]
    fn a_list_made_twice_is_one_number() {
        let mut sets = Nested::new();
        let (one, _) = sets.intern(vec![(v(2), f(1)), (v(1), f(2))]);
        let (other, _) = sets.intern(vec![(v(1), f(2)), (v(2), f(1))]);
        assert_eq!(one, other);
        let (third, _) = sets.intern(vec![(v(1), f(3))]);
        assert_ne!(one, third);
        assert_eq!(sets.table.len(), 2);
        assert_eq!(sets.depth.len(), 2);
    }

    #[test]
    fn a_closure_nested_two_deep_is_cut_to_the_depth_limit() {
        let mut sets = Nested::new();
        // depth 1: a list with no closure
        let (one, cut) = sets.intern(vec![(v(1), f(1))]);
        assert_eq!(cut, 0);
        // depth 2: a list holding a closure of the depth-1 list: not cut (`1 + 1` is the limit)
        let (two, cut) = sets.intern(vec![(v(1), closure(2, one.unwrap()))]);
        assert_eq!(cut, 0);
        assert_eq!(sets.depth_of(two.unwrap()), 2);
        // a list holding a closure of the depth-2 list would be depth 3: that inner closure is written as its `Fn`, and counted
        let (three, cut) = sets.intern(vec![(v(1), closure(3, two.unwrap()))]);
        assert_eq!(cut, 1);
        let list = sets.get(three.unwrap());
        assert_eq!(list[0].1, f(3));
        assert_eq!(sets.depth_of(three.unwrap()), 1);
    }

    #[test]
    fn a_closure_with_a_shallow_enough_list_is_not_cut() {
        let mut sets = Nested::new();
        let (inner, _) = sets.intern(vec![(v(1), f(1))]);
        let (outer, cut) = sets.intern(vec![(v(2), closure(5, inner.unwrap()))]);
        assert_eq!(cut, 0);
        assert_eq!(
            sets.get(outer.unwrap())[0].1,
            closure(5, inner.unwrap()),
            "kept as a closure"
        );
    }

    fn object_of(sets: &mut Nested, members: Vec<(&str, Value)>) -> (ObjectId, u32) {
        let members = members.into_iter().map(|(n, v)| (n.into(), v)).collect();
        let (made, cut) = sets.intern_object(members, true);
        (made.expect("an object"), cut)
    }

    fn object(id: ObjectId) -> Value {
        Value::of([One::Object(id)], false)
    }

    #[test]
    fn an_object_is_sorted_by_name_keeps_unknown_members_and_writes_arrows_as_one() {
        let mut sets = Nested::new();
        let (id, cut) = object_of(
            &mut sets,
            vec![
                ("b", f(2)),
                ("a", Value::unknown()),
                ("c", Value::of([One::Anon(d(4), 2), One::Fn(d(1))], false)),
            ],
        );
        assert_eq!(cut, 0);
        let data = sets.object(id);
        assert!(data.complete);
        let names: Vec<&str> = data.members.iter().map(|(n, _)| n.as_ref()).collect();
        assert_eq!(names, ["a", "b", "c"]);
        // an unknown member is kept: it is not a missing one
        assert_eq!(data.members[0].1, Value::unknown());
        assert_eq!(
            data.members[2].1,
            Value::of([One::Fn(d(1)), One::AnonAny], false)
        );
    }

    #[test]
    fn an_object_made_twice_is_one_number_and_complete_tells_two_apart() {
        let mut sets = Nested::new();
        let (one, _) = object_of(&mut sets, vec![("a", f(1)), ("b", f(2))]);
        let (other, _) = object_of(&mut sets, vec![("b", f(2)), ("a", f(1))]);
        assert_eq!(one, other);
        let (open, _) = sets.intern_object(vec![("a".into(), f(1)), ("b".into(), f(2))], false);
        assert_ne!(Some(one), open);
        assert_eq!(sets.objects.len(), 2);
        assert_eq!(sets.object_depth.len(), 2);
    }

    #[test]
    fn more_than_the_member_cap_is_no_object() {
        let mut sets = Nested::new();
        let many = |n: usize| -> Vec<(Box<str>, Value)> {
            (0..n).map(|i| (format!("k{i}").into(), f(1))).collect()
        };
        assert!(sets.intern_object(many(MAX_MEMBERS), true).0.is_some());
        assert_eq!(sets.intern_object(many(MAX_MEMBERS + 1), true), (None, 0));
        assert_eq!(sets.objects.len(), 1);
    }

    #[test]
    fn an_object_in_an_object_is_depth_two_and_one_more_is_cut_from_its_value() {
        let mut sets = Nested::new();
        let (inner, cut) = object_of(&mut sets, vec![("y", f(1))]);
        assert_eq!(cut, 0);
        let (middle, cut) = object_of(&mut sets, vec![("x", object(inner))]);
        assert_eq!(cut, 0);
        assert_eq!(sets.object(middle).members[0].1, object(inner));
        // a third level would be depth 3: the middle object is removed from `w`'s value, which is then open (nothing else is left of it)
        let (outer, cut) = object_of(&mut sets, vec![("w", object(middle)), ("v", f(3))]);
        assert_eq!(cut, 1);
        let members = &sets.object(outer).members;
        assert_eq!(members[0], ("v".into(), f(3)));
        assert_eq!(members[1], ("w".into(), Value::unknown()));
        // what else the value held stays, open
        let both = Value::of([One::Object(middle), One::Fn(d(5))], false);
        let (outer, cut) = object_of(&mut sets, vec![("w", both)]);
        assert_eq!(cut, 1);
        assert_eq!(
            sets.object(outer).members[0].1,
            Value::of([One::Fn(d(5))], true)
        );
    }

    #[test]
    fn one_depth_counts_lists_and_objects_in_both_directions() {
        let mut sets = Nested::new();
        let (inner, _) = object_of(&mut sets, vec![("y", f(1))]);
        let (middle, _) = object_of(&mut sets, vec![("x", object(inner))]);
        // an object of depth 2 in a list would make the list depth 3: removed, and a list left with nothing known is no list
        assert_eq!(sets.intern(vec![(v(1), object(middle))]), (None, 1));
        // the other entries stay
        let (made, cut) = sets.intern(vec![(v(1), object(middle)), (v(2), f(2))]);
        assert_eq!(cut, 1);
        assert_eq!(sets.get(made.unwrap()), [(v(2), f(2))]);
        // an object of depth 1 in a list is kept, and the list is depth 2
        let (made, cut) = sets.intern(vec![(v(1), object(inner))]);
        assert_eq!(cut, 0);
        assert_eq!(sets.depth_of(made.unwrap()), 2);
        // a closure whose list is depth 2 in an object would make the object depth 3: it is written as its function
        let (one, _) = sets.intern(vec![(v(1), f(1))]);
        let (two, _) = sets.intern(vec![(v(1), closure(2, one.unwrap()))]);
        let (id, cut) = object_of(&mut sets, vec![("k", closure(3, two.unwrap()))]);
        assert_eq!(cut, 1);
        assert_eq!(sets.object(id).members[0].1, f(3));
        // and with a list of depth 1 it is kept
        let (id, cut) = object_of(&mut sets, vec![("k", closure(3, one.unwrap()))]);
        assert_eq!(cut, 0);
        assert_eq!(sets.object(id).members[0].1, closure(3, one.unwrap()));
    }
}
