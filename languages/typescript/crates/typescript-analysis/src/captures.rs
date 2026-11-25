//! The lists of captured variables a request holds (`docs/03-typescript/12-context-layer/07-tc3-variables-and-captures.md` §4, "Step 3, part 2"). A
//! `One::Closure(def, captures)` is a number into `CaptureSets`, as a key is a number into `Keys`, so a value that holds other values stays `Copy`.
//!
//! Neutral: it imports nothing from `sem`. It keeps the depth limit: a list nested deeper than `MAX_CAPTURE_DEPTH` is cut where it is interned, which is also
//! what ends a def that captures a variable that holds itself.

use indexmap::IndexSet;

use crate::flow::VarId;
use crate::value::{Captures, One, Value};

/// How deep closures may nest in captured values: a closure's list holds a closure whose list holds a closure is one too many (Python's `fp_depth`).
pub(crate) const MAX_CAPTURE_DEPTH: u8 = 2;

/// The key of the counter in `Stats.language` for a closure written as its bare function because it nested deeper than `MAX_CAPTURE_DEPTH`.
pub(crate) const VALUE_WIDENED_DEPTH: &str = "value_widened_depth";

/// What a closure captured: each variable and its value, sorted by `VarId`.
pub(crate) type CaptureList = Box<[(VarId, Value)]>;

/// Every list of one request, each once: a list's number is its place in `table`, in the order lists are first made (the solver's order, so the same on every
/// run, rule 9). `depth` is at the same place.
#[derive(Clone, Default)]
pub(crate) struct CaptureSets {
    table: IndexSet<CaptureList>,
    depth: Vec<u8>,
}

impl CaptureSets {
    pub(crate) fn new() -> CaptureSets {
        CaptureSets::default()
    }

    /// `list` in its normal form, interned: every inner `Closure` whose list would make this one deeper than `MAX_CAPTURE_DEPTH` written as its `Fn`, every
    /// `Anon` written `AnonAny` (callers that pass different arrows share a list), unknown entries dropped, sorted by `VarId`. `None` when nothing is left.
    /// Also the number of inner closures cut: the caller counts them (this table counts nothing, so a quiet world stays quiet).
    pub(crate) fn intern(&mut self, list: Vec<(VarId, Value)>) -> (Option<Captures>, u32) {
        let unknown = Value::unknown();
        let mut cut = 0;
        let mut kept: Vec<(VarId, Value)> = Vec::new();
        for (var, value) in list {
            if value == unknown {
                continue;
            }
            let value = value.map(|one| match one {
                One::Anon(..) => One::AnonAny,
                One::Closure(def, inner) if 1 + self.depth_of(inner) > MAX_CAPTURE_DEPTH => {
                    cut += 1;
                    One::Fn(def)
                }
                other => other,
            });
            kept.push((var, value));
        }
        if kept.is_empty() {
            return (None, cut);
        }
        kept.sort_by_key(|(var, _)| *var);
        let depth = 1 + kept
            .iter()
            .flat_map(|(_, value)| value.known())
            .filter_map(|one| match one {
                One::Closure(_, inner) => Some(self.depth_of(*inner)),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let (index, new) = self.table.insert_full(kept.into());
        if new {
            self.depth.push(depth);
        }
        let number = u32::try_from(index).expect("a request has fewer than u32::MAX capture lists");
        (Some(Captures::new(number)), cut)
    }

    /// The list `captures` stands for, a number of this table.
    pub(crate) fn get(&self, captures: Captures) -> &[(VarId, Value)] {
        &self.table[captures.index() as usize]
    }

    /// The depth of the list `captures` stands for: 1 plus the deepest list of a closure among its values.
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
        let mut sets = CaptureSets::new();
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
        let mut sets = CaptureSets::new();
        assert_eq!(sets.intern(vec![]), (None, 0));
        assert_eq!(sets.intern(vec![(v(1), Value::unknown())]), (None, 0));
    }

    #[test]
    fn a_list_made_twice_is_one_number() {
        let mut sets = CaptureSets::new();
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
        let mut sets = CaptureSets::new();
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
        let mut sets = CaptureSets::new();
        let (inner, _) = sets.intern(vec![(v(1), f(1))]);
        let (outer, cut) = sets.intern(vec![(v(2), closure(5, inner.unwrap()))]);
        assert_eq!(cut, 0);
        assert_eq!(
            sets.get(outer.unwrap())[0].1,
            closure(5, inner.unwrap()),
            "kept as a closure"
        );
    }
}
