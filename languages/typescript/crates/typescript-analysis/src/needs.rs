//! What a nested def needs from the functions around it (`docs/03-typescript/12-context-layer/07-tc3-variables-and-captures.md` §4, "Step 3, part 1").
//!
//! Neutral, as `eval` is: it imports nothing from `sem` and asks only `World::flow`, so a test answers it with tables made by hand. Nothing reads the result
//! yet; part 2 gives a nested def's value the variables it lists.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crate::eval::World;
use crate::flow::{DefId, Expr, Flow, VarId};

/// The variables of the functions around `def` that `def` needs to be given (`07-tc3` §4), sorted by `VarId`. Empty for a def at the top of its file, and for
/// a def whose table the world does not have.
#[cfg(test)]
pub(crate) fn needs(world: &dyn World, def: DefId) -> Box<[VarId]> {
    needs_of_component(world, def).swap_remove(0).1
}

/// `needs` of every def reachable from `def` through `named`, `def` first, each with its exact set: they are solved together, so the others come free
/// (`World::needs` keeps them all). One entry `(def, [])` when `def`'s `around` is `None` or the world has no table for it.
pub(crate) fn needs_of_component(world: &dyn World, def: DefId) -> Vec<(DefId, Box<[VarId]>)> {
    let Some(flow) = world.flow(def) else {
        return vec![(def, Box::new([]))];
    };
    if flow.around.is_none() {
        return vec![(def, Box::new([]))];
    }
    // the defs reachable from `def` through `named`, `def` first, in the order found: a `Vec` and a position found by a map that is only looked up, never
    // iterated (rule 9)
    let mut defs: Vec<DefId> = vec![def];
    let mut place_of: HashMap<DefId, usize> = HashMap::from([(def, 0)]);
    let mut named_of: Vec<Vec<usize>> = Vec::new();
    let mut sets: Vec<BTreeSet<VarId>> = Vec::new();
    let mut next = 0;
    while next < defs.len() {
        let at = defs[next];
        let table = if next == 0 {
            Some(Arc::clone(&flow))
        } else {
            world.flow(at)
        };
        let mut places = Vec::new();
        let mut set = BTreeSet::new();
        if let Some(table) = table {
            set.extend(reads(world, at, &table));
            for handle in named(world, &table) {
                let place = *place_of.entry(handle).or_insert_with(|| {
                    defs.push(handle);
                    defs.len() - 1
                });
                if !places.contains(&place) {
                    places.push(place);
                }
            }
        }
        named_of.push(places);
        sets.push(set);
        next += 1;
    }
    // what a def names carries into it, except its own variables, until a round adds nothing
    loop {
        let mut grew = false;
        for at in 0..defs.len() {
            for &place in &named_of[at] {
                let carried: Vec<VarId> = sets[place]
                    .iter()
                    .copied()
                    .filter(|var| var.def != defs[at])
                    .collect();
                for var in carried {
                    grew |= sets[at].insert(var);
                }
            }
        }
        if !grew {
            break;
        }
    }
    defs.into_iter()
        .zip(sets)
        .map(|(def, set)| (def, set.into_iter().collect()))
        .collect()
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
fn each_expr(table: &Flow, each: &mut dyn FnMut(&Expr)) {
    fn walk(expr: &Expr, each: &mut dyn FnMut(&Expr)) {
        each(expr);
        if let Expr::OneOf(members) = expr {
            for member in members {
                walk(member, each);
            }
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
    use std::collections::HashMap;

    use super::*;
    use crate::caps::Caps;
    use crate::captures::CaptureList;
    use crate::flow::{Arg, FlowKind, SiteFlow, Var};
    use crate::value::{Captures, Value};
    use typescript_syntax::Span;

    /// A world of hand-made tables.
    struct Tables(HashMap<DefId, Arc<Flow>>);

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

        fn capture(&self, _list: Vec<(VarId, Value)>) -> (Option<Captures>, u32) {
            unreachable!("these tests do not evaluate")
        }

        fn captured(&self, _captures: Captures) -> CaptureList {
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
        Tables(tables.into_iter().collect())
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
}
