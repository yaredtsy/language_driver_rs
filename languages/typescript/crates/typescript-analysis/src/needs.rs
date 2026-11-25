//! What a nested def needs from the functions around it (`docs/03-typescript/12-context-layer/07-tc3-variables-and-captures.md` §4, "Step 3, part 1").
//!
//! Neutral, as `eval` is: it imports nothing from `sem` and asks only `World::flow`, so a test answers it with tables made by hand. Nothing reads the result
//! yet; part 2 gives a nested def's value the variables it lists.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

use crate::eval::World;
use crate::flow::{DefId, Expr, Flow, VarId};

/// The variables of the functions around `def` that `def` needs to be given (`07-tc3` §4), sorted by `VarId`. Empty for a def at the top of its file, and for
/// a def whose table the world does not have.
#[cfg(test)]
pub(crate) fn needs(world: &dyn World, def: DefId) -> Box<[VarId]> {
    needs_of_component(world, def).swap_remove(0).1
}

/// Each def of a component with the variables it needs.
pub(crate) type Component = Vec<(DefId, Box<[VarId]>)>;

/// `needs` of every def reachable from `def` through `named`, `def` first, each with its exact set: they are solved together, so the others come free
/// (`Sem::remember_needs` keeps them all). One entry `(def, [])` when `def`'s `around` is `None` or the world has no table for it.
pub(crate) fn needs_of_component(world: &dyn World, def: DefId) -> Component {
    component(world, def).0
}

/// What a solve did, for the tests: how many variables it tried to put into a set (what the cost of the solve is made of), and how many defs it expanded
/// (read the table of, and looked for the names in).
#[derive(Default)]
struct Work {
    attempts: usize,
    expanded: usize,
}

/// `needs_of_component`, and what it cost. A def the world already knows (`World::needs_known`) is not expanded: it is a fixed node of the solve with its known
/// set (which covers everything it reaches, because a component is remembered whole), it passes that set on to the defs that name it, and nothing is put into it.
/// Only the new defs are returned. `def` itself is always solved: it is what the caller is asking for.
fn component(world: &dyn World, def: DefId) -> (Component, Work) {
    let mut work = Work::default();
    let Some(flow) = world.flow(def) else {
        return (vec![(def, Box::new([]))], work);
    };
    if flow.around.is_none() {
        return (vec![(def, Box::new([]))], work);
    }
    // the defs reachable from `def` through `named`, `def` first, in the order found: a `Vec` and a position found by a map that is only looked up, never
    // iterated (rule 9)
    let mut defs: Vec<DefId> = vec![def];
    let mut place_of: HashMap<DefId, usize> = HashMap::from([(def, 0)]);
    // for each def, the defs that name it: where a set that grew has to go
    let mut named_by: Vec<Vec<usize>> = vec![Vec::new()];
    let mut sets: Vec<BTreeSet<VarId>> = Vec::new();
    // whether a def is a fixed node: known already, so its set is exact and nothing goes into it
    let mut fixed: Vec<bool> = Vec::new();
    let mut next = 0;
    while next < defs.len() {
        let at = defs[next];
        let known = if next == 0 {
            None
        } else {
            world.needs_known(at)
        };
        if let Some(known) = known {
            fixed.push(true);
            sets.push(known.iter().copied().collect());
            next += 1;
            continue;
        }
        fixed.push(false);
        let table = if next == 0 {
            Some(Arc::clone(&flow))
        } else {
            world.flow(at)
        };
        let mut set = BTreeSet::new();
        if let Some(table) = table {
            work.expanded += 1;
            set.extend(reads(world, at, &table));
            for handle in named(world, &table) {
                let place = *place_of.entry(handle).or_insert_with(|| {
                    defs.push(handle);
                    named_by.push(Vec::new());
                    defs.len() - 1
                });
                // a def that names itself adds nothing to itself
                if place != next && !named_by[place].contains(&next) {
                    named_by[place].push(next);
                }
            }
        }
        sets.push(set);
        next += 1;
    }
    // what a def names carries into the def that names it, except the variables of that def, until nothing grows. A worklist: a set is carried onward only when
    // it grew, and only to the defs that name it. A fixed node is never a `to`: it names nothing here
    let mut queue: VecDeque<usize> = (0..defs.len()).collect();
    let mut queued = vec![true; defs.len()];
    while let Some(from) = queue.pop_front() {
        queued[from] = false;
        // taken out while it is read, so the set of the def that names it can be changed; `from` is never in `named_by[from]`
        let source = std::mem::take(&mut sets[from]);
        for &to in &named_by[from] {
            debug_assert!(!fixed[to], "a fixed node names nothing");
            let mut grew = false;
            for &var in &source {
                if var.def == defs[to] {
                    continue;
                }
                work.attempts += 1;
                grew |= sets[to].insert(var);
            }
            if grew && !queued[to] {
                queued[to] = true;
                queue.push_back(to);
            }
        }
        sets[from] = source;
    }
    let found = defs
        .into_iter()
        .zip(sets)
        .zip(fixed)
        .filter(|(_, fixed)| !fixed)
        .map(|((def, set), _)| (def, set.into_iter().collect()))
        .collect();
    (found, work)
}

/// The variables `table` (the table of `own`) reads that belong to a def around it: an `Expr::Var` of another def of the outline (not the file's code) whose
/// row is capable. A variable whose declaring table or row the world does not have is not a read.
fn reads(world: &dyn World, own: DefId, table: &Flow) -> BTreeSet<VarId> {
    let mut found = BTreeSet::new();
    each_expr(table, &mut |expr| {
        if let Expr::Var(id) = expr
            && id.def != own
            && id.def.index().is_some()
            && world
                .flow(id.def)
                .and_then(|flow| flow.var_index(id.at).map(|row| flow.vars[row].capable))
                == Some(true)
        {
            found.insert(*id);
        }
    });
    found
}

/// The defs `table` names that are nested in another (`Flow::around` is `Some`): a def at the top of a file captures nothing. In the order of the table.
fn named(world: &dyn World, table: &Flow) -> Vec<DefId> {
    let mut found: Vec<DefId> = Vec::new();
    each_expr(table, &mut |expr| {
        if let Expr::Def(handle) = expr
            && !found.contains(handle)
            && world
                .flow(*handle)
                .is_some_and(|flow| flow.around.is_some())
        {
            found.push(*handle);
        }
    });
    found
}

/// Every expression of the table, `OneOf` members included: each site's callee and arguments, each hand-over's value, each parameter's default, and each
/// row's assigned expressions.
pub(crate) fn each_expr(table: &Flow, each: &mut dyn FnMut(&Expr)) {
    fn walk(expr: &Expr, each: &mut dyn FnMut(&Expr)) {
        each(expr);
        match expr {
            Expr::OneOf(members) => {
                for member in members {
                    walk(member, each);
                }
            }
            // entered from TC4 step 2
            Expr::Member(..) | Expr::Object(_) | Expr::Omit(..) | Expr::Default(..) => {}
            Expr::Unknown
            | Expr::Data
            | Expr::Undefined
            | Expr::Def(_)
            | Expr::Param(_)
            | Expr::Anon(_)
            | Expr::Var(_) => {}
        }
    }
    for site in &table.sites {
        walk(&site.callee, each);
        for arg in site.args.iter().flatten() {
            walk(&arg.value, each);
        }
    }
    for held in &table.handed {
        walk(&held.value, each);
    }
    for param in &table.params {
        if let Some(default) = &param.default {
            walk(default, each);
        }
    }
    for var in &table.vars {
        for value in &var.assigned {
            walk(value, each);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;
    use crate::caps::Caps;
    use crate::captures::CaptureList;
    use crate::flow::{Arg, FlowKind, SiteFlow, Var};
    use crate::value::{Captures, Value};
    use typescript_syntax::Span;

    /// A world of hand-made tables, and what its requests have worked out (the memo of the `Sem`).
    struct Tables(
        HashMap<DefId, Arc<Flow>>,
        RefCell<HashMap<DefId, Arc<[VarId]>>>,
    );

    impl World for Tables {
        fn flow(&self, def: DefId) -> Option<Arc<Flow>> {
            self.0.get(&def).map(Arc::clone)
        }

        fn widened(&self) {}

        fn knows(&self, _caps: Caps) -> bool {
            true
        }

        fn note(&self, _key: &'static str) {}

        fn needs(&self, def: DefId) -> Arc<[VarId]> {
            Arc::from(needs(self, def))
        }

        fn needs_known(&self, def: DefId) -> Option<Arc<[VarId]>> {
            self.1.borrow().get(&def).map(Arc::clone)
        }

        fn relevant(&self, def: DefId) -> Arc<[bool]> {
            Arc::from(crate::relevance::relevant(self, def))
        }

        fn capture(&self, _list: Vec<(VarId, Value)>) -> (Option<Captures>, u32) {
            unreachable!("these tests do not evaluate")
        }

        fn captured(&self, _captures: Captures) -> CaptureList {
            unreachable!("these tests do not evaluate")
        }

        fn module_value(&self, _var: VarId) -> Value {
            unreachable!("these tests do not evaluate")
        }
    }

    fn d(index: u32) -> DefId {
        DefId::new(0, 0, Some(index))
    }

    /// The file's own code.
    fn file() -> DefId {
        DefId::new(0, 0, None)
    }

    fn var(def: DefId, at: u32) -> VarId {
        VarId { def, at }
    }

    /// A row of a variable at `at`.
    fn row(at: u32, capable: bool) -> Var {
        Var {
            at,
            param: None,
            capable,
            assigned: Box::new([]),
            open: false,
        }
    }

    /// A table nested in `around`, declaring these rows, whose body calls each of these.
    fn table(around: Option<DefId>, vars: Vec<Var>, calls: Vec<Expr>) -> Arc<Flow> {
        Arc::new(Flow {
            params: Box::new([]),
            sites: calls
                .into_iter()
                .map(|callee| SiteFlow {
                    span: Span { start: 0, end: 0 },
                    kind: FlowKind::Call,
                    callee,
                    args: Some(Box::new([] as [Arg; 0])),
                    anon: None,
                })
                .collect(),
            anons: Box::new([]),
            handed: Box::new([]),
            unhandled: 0,
            vars: vars.into(),
            unmodelled: 0,
            around,
        })
    }

    fn world(tables: Vec<(DefId, Arc<Flow>)>) -> Tables {
        Tables(tables.into_iter().collect(), RefCell::default())
    }

    #[test]
    fn siblings_that_name_each_other_each_need_both() {
        // D = d(0) declares x@1 and y@2; g = d(1) reads x and names h; h = d(2) reads y and names g
        let (dd, g, h) = (d(0), d(1), d(2));
        let world = world(vec![
            (dd, table(None, vec![row(1, true), row(2, true)], vec![])),
            (
                g,
                table(Some(dd), vec![], vec![Expr::Var(var(dd, 1)), Expr::Def(h)]),
            ),
            (
                h,
                table(Some(dd), vec![], vec![Expr::Var(var(dd, 2)), Expr::Def(g)]),
            ),
        ]);
        let both = [var(dd, 1), var(dd, 2)];
        assert_eq!(*needs(&world, g), both);
        assert_eq!(*needs(&world, h), both);
    }

    #[test]
    fn what_a_def_needs_from_two_levels_up_goes_through_the_def_between() {
        // D = d(0) declares w@1; k = d(1) in D declares v@2 and names g; g = d(2) in k reads v and w
        let (dd, k, g) = (d(0), d(1), d(2));
        let world = world(vec![
            (dd, table(None, vec![row(1, true)], vec![])),
            (k, table(Some(dd), vec![row(2, true)], vec![Expr::Def(g)])),
            (
                g,
                table(
                    Some(k),
                    vec![],
                    vec![Expr::Var(var(k, 2)), Expr::Var(var(dd, 1))],
                ),
            ),
        ]);
        assert_eq!(*needs(&world, g), [var(dd, 1), var(k, 2)]);
        // k's own `v` is read from its own env when it makes g's value: only `w` comes from around k
        assert_eq!(*needs(&world, k), [var(dd, 1)]);
    }

    #[test]
    fn a_read_that_is_not_capable_or_not_found_is_not_a_need() {
        let (dd, g, gone) = (d(0), d(1), d(5));
        let world = world(vec![
            (dd, table(None, vec![row(1, false), row(2, true)], vec![])),
            (
                // the file's code declares a capable variable at 3
                file(),
                table(None, vec![row(3, true)], vec![]),
            ),
            (
                g,
                table(
                    Some(dd),
                    vec![],
                    vec![
                        // not capable
                        Expr::Var(var(dd, 1)),
                        // the file's code
                        Expr::Var(var(file(), 3)),
                        // a declaring def with no table
                        Expr::Var(var(gone, 1)),
                        // a declaring def with a table and no row at that place
                        Expr::Var(var(dd, 9)),
                        // the one real need
                        Expr::Var(var(dd, 2)),
                    ],
                ),
            ),
        ]);
        assert_eq!(*needs(&world, g), [var(dd, 2)]);
    }

    #[test]
    fn a_def_at_the_top_of_its_file_needs_nothing_and_passes_nothing_on() {
        // top = d(1) is at the top of the file; its (hand-made) table reads D's x. g = d(2) in D names top.
        let (dd, top, g) = (d(0), d(1), d(2));
        let world = world(vec![
            (dd, table(None, vec![row(1, true)], vec![])),
            (top, table(None, vec![], vec![Expr::Var(var(dd, 1))])),
            (g, table(Some(dd), vec![], vec![Expr::Def(top)])),
        ]);
        assert!(needs(&world, top).is_empty());
        assert!(needs(&world, g).is_empty());
        // and a def whose table the world does not have
        assert!(needs(&world, d(7)).is_empty());
    }

    #[test]
    fn a_def_that_names_itself_ends_and_needs_what_it_reads() {
        let (dd, g) = (d(0), d(1));
        let world = world(vec![
            (dd, table(None, vec![row(1, true)], vec![])),
            (
                g,
                table(Some(dd), vec![], vec![Expr::Var(var(dd, 1)), Expr::Def(g)]),
            ),
        ]);
        assert_eq!(*needs(&world, g), [var(dd, 1)]);
    }

    #[test]
    fn a_chain_of_fifty_nested_defs_all_need_the_one_variable_and_the_solve_stays_small() {
        // `D` = d(0) declares `x`; `d(k)` for k in 1..=50 is nested in it; each names the next, and the last reads `x`
        let dd = d(0);
        let mut tables = vec![(dd, table(None, vec![row(1, true)], vec![]))];
        for k in 1..=50u32 {
            let calls = if k == 50 {
                vec![Expr::Var(var(dd, 1))]
            } else {
                vec![Expr::Def(d(k + 1))]
            };
            tables.push((d(k), table(Some(dd), vec![], calls)));
        }
        let world = world(tables);
        let (found, work) = component(&world, d(1));
        let attempts = work.attempts;
        assert_eq!(work.expanded, 50);
        assert_eq!(found.len(), 50);
        for (def, set) in &found {
            assert_eq!(**set, [var(dd, 1)], "{def:?}");
        }
        // the edges are 49 and the final sets hold one variable: each edge is tried a few times at most, not once for every round of a settle
        let (edges, size) = (49, 1);
        assert!(
            attempts < 4 * edges * size,
            "{attempts} attempts for {edges} edges of sets of {size}"
        );
        eprintln!("{attempts} attempts for {edges} edges");
        // and what a def in the middle of the chain needs is the same through the `needs` that tests use
        assert_eq!(*needs(&world, d(25)), [var(dd, 1)]);
    }

    /// Ask the way a request does: solve, and remember every def the component returns.
    fn ask(world: &Tables, def: DefId) -> (Component, Work) {
        let (found, work) = component(world, def);
        let mut memo = world.1.borrow_mut();
        for (def, set) in &found {
            memo.insert(*def, Arc::from(set.clone()));
        }
        (found, work)
    }

    #[test]
    fn a_def_that_is_already_solved_is_not_solved_again() {
        // as the chain above, with a second variable `y` read by d(30) and a def `e` = d(60) nested in d(5) that names d(40): fifty-odd defs, two components
        let dd = d(0);
        let build = || {
            let mut tables = vec![(dd, table(None, vec![row(1, true), row(2, true)], vec![]))];
            for k in 1..=50u32 {
                let mut calls = if k == 50 {
                    vec![Expr::Var(var(dd, 1))]
                } else {
                    vec![Expr::Def(d(k + 1))]
                };
                if k == 30 {
                    calls.push(Expr::Var(var(dd, 2)));
                }
                tables.push((d(k), table(Some(dd), vec![], calls)));
            }
            tables.push((
                d(60),
                table(Some(d(5)), vec![], vec![Expr::Def(d(40)), Expr::Def(d(3))]),
            ));
            world(tables)
        };
        // asked one after the other: d(25) first (it reaches d(26)..d(50)), then d(60), which reaches the first component and the defs of its own
        let world = build();
        let (first, first_work) = ask(&world, d(25));
        assert_eq!(first.len(), 26);
        assert_eq!(first_work.expanded, 26);
        let (second, second_work) = ask(&world, d(60));
        // d(60) names d(40) (known) and d(3), which names d(4)..d(24) and then d(25) (known): the new defs are d(60), d(3)..d(24), and nothing of the first
        // component is read again
        assert_eq!(second.len(), 23);
        assert_eq!(second_work.expanded, 23);
        assert!(
            second
                .iter()
                .all(|(def, _)| *def == d(60) || (3..25).any(|k| *def == d(k)))
        );
        // and every set equals what one solve of everything gives
        let all = build();
        let (whole, _) = component(&all, d(1));
        let one_solve: HashMap<DefId, Box<[VarId]>> = whole.into_iter().collect();
        let (everything, _) = component(&all, d(60));
        let one_solve_60 = &everything[0].1;
        for (def, set) in first.iter().chain(&second) {
            if *def == d(60) {
                assert_eq!(set, one_solve_60);
            } else {
                assert_eq!(set, &one_solve[def], "{def:?}");
            }
        }
    }
}
