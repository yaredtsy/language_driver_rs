//! What a context holds besides the function. Empty on purpose: Rust is not context sensitive yet, so one context is one function
//! (`docs/04-rust/02-architecture.md` §6). Changing this line to a real type is how context sensitivity starts; the solver compiles unchanged.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "RS4 step 4's calls.rs names the key of its `Lang`"
    )
)]

pub(crate) type Key = ();
