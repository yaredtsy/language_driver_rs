//! What a context holds besides the function. Empty on purpose: Rust is not context sensitive yet, so one context is one function
//! (`docs/04-rust/02-architecture.md` §6). Changing this line to a real type is how context sensitivity starts; the solver compiles unchanged.

pub(crate) type Key = ();
