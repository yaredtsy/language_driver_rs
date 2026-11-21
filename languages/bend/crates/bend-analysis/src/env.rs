//! What is known on the path to a call (`docs/02-tree-rules/03-the-seam.md` §3, `docs/08-bend/06-context-layer-design.md` §5). Bend has no `let` of a function to track
//! inside one body in this step, so the env **is** the context's key seen as a map: what the callers named for the def's parameters. This file stays the one place a
//! later phase would add locals (BC3).

use crate::key::{Key, Known};

pub(crate) struct Env<'k> {
    key: &'k Key,
}

impl<'k> Env<'k> {
    pub(crate) fn new(key: &'k Key) -> Self {
        Env { key }
    }

    /// What the context's callers put in the def's parameter at `position`, when they named it.
    pub(crate) fn known(&self, position: u32) -> Option<&'k Known> {
        self.key.known(position)
    }
}
