//! What an expression of a table can be (`docs/03-typescript/12-context-layer/03-design.md` §3, §7). What it needs that is not in the table it asks
//! through `World`, so a test answers by hand and the file has no tie to the engine (§15.4).

use std::sync::Arc;

use crate::caps::Caps;
use crate::captures::{CaptureList, VALUE_WIDENED_DEPTH};
use crate::env::{Env, MODULE_READS_MODULE, VARIABLE_ROUNDS_CAPPED};
use crate::flow::{DefId, Expr, Flow, VarId};
use crate::value::{Captures, One, Value};

/// How many rounds `solve` repeats before it gives up and opens every variable of the body (`07-tc3` §3). A chain of rows each reading the next settles in as
/// many rounds as it is long, so this is also the longest chain that is read exactly.
const MAX_ROUNDS: usize = 32;

// 🦀 Inside this file the world is a `&dyn World` (a reference that carries a table of the trait's methods), not a generic `&impl World`: `solve` wraps the world
// in a `Quiet` and evaluates through it, and a generic would then be instantiated for `Quiet<Quiet<Quiet<…>>>` without end, which the compiler refuses.

/// What the evaluator asks that is not in a `Flow`. `TypeScript` answers it (`calls.rs`); a test answers it by hand. `member` comes with TC5.
pub(crate) trait World {
    /// The table of `def`'s body; `None` when it has none.
    fn flow(&self, def: DefId) -> Option<Arc<Flow>>;
    /// A join passed the cap of a value set: the world counts it (`value_widened_set`). No default body, so a world cannot forget to count.
    fn widened(&self);
    /// Whether the layer runs with all of `caps` (a phase whose bit is off is the phase before it).
    fn knows(&self, caps: Caps) -> bool;
    /// Count one more of `key`, a fact of what this context did.
    fn note(&self, key: &'static str);
    /// The variables of the functions around `def` that `def` needs to be given (`needs::needs_of_component`), sorted by `VarId`: kept by the world for the
    /// request, so each def is asked once.
    fn needs(&self, def: DefId) -> Arc<[VarId]>;
    /// What `def` needs when the world has already worked it out, without working anything out: `needs::needs_of_component` does not solve such a def again
    /// (its set covers everything it reaches).
    fn needs_known(&self, def: DefId) -> Option<Arc<[VarId]>>;
    /// For each parameter of `def` (in `Flow::params` order): whether its value can reach a call of the body (`relevance::relevant`). Kept by the world, as
    /// `needs` is, since it is a fact of the code.
    fn relevant(&self, def: DefId) -> Arc<[bool]>;
    /// `list` interned (`CaptureSets::intern`): the number of its normal form, `None` when nothing is left, and how many inner closures were cut. It counts
    /// nothing: the caller notes the cuts, so a quiet world stays quiet.
    fn capture(&self, list: Vec<(VarId, Value)>) -> (Option<Captures>, u32);
    /// A copy of the list `captures` stands for.
    fn captured(&self, captures: Captures) -> CaptureList;
    /// The value of `var`, a variable of a file's own code (`DefId::index` is `None`): its row of that file's table, solved under the open key, kept for the
    /// request. Unknown when the file has no table.
    fn module_value(&self, var: VarId) -> Value;
}

/// A world that counts nothing: `solve` asks the same questions many times over while it settles, and the facts are said once, in its last round.
/// Everything else it passes through to the world it wraps.
struct Quiet<'w>(&'w dyn World);

impl World for Quiet<'_> {
    fn flow(&self, def: DefId) -> Option<Arc<Flow>> {
        self.0.flow(def)
    }

    fn widened(&self) {}

    fn knows(&self, caps: Caps) -> bool {
        self.0.knows(caps)
    }

    fn note(&self, _key: &'static str) {}

    fn needs(&self, def: DefId) -> Arc<[VarId]> {
        self.0.needs(def)
    }

    fn needs_known(&self, def: DefId) -> Option<Arc<[VarId]>> {
        self.0.needs_known(def)
    }

    fn relevant(&self, def: DefId) -> Arc<[bool]> {
        self.0.relevant(def)
    }

    fn capture(&self, list: Vec<(VarId, Value)>) -> (Option<Captures>, u32) {
        self.0.capture(list)
    }

    fn captured(&self, captures: Captures) -> CaptureList {
        self.0.captured(captures)
    }

    fn module_value(&self, var: VarId) -> Value {
        self.0.module_value(var)
    }
}

/// Where the value of a variable comes from while an expression is evaluated.
#[derive(Clone, Copy)]
enum Vars<'a> {
    /// The settled values of `Env::vars`, solved the first time they are read.
    Solved,
    /// The values of the round `solve` is in the middle of.
    Current(&'a [Value]),
}

/// `a` joined with `b`: the one place `eval` joins. A join whose result is wide when neither side was is where a set is cut, so it is counted here.
fn join(world: &dyn World, a: &Value, b: &Value) -> Value {
    let joined = a.join(b);
    if joined.is_wide() && !a.is_wide() && !b.is_wide() {
        world.widened();
    }
    joined
}

/// What `expr` can be, in `env` (`06-tc2` §3, step 3). One arm per variant and no `_`, so a variant a later phase adds does not compile until this
/// decides it.
pub(crate) fn eval(world: &impl World, env: &Env, expr: &Expr) -> Value {
    eval_below(world, env, expr, u32::MAX, Vars::Solved)
}

/// `eval`, where a parameter at place `below` or after it is unknown: a default sees only the parameters before its own (a later one is not bound yet,
/// and its own name would loop: `function f(a = a)`).
fn eval_below(world: &dyn World, env: &Env, expr: &Expr, below: u32, vars: Vars) -> Value {
    match expr {
        Expr::Unknown => Value::unknown(),
        Expr::Data => Value::data(),
        Expr::Def(def) => function(world, env, *def, vars),
        Expr::Param(index) if *index < below => param(world, env, *index, vars),
        Expr::Param(_) => Value::unknown(),
        Expr::Anon(index) => Value::of([One::Anon(env.def, *index)], false),
        Expr::Var(id) => variable(world, env, *id, vars),
        Expr::OneOf(members) => members
            .iter()
            .map(|member| eval_below(world, env, member, below, vars))
            .reduce(|a, b| join(world, &a, &b))
            .unwrap_or_else(Value::unknown),
    }
}

/// The value of a def named in a body (`07-tc3` §4): the function itself, or, when the layer knows `CAPTURES` and the def is nested and needs variables from
/// around it, the function with the value each of them has **here**. Nothing captured, or nothing known of it, is the bare function.
///
/// The list is made from the sparse sides: `needs` of a def can be hundreds of variables (the compiler's `createTypeChecker`), and few of them have a value
/// here that is not unknown. So it walks what can have one, this body's own rows and the key's entries, and keeps those that are in `needs` (sorted: a binary
/// search). `CaptureSets::intern` sorts the list.
fn function(world: &dyn World, env: &Env, def: DefId, vars: Vars) -> Value {
    if world.knows(Caps::CAPTURES) {
        let needed = world.needs(def);
        if !needed.is_empty() {
            let unknown = Value::unknown();
            let wanted = |var: &VarId| needed.binary_search(var).is_ok();
            let mut list: Vec<(VarId, Value)> = Vec::new();
            for row in env.flow.vars.iter() {
                let id = VarId {
                    def: env.def,
                    at: row.at,
                };
                if wanted(&id) {
                    let value = variable(world, env, id, vars);
                    if value != unknown {
                        list.push((id, value));
                    }
                }
            }
            for (id, value) in env.key.captured.iter() {
                if id.def != env.def && id.def.index().is_some() && wanted(id) && *value != unknown
                {
                    list.push((*id, value.clone()));
                }
            }
            let (made, cut) = world.capture(list);
            for _ in 0..cut {
                world.note(VALUE_WIDENED_DEPTH);
            }
            if let Some(captures) = made {
                return Value::of([One::Closure(def, captures)], false);
            }
        }
    }
    Value::of([One::Fn(def)], false)
}

/// The value of a variable: of a row of this body's table when the layer knows `LOCALS`; of a variable of a def around, the key's entry for it when the layer
/// knows `CAPTURES`; of a variable at the top of a file, when the layer knows `MODULES`, the world's answer (`World::module_value`) from a function, and unknown
/// from a file's own code (one module's value does not read another's: counted `module_reads_module`); unknown otherwise (with the bit off the layer is the
/// one before it, which had no value for that name).
fn variable(world: &dyn World, env: &Env, id: VarId, vars: Vars) -> Value {
    if id.def == env.def {
        if !world.knows(Caps::LOCALS) {
            return Value::unknown();
        }
        return match env.flow.var_index(id.at) {
            Some(row) => row_of(world, env, row, vars),
            None => Value::unknown(),
        };
    }
    if world.knows(Caps::CAPTURES) && id.def.index().is_some() {
        return env
            .key
            .captured
            .binary_search_by_key(&id, |(var, _)| *var)
            .map_or_else(|_| Value::unknown(), |at| env.key.captured[at].1.clone());
    }
    if world.knows(Caps::MODULES) && id.def.index().is_none() {
        if env.def.index().is_some() {
            return world.module_value(id);
        }
        world.note(MODULE_READS_MODULE);
    }
    Value::unknown()
}

/// The value of row `row` of the table: the settled one, or the current round's.
fn row_of(world: &dyn World, env: &Env, row: usize, vars: Vars) -> Value {
    match vars {
        Vars::Solved => env.vars.get_or_init(|| solve(world, env))[row].clone(),
        Vars::Current(current) => current[row].clone(),
    }
}

/// What was passed for parameter `index` in this context, before the body writes it: unknown for a rest parameter (checked first); data for a parameter
/// that cannot hold a function (`capable` is false), whatever the key holds, since `admit` (step 4) never lists such a slot; otherwise what the key holds
/// for it, unknown for a slot the key does not list. Where nothing may have been passed (the key holds `Undefined`, or is open) and there is a default, the
/// default's value takes `Undefined`'s place.
fn passed(world: &dyn World, env: &Env, index: u32, vars: Vars) -> Value {
    let Some(row) = env.flow.params.get(index as usize) else {
        return Value::unknown();
    };
    if row.rest {
        return Value::unknown();
    }
    if !row.capable {
        return Value::data();
    }
    // a key lists no trailing unknowns (`KeyData::normal`), so a slot it does not list is unknown
    let passed = env
        .key
        .params
        .get(index as usize)
        .cloned()
        .unwrap_or_else(Value::unknown);
    match &row.default {
        Some(default) if passed.contains(One::Undefined) || passed.is_open() => join(
            world,
            &passed.without(One::Undefined),
            &eval_below(world, env, default, index, vars),
        ),
        _ => passed,
    }
}

/// The value of parameter `index` in this context. A parameter the body writes holds what was passed **and** what the body assigns: its row of the table when
/// the layer knows `LOCALS` (`row_value` starts it from `passed`); before that, what was passed joined with unknown (TC2).
fn param(world: &dyn World, env: &Env, index: u32, vars: Vars) -> Value {
    let Some(row) = env.flow.params.get(index as usize) else {
        return Value::unknown();
    };
    if !row.assigned {
        return passed(world, env, index, vars);
    }
    if world.knows(Caps::LOCALS)
        && let Some(var) = env.flow.param_var(index)
    {
        return row_of(world, env, var, vars);
    }
    join(world, &passed(world, env, index, vars), &Value::unknown())
}

/// The value of row `i` of the table, given the values `current` of all the rows: what a parameter was passed, or nothing for a local, joined with the value
/// of every expression assigned to it, and with unknown when a write is not read (`open`). A row that cannot hold a function is data, whatever is written.
fn row_value(world: &dyn World, env: &Env, i: usize, current: &[Value]) -> Value {
    let row = &env.flow.vars[i];
    if !row.capable {
        return Value::data();
    }
    let mut value = match row.param {
        Some(index) => passed(world, env, index, Vars::Current(current)),
        None => Value::data(),
    };
    for assigned in &row.assigned {
        let written = eval_below(world, env, assigned, u32::MAX, Vars::Current(current));
        value = join(world, &value, &written);
    }
    if row.open {
        value = join(world, &value, &Value::unknown());
    }
    value
}

/// The value of every row of the table, solved together (`07-tc3` §3). Start from data, the empty closed value (nothing can be there), and compute every row
/// from the last round's values until a round changes nothing: values only grow, so this ends, and what it ends at is the least value that satisfies every row.
/// Past `MAX_ROUNDS` the rows are opened instead, and `variable_rounds_capped` says so.
///
/// Each round joins the last round's value in, so the rounds grow whatever the rows do (see the comment in the loop).
///
/// The rounds are quiet. One last round with the real world, from the values the rounds ended on, gives the answer and says what there is to say once
/// (`value_widened_set`), however many rounds it took.
fn solve(world: &dyn World, env: &Env) -> Box<[Value]> {
    let rows = env.flow.vars.len();
    let quiet = Quiet(world);
    let mut current: Vec<Value> = vec![Value::data(); rows];
    let mut capped = true;
    for _ in 0..MAX_ROUNDS {
        // each round keeps what the last had: every row of TC2 and TC3 step 2 only grows from round to round, so this changes none of them, but the depth cut of
        // a closure (`captures`) writes a deeper one as its bare function, which is not growth, and a def that captures a variable that holds itself would
        // then swap between two values for ever
        let next: Vec<Value> = (0..rows)
            .map(|i| join(&quiet, &current[i], &row_value(&quiet, env, i, &current)))
            .collect();
        if next == current {
            capped = false;
            break;
        }
        current = next;
    }
    if capped {
        for value in &mut current {
            *value = value.join(&Value::unknown());
        }
        world.note(VARIABLE_ROUNDS_CAPPED);
    }
    (0..rows)
        .map(|i| {
            let value = join(world, &current[i], &row_value(world, env, i, &current));
            // a body that did not settle is open in every row, whatever the last round says
            if capped {
                value.join(&Value::unknown())
            } else {
                value
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::{BTreeMap, HashMap};

    use super::*;
    use crate::captures::CaptureSets;
    use crate::flow::{Param, Var};
    use crate::key::KeyData;
    use crate::value::MAX_SET;

    /// A `World` answered by hand: a table per def, looked up, how many times a join passed the cap (a `Cell`, since `widened` takes `&self`), the bits the
    /// layer knows, and the counters noted.
    struct ByHand {
        flows: HashMap<DefId, Arc<Flow>>,
        widened: Cell<u32>,
        caps: Caps,
        notes: RefCell<BTreeMap<&'static str, u32>>,
        captures: RefCell<CaptureSets>,
        /// What `module_value` answers, by variable; unknown for one not listed.
        modules: HashMap<VarId, Value>,
    }

    impl World for ByHand {
        fn flow(&self, def: DefId) -> Option<Arc<Flow>> {
            self.flows.get(&def).map(Arc::clone)
        }

        fn widened(&self) {
            self.widened.set(self.widened.get() + 1);
        }

        fn knows(&self, caps: Caps) -> bool {
            self.caps.contains(caps)
        }

        fn note(&self, key: &'static str) {
            *self.notes.borrow_mut().entry(key).or_default() += 1;
        }

        fn needs(&self, def: DefId) -> Arc<[VarId]> {
            Arc::from(crate::needs::needs(self, def))
        }

        fn needs_known(&self, _def: DefId) -> Option<Arc<[VarId]>> {
            None
        }

        fn relevant(&self, def: DefId) -> Arc<[bool]> {
            Arc::from(crate::relevance::relevant(self, def))
        }

        fn capture(&self, list: Vec<(VarId, Value)>) -> (Option<Captures>, u32) {
            self.captures.borrow_mut().intern(list)
        }

        fn captured(&self, captures: Captures) -> CaptureList {
            self.captures.borrow().get(captures).into()
        }

        fn module_value(&self, var: VarId) -> Value {
            self.modules
                .get(&var)
                .cloned()
                .unwrap_or_else(Value::unknown)
        }
    }

    fn d(index: u32) -> DefId {
        DefId::new(0, 0, Some(index))
    }

    fn func(index: u32) -> Value {
        Value::of([One::Fn(d(index))], false)
    }

    fn undefined() -> Value {
        Value::of([One::Undefined], false)
    }

    /// A named, capable parameter.
    fn row(rest: bool, default: Option<Expr>, assigned: bool) -> Param {
        Param {
            rest,
            named: true,
            default,
            assigned,
            capable: true,
        }
    }

    /// An env over a table of these parameters and nothing else, for the body of `d(9)`, under a key that holds these values.
    fn env(rows: Vec<Param>, key: Vec<Value>) -> Env {
        env_with_vars(rows, key, vec![])
    }

    /// `env`, and these rows of variables.
    fn env_with_vars(rows: Vec<Param>, key: Vec<Value>, vars: Vec<Var>) -> Env {
        Env::new(
            Arc::new(Flow {
                params: rows.into(),
                sites: Box::new([]),
                anons: Box::new([]),
                handed: Box::new([]),
                unhandled: 0,
                vars: vars.into(),
                unmodelled: 0,
                around: None,
            }),
            d(9),
            KeyData {
                params: key.into(),
                captured: Box::new([]),
            },
        )
    }

    /// The world is not asked in this step; a hand-made one answers, so it is a world a test can use. It knows `PARAMS` and `LOCALS`.
    fn world(env: &Env) -> ByHand {
        world_knowing(env, Caps::PARAMS | Caps::LOCALS)
    }

    fn world_knowing(env: &Env, caps: Caps) -> ByHand {
        let world = ByHand {
            flows: HashMap::from([(env.def, Arc::clone(&env.flow))]),
            widened: Cell::new(0),
            caps,
            notes: RefCell::new(BTreeMap::new()),
            captures: RefCell::new(CaptureSets::new()),
            modules: HashMap::new(),
        };
        assert!(world.flow(env.def).is_some());
        assert!(world.flow(DefId::new(0, 0, None)).is_none());
        world
    }

    #[test]
    fn each_expression_has_its_value() {
        let env = env(vec![], vec![]);
        let world = world(&env);
        let of = |expr: Expr| eval(&world, &env, &expr);
        assert_eq!(of(Expr::Unknown), Value::unknown());
        assert_eq!(of(Expr::Data), Value::data());
        assert_eq!(of(Expr::Def(d(1))), func(1));
        assert_eq!(of(Expr::Anon(2)), Value::of([One::Anon(d(9), 2)], false));
        assert_eq!(
            of(Expr::OneOf([Expr::Def(d(1)), Expr::Data].into())),
            func(1)
        );
        assert_eq!(
            of(Expr::OneOf([Expr::Def(d(1)), Expr::Unknown].into())),
            Value::of([One::Fn(d(1))], true)
        );
        assert_eq!(
            of(Expr::OneOf([Expr::Def(d(1)), Expr::Def(d(2))].into())),
            Value::of([One::Fn(d(1)), One::Fn(d(2))], false)
        );
        assert_eq!(of(Expr::Param(0)), Value::unknown());
    }

    #[test]
    fn a_parameter_is_what_its_key_holds() {
        let rows = || {
            vec![
                row(false, None, false),
                row(true, None, false),
                row(false, None, true),
            ]
        };
        let held = env(rows(), vec![func(1), func(2), func(3)]);
        let world_held = world(&held);
        let at = |env: &Env, world: &ByHand, index: u32| eval(world, env, &Expr::Param(index));
        assert_eq!(at(&held, &world_held, 0), func(1));
        // a rest parameter is unknown, whatever the key holds
        assert_eq!(at(&held, &world_held, 1), Value::unknown());
        // the body writes the third
        assert_eq!(at(&held, &world_held, 2), Value::of([One::Fn(d(3))], true));
        // the open key: every slot is unknown
        let open = env(rows(), vec![]);
        let world_open = world(&open);
        assert_eq!(at(&open, &world_open, 0), Value::unknown());
        assert_eq!(at(&open, &world_open, 2), Value::unknown());
        let wide = env(rows(), vec![Value::wide()]);
        let world_wide = world(&wide);
        assert_eq!(at(&wide, &world_wide, 0), Value::wide());
        // no such row
        assert_eq!(at(&held, &world_held, 7), Value::unknown());
    }

    #[test]
    fn a_parameter_that_cannot_hold_a_function_is_data() {
        // struct update syntax (`..base`): every field not named is copied from `base`
        let not_capable = Param {
            capable: false,
            ..row(false, None, false)
        };
        let env = env(vec![not_capable], vec![func(1)]);
        let world = world(&env);
        assert_eq!(eval(&world, &env, &Expr::Param(0)), Value::data());
        // data joined with a function is that function, closed
        let joined = Expr::OneOf([Expr::Param(0), Expr::Def(d(2))].into());
        assert_eq!(eval(&world, &env, &joined), func(2));
    }

    #[test]
    fn a_default_fills_what_was_not_passed() {
        let rows = |assigned| vec![row(false, Some(Expr::Def(d(5))), assigned)];
        let value = |assigned: bool, key: Value| {
            let env = env(rows(assigned), vec![key]);
            let world = world(&env);
            eval(&world, &env, &Expr::Param(0))
        };
        assert_eq!(value(false, undefined()), func(5));
        // passed: the default is not used
        assert_eq!(value(false, func(1)), func(1));
        // the open key
        let open = env(rows(false), vec![]);
        assert_eq!(
            eval(&world(&open), &open, &Expr::Param(0)),
            Value::of([One::Fn(d(5))], true)
        );
        assert_eq!(
            value(false, Value::of([One::Fn(d(1)), One::Undefined], false)),
            Value::of([One::Fn(d(1)), One::Fn(d(5))], false)
        );
        // `null` was passed: a default is for `undefined` only
        assert_eq!(value(false, Value::data()), Value::data());
        assert_eq!(value(true, undefined()), Value::of([One::Fn(d(5))], true));
    }

    #[test]
    fn a_default_sees_only_the_parameters_before_it() {
        let value = |rows: Vec<Param>, key: Vec<Value>, index: u32| {
            let env = env(rows, key);
            let world = world(&env);
            eval(&world, &env, &Expr::Param(index))
        };
        assert_eq!(
            value(
                vec![
                    row(false, None, false),
                    row(false, Some(Expr::Param(0)), false)
                ],
                vec![func(1), undefined()],
                1
            ),
            func(1)
        );
        // `a = a` names itself: unknown, and it returns
        assert_eq!(
            value(
                vec![row(false, Some(Expr::Param(0)), false)],
                vec![undefined()],
                0
            ),
            Value::unknown()
        );
        // a later parameter is not bound yet
        assert_eq!(
            value(
                vec![
                    row(false, Some(Expr::Param(1)), false),
                    row(false, None, false)
                ],
                vec![undefined()],
                0
            ),
            Value::unknown()
        );
    }

    /// `expr` in an env whose parameter 0 holds `slot`: what it is, and how many cuts were counted.
    fn counted(expr: Expr, slot: Value) -> (Value, u32) {
        let env = env(vec![row(false, None, false)], vec![slot]);
        let world = world(&env);
        let value = eval(&world, &env, &expr);
        (value, world.widened.get())
    }

    fn defs(n: usize) -> Expr {
        Expr::OneOf((1..=n as u32).map(|i| Expr::Def(d(i))).collect())
    }

    #[test]
    fn nine_functions_pass_the_cap_and_count_once() {
        let (value, count) = counted(defs(MAX_SET + 1), Value::unknown());
        assert!(value.is_wide());
        assert_eq!(count, 1);
    }

    #[test]
    fn eight_functions_are_a_set_and_count_nothing() {
        let (value, count) = counted(defs(MAX_SET), Value::unknown());
        assert!(!value.is_wide());
        assert_eq!(count, 0);
    }

    #[test]
    fn a_slot_that_was_wide_is_not_cut_by_the_join() {
        let expr = Expr::OneOf([Expr::Param(0), Expr::Def(d(1))].into());
        let (value, count) = counted(expr, Value::wide());
        assert!(value.is_wide());
        assert_eq!(count, 0);
    }

    /// The id of the variable of `d(9)`'s body whose name starts at `at`.
    fn vid(at: u32) -> VarId {
        VarId { def: d(9), at }
    }

    fn var_row(at: u32, param: Option<u32>, capable: bool, assigned: Vec<Expr>, open: bool) -> Var {
        Var {
            at,
            param,
            capable,
            assigned: assigned.into(),
            open,
        }
    }

    /// A capable local at `at`, closed, with these assignments.
    fn local(at: u32, assigned: Vec<Expr>) -> Var {
        var_row(at, None, true, assigned, false)
    }

    fn of_var(world: &ByHand, env: &Env, at: u32) -> Value {
        eval(world, env, &Expr::Var(vid(at)))
    }

    #[test]
    fn a_local_is_the_join_of_what_is_assigned_to_it() {
        let env = env_with_vars(
            vec![],
            vec![],
            vec![
                // `let h = a; h = b;`
                local(10, vec![Expr::Def(d(1)), Expr::Def(d(2))]),
                // a write the table does not read
                var_row(20, None, true, vec![Expr::Def(d(1))], true),
                // never assigned
                local(30, vec![]),
                // a type that holds only data: whatever is written is not read
                var_row(40, None, false, vec![Expr::Def(d(1))], true),
                // read through another
                local(50, vec![Expr::Var(vid(10))]),
            ],
        );
        let world = world(&env);
        assert_eq!(
            of_var(&world, &env, 10),
            Value::of([One::Fn(d(1)), One::Fn(d(2))], false)
        );
        assert_eq!(
            of_var(&world, &env, 20),
            Value::of([One::Fn(d(1))], true),
            "open adds unknown"
        );
        assert_eq!(of_var(&world, &env, 30), Value::data());
        assert_eq!(of_var(&world, &env, 40), Value::data());
        assert_eq!(
            of_var(&world, &env, 50),
            Value::of([One::Fn(d(1)), One::Fn(d(2))], false)
        );
        // a name that is not a row of the table
        assert_eq!(of_var(&world, &env, 99), Value::unknown());
        // nothing was counted
        assert!(world.notes.borrow().is_empty());
        assert_eq!(world.widened.get(), 0);
    }

    /// `cb = cb || f`, the parameter being `d(9)`'s first.
    fn written_parameter() -> Env {
        env_with_vars(
            vec![row(false, None, true)],
            vec![func(1)],
            vec![var_row(
                5,
                Some(0),
                true,
                vec![Expr::OneOf([Expr::Param(0), Expr::Def(d(2))].into())],
                false,
            )],
        )
    }

    #[test]
    fn a_parameter_the_body_writes_is_what_was_passed_and_what_is_assigned() {
        let env = written_parameter();
        let by_hand = world(&env);
        assert_eq!(
            eval(&by_hand, &env, &Expr::Param(0)),
            Value::of([One::Fn(d(1)), One::Fn(d(2))], false),
            "closed: nothing unknown can be there"
        );
        // the open key passes nothing known
        let mut open = written_parameter();
        open.key = KeyData::default();
        let world_open = world(&open);
        assert_eq!(
            eval(&world_open, &open, &Expr::Param(0)),
            Value::of([One::Fn(d(2))], true)
        );
        // read through its name too
        assert_eq!(
            of_var(&world_open, &open, 5),
            Value::of([One::Fn(d(2))], true)
        );
    }

    #[test]
    fn two_variables_read_each_other_and_settle_whichever_is_read_first() {
        let rows = || {
            env_with_vars(
                vec![],
                vec![],
                vec![
                    // `g = a; g = h`, `h = g; h = b`
                    local(10, vec![Expr::Def(d(1)), Expr::Var(vid(20))]),
                    local(20, vec![Expr::Var(vid(10)), Expr::Def(d(2))]),
                ],
            )
        };
        let both = Value::of([One::Fn(d(1)), One::Fn(d(2))], false);
        let first = rows();
        let world_first = world(&first);
        assert_eq!(of_var(&world_first, &first, 20), both);
        assert_eq!(of_var(&world_first, &first, 10), both);
        let second = rows();
        let world_second = world(&second);
        assert_eq!(of_var(&world_second, &second, 10), both);
        assert_eq!(of_var(&world_second, &second, 20), both);
        assert!(world_second.notes.borrow().is_empty());
    }

    #[test]
    fn a_chain_that_needs_more_rounds_than_the_cap_opens_every_row() {
        // row k reads row k + 1, and the last is `a`: a value moves one row a round, so row 0 needs 40
        let rows: Vec<Var> = (0..40)
            .map(|k| {
                let assigned = if k == 39 {
                    Expr::Def(d(1))
                } else {
                    Expr::Var(vid((k + 1) * 10))
                };
                local(k * 10, vec![assigned])
            })
            .collect();
        let env = env_with_vars(vec![], vec![], rows);
        let long = world(&env);
        for k in 0..40 {
            assert!(of_var(&long, &env, k * 10).is_open(), "row {k}");
        }
        // however many rows were read, the body was capped once
        assert_eq!(
            *long.notes.borrow(),
            BTreeMap::from([(VARIABLE_ROUNDS_CAPPED, 1)])
        );
        // a chain the cap holds is read exactly
        let short: Vec<Var> = (0..20)
            .map(|k| {
                let assigned = if k == 19 {
                    Expr::Def(d(1))
                } else {
                    Expr::Var(vid((k + 1) * 10))
                };
                local(k * 10, vec![assigned])
            })
            .collect();
        let env = env_with_vars(vec![], vec![], short);
        let world = world(&env);
        assert_eq!(of_var(&world, &env, 0), func(1));
        assert!(world.notes.borrow().is_empty());
    }

    #[test]
    fn a_cut_set_in_a_variable_is_counted_once_however_many_rounds_it_took() {
        let env = env_with_vars(
            vec![],
            vec![],
            vec![
                local(10, vec![defs(MAX_SET + 1)]),
                // reads the first, so the body needs a second round to settle
                local(20, vec![Expr::Var(vid(10))]),
            ],
        );
        let world = world(&env);
        assert!(of_var(&world, &env, 10).is_wide());
        assert!(of_var(&world, &env, 20).is_wide());
        assert_eq!(world.widened.get(), 1);
        assert!(world.notes.borrow().is_empty());
    }

    #[test]
    fn with_locals_off_a_variable_is_unknown_and_a_written_parameter_is_tc2s() {
        let env = written_parameter();
        let world = world_knowing(&env, Caps::PARAMS);
        assert_eq!(of_var(&world, &env, 5), Value::unknown());
        assert_eq!(
            eval(&world, &env, &Expr::Param(0)),
            Value::of([One::Fn(d(1))], true),
            "what was passed, joined with unknown"
        );
    }

    #[test]
    fn a_variable_of_another_def_is_unknown() {
        let env = env_with_vars(vec![], vec![], vec![local(10, vec![Expr::Def(d(1))])]);
        let world = world(&env);
        let elsewhere = VarId { def: d(8), at: 10 };
        assert_eq!(eval(&world, &env, &Expr::Var(elsewhere)), Value::unknown());
        assert_eq!(of_var(&world, &env, 10), func(1));
    }

    /// A variable of the file's own code (`DefId::index` is `None`), asked of a function (`d(9)`): `module_value`'s answer when the layer knows `MODULES`.
    #[test]
    fn a_variable_of_a_files_code_read_from_a_function_is_the_worlds_answer() {
        let env = env(vec![], vec![]);
        let top = VarId {
            def: DefId::new(0, 0, None),
            at: 10,
        };
        let mut world = world_knowing(&env, Caps::BUILT);
        world
            .modules
            .insert(top, Value::of([One::Fn(d(1)), One::Fn(d(2))], false));
        assert_eq!(
            eval(&world, &env, &Expr::Var(top)),
            Value::of([One::Fn(d(1)), One::Fn(d(2))], false)
        );
        assert!(world.notes.borrow().is_empty());
        // a variable the world has no value for is unknown
        let other = VarId { at: 11, ..top };
        assert_eq!(eval(&world, &env, &Expr::Var(other)), Value::unknown());
    }

    /// Read from a file's own code the same variable of another file is unknown and counted once (one module's value does not read another's).
    #[test]
    fn a_variable_of_another_file_read_from_a_files_code_is_unknown_and_noted() {
        let function = env(vec![], vec![]);
        let file = DefId::new(0, 0, None);
        let env = Env::new(
            Arc::clone(&function.flow),
            DefId::new(0, 1, None),
            KeyData::default(),
        );
        let top = VarId { def: file, at: 10 };
        let mut world = world_knowing(&env, Caps::BUILT);
        world.modules.insert(top, func(1));
        assert_eq!(eval(&world, &env, &Expr::Var(top)), Value::unknown());
        assert_eq!(
            *world.notes.borrow(),
            BTreeMap::from([("module_reads_module", 1)])
        );
    }

    /// With `MODULES` off the layer is step 3's: unknown, and nothing is noted, from a function and from a file's code.
    #[test]
    fn with_modules_off_a_variable_of_a_file_is_unknown_and_nothing_is_noted() {
        let function = env(vec![], vec![]);
        let file = DefId::new(0, 0, None);
        let top = VarId { def: file, at: 10 };
        let step_3 = Caps::PARAMS | Caps::LOCALS | Caps::CAPTURES;
        let from_file = Env::new(
            Arc::clone(&function.flow),
            DefId::new(0, 1, None),
            KeyData::default(),
        );
        for env in [&function, &from_file] {
            let mut world = world_knowing(env, step_3);
            world.modules.insert(top, func(1));
            assert_eq!(eval(&world, env, &Expr::Var(top)), Value::unknown());
            assert!(world.notes.borrow().is_empty());
        }
    }

    /// A table of `d(index)`, nested in `around`, that hands over each of `reads` (so `needs` finds them), and declares these rows.
    fn nested(around: Option<DefId>, vars: Vec<Var>, reads: Vec<Expr>) -> Arc<Flow> {
        Arc::new(Flow {
            params: Box::new([]),
            sites: Box::new([]),
            anons: Box::new([]),
            handed: reads
                .into_iter()
                .map(|value| crate::flow::HandedFlow {
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

    /// `env`'s world, with these tables more and the layer knowing every built bit.
    fn world_with(env: &Env, tables: Vec<(DefId, Arc<Flow>)>, caps: Caps) -> ByHand {
        let mut world = world_knowing(env, caps);
        world.flows.extend(tables);
        world
    }

    /// The variable of `d(8)` at `at`: a def around `d(9)`.
    fn outer(at: u32) -> VarId {
        VarId { def: d(8), at }
    }

    fn all_caps() -> Caps {
        Caps::PARAMS | Caps::LOCALS | Caps::CAPTURES
    }

    /// The list a closure holds, through the world.
    fn list_of(world: &ByHand, value: &Value) -> (DefId, Vec<(VarId, Value)>) {
        match value.known() {
            [One::Closure(def, captures)] => (*def, world.captured(*captures).into_vec()),
            other => panic!("not one closure: {other:?}"),
        }
    }

    #[test]
    fn a_nested_def_is_a_closure_of_what_it_needs_here() {
        // `d(9)` declares `v` (at 10, holds `a`) and names `g` = `d(5)`, which reads `v` and `w`, a variable of `d(8)` that `d(9)` was given in its key
        let mut env = env_with_vars(vec![], vec![], vec![local(10, vec![Expr::Def(d(1))])]);
        env.key = KeyData {
            params: Box::new([]),
            captured: vec![(outer(77), func(3))].into(),
        };
        let tables = vec![
            (d(8), nested(None, vec![local(77, vec![])], vec![])),
            (
                d(5),
                nested(
                    Some(d(9)),
                    vec![],
                    vec![Expr::Var(vid(10)), Expr::Var(outer(77))],
                ),
            ),
        ];
        let on = world_with(&env, tables.clone(), all_caps());
        let value = eval(&on, &env, &Expr::Def(d(5)));
        let (def, list) = list_of(&on, &value);
        assert_eq!(def, d(5));
        assert_eq!(list, [(outer(77), func(3)), (vid(10), func(1))]);
        // a def that is not nested, or captures nothing, is the function
        assert_eq!(eval(&on, &env, &Expr::Def(d(1))), func(1));
        // with `CAPTURES` off the layer is step 2: the function, nothing captured
        let off = world_with(&env, tables, Caps::PARAMS | Caps::LOCALS);
        assert_eq!(eval(&off, &env, &Expr::Def(d(5))), func(5));
        assert!(on.notes.borrow().is_empty());
    }

    #[test]
    fn a_variable_of_a_def_around_is_what_the_key_captured() {
        let mut env = env_with_vars(vec![], vec![], vec![]);
        env.key = KeyData {
            params: Box::new([]),
            // sorted by `VarId`, as a key's list is
            captured: vec![
                // the file's own code is never captured, even if a key says so
                (
                    VarId {
                        def: DefId::new(0, 0, None),
                        at: 5,
                    },
                    func(4),
                ),
                (outer(77), func(3)),
            ]
            .into(),
        };
        let on = world_with(&env, vec![], all_caps());
        let of = |world: &ByHand, id: VarId| eval(world, &env, &Expr::Var(id));
        assert_eq!(of(&on, outer(77)), func(3));
        assert_eq!(of(&on, outer(78)), Value::unknown(), "the key has none");
        assert_eq!(
            of(
                &on,
                VarId {
                    def: DefId::new(0, 0, None),
                    at: 5
                }
            ),
            Value::unknown()
        );
        let off = world_with(&env, vec![], Caps::PARAMS | Caps::LOCALS);
        assert_eq!(of(&off, outer(77)), Value::unknown());
    }

    #[test]
    fn a_def_that_captures_a_variable_that_holds_itself_settles_without_the_cap() {
        // `let next = () => {}; function tick() { later(next) } next = tick;`: `next` holds an arrow and `tick`, which captures `next`
        let env = env_with_vars(
            vec![],
            vec![],
            vec![local(10, vec![Expr::Anon(0), Expr::Def(d(5))])],
        );
        let tables = vec![(d(5), nested(Some(d(9)), vec![], vec![Expr::Var(vid(10))]))];
        let world = world_with(&env, tables, all_caps());
        let value = of_var(&world, &env, 10);
        let notes = world.notes.borrow().clone();
        assert!(
            !notes.contains_key(VARIABLE_ROUNDS_CAPPED),
            "it settled: {notes:?}"
        );
        // every closure of `tick` it holds, and the arrow
        assert!(value.known().contains(&One::Anon(d(9), 0)));
        assert!(
            value
                .known()
                .iter()
                .any(|one| matches!(one, One::Closure(def, _) if *def == d(5)))
        );
        assert!(!value.is_open());
        // the cuts are said once each, by the last round alone: the rounds before it are quiet
        assert_eq!(notes, BTreeMap::from([(VALUE_WIDENED_DEPTH, 2)]));
        // read again, it is the same value and says nothing more
        assert_eq!(of_var(&world, &env, 10), value);
        assert_eq!(*world.notes.borrow(), notes);
    }

    #[test]
    fn a_closure_of_a_hundred_needs_holds_only_the_two_that_have_a_value() {
        // `g` = d(5), nested in `d(9)`, reads a hundred variables: `v` of `d(9)` (a row that holds `a`) and 99 of `d(8)`'s, of which the key holds one. `d(9)` has
        // more rows that `g` does not read: they are not in the closure, whatever they hold
        let mut env = env_with_vars(
            vec![],
            vec![],
            vec![
                local(10, vec![Expr::Def(d(1))]),
                local(20, vec![Expr::Def(d(2))]),
                local(30, vec![]),
            ],
        );
        env.key = KeyData {
            params: Box::new([]),
            captured: vec![(outer(500), func(3))].into(),
        };
        let rows: Vec<Var> = (0..99).map(|k| local(500 + k, vec![])).collect();
        let mut reads: Vec<Expr> = (0..99).map(|k| Expr::Var(outer(500 + k))).collect();
        reads.push(Expr::Var(vid(10)));
        let tables = vec![
            (d(8), nested(None, rows, vec![])),
            (d(5), nested(Some(d(9)), vec![], reads)),
        ];
        let world = world_with(&env, tables, all_caps());
        assert_eq!(world.needs(d(5)).len(), 100);
        let value = eval(&world, &env, &Expr::Def(d(5)));
        let (def, list) = list_of(&world, &value);
        assert_eq!(def, d(5));
        assert_eq!(list, [(outer(500), func(3)), (vid(10), func(1))]);
    }
}
