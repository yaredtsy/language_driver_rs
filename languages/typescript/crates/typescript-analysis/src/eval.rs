//! What an expression of a table can be (`docs/03-typescript/12-context-layer/03-design.md` §3, §7). What it needs that is not in the table it asks
//! through `World`, so a test answers by hand and the file has no tie to the engine (§15.4).

use std::sync::Arc;

use crate::env::Env;
use crate::flow::{DefId, Expr, Flow};
use crate::value::Value;

/// What the evaluator asks that is not in a `Flow`. `TypeScript` answers it (`calls.rs`); a test answers it by hand. `member` comes with TC5.
pub(crate) trait World {
    /// The table of `def`'s body; `None` when it has none.
    fn flow(&self, def: DefId) -> Option<Arc<Flow>>;
}

/// What `expr` can be, in `env`. TC1 knows nothing: every expression is `Value::unknown()`, so every site gets the engine's answer. One arm per
/// variant and no `_`, so a variant a later phase adds does not compile until this decides it.
pub(crate) fn eval(_world: &impl World, _env: &Env, expr: &Expr) -> Value {
    match expr {
        Expr::Unknown => Value::unknown(),
        Expr::Data => Value::unknown(),
        Expr::Def(_) => Value::unknown(),
        // step 3: the key's value for that parameter
        Expr::Param(_) => Value::unknown(),
        Expr::Anon(_) => Value::unknown(),
        Expr::OneOf(_) => Value::unknown(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// A `World` answered by hand: a table per def, looked up.
    struct ByHand(HashMap<DefId, Arc<Flow>>);

    impl World for ByHand {
        fn flow(&self, def: DefId) -> Option<Arc<Flow>> {
            self.0.get(&def).map(Arc::clone)
        }
    }

    #[test]
    fn every_expression_is_unknown_in_tc1() {
        let def = DefId::new(0, 0, Some(0));
        let flow = Arc::new(Flow {
            params: Box::new([]),
            sites: Box::new([]),
            anons: Box::new([]),
            handed: Box::new([]),
            unhandled: 0,
        });
        let world = ByHand(HashMap::from([(def, Arc::clone(&flow))]));
        // the hand-made world answers, so it is a world a test can use
        assert!(world.flow(def).is_some());
        assert!(world.flow(DefId::new(0, 0, None)).is_none());
        let env = Env { flow };
        let exprs = [
            Expr::Unknown,
            Expr::Data,
            Expr::Def(def),
            Expr::Param(0),
            Expr::Anon(0),
            Expr::OneOf([Expr::Def(def), Expr::Data].into()),
        ];
        for expr in &exprs {
            assert_eq!(eval(&world, &env, expr), Value::unknown(), "{expr:?}");
        }
    }
}
