//! Which parameters of a def can reach a call (`docs/03-typescript/12-context-layer/07-tc3-variables-and-captures.md`, "Relevance (step 6)"; the rule of
//! `06-tc2` §8, through variables). A parameter that cannot is unknown in every key, so callers that differ only in it share a context.
//!
//! Neutral, as `needs` is: it imports nothing from `sem` and asks only `World`, so a test answers it with tables made by hand.

use std::collections::HashSet;

use crate::eval::World;
use crate::flow::{DefId, Expr, Flow};
use crate::needs::each_expr;

/// For each parameter of `def` (in `Flow::params` order): whether its value can reach a call of the body. Empty for a def whose table the world does not
/// have (a key is never made for one: `walk::key_of` gives the open key), and for a def with no parameter.
///
/// The **sinks** are every site's callee and arguments, every hand-over's value, and each variable of `def` that a nested def named in the table needs
/// (`World::needs`): that def is handed over or called with the variable's value in its captures. From the sinks a worklist marks the **atoms** an expression
/// is made of, `OneOf` members included: `Param(i)` marks parameter `i`; `Var(v)` of this def marks `v`'s row. A marked row marks the atoms of everything
/// assigned to it, and the parameter it is the row of; a marked parameter marks the atoms of its default and its own row (the body may assign it). A
/// condition is not in the table, so a parameter read only in one is not marked; a return is not there either until TC6, which adds it to the sinks.
pub(crate) fn relevant(world: &dyn World, def: DefId) -> Box<[bool]> {
    let Some(flow) = world.flow(def) else {
        return Box::new([]);
    };
    if flow.params.is_empty() {
        return Box::new([]);
    }
    let mut marks = Marks {
        flow: &flow,
        def,
        params: vec![false; flow.params.len()],
        rows: vec![false; flow.vars.len()],
        work: Vec::new(),
    };
    for site in &flow.sites {
        marks.atoms(&site.callee);
        for arg in site.args.iter().flatten() {
            marks.atoms(&arg.value);
        }
    }
    for held in &flow.handed {
        marks.atoms(&held.value);
    }
    // a nested def named anywhere in the table is given what it needs: a variable of this def that it needs is read by the code it makes
    let mut named: HashSet<DefId> = HashSet::new();
    let mut nested: Vec<DefId> = Vec::new();
    each_expr(&flow, &mut |expr| {
        if let Expr::Def(handle) = expr
            && named.insert(*handle)
        {
            nested.push(*handle);
        }
    });
    for handle in nested {
        for var in world.needs(handle).iter().filter(|var| var.def == def) {
            if let Some(row) = flow.var_index(var.at) {
                marks.row(row);
            }
        }
    }
    marks.settle();
    marks.params.into()
}

/// What a worklist still has to follow: a marked row (its assignments) or a marked parameter (its default).
enum Work {
    Row(usize),
    Param(usize),
}

struct Marks<'a> {
    flow: &'a Flow,
    def: DefId,
    params: Vec<bool>,
    rows: Vec<bool>,
    work: Vec<Work>,
}

impl Marks<'_> {
    /// Marks each atom of `expr`.
    fn atoms(&mut self, expr: &Expr) {
        match expr {
            Expr::Param(i) => self.param(*i as usize),
            Expr::Var(id) if id.def == self.def => {
                if let Some(row) = self.flow.var_index(id.at) {
                    self.row(row);
                }
            }
            Expr::OneOf(members) => members.iter().for_each(|member| self.atoms(member)),
            // TC4 step 2 reads what is inside these
            Expr::Unknown
            | Expr::Data
            | Expr::Undefined
            | Expr::Def(_)
            | Expr::Anon(_)
            | Expr::Var(_)
            | Expr::Member(..)
            | Expr::Object(_)
            | Expr::Omit(..)
            | Expr::Default(..) => {}
        }
    }

    fn param(&mut self, index: usize) {
        if index < self.params.len() && !self.params[index] {
            self.params[index] = true;
            self.work.push(Work::Param(index));
            // the body may assign the parameter: what it is assigned reaches a call through it
            if let Some(row) = self.flow.param_var(index as u32) {
                self.row(row);
            }
        }
    }

    fn row(&mut self, row: usize) {
        if !self.rows[row] {
            self.rows[row] = true;
            self.work.push(Work::Row(row));
        }
    }

    /// Follows what each marked row and parameter reads until nothing new is marked.
    fn settle(&mut self) {
        let flow = self.flow;
        while let Some(next) = self.work.pop() {
            match next {
                Work::Row(row) => {
                    let var = &flow.vars[row];
                    for value in &var.assigned {
                        self.atoms(value);
                    }
                    if let Some(index) = var.param {
                        self.param(index as usize);
                    }
                }
                Work::Param(index) => {
                    if let Some(default) = &flow.params[index].default {
                        self.atoms(default);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use typescript_syntax::Span;

    use super::*;
    use crate::caps::Caps;
    use crate::flow::{Arg, FlowKind, HandedFlow, Param, SiteFlow, Var, VarId};
    use crate::nested::CaptureList;
    use crate::value::{Captures, Value};

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
            Arc::from(crate::needs::needs(self, def))
        }

        fn needs_known(&self, _def: DefId) -> Option<Arc<[VarId]>> {
            None
        }

        fn relevant(&self, def: DefId) -> Arc<[bool]> {
            Arc::from(relevant(self, def))
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
        ) -> (Option<crate::value::ObjectId>, u32) {
            (None, 0)
        }

        fn object_data(&self, _id: crate::value::ObjectId) -> crate::nested::ObjectData {
            unreachable!("these tests do not evaluate")
        }

        fn module_value(&self, _var: VarId) -> Value {
            unreachable!("these tests do not evaluate")
        }
    }

    fn d(index: u32) -> DefId {
        DefId::new(0, 0, Some(index))
    }

    fn param(default: Option<Expr>) -> Param {
        Param {
            rest: false,
            named: true,
            default,
            assigned: false,
            capable: true,
        }
    }

    /// The row of the variable at `at`: the parameter `param` when it is one, with these assignments.
    fn row(at: u32, param: Option<u32>, assigned: Vec<Expr>) -> Var {
        Var {
            at,
            param,
            capable: true,
            assigned: assigned.into(),
            open: false,
        }
    }

    fn var(def: DefId, at: u32) -> Expr {
        Expr::Var(VarId { def, at })
    }

    fn site(callee: Expr, args: Vec<Expr>) -> SiteFlow {
        SiteFlow {
            span: Span { start: 0, end: 0 },
            kind: FlowKind::Call,
            callee,
            args: Some(
                args.into_iter()
                    .map(|value| Arg {
                        value,
                        spread: false,
                    })
                    .collect(),
            ),
            anon: None,
        }
    }

    /// A table with these parameters, rows and sites, nested in `around`.
    fn table(
        around: Option<DefId>,
        params: Vec<Param>,
        vars: Vec<Var>,
        sites: Vec<SiteFlow>,
        handed: Vec<Expr>,
    ) -> Arc<Flow> {
        Arc::new(Flow {
            params: params.into(),
            sites: sites.into(),
            anons: Box::new([]),
            handed: handed
                .into_iter()
                .map(|value| HandedFlow {
                    at: None,
                    value,
                    written: false,
                })
                .collect(),
            unhandled: 0,
            vars: vars.into(),
            unmodelled: 0,
            around,
        })
    }

    fn world(tables: Vec<(DefId, Arc<Flow>)>) -> Tables {
        Tables(tables.into_iter().collect())
    }

    /// One def, `d(0)`, at the top of its file, and what `relevant` says of it.
    fn of(
        params: Vec<Param>,
        vars: Vec<Var>,
        sites: Vec<SiteFlow>,
        handed: Vec<Expr>,
    ) -> Vec<bool> {
        let world = world(vec![(d(0), table(None, params, vars, sites, handed))]);
        relevant(&world, d(0)).to_vec()
    }

    #[test]
    fn a_parameter_the_body_does_not_use_is_not_relevant() {
        assert_eq!(of(vec![param(None)], vec![], vec![], vec![]), [false]);
        // a call that does not use it
        assert_eq!(
            of(
                vec![param(None)],
                vec![],
                vec![site(Expr::Def(d(5)), vec![Expr::Data])],
                vec![]
            ),
            [false]
        );
    }

    #[test]
    fn a_callee_an_argument_and_a_hand_over_are_sinks() {
        let two = || vec![param(None), param(None)];
        assert_eq!(
            of(two(), vec![], vec![site(Expr::Param(0), vec![])], vec![]),
            [true, false]
        );
        assert_eq!(
            of(
                two(),
                vec![],
                vec![site(Expr::Def(d(5)), vec![Expr::Data, Expr::Param(1)])],
                vec![]
            ),
            [false, true]
        );
        assert_eq!(
            of(two(), vec![], vec![], vec![Expr::Param(1)]),
            [false, true]
        );
        // a member of a join is an atom
        assert_eq!(
            of(
                two(),
                vec![],
                vec![site(
                    Expr::OneOf([Expr::Def(d(5)), Expr::Param(0)].into()),
                    vec![]
                )],
                vec![]
            ),
            [true, false]
        );
    }

    #[test]
    fn a_parameter_reaches_a_call_through_a_local_and_through_a_chain_of_locals() {
        // `const h = cb; h();`
        assert_eq!(
            of(
                vec![param(None)],
                vec![row(10, None, vec![Expr::Param(0)])],
                vec![site(var(d(0), 10), vec![])],
                vec![]
            ),
            [true]
        );
        // `const a = p; const b = a; b();` and `q` is never read
        assert_eq!(
            of(
                vec![param(None), param(None)],
                vec![
                    row(10, None, vec![Expr::Param(0)]),
                    row(20, None, vec![var(d(0), 10)])
                ],
                vec![site(var(d(0), 20), vec![])],
                vec![]
            ),
            [true, false]
        );
        // a local nothing reads does not mark what it holds
        assert_eq!(
            of(
                vec![param(None)],
                vec![row(10, None, vec![Expr::Param(0)])],
                vec![],
                vec![]
            ),
            [false]
        );
    }

    #[test]
    fn a_parameter_assigned_from_another_makes_the_other_relevant() {
        // `function f(x, y) { x = y; x(); }`: the read of `x` is its row, which holds `y`
        assert_eq!(
            of(
                vec![param(None), param(None)],
                vec![
                    row(1, Some(0), vec![Expr::Param(0), Expr::Param(1)]),
                    row(2, Some(1), vec![Expr::Param(1)])
                ],
                vec![site(var(d(0), 1), vec![])],
                vec![]
            ),
            [true, true]
        );
    }

    #[test]
    fn a_default_is_followed_from_a_relevant_parameter_only() {
        // `function f(a, b = a) { b(); }`
        let params = || vec![param(None), param(Some(Expr::Param(0)))];
        assert_eq!(
            of(params(), vec![], vec![site(Expr::Param(1), vec![])], vec![]),
            [true, true]
        );
        // `b` is never used, so its default does not make `a` relevant
        assert_eq!(of(params(), vec![], vec![], vec![]), [false, false]);
    }

    #[test]
    fn a_variable_a_nested_def_needs_is_a_sink_when_the_def_is_named() {
        // `function wrap(cb) { function inner() { cb(); } later(inner); }`: `inner` = d(1) reads wrap's row of `cb`
        let (wrap, inner) = (d(0), d(1));
        let tables = |named: bool| {
            world(vec![
                (
                    wrap,
                    table(
                        None,
                        vec![param(None), param(None)],
                        vec![row(1, Some(0), vec![]), row(2, Some(1), vec![])],
                        vec![site(
                            Expr::Def(d(7)),
                            if named {
                                vec![Expr::Def(inner)]
                            } else {
                                vec![]
                            },
                        )],
                        vec![],
                    ),
                ),
                (
                    inner,
                    table(
                        Some(wrap),
                        vec![],
                        vec![],
                        vec![site(var(wrap, 1), vec![])],
                        vec![],
                    ),
                ),
            ])
        };
        // the nested def is handed over: `cb` is captured, `other` is not
        assert_eq!(relevant(&tables(true), wrap).to_vec(), [true, false]);
        // not named anywhere: nothing needs it
        assert_eq!(relevant(&tables(false), wrap).to_vec(), [false, false]);
    }

    #[test]
    fn a_variable_of_another_def_is_not_this_defs_row_and_a_def_without_a_table_has_no_flags() {
        let world = world(vec![(
            d(0),
            table(
                None,
                vec![param(None)],
                vec![],
                // a variable of another def with the same place as nothing here: not a row of this table
                vec![site(var(d(9), 1), vec![])],
                vec![],
            ),
        )]);
        assert_eq!(relevant(&world, d(0)).to_vec(), [false]);
        assert!(relevant(&world, d(3)).is_empty());
    }
}
