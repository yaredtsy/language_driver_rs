//! What is known on the path to a call (`docs/03-typescript/12-context-layer/03-design.md` §5): built by `Lang::walk` for one body when the context
//! layer is on, and absent when it is off. In TC1 it holds the body's table and nothing else; TC2 adds the key the body is walked under.

use std::sync::Arc;

use crate::flow::Flow;

pub(crate) struct Env {
    /// The table of the body being walked (`Sem::flow`).
    pub(crate) flow: Arc<Flow>,
}
