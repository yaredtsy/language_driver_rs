//! What is known on the path to a call: names to values. Empty on purpose (`docs/02-tree-rules/03-the-seam.md` §3). `walk::resolve` already takes one, so the
//! day something is tracked (a template `~f` bound to the def a caller passed), `walk::resolve_from_env` learns that case and nothing else moves.

pub(crate) struct Env;
