//! What a context holds besides the function. Empty on purpose: TypeScript is not context sensitive yet, so one context is one function
//! (`docs/02-tree-rules/03-the-seam.md` §3). Changing this line to a real type is how context sensitivity starts; the solver compiles unchanged.

pub(crate) type Key = ();
