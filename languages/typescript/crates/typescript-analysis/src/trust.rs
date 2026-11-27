//! When an object can be trusted (`docs/03-typescript/12-context-layer/08-tc4-objects-and-props.md` §5 and §10, step 3b).
//!
//! Neutral, as `needs` and `relevance` are: it imports nothing from `sem` and asks only `World`, so a test answers it with tables made by hand. An object is
//! trusted when no use of the variable that holds it, in the whole scope of that variable and in every function it is handed to, can change its members. The
//! table says what each variable is used for (`Uses`, step 3a); this module follows the variables a value is passed on to and answers for all of them at once.

use std::collections::{BTreeSet, HashMap};

use crate::eval::World;
use crate::flow::{DefId, Expr, Flow, Part, Uses, VarId};
use crate::needs::each_expr;

/// Each variable of a component with whether its object can be trusted.
pub(crate) type Component = Vec<(VarId, bool)>;

/// Whether the object `var` holds can be trusted, and the same for every variable reached from it, `var` first, then in the order reached.
///
/// The variables reached are the nodes of a graph: a node follows the variables named by it (`const o = x`) and the parameters it is passed to. "Not trusted"
/// spreads from a node back to every node that follows it, starting at the nodes with a use the rule forbids, so the answer is the least one: a cycle with no
/// bad use is trusted, and the answers do not depend on which variable was asked first (`08-tc4` §10, row 12). A variable the world already knows
/// (`World::trusted_known`, other than `var` itself) is a fixed node: its answer covers everything it reaches, so nothing is followed from it, and it is not in
/// the result. No limit is needed: every node is a row of a table of the program, taken once.
pub(crate) fn trust_of_component(world: &dyn World, var: VarId) -> Component {
    // the variables reached, in the order found: a `Vec` and a position found by a map that is only looked up, never iterated (rule 9)
    let mut nodes: Vec<VarId> = vec![var];
    let mut place_of: HashMap<VarId, usize> = HashMap::from([(var, 0)]);
    // for each node, the nodes that follow to it: where "not trusted" has to go
    let mut named_by: Vec<Vec<usize>> = vec![Vec::new()];
    // the answer of a fixed node, `None` for a node that is solved here
    let mut fixed: Vec<Option<bool>> = Vec::new();
    // a node that is itself a use the rule forbids
    let mut bad: Vec<bool> = Vec::new();
    let mut next = 0;
    while next < nodes.len() {
        let at = nodes[next];
        if next != 0
            && let Some(answer) = world.trusted_known(at)
        {
            fixed.push(Some(answer));
            bad.push(false);
            next += 1;
            continue;
        }
        fixed.push(None);
        let (is_bad, follows) = examine(world, at);
        bad.push(is_bad);
        for followed in follows {
            let place = *place_of.entry(followed).or_insert_with(|| {
                nodes.push(followed);
                named_by.push(Vec::new());
                nodes.len() - 1
            });
            // a node that follows itself adds nothing
            if place != next && !named_by[place].contains(&next) {
                named_by[place].push(next);
            }
        }
        next += 1;
    }
    // "not trusted" goes back to every node that follows a node that is not
    let mut marked = vec![false; nodes.len()];
    let mut worklist: Vec<usize> = Vec::new();
    for place in 0..nodes.len() {
        if bad[place] || fixed[place] == Some(false) {
            marked[place] = true;
            worklist.push(place);
        }
    }
    while let Some(place) = worklist.pop() {
        for &follower in &named_by[place] {
            if !marked[follower] {
                marked[follower] = true;
                worklist.push(follower);
            }
        }
    }
    nodes
        .into_iter()
        .zip(marked)
        .zip(fixed)
        .filter(|(_, fixed)| fixed.is_none())
        .map(|((node, marked), _)| (node, !marked))
        .collect()
}

/// What one variable is on its own: whether its own uses are forbidden (so it is bad), and the variables it follows. A variable with no table or no row is bad;
/// one that is not capable holds no member to write, so it is neither bad nor followed. `receiver` is not read (Q-TC9 is open).
fn examine(world: &dyn World, var: VarId) -> (bool, Vec<VarId>) {
    let Some(flow) = world.flow(var.def) else {
        return (true, Vec::new());
    };
    let Some(row) = flow.var_index(var.at) else {
        return (true, Vec::new());
    };
    let row = &flow.vars[row];
    if !row.capable {
        return (false, Vec::new());
    }
    judge(world, &row.uses)
}

/// What `uses` says on its own: whether it is a use the rule forbids, and the variables it follows. The rule of a row, for any value's uses.
fn judge(world: &dyn World, uses: &Uses) -> (bool, Vec<VarId>) {
    let mut bad = uses.escapes || uses.exported;
    let mut follows: Vec<VarId> = uses.names.to_vec();
    for &(def, index) in uses.passed.iter() {
        match parameter(world, def, index) {
            Some(variables) => follows.extend(variables),
            None => bad = true,
        }
    }
    (bad, follows)
}

/// Whether a value used as `uses` says can be trusted: the rule of a row, for a use that is not a row (a def's `this`). Asks the world for each variable the
/// value follows.
pub(crate) fn uses_trusted(world: &dyn World, uses: &Uses) -> bool {
    let (bad, follows) = judge(world, uses);
    !bad && follows.iter().all(|&var| world.trusted(var))
}

/// The key of the counter in `Stats.language`: a body's rows that are trusted and on which a method is called (Q-TC9).
pub(crate) const MEMBER_CALL_ON_TRUSTED: &str = "member_call_on_trusted";
/// A body that is not a class member and whose `this` is not trusted (Q-TC9).
pub(crate) const THIS_WRITTEN_OUTSIDE_CLASS: &str = "this_written_outside_class";
/// The exported rows of a file's own code that a body reads: never trusted, so their objects are read open (`08-tc4` §5, N5).
pub(crate) const MODULE_OBJECT_SHARED: &str = "module_object_shared";

/// The three counts of one body (`08-tc4` §10, step 3c).
pub(crate) struct BodyCounts {
    pub(crate) member_call_on_trusted: u64,
    pub(crate) this_written_outside_class: u64,
    pub(crate) module_object_shared: u64,
}

/// What `flow`, the table of `def`, says for the three counters.
pub(crate) fn body_counts(world: &dyn World, def: DefId, flow: &Flow) -> BodyCounts {
    let member_call_on_trusted = flow
        .vars
        .iter()
        .filter(|row| row.capable && row.uses.receiver)
        .filter(|row| world.trusted(VarId { def, at: row.at }))
        .count() as u64;
    let this_written_outside_class = u64::from(
        flow.this_uses
            .as_ref()
            .is_some_and(|uses| !uses_trusted(world, uses)),
    );
    // the variables of the file's own code that the table reads, anywhere in an expression, each once
    let mut read: BTreeSet<VarId> = BTreeSet::new();
    each_expr(flow, &mut |expr| {
        each_var(expr, &mut |var| {
            if var.def.index().is_none() {
                read.insert(var);
            }
        });
    });
    let module_object_shared = read
        .into_iter()
        .filter(|var| {
            world.flow(var.def).is_some_and(|table| {
                table.var_index(var.at).is_some_and(|row| {
                    let row = &table.vars[row];
                    row.capable && row.uses.exported
                })
            })
        })
        .count() as u64;
    BodyCounts {
        member_call_on_trusted,
        this_written_outside_class,
        module_object_shared,
    }
}

/// Every variable `expr` names, however deep: a member of a `OneOf`, the object of a `Member` or an `Omit`, a part of an `Object`, both sides of a `Default`.
fn each_var(expr: &Expr, found: &mut impl FnMut(VarId)) {
    match expr {
        Expr::Var(var) => found(*var),
        Expr::OneOf(members) => {
            for member in members {
                each_var(member, found);
            }
        }
        Expr::Member(object, _) | Expr::Omit(object, _) => each_var(object, found),
        Expr::Object(parts) => {
            for part in parts {
                match part {
                    Part::Named(_, value) | Part::Spread(value) => each_var(value, found),
                    Part::Unnamed => {}
                }
            }
        }
        Expr::Default(value, default) => {
            each_var(value, found);
            each_var(default, found);
        }
        Expr::Unknown
        | Expr::Data
        | Expr::Undefined
        | Expr::Def(_)
        | Expr::Param(_)
        | Expr::Anon(_) => {}
    }
}

/// The variables that hold parameter `index` of `def` in its body: the parameter's own row when it is named, the variables of its pattern when it is not.
/// `None` when the parameter cannot be followed, which makes what was passed to it bad: no table for `def`; an argument past the last parameter (`arguments`
/// can read it); a rest parameter (it holds a list); a pattern parameter that escapes; a named parameter with no row.
fn parameter(world: &dyn World, def: DefId, index: u32) -> Option<Vec<VarId>> {
    let flow = world.flow(def)?;
    let param = flow.params.get(index as usize)?;
    if param.rest {
        return None;
    }
    if param.named {
        let row = flow.param_var(index)?;
        Some(vec![VarId {
            def,
            at: flow.vars[row].at,
        }])
    } else if param.uses.escapes {
        None
    } else {
        Some(param.uses.names.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::caps::Caps;
    use crate::flow::{Expr, Flow, Param, Part, Uses, Var};
    use crate::nested::{CaptureList, ObjectData};
    use crate::value::{Captures, ObjectId, Value};

    /// A world of hand-made tables, and the variables it already knows the answer for.
    struct Tables {
        flows: HashMap<DefId, Arc<Flow>>,
        known: HashMap<VarId, bool>,
    }

    impl World for Tables {
        fn flow(&self, def: DefId) -> Option<Arc<Flow>> {
            self.flows.get(&def).map(Arc::clone)
        }

        fn widened(&self) {}

        fn knows(&self, _caps: Caps) -> bool {
            true
        }

        fn note(&self, _key: &'static str) {}

        fn needs(&self, _def: DefId) -> Arc<[VarId]> {
            unreachable!("trust does not ask what a def needs")
        }

        fn needs_known(&self, _def: DefId) -> Option<Arc<[VarId]>> {
            unreachable!("trust does not ask what a def needs")
        }

        fn relevant(&self, _def: DefId) -> Arc<[bool]> {
            unreachable!("trust does not ask what is relevant")
        }

        fn trusted(&self, var: VarId) -> bool {
            // what the `Sem` answers: the first of the variable's own component
            trust_of_component(self, var)[0].1
        }

        fn trusted_known(&self, var: VarId) -> Option<bool> {
            self.known.get(&var).copied()
        }

        fn capture(&self, _list: Vec<(VarId, Value)>) -> (Option<Captures>, u32) {
            unreachable!("these tests do not evaluate")
        }

        fn captured(&self, _captures: Captures) -> CaptureList {
            unreachable!("these tests do not evaluate")
        }

        fn object(
            &self,
            _members: Vec<(Box<str>, Value)>,
            _complete: bool,
        ) -> (Option<ObjectId>, u32) {
            unreachable!("these tests do not evaluate")
        }

        fn object_data(&self, _id: ObjectId) -> ObjectData {
            unreachable!("these tests do not evaluate")
        }

        fn module_value(&self, _var: VarId) -> Value {
            unreachable!("these tests do not evaluate")
        }
    }

    fn d(index: u32) -> DefId {
        DefId::new(0, 0, Some(index))
    }

    fn var(def: u32, at: u32) -> VarId {
        VarId { def: d(def), at }
    }

    /// A capable row of a variable at `at` with these uses; `param` makes it the def's own parameter.
    fn row(at: u32, param: Option<u32>, uses: Uses) -> Var {
        Var {
            at,
            param,
            capable: true,
            assigned: Box::new([]),
            open: false,
            uses,
        }
    }

    fn clean() -> Uses {
        Uses::default()
    }

    fn escapes() -> Uses {
        Uses {
            escapes: true,
            ..Uses::default()
        }
    }

    fn passed(to: &[(u32, u32)]) -> Uses {
        Uses {
            passed: to.iter().map(|&(def, index)| (d(def), index)).collect(),
            ..Uses::default()
        }
    }

    fn names(of: &[VarId]) -> Uses {
        Uses {
            names: of.into(),
            ..Uses::default()
        }
    }

    fn named_param() -> Param {
        Param {
            rest: false,
            named: true,
            default: None,
            assigned: false,
            capable: true,
            uses: Uses::default(),
        }
    }

    fn table(params: Vec<Param>, vars: Vec<Var>) -> Arc<Flow> {
        Arc::new(Flow {
            params: params.into(),
            sites: Box::new([]),
            anons: Box::new([]),
            handed: Box::new([]),
            unhandled: 0,
            vars: vars.into(),
            unmodelled: 0,
            around: None,
            this_uses: None,
        })
    }

    /// A table of a def that is not function-like, with these rows and what it does with its `this`.
    fn with_this(vars: Vec<Var>, this_uses: Option<Uses>) -> Arc<Flow> {
        Arc::new(Flow {
            this_uses,
            ..Arc::try_unwrap(table(vec![], vars)).expect("made here, held once")
        })
    }

    fn world(tables: Vec<(u32, Arc<Flow>)>) -> Tables {
        Tables {
            flows: tables
                .into_iter()
                .map(|(index, flow)| (d(index), flow))
                .collect(),
            known: HashMap::new(),
        }
    }

    /// The answer for `var` alone.
    fn trusted(world: &Tables, var: VarId) -> bool {
        let component = trust_of_component(world, var);
        assert_eq!(component[0].0, var, "the asked variable comes first");
        component[0].1
    }

    /// A def `index` with one variable at 1, with these uses.
    fn def_with(index: u32, uses: Uses) -> (u32, Arc<Flow>) {
        (index, table(vec![], vec![row(1, None, uses)]))
    }

    /// A def `index` with one named parameter whose row is at 1, with these uses.
    fn def_with_parameter(index: u32, uses: Uses) -> (u32, Arc<Flow>) {
        (
            index,
            table(vec![named_param()], vec![row(1, Some(0), uses)]),
        )
    }

    #[test]
    fn a_variable_with_no_use_the_rule_forbids_is_trusted() {
        let receiver = Uses {
            receiver: true,
            ..Uses::default()
        };
        let not_capable = Var {
            capable: false,
            // not read: a variable that cannot hold a member has nothing to write
            ..row(3, None, escapes())
        };
        let world = world(vec![(
            1,
            table(
                vec![],
                vec![row(1, None, clean()), row(2, None, receiver), not_capable],
            ),
        )]);
        for at in 1..=3 {
            assert_eq!(
                trust_of_component(&world, var(1, at)),
                [(var(1, at), true)],
                "{at}"
            );
        }
    }

    #[test]
    fn an_escape_or_an_export_is_not_trusted() {
        let exported = Uses {
            exported: true,
            ..Uses::default()
        };
        let world = world(vec![(
            1,
            table(
                vec![],
                vec![row(1, None, escapes()), row(2, None, exported)],
            ),
        )]);
        assert!(!trusted(&world, var(1, 1)));
        assert!(!trusted(&world, var(1, 2)));
    }

    #[test]
    fn a_variable_is_trusted_only_if_the_parameters_it_is_passed_to_are() {
        let passes = (1, table(vec![], vec![row(1, None, passed(&[(2, 0)]))]));
        let clean_world = world(vec![passes, def_with_parameter(2, clean())]);
        assert_eq!(
            trust_of_component(&clean_world, var(1, 1)),
            [(var(1, 1), true), (var(2, 1), true)]
        );
        let passes = (1, table(vec![], vec![row(1, None, passed(&[(2, 0)]))]));
        let bad_world = world(vec![passes, def_with_parameter(2, escapes())]);
        assert_eq!(
            trust_of_component(&bad_world, var(1, 1)),
            [(var(1, 1), false), (var(2, 1), false)]
        );
    }

    #[test]
    fn a_parameter_that_cannot_be_followed_is_not_trusted() {
        let pattern_that_escapes = Param {
            named: false,
            uses: escapes(),
            ..named_param()
        };
        let rest = Param {
            rest: true,
            ..named_param()
        };
        let world = world(vec![
            // passed to a def with no table, past the last parameter, to a rest parameter, to a pattern parameter that escapes, to a named parameter with no row
            (
                1,
                table(
                    vec![],
                    vec![
                        row(1, None, passed(&[(3, 0)])),
                        row(2, None, passed(&[(2, 2)])),
                        row(3, None, passed(&[(4, 0)])),
                        row(4, None, passed(&[(5, 0)])),
                        row(5, None, passed(&[(6, 0)])),
                    ],
                ),
            ),
            (2, table(vec![named_param(), named_param()], vec![])),
            (4, table(vec![rest], vec![row(1, Some(0), clean())])),
            (5, table(vec![pattern_that_escapes], vec![])),
            (6, table(vec![named_param()], vec![])),
        ]);
        for at in 1..=5 {
            assert!(!trusted(&world, var(1, at)), "row {at}");
        }
        // a variable of a def with no table, and one with no row
        assert!(!trusted(&world, var(7, 1)));
        assert!(!trusted(&world, var(1, 99)));
    }

    #[test]
    fn a_pattern_parameter_is_its_names() {
        let pattern = |uses: Uses| Param {
            named: false,
            uses,
            ..named_param()
        };
        let with = |second: Uses| {
            world(vec![
                (1, table(vec![], vec![row(1, None, passed(&[(2, 0)]))])),
                (
                    2,
                    table(
                        vec![pattern(names(&[var(2, 10), var(2, 11)]))],
                        vec![row(10, None, clean()), row(11, None, second)],
                    ),
                ),
            ])
        };
        // both names are clean
        assert!(trusted(&with(clean()), var(1, 1)));
        // one of them escapes
        assert!(!trusted(&with(escapes()), var(1, 1)));
        // no names: nothing to follow
        let none = world(vec![
            (1, table(vec![], vec![row(1, None, passed(&[(2, 0)]))])),
            (2, table(vec![pattern(clean())], vec![])),
        ]);
        assert_eq!(trust_of_component(&none, var(1, 1)), [(var(1, 1), true)]);
    }

    #[test]
    fn a_second_name_carries_its_uses_back() {
        let world = world(vec![(
            1,
            table(
                vec![],
                vec![
                    // x names o, o escapes
                    row(1, None, names(&[var(1, 2)])),
                    row(2, None, escapes()),
                    // x → o → p, p escapes
                    row(11, None, names(&[var(1, 12)])),
                    row(12, None, names(&[var(1, 13)])),
                    row(13, None, escapes()),
                    // x escapes and names o, o is clean
                    row(
                        21,
                        None,
                        Uses {
                            escapes: true,
                            names: Box::new([var(1, 22)]),
                            ..Uses::default()
                        },
                    ),
                    row(22, None, clean()),
                ],
            ),
        )]);
        assert!(!trusted(&world, var(1, 1)));
        assert_eq!(
            trust_of_component(&world, var(1, 11)),
            [
                (var(1, 11), false),
                (var(1, 12), false),
                (var(1, 13), false)
            ]
        );
        // `o` follows nothing back: asked alone it is trusted, whatever `x` does
        assert!(trusted(&world, var(1, 22)));
        assert!(!trusted(&world, var(1, 21)));
    }

    #[test]
    fn a_cycle_is_solved_whole_whichever_is_asked_first() {
        // f's parameter is passed to g's, g's back to f's
        let cycle = |extra: Option<Uses>| {
            let g_rows = match extra {
                Some(extra) => vec![
                    row(
                        1,
                        Some(0),
                        Uses {
                            passed: Box::new([(d(1), 0)]),
                            names: Box::new([var(2, 2)]),
                            ..Uses::default()
                        },
                    ),
                    row(2, None, extra),
                ],
                None => vec![row(1, Some(0), passed(&[(1, 0)]))],
            };
            world(vec![
                (
                    1,
                    table(
                        vec![named_param()],
                        vec![row(1, Some(0), passed(&[(2, 0)]))],
                    ),
                ),
                (2, table(vec![named_param()], g_rows)),
            ])
        };
        let clean_cycle = cycle(None);
        assert!(trusted(&clean_cycle, var(1, 1)));
        assert!(trusted(&clean_cycle, var(2, 1)));
        // one more row hangs off one end and escapes
        let bad_cycle = cycle(Some(escapes()));
        assert!(!trusted(&bad_cycle, var(1, 1)));
        assert!(!trusted(&bad_cycle, var(2, 1)));
        // recursion: a variable passed to its own def's parameter
        let recursion = world(vec![(
            1,
            table(
                vec![named_param()],
                vec![row(1, Some(0), passed(&[(1, 0)]))],
            ),
        )]);
        assert_eq!(
            trust_of_component(&recursion, var(1, 1)),
            [(var(1, 1), true)]
        );
    }

    #[test]
    fn a_known_variable_is_not_followed() {
        // by a second name: the known variable's def has no table, which would make it bad if it were followed
        let by_name = |answer: Option<bool>| {
            let mut world = world(vec![def_with(1, names(&[var(3, 1)]))]);
            world.known.extend(answer.map(|answer| (var(3, 1), answer)));
            world
        };
        assert_eq!(
            trust_of_component(&by_name(Some(true)), var(1, 1)),
            [(var(1, 1), true)]
        );
        assert_eq!(
            trust_of_component(&by_name(Some(false)), var(1, 1)),
            [(var(1, 1), false)]
        );
        // not known: followed, and there is no table
        assert_eq!(
            trust_of_component(&by_name(None), var(1, 1)),
            [(var(1, 1), false), (var(3, 1), false)]
        );
        // by a parameter (its def has a table, since the parameter's row is read to name it): its own row would escape if it were followed
        let by_parameter = |answer: bool| {
            let mut world = world(vec![
                def_with(1, passed(&[(2, 0)])),
                def_with_parameter(2, escapes()),
            ]);
            world.known.insert(var(2, 1), answer);
            world
        };
        assert_eq!(
            trust_of_component(&by_parameter(true), var(1, 1)),
            [(var(1, 1), true)]
        );
        assert_eq!(
            trust_of_component(&by_parameter(false), var(1, 1)),
            [(var(1, 1), false)]
        );
        // the asked variable is solved even when the world knows it
        let mut asked = world(vec![def_with(1, escapes())]);
        asked.known.insert(var(1, 1), true);
        assert_eq!(trust_of_component(&asked, var(1, 1)), [(var(1, 1), false)]);
    }

    #[test]
    fn the_component_answers_every_variable_it_reached() {
        // x names o and is passed to g; o names p; p escapes; g's parameter is clean
        let world = world(vec![
            (
                1,
                table(
                    vec![],
                    vec![
                        row(
                            1,
                            None,
                            Uses {
                                names: Box::new([var(1, 2)]),
                                passed: Box::new([(d(2), 0)]),
                                ..Uses::default()
                            },
                        ),
                        row(2, None, names(&[var(1, 3)])),
                        row(3, None, escapes()),
                    ],
                ),
            ),
            def_with_parameter(2, clean()),
        ]);
        let component = trust_of_component(&world, var(1, 1));
        assert_eq!(
            component,
            [
                (var(1, 1), false),
                (var(1, 2), false),
                (var(2, 1), true),
                (var(1, 3), false),
            ]
        );
        for (reached, answer) in component {
            assert_eq!(trusted(&world, reached), answer, "{reached:?}");
        }
    }

    #[test]
    fn uses_trusted_is_the_rule_of_a_row() {
        let world = world(vec![
            (
                1,
                table(vec![], vec![row(1, None, escapes()), row(2, None, clean())]),
            ),
            def_with_parameter(2, clean()),
            def_with_parameter(3, escapes()),
            (
                4,
                table(
                    vec![Param {
                        rest: true,
                        ..named_param()
                    }],
                    vec![row(1, Some(0), clean())],
                ),
            ),
        ]);
        assert!(uses_trusted(&world, &clean()));
        assert!(!uses_trusted(&world, &escapes()));
        // a name whose row escapes, and one whose row does not
        assert!(!uses_trusted(&world, &names(&[var(1, 1)])));
        assert!(uses_trusted(&world, &names(&[var(1, 2)])));
        // passed to a parameter whose row has no uses, to one that escapes, to a rest parameter
        assert!(uses_trusted(&world, &passed(&[(2, 0)])));
        assert!(!uses_trusted(&world, &passed(&[(3, 0)])));
        assert!(!uses_trusted(&world, &passed(&[(4, 0)])));
    }

    #[test]
    fn the_counts_of_a_body() {
        let receiver = |escapes: bool| Uses {
            receiver: true,
            escapes,
            ..Uses::default()
        };
        let exported = Uses {
            exported: true,
            ..Uses::default()
        };
        let module = DefId::new(0, 0, None);
        let at = |at: u32| VarId { def: module, at };
        // the file's rows: 1 and 2 are exported, 3 is not
        let file = with_this(
            vec![
                row(1, None, exported.clone()),
                row(2, None, exported),
                row(3, None, clean()),
            ],
            None,
        );
        let counts = |this_uses: Option<Uses>, reads: Vec<Expr>| {
            // one trusted receiver, one that escapes, and one trusted with no method called on it
            let body = with_this(
                vec![
                    Var {
                        assigned: reads.into(),
                        ..row(1, None, receiver(false))
                    },
                    row(2, None, receiver(true)),
                    row(3, None, clean()),
                ],
                this_uses,
            );
            let mut world = world(vec![(1, Arc::clone(&body))]);
            world.flows.insert(module, Arc::clone(&file));
            body_counts(&world, d(1), &body)
        };
        let nothing = counts(None, vec![]);
        assert_eq!(nothing.member_call_on_trusted, 1);
        assert_eq!(nothing.this_written_outside_class, 0);
        assert_eq!(nothing.module_object_shared, 0);
        assert_eq!(
            counts(Some(Uses::default()), vec![]).this_written_outside_class,
            0
        );
        assert_eq!(
            counts(Some(escapes()), vec![]).this_written_outside_class,
            1
        );
        // the file's row 1 is read inside an object's part and again as the default of a pattern element; row 2 only inside a member inside that default;
        // row 3 is not exported; a row of the body itself is not the file's
        let reads = vec![
            Expr::Object(Box::new([
                Part::Named("a".into(), Expr::Var(at(1))),
                Part::Named("b".into(), Expr::Var(at(3))),
                Part::Named("c".into(), Expr::Var(VarId { def: d(1), at: 1 })),
            ])),
            Expr::Default(
                Box::new(Expr::Member(Box::new(Expr::Var(at(2))), "k".into())),
                Box::new(Expr::Var(at(1))),
            ),
        ];
        assert_eq!(counts(None, reads).module_object_shared, 2);
    }
}
