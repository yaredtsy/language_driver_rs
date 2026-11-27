//! What an expression of a table can be (`docs/03-typescript/12-context-layer/03-design.md` §3, §7). What it needs that is not in the table it asks
//! through `World`, so a test answers by hand and the file has no tie to the engine (§15.4).

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::caps::Caps;
use crate::env::{Env, MODULE_READS_MODULE, VARIABLE_ROUNDS_CAPPED};
use crate::flow::{DefId, Expr, Flow, Part, VarId};
use crate::nested::{CaptureList, ObjectData, VALUE_WIDENED_DEPTH, VALUE_WIDENED_MEMBERS};
use crate::value::{Captures, ObjectId, One, Value};

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
    /// Whether the object `var` holds can be trusted (`trust::trust_of_component`, `08-tc4` §5): no use of it, in its whole scope and in every function it
    /// is handed to, can change its members. Kept by the world, as `relevant` is, since it is a fact of the code.
    fn trusted(&self, var: VarId) -> bool;
    /// `trusted` when the world has already worked it out, without working anything out: `trust_of_component` does not follow such a variable (its
    /// answer covers everything it reaches).
    fn trusted_known(&self, var: VarId) -> Option<bool>;
    /// `list` interned (`Nested::intern`): the number of its normal form, `None` when nothing is left, and how many inner closures were cut. It counts
    /// nothing: the caller notes the cuts, so a quiet world stays quiet.
    fn capture(&self, list: Vec<(VarId, Value)>) -> (Option<Captures>, u32);
    /// A copy of the list `captures` stands for.
    fn captured(&self, captures: Captures) -> CaptureList;
    /// An object with these members interned (`Nested::intern_object`): the number of its normal form, `None` when it has more than `MAX_MEMBERS` (the caller
    /// makes the value unknown and counts `value_widened_members`), and how many closures and objects inside it were cut. It counts nothing, as `capture` does.
    fn object(&self, members: Vec<(Box<str>, Value)>, complete: bool) -> (Option<ObjectId>, u32);
    /// A copy of the object `id` stands for.
    fn object_data(&self, id: ObjectId) -> ObjectData;
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

    fn trusted(&self, var: VarId) -> bool {
        self.0.trusted(var)
    }

    fn trusted_known(&self, var: VarId) -> Option<bool> {
        self.0.trusted_known(var)
    }

    fn capture(&self, list: Vec<(VarId, Value)>) -> (Option<Captures>, u32) {
        self.0.capture(list)
    }

    fn captured(&self, captures: Captures) -> CaptureList {
        self.0.captured(captures)
    }

    // an object must be interned the same in every round, so the quiet world asks the one it wraps
    fn object(&self, members: Vec<(Box<str>, Value)>, complete: bool) -> (Option<ObjectId>, u32) {
        self.0.object(members, complete)
    }

    fn object_data(&self, id: ObjectId) -> ObjectData {
        self.0.object_data(id)
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
        Expr::Undefined => Value::of([One::Undefined], false),
        Expr::Def(def) => function(world, env, *def, vars),
        Expr::Param(index) if *index < below => param(world, env, *index, vars),
        Expr::Param(_) => Value::unknown(),
        Expr::Anon(index) => Value::of([One::Anon(env.def, *index)], false),
        Expr::Var(id) => variable(world, env, *id, vars),
        // TC4 (`08-tc4` §3, §4): behind `OBJECTS`, which is not built yet; with it off these are unknown, as before step 2
        Expr::Member(..) | Expr::Object(_) | Expr::Omit(..) | Expr::Default(..)
            if !world.knows(Caps::OBJECTS) =>
        {
            Value::unknown()
        }
        Expr::Object(parts) => object_literal(world, env, parts, below, vars),
        Expr::Member(object, name) => {
            let value = eval_below(world, env, object, below, vars);
            member(world, &value, name)
        }
        Expr::Omit(object, names) => {
            let value = eval_below(world, env, object, below, vars);
            match (value.known(), value.is_open()) {
                ([One::Object(id)], false) => {
                    let data = world.object_data(*id);
                    let members = data
                        .members
                        .iter()
                        .filter(|(name, _)| !names.contains(name))
                        .cloned()
                        .collect();
                    made_object(world, members, data.complete)
                }
                _ => Value::unknown(),
            }
        }
        Expr::Default(value, default) => {
            let value = eval_below(world, env, value, below, vars);
            defaulted(world, value, || {
                eval_below(world, env, default, below, vars)
            })
        }
        Expr::OneOf(members) => members
            .iter()
            .map(|member| eval_below(world, env, member, below, vars))
            .reduce(|a, b| join(world, &a, &b))
            .unwrap_or_else(Value::unknown),
    }
}

/// `value` where something may not have been passed: when it holds `Undefined` or is open, `default`'s value takes `Undefined`'s place (the rest of `value`
/// stays); else `value`. One rule for a parameter's default (`passed`) and a pattern element's `= d` (`Expr::Default`).
fn defaulted(world: &dyn World, value: Value, default: impl FnOnce() -> Value) -> Value {
    if value.contains(One::Undefined) || value.is_open() {
        join(world, &value.without(One::Undefined), &default())
    } else {
        value
    }
}

/// The object with these members, as a value: interned by the world; no object (more than `MAX_MEMBERS` members) is unknown, counted
/// `value_widened_members`; each closure or object cut by the depth is counted `value_widened_depth`.
fn made_object(world: &dyn World, members: Vec<(Box<str>, Value)>, complete: bool) -> Value {
    let (made, cut) = world.object(members, complete);
    for _ in 0..cut {
        world.note(VALUE_WIDENED_DEPTH);
    }
    match made {
        Some(id) => Value::of([One::Object(id)], false),
        None => {
            world.note(VALUE_WIDENED_MEMBERS);
            Value::unknown()
        }
    }
}

/// An object literal (`08-tc4` §3): its parts in order. A later member of a name replaces an earlier one; a part the table cannot name, and a spread of
/// anything but one object or `undefined`, makes the object not complete and every member set before it open (the part may replace any of them).
fn object_literal(world: &dyn World, env: &Env, parts: &[Part], below: u32, vars: Vars) -> Value {
    let mut members: BTreeMap<Box<str>, Value> = BTreeMap::new();
    let mut complete = true;
    let open_every = |members: &mut BTreeMap<Box<str>, Value>| {
        for value in members.values_mut() {
            *value = value.join(&Value::unknown());
        }
    };
    for part in parts {
        match part {
            Part::Named(name, expr) => {
                members.insert(name.clone(), eval_below(world, env, expr, below, vars));
            }
            Part::Unnamed => {
                complete = false;
                open_every(&mut members);
            }
            Part::Spread(expr) => {
                let value = eval_below(world, env, expr, below, vars);
                match (value.known(), value.is_open()) {
                    // spreading nothing adds nothing
                    ([One::Undefined], false) => {}
                    ([One::Object(id)], false) => {
                        let data = world.object_data(*id);
                        if !data.complete {
                            complete = false;
                            open_every(&mut members);
                        }
                        for (name, value) in data.members.iter() {
                            members.insert(name.clone(), value.clone());
                        }
                    }
                    _ => {
                        complete = false;
                        open_every(&mut members);
                    }
                }
            }
        }
    }
    made_object(world, members.into_iter().collect(), complete)
}

/// What a read of `var` gives, its value being `value` (`08-tc4` §10 row 11): when the world knows `OBJECTS`, `value` holds an object and `var` is not trusted
/// (`World::trusted`; `None`, a parameter with no row, is not trusted), `value` with its objects opened (`objects_opened`); else `value`. Trust is asked
/// only of a value that holds an object.
fn as_read(world: &dyn World, var: Option<VarId>, value: Value) -> Value {
    if !world.knows(Caps::OBJECTS)
        || !value
            .known()
            .iter()
            .any(|one| matches!(one, One::Object(_)))
    {
        return value;
    }
    match var {
        Some(var) if world.trusted(var) => value,
        _ => objects_opened(world, value),
    }
}

/// `value` with each object in it opened: made again (`made_object`) with every member joined with unknown and `complete: false`, since the code may have
/// written any member or added one. Every other member of the value, and whether it is open, as it was, so a function is never opened by the trust rule.
/// The same object opened twice is the same object (an object is interned by its members).
fn objects_opened(world: &dyn World, value: Value) -> Value {
    let rest = Value::of(
        value
            .known()
            .iter()
            .filter(|one| !matches!(one, One::Object(_)))
            .copied(),
        value.is_open(),
    );
    let mut opened = rest;
    for one in value.known() {
        if let One::Object(id) = one {
            let members = world
                .object_data(*id)
                .members
                .iter()
                .map(|(name, member)| (name.clone(), member.join(&Value::unknown())))
                .collect();
            let object = made_object(world, members, false);
            opened = join(world, &opened, &object);
        }
    }
    opened
}

/// The member `name` of `value` (`08-tc4` §4): the join over its members of what each has there. An object with the name, its value; a complete one without
/// it, `undefined`; one that is not complete, unknown; `undefined`, `undefined` (reading through nothing calls nothing); any other member, unknown. A value
/// with no member (data: a string has methods) is unknown, and an open value is open.
fn member(world: &dyn World, value: &Value, name: &str) -> Value {
    let mut found: Option<Value> = None;
    for one in value.known() {
        let got = match one {
            One::Object(id) => {
                let data = world.object_data(*id);
                match data
                    .members
                    .binary_search_by(|(member, _)| member.as_ref().cmp(name))
                {
                    Ok(at) => data.members[at].1.clone(),
                    Err(_) if data.complete => Value::of([One::Undefined], false),
                    Err(_) => Value::unknown(),
                }
            }
            One::Undefined => Value::of([One::Undefined], false),
            One::Fn(_) | One::Closure(..) | One::Anon(..) | One::AnonAny => Value::unknown(),
        };
        found = Some(match found {
            Some(so_far) => join(world, &so_far, &got),
            None => got,
        });
    }
    let found = found.unwrap_or_else(Value::unknown);
    if value.is_open() {
        join(world, &found, &Value::unknown())
    } else {
        found
    }
}

/// The value of a def named in a body (`07-tc3` §4): the function itself, or, when the layer knows `CAPTURES` and the def is nested and needs variables from
/// around it, the function with the value each of them has **here**. Nothing captured, or nothing known of it, is the bare function.
///
/// The list is made from the sparse sides: `needs` of a def can be hundreds of variables (the compiler's `createTypeChecker`), and few of them have a value
/// here that is not unknown. So it walks what can have one, this body's own rows and the key's entries, and keeps those that are in `needs` (sorted: a binary
/// search). `Nested::intern` sorts the list.
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
            Some(row) => as_read(world, Some(id), row_of(world, env, row, vars)),
            None => Value::unknown(),
        };
    }
    if world.knows(Caps::CAPTURES) && id.def.index().is_some() {
        let value = env
            .key
            .captured
            .binary_search_by_key(&id, |(var, _)| *var)
            .map_or_else(|_| Value::unknown(), |at| env.key.captured[at].1.clone());
        return as_read(world, Some(id), value);
    }
    if world.knows(Caps::MODULES) && id.def.index().is_none() {
        if env.def.index().is_some() {
            return as_read(world, Some(id), world.module_value(id));
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
        Some(default) => defaulted(world, passed, || {
            eval_below(world, env, default, index, vars)
        }),
        None => passed,
    }
}

/// The value of parameter `index` in this context. A parameter the body writes holds what was passed **and** what the body assigns: its row of the table when
/// the layer knows `LOCALS` (`row_value` starts it from `passed`); before that, what was passed joined with unknown (TC2).
///
/// A named parameter is read through the trust rule (`as_read`); a pattern parameter never is: its elements are read at the entry, before any code of the body
/// runs (`08-tc4` §10, step 3).
fn param(world: &dyn World, env: &Env, index: u32, vars: Vars) -> Value {
    let value = param_value(world, env, index, vars);
    match env.flow.params.get(index as usize) {
        Some(row) if row.named => {
            let var = env.flow.param_var(index).map(|row| VarId {
                def: env.def,
                at: env.flow.vars[row].at,
            });
            as_read(world, var, value)
        }
        _ => value,
    }
}

/// `param` before the trust rule.
fn param_value(world: &dyn World, env: &Env, index: u32, vars: Vars) -> Value {
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
    use std::collections::{BTreeMap, HashMap, HashSet};

    use super::*;
    use crate::flow::{Param, Var};
    use crate::key::KeyData;
    use crate::nested::Nested;
    use crate::value::MAX_SET;

    /// A `World` answered by hand: a table per def, looked up, how many times a join passed the cap (a `Cell`, since `widened` takes `&self`), the bits the
    /// layer knows, and the counters noted.
    struct ByHand {
        flows: HashMap<DefId, Arc<Flow>>,
        widened: Cell<u32>,
        caps: Caps,
        notes: RefCell<BTreeMap<&'static str, u32>>,
        nested: RefCell<Nested>,
        /// How many times an object was asked for (`World::object`): one per object literal evaluated, so a test can count the rounds of `solve`.
        made: Cell<u32>,
        /// What `module_value` answers, by variable; unknown for one not listed.
        modules: HashMap<VarId, Value>,
        /// The variables `trusted` says no to; every other is trusted.
        untrusted: RefCell<HashSet<VarId>>,
        /// How many times `trusted` was asked.
        trust_asked: Cell<u32>,
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

        fn trusted(&self, var: VarId) -> bool {
            self.trust_asked.set(self.trust_asked.get() + 1);
            !self.untrusted.borrow().contains(&var)
        }

        fn trusted_known(&self, _var: VarId) -> Option<bool> {
            None
        }

        fn capture(&self, list: Vec<(VarId, Value)>) -> (Option<Captures>, u32) {
            self.nested.borrow_mut().intern(list)
        }

        fn captured(&self, captures: Captures) -> CaptureList {
            self.nested.borrow().get(captures).into()
        }

        fn object(
            &self,
            members: Vec<(Box<str>, Value)>,
            complete: bool,
        ) -> (Option<ObjectId>, u32) {
            self.made.set(self.made.get() + 1);
            self.nested.borrow_mut().intern_object(members, complete)
        }

        fn object_data(&self, id: ObjectId) -> ObjectData {
            self.nested.borrow().object(id).clone()
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
            uses: Default::default(),
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
                this_uses: None,
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
            nested: RefCell::new(Nested::new()),
            made: Cell::new(0),
            modules: HashMap::new(),
            untrusted: RefCell::new(HashSet::new()),
            trust_asked: Cell::new(0),
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
            uses: Default::default(),
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
            this_uses: None,
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

    // ---- TC4 step 2: objects, behind `Caps::OBJECTS` --------------------------------------------------------------------------------------------------

    fn objects_on() -> Caps {
        Caps::PARAMS | Caps::LOCALS | Caps::OBJECTS
    }

    /// An env with no parameter, and a world that knows `PARAMS`, `LOCALS` and `OBJECTS`.
    fn with_objects() -> (Env, ByHand) {
        let env = env(vec![], vec![]);
        let world = world_knowing(&env, objects_on());
        (env, world)
    }

    fn def(index: u32) -> Expr {
        Expr::Def(d(index))
    }

    fn named(name: &str, value: Expr) -> Part {
        Part::Named(name.into(), value)
    }

    fn object_expr(parts: Vec<Part>) -> Expr {
        Expr::Object(parts.into())
    }

    fn member_expr(object: Expr, name: &str) -> Expr {
        Expr::Member(Box::new(object), name.into())
    }

    /// The members and completeness of the one object `value` is.
    fn members_of(world: &ByHand, value: &Value) -> (Vec<(String, Value)>, bool) {
        match value.known() {
            [One::Object(id)] if !value.is_open() => {
                let data = world.object_data(*id);
                let members = data
                    .members
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.clone()))
                    .collect();
                (members, data.complete)
            }
            other => panic!("not one closed object: {other:?}"),
        }
    }

    fn opened(value: Value) -> Value {
        value.join(&Value::unknown())
    }

    #[test]
    fn objects_are_off_without_their_bit() {
        let env = env(vec![], vec![]);
        let world = world_knowing(&env, Caps::BUILT);
        let of = |expr: Expr| eval(&world, &env, &expr);
        let literal = || object_expr(vec![named("a", def(1))]);
        assert_eq!(of(literal()), Value::unknown());
        assert_eq!(of(member_expr(literal(), "a")), Value::unknown());
        assert_eq!(
            of(Expr::Omit(Box::new(literal()), ["a".into()].into())),
            Value::unknown()
        );
        assert_eq!(
            of(Expr::Default(Box::new(Expr::Undefined), Box::new(def(1)))),
            Value::unknown()
        );
        assert!(world.notes.borrow().is_empty());
    }

    #[test]
    fn an_object_literal_and_its_members() {
        let (env, world) = with_objects();
        let of = |expr: Expr| eval(&world, &env, &expr);
        let literal = || object_expr(vec![named("a", def(1))]);
        let value = of(literal());
        assert_eq!(
            members_of(&world, &value),
            (vec![("a".to_owned(), func(1))], true)
        );
        assert_eq!(of(member_expr(literal(), "a")), func(1));
        assert_eq!(of(member_expr(literal(), "b")), undefined());
        // the same literal twice is the same object
        assert_eq!(of(literal()), value);
        // a later member of a name replaces an earlier one
        let twice = of(object_expr(vec![named("a", def(1)), named("a", def(2))]));
        assert_eq!(members_of(&world, &twice).0, [("a".to_owned(), func(2))]);
    }

    #[test]
    fn a_spread_before_and_after() {
        let (env, world) = with_objects();
        let of = |expr: Expr| eval(&world, &env, &expr);
        let s = || object_expr(vec![named("a", def(1)), named("b", def(3))]);
        // the spread then `a`: `a` is `g`, `b` is `h`
        let after = of(object_expr(vec![Part::Spread(s()), named("a", def(2))]));
        assert_eq!(
            members_of(&world, &after),
            (
                vec![("a".to_owned(), func(2)), ("b".to_owned(), func(3))],
                true
            )
        );
        // `a`, then the spread: the spread replaces it
        let before = of(object_expr(vec![named("a", def(2)), Part::Spread(s())]));
        assert_eq!(
            members_of(&world, &before),
            (
                vec![("a".to_owned(), func(1)), ("b".to_owned(), func(3))],
                true
            )
        );
        // a spread with only `b` leaves `a` as written
        let only_b = || object_expr(vec![named("b", def(3))]);
        let kept = of(object_expr(vec![
            named("a", def(1)),
            Part::Spread(only_b()),
        ]));
        assert_eq!(
            members_of(&world, &kept),
            (
                vec![("a".to_owned(), func(1)), ("b".to_owned(), func(3))],
                true
            )
        );
        // a spread of nothing adds nothing
        let nothing = of(object_expr(vec![
            named("a", def(1)),
            Part::Spread(Expr::Undefined),
        ]));
        assert_eq!(
            members_of(&world, &nothing),
            (vec![("a".to_owned(), func(1))], true)
        );
        // a spread of something unknown: not complete, `a` may be replaced
        let unknown = object_expr(vec![named("a", def(1)), Part::Spread(Expr::Unknown)]);
        let value = of(unknown.clone());
        let (members, complete) = match value.known() {
            [One::Object(id)] => {
                let data = world.object_data(*id);
                (data.members.to_vec(), data.complete)
            }
            other => panic!("not one object: {other:?}"),
        };
        assert!(!complete);
        assert_eq!(members, [("a".into(), opened(func(1)))]);
        assert_eq!(of(member_expr(unknown, "b")), Value::unknown());
    }

    #[test]
    fn a_spread_that_is_not_one_complete_object_opens_what_came_before() {
        let (env, world) = with_objects();
        let of = |expr: Expr| eval(&world, &env, &expr);
        let data_and_function = |spread: Expr| {
            let value = of(object_expr(vec![named("a", def(1)), Part::Spread(spread)]));
            match value.known() {
                [One::Object(id)] => world.object_data(*id),
                other => panic!("not one object: {other:?}"),
            }
        };
        for spread in [
            Expr::Data,
            def(2),
            Expr::OneOf([object_expr(vec![]), object_expr(vec![named("b", def(2))])].into()),
            Expr::OneOf([Expr::Undefined, Expr::Unknown].into()),
        ] {
            let data = data_and_function(spread.clone());
            assert!(!data.complete, "{spread:?}");
            assert_eq!(
                data.members.to_vec(),
                [("a".into(), opened(func(1)))],
                "{spread:?}"
            );
        }
        // one object that is itself not complete: its members are copied, and what came before is open
        let incomplete = object_expr(vec![named("b", def(2)), Part::Unnamed]);
        let data = data_and_function(incomplete);
        assert!(!data.complete);
        assert_eq!(
            data.members.to_vec(),
            [("a".into(), opened(func(1))), ("b".into(), opened(func(2)))]
        );
    }

    #[test]
    fn an_unnamed_part_opens_what_came_before() {
        let (env, world) = with_objects();
        let value = eval(
            &world,
            &env,
            &object_expr(vec![named("a", def(1)), Part::Unnamed, named("b", def(2))]),
        );
        let data = match value.known() {
            [One::Object(id)] => world.object_data(*id),
            other => panic!("not one object: {other:?}"),
        };
        assert!(!data.complete);
        assert_eq!(
            data.members.to_vec(),
            [("a".into(), opened(func(1))), ("b".into(), func(2))]
        );
    }

    #[test]
    fn a_variable_that_holds_an_object_settles() {
        // `x = f; x = g;  y = { a: x }`
        let env = env_with_vars(
            vec![],
            vec![],
            vec![
                local(10, vec![def(1), def(2)]),
                local(20, vec![object_expr(vec![named("a", Expr::Var(vid(10)))])]),
            ],
        );
        let world = world_knowing(&env, objects_on());
        let a_of_y = eval(&world, &env, &member_expr(Expr::Var(vid(20)), "a"));
        assert_eq!(a_of_y, Value::of([One::Fn(d(1)), One::Fn(d(2))], false));
        // one object literal is evaluated once a row and round, and once more for the answer: the rounds are the calls less that one
        let rounds = world.made.get() - 1;
        let y = of_var(&world, &env, 20);
        let objects = y.known().len();
        assert_eq!(rounds, 3);
        assert!(!y.is_open());
        assert_eq!(
            objects, 2,
            "round one made `{{ a: data }}`, round two `{{ a: {{f, g}} }}`, and the value keeps both"
        );
    }

    #[test]
    fn omit_and_default() {
        let (env, world) = with_objects();
        let of = |expr: Expr| eval(&world, &env, &expr);
        let ab = || object_expr(vec![named("a", def(1)), named("b", def(3))]);
        let rest = of(Expr::Omit(Box::new(ab()), ["a".into()].into()));
        assert_eq!(
            members_of(&world, &rest),
            (vec![("b".to_owned(), func(3))], true)
        );
        // an object that was not complete stays so
        let incomplete = of(Expr::Omit(
            Box::new(object_expr(vec![named("a", def(1)), Part::Unnamed])),
            ["a".into()].into(),
        ));
        match incomplete.known() {
            [One::Object(id)] => assert!(!world.object_data(*id).complete),
            other => panic!("not one object: {other:?}"),
        }
        // anything but one closed object is unknown
        assert_eq!(
            of(Expr::Omit(Box::new(def(1)), ["a".into()].into())),
            Value::unknown()
        );
        assert_eq!(
            of(Expr::Omit(Box::new(Expr::Unknown), ["a".into()].into())),
            Value::unknown()
        );
        let default = |value: Expr| of(Expr::Default(Box::new(value), Box::new(def(2))));
        // nothing there: the default
        assert_eq!(
            default(member_expr(object_expr(vec![]), "a")),
            func(2),
            "a missing member of a complete object is undefined"
        );
        // something there: it
        assert_eq!(
            default(member_expr(object_expr(vec![named("a", def(1))]), "a")),
            func(1)
        );
        // nothing known: the default, and open
        assert_eq!(default(Expr::Unknown), opened(func(2)));
        // a value that is not nothing is left as it is (`null` is data)
        assert_eq!(default(Expr::Data), Value::data());
    }

    /// A parameter's default and a pattern element's default are one rule (`defaulted`): the same answers for the same value.
    #[test]
    fn a_parameters_default_and_an_elements_default_are_one_rule() {
        for key in [
            undefined(),
            func(1),
            Value::unknown(),
            Value::data(),
            Value::of([One::Fn(d(1)), One::Undefined], false),
        ] {
            let env = env(vec![row(false, Some(def(5)), false)], vec![key.clone()]);
            let world = world_knowing(&env, objects_on());
            let as_parameter = eval(&world, &env, &Expr::Param(0));
            let as_element = defaulted(&world, key.clone(), || func(5));
            assert_eq!(as_parameter, as_element, "{key:?}");
        }
    }

    #[test]
    fn a_member_of_other_values() {
        // the parameter has a row of variables: a parameter with none is not trusted, and an object read through it is open
        let base = env_with_vars(
            vec![row(false, None, false)],
            vec![],
            vec![var_row(5, Some(0), true, vec![], false)],
        );
        let world = world_knowing(&base, objects_on());
        let object = |members: Vec<(&str, Value)>| {
            let members = members.into_iter().map(|(n, v)| (n.into(), v)).collect();
            world.object(members, true).0.unwrap()
        };
        let (o1, o2) = (object(vec![("a", func(1))]), object(vec![("a", func(2))]));
        // `a` of a value held in the parameter slot of the key
        let of = |held: Value| {
            let env = Env::new(
                Arc::clone(&base.flow),
                base.def,
                KeyData {
                    params: vec![held].into(),
                    captured: Box::new([]),
                },
            );
            eval(&world, &env, &member_expr(Expr::Param(0), "a"))
        };
        // a function, and data, have members we do not know
        assert_eq!(of(func(9)), Value::unknown());
        assert_eq!(of(Value::data()), Value::unknown());
        // nothing: reading through it gives nothing
        assert_eq!(of(undefined()), undefined());
        // two objects: the join
        assert_eq!(
            of(Value::of([One::Object(o1), One::Object(o2)], false)),
            Value::of([One::Fn(d(1)), One::Fn(d(2))], false)
        );
        // an object that may be something else too
        assert_eq!(of(Value::of([One::Object(o1)], true)), opened(func(1)));
        // an object and nothing: the member, or `undefined`
        assert_eq!(
            of(Value::of([One::Object(o1), One::Undefined], false)),
            Value::of([One::Fn(d(1)), One::Undefined], false)
        );
        // an unknown value
        assert_eq!(of(Value::unknown()), Value::unknown());
    }

    #[test]
    fn objects_nest_two_deep() {
        let (env, world) = with_objects();
        let of = |expr: Expr| eval(&world, &env, &expr);
        let counted = || world.notes.borrow().clone();
        // `{x: {y: f}}` keeps `y`
        let two = object_expr(vec![named("x", object_expr(vec![named("y", def(1))]))]);
        assert_eq!(of(member_expr(member_expr(two.clone(), "x"), "y")), func(1));
        assert!(counted().is_empty());
        // `{x: {y: {z: f}}}` is one level too deep: `x`, the object of depth 2, is removed from its member, which is open, and the cut is counted once
        let three = object_expr(vec![named(
            "x",
            object_expr(vec![named("y", object_expr(vec![named("z", def(1))]))]),
        )]);
        assert_eq!(of(member_expr(three.clone(), "x")), Value::unknown());
        assert_eq!(counted(), BTreeMap::from([(VALUE_WIDENED_DEPTH, 1)]));
    }

    #[test]
    fn one_depth_for_lists_and_objects_in_both_directions() {
        let (_, world) = with_objects();
        let (inner, _) = world.object(vec![("y".into(), func(1))], true);
        let (middle, _) = world.object(
            vec![("x".into(), Value::of([One::Object(inner.unwrap())], false))],
            true,
        );
        let middle = Value::of([One::Object(middle.unwrap())], false);
        // a captured value that is an object of depth 2 makes a list of depth 3: removed, counted once
        let var = |at: u32| VarId { def: d(8), at };
        assert_eq!(world.capture(vec![(var(1), middle.clone())]), (None, 1));
        let (made, cut) = world.capture(vec![(var(1), middle), (var(2), func(2))]);
        assert_eq!(cut, 1);
        assert_eq!(world.captured(made.unwrap()).to_vec(), [(var(2), func(2))]);
        // a closure whose list is depth 2 as a member makes an object of depth 3: written as its function, counted once
        let (one, _) = world.capture(vec![(var(1), func(1))]);
        let (two, _) = world.capture(vec![(
            var(1),
            Value::of([One::Closure(d(2), one.unwrap())], false),
        )]);
        let held = Value::of([One::Closure(d(3), two.unwrap())], false);
        let (made, cut) = world.object(vec![("k".into(), held)], true);
        assert_eq!(cut, 1);
        assert_eq!(world.object_data(made.unwrap()).members[0].1, func(3));
    }

    #[test]
    fn too_many_members_is_unknown() {
        let literal =
            |n: usize| object_expr((0..n).map(|i| named(&format!("k{i}"), def(1))).collect());
        let (env, world) = with_objects();
        let value = eval(&world, &env, &literal(64));
        assert!(matches!(value.known(), [One::Object(_)]));
        assert!(world.notes.borrow().is_empty());
        let value = eval(&world, &env, &literal(65));
        assert_eq!(value, Value::unknown());
        assert_eq!(
            *world.notes.borrow(),
            BTreeMap::from([(VALUE_WIDENED_MEMBERS, 1)])
        );
    }

    // ---- TC4 step 3b: an object that cannot be trusted is read open ---------------------------------------------------------------------------------

    /// An env with one named, capable parameter whose row of variables is at 5 (`vid(5)`), the key holding `held` for it.
    fn param_env(held: Value) -> Env {
        env_with_vars(
            vec![row(false, None, false)],
            vec![held],
            vec![var_row(5, Some(0), true, vec![], false)],
        )
    }

    /// A world for `param_env`, and the object `{a: f1}` interned in it.
    fn world_and_object(caps: Caps) -> (ByHand, ObjectId) {
        let world = world_knowing(&param_env(Value::unknown()), caps);
        let (made, _) = world.object(vec![("a".into(), func(1))], true);
        (world, made.unwrap())
    }

    #[test]
    fn an_untrusted_parameter_gives_its_objects_opened() {
        let (world, o) = world_and_object(objects_on());
        world.untrusted.borrow_mut().insert(vid(5));
        let env = param_env(Value::of([One::Object(o)], false));
        let read = eval(&world, &env, &Expr::Param(0));
        assert_eq!(
            members_of(&world, &read),
            (vec![("a".to_owned(), opened(func(1)))], false)
        );
        assert_eq!(
            eval(&world, &env, &member_expr(Expr::Param(0), "a")),
            opened(func(1))
        );
        // a member that is not there reads unknown: the object is not complete
        assert_eq!(
            eval(&world, &env, &member_expr(Expr::Param(0), "b")),
            Value::unknown()
        );
        // the slot holds the function and the object: the function stays, the value stays closed, the object is opened
        let both = param_env(Value::of([One::Fn(d(1)), One::Object(o)], false));
        let read = eval(&world, &both, &Expr::Param(0));
        assert!(!read.is_open());
        assert!(read.contains(One::Fn(d(1))));
        let objects: Vec<ObjectId> = read
            .known()
            .iter()
            .filter_map(|one| match one {
                One::Object(id) => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(objects.len(), 1);
        assert_ne!(objects[0], o);
        let data = world.object_data(objects[0]);
        assert!(!data.complete);
        assert_eq!(data.members.to_vec(), [("a".into(), opened(func(1)))]);
    }

    #[test]
    fn a_trusted_parameter_gives_its_object_as_it_is() {
        let (world, o) = world_and_object(objects_on());
        let env = param_env(Value::of([One::Object(o)], false));
        assert_eq!(
            eval(&world, &env, &member_expr(Expr::Param(0), "a")),
            func(1)
        );
        assert!(world.trust_asked.get() > 0);
    }

    #[test]
    fn a_value_with_no_object_is_not_asked_and_not_changed() {
        let (world, _) = world_and_object(objects_on());
        world.untrusted.borrow_mut().insert(vid(5));
        let env = param_env(func(1));
        assert_eq!(eval(&world, &env, &Expr::Param(0)), func(1));
        assert_eq!(world.trust_asked.get(), 0);
    }

    #[test]
    fn a_pattern_parameter_is_never_opened() {
        let (world, o) = world_and_object(objects_on());
        // every variable of the test is untrusted
        for at in 0..10 {
            world.untrusted.borrow_mut().insert(vid(at));
        }
        let env = env(
            vec![Param {
                named: false,
                ..row(false, None, false)
            }],
            vec![Value::of([One::Object(o)], false)],
        );
        assert_eq!(
            eval(&world, &env, &member_expr(Expr::Param(0), "a")),
            func(1)
        );
        assert_eq!(world.trust_asked.get(), 0);
    }

    #[test]
    fn with_objects_off_nothing_is_opened_or_asked() {
        let (world, o) = world_and_object(Caps::PARAMS | Caps::LOCALS);
        world.untrusted.borrow_mut().insert(vid(5));
        let held = Value::of([One::Object(o)], false);
        let env = param_env(held.clone());
        assert_eq!(eval(&world, &env, &Expr::Param(0)), held);
        assert_eq!(world.trust_asked.get(), 0);
    }

    #[test]
    fn a_second_name_reads_the_object_its_first_name_gave() {
        let (world, o) = world_and_object(objects_on());
        // `x` is parameter 0 (row 5); `o = x` (row 6); `x` is untrusted, `o` is not
        world.untrusted.borrow_mut().insert(vid(5));
        let env = env_with_vars(
            vec![row(false, None, false)],
            vec![Value::of([One::Object(o)], false)],
            vec![
                var_row(5, Some(0), true, vec![], false),
                local(6, vec![Expr::Param(0)]),
            ],
        );
        assert_eq!(
            eval(&world, &env, &member_expr(Expr::Var(vid(6)), "a")),
            opened(func(1))
        );
    }

    #[test]
    fn a_captured_variable_is_opened_by_its_declaring_defs_trust() {
        let (world, o) =
            world_and_object(Caps::PARAMS | Caps::LOCALS | Caps::CAPTURES | Caps::OBJECTS);
        let env = Env::new(
            Arc::new(Flow {
                params: Box::new([]),
                sites: Box::new([]),
                anons: Box::new([]),
                handed: Box::new([]),
                unhandled: 0,
                vars: Box::new([]),
                unmodelled: 0,
                around: Some(d(8)),
                this_uses: None,
            }),
            d(9),
            KeyData {
                params: Box::new([]),
                captured: Box::new([(outer(5), Value::of([One::Object(o)], false))]),
            },
        );
        let read = || eval(&world, &env, &member_expr(Expr::Var(outer(5)), "a"));
        assert_eq!(read(), func(1));
        world.untrusted.borrow_mut().insert(outer(5));
        assert_eq!(read(), opened(func(1)));
    }

    #[test]
    fn a_module_variable_is_opened_when_not_trusted() {
        let (mut world, o) =
            world_and_object(Caps::PARAMS | Caps::LOCALS | Caps::MODULES | Caps::OBJECTS);
        let top = VarId {
            def: DefId::new(0, 0, None),
            at: 3,
        };
        world
            .modules
            .insert(top, Value::of([One::Object(o)], false));
        let env = env(vec![], vec![]);
        let read = || eval(&world, &env, &member_expr(Expr::Var(top), "a"));
        assert_eq!(read(), func(1));
        world.untrusted.borrow_mut().insert(top);
        assert_eq!(read(), opened(func(1)));
    }

    #[test]
    fn opening_twice_is_the_same_object() {
        let (world, o) = world_and_object(objects_on());
        let once = objects_opened(&world, Value::of([One::Fn(d(2)), One::Object(o)], false));
        assert_eq!(objects_opened(&world, once.clone()), once);
    }
}
