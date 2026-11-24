//! What an expression of a table can be (`docs/03-typescript/12-context-layer/03-design.md` §3, §7). What it needs that is not in the table it asks
//! through `World`, so a test answers by hand and the file has no tie to the engine (§15.4).

use std::sync::Arc;

use crate::env::Env;
use crate::flow::{DefId, Expr, Flow};
use crate::value::{One, Value};

/// What the evaluator asks that is not in a `Flow`. `TypeScript` answers it (`calls.rs`); a test answers it by hand. `member` comes with TC5.
pub(crate) trait World {
    /// The table of `def`'s body; `None` when it has none.
    fn flow(&self, def: DefId) -> Option<Arc<Flow>>;
}

/// What `expr` can be, in `env` (`06-tc2` §3, step 3). One arm per variant and no `_`, so a variant a later phase adds does not compile until this
/// decides it.
pub(crate) fn eval(_world: &impl World, env: &Env, expr: &Expr) -> Value {
    eval_below(env, expr, u32::MAX)
}

/// `eval`, where a parameter at place `below` or after it is unknown: a default sees only the parameters before its own (a later one is not bound yet,
/// and its own name would loop: `function f(a = a)`).
fn eval_below(env: &Env, expr: &Expr, below: u32) -> Value {
    match expr {
        Expr::Unknown => Value::unknown(),
        Expr::Data => Value::data(),
        Expr::Def(def) => Value::of([One::Fn(*def)], false),
        Expr::Param(index) if *index < below => param(env, *index),
        Expr::Param(_) => Value::unknown(),
        Expr::Anon(index) => Value::of([One::Anon(env.def, *index)], false),
        Expr::OneOf(members) => members
            .iter()
            .map(|member| eval_below(env, member, below))
            .reduce(|a, b| a.join(&b))
            .unwrap_or_else(Value::unknown),
    }
}

/// The value of parameter `index` in this context: what the key holds for it, unknown for a rest parameter or a slot the key does not list; where nothing
/// may have been passed (the key holds `Undefined`, or is open) and there is a default, the default's value takes `Undefined`'s place; joined with unknown
/// when the body writes the parameter (TC3 reads the writes). Whether a slot may hold a value at all (`capable`) is the key's rule (`admit`, step 4), not
/// this.
fn param(env: &Env, index: u32) -> Value {
    let Some(row) = env.flow.params.get(index as usize) else {
        return Value::unknown();
    };
    if row.rest {
        return Value::unknown();
    }
    // a key lists no trailing unknowns (`KeyData::normal`), so a slot it does not list is unknown
    let passed = env
        .key
        .params
        .get(index as usize)
        .cloned()
        .unwrap_or_else(Value::unknown);
    let value = match &row.default {
        Some(default) if passed.contains(One::Undefined) || passed.is_open() => passed
            .without(One::Undefined)
            .join(&eval_below(env, default, index)),
        _ => passed,
    };
    if row.assigned {
        value.join(&Value::unknown())
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::flow::Param;
    use crate::key::KeyData;

    /// A `World` answered by hand: a table per def, looked up.
    struct ByHand(HashMap<DefId, Arc<Flow>>);

    impl World for ByHand {
        fn flow(&self, def: DefId) -> Option<Arc<Flow>> {
            self.0.get(&def).map(Arc::clone)
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
        Env {
            flow: Arc::new(Flow {
                params: rows.into(),
                sites: Box::new([]),
                anons: Box::new([]),
                handed: Box::new([]),
                unhandled: 0,
            }),
            def: d(9),
            key: KeyData { params: key.into() },
        }
    }

    /// The world is not asked in this step; a hand-made one answers, so it is a world a test can use.
    fn world(env: &Env) -> ByHand {
        let world = ByHand(HashMap::from([(env.def, Arc::clone(&env.flow))]));
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
}
