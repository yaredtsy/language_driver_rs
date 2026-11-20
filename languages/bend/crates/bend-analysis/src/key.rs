//! What a context holds besides the def. Empty on purpose: Bend is not context sensitive yet, so one context is one def (`docs/08-bend/04-phase-3-context-layer.md`).
//! Changing this line to a real type is how context sensitivity starts; the solver compiles unchanged (`docs/02-tree-rules/03-the-seam.md` §3).

pub(crate) type Key = ();
