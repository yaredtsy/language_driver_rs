//! What is known on the path to a call (`docs/03-typescript/12-context-layer/03-design.md` §5): built by `Lang::walk` for one body when the context
//! layer is on, and absent when it is off. In TC1 it held the body's table and nothing else; TC2 adds the def and the key the body is walked under.

use std::cell::OnceCell;
use std::sync::Arc;

use crate::flow::{DefId, Flow};
use crate::key::KeyData;
use crate::value::Value;

/// The key of the counter in `Stats.language` for a write to a variable that the table has no row for (`Flow::unmodelled`): counted once for each, by the
/// body's first walk.
pub(crate) const VARIABLE_WRITE_UNMODELLED: &str = "variable_write_unmodelled";

/// The key of the counter for a body whose variables did not settle within `eval::MAX_ROUNDS` rounds: they are then all open, and this counts the body once for
/// each context that did it.
pub(crate) const VARIABLE_ROUNDS_CAPPED: &str = "variable_rounds_capped";

pub(crate) struct Env {
    /// The table of the body being walked (`Sem::flow`).
    pub(crate) flow: Arc<Flow>,
    /// The def whose body it is: what `Expr::Anon` names.
    pub(crate) def: DefId,
    /// What the context's key holds (`Keys::data`): what `Expr::Param` reads. Under the open key, every slot is unknown.
    pub(crate) key: KeyData,
    /// The value of each row of `flow.vars` in this context (`07-tc3` §3): solved together the first time the walk reads a variable (`eval::solve`), then
    /// kept for the walk.
    ///
    /// 🦀 A `OnceCell` is a slot that is empty until the first `get_or_init`, which fills it and gives a shared reference; later calls give the same one.
    /// It is what lets `eval`, which only has `&Env`, compute this once without `&mut`.
    pub(crate) vars: OnceCell<Box<[Value]>>,
}

impl Env {
    /// An env whose variables are not solved yet.
    pub(crate) fn new(flow: Arc<Flow>, def: DefId, key: KeyData) -> Env {
        Env {
            flow,
            def,
            key,
            vars: OnceCell::new(),
        }
    }
}
