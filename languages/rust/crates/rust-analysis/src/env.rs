//! What is known on the path to a call: names to values. Empty on purpose (`docs/02-tree-rules/03-the-seam.md` §3). `walk::resolve` already takes one,
//! so the day something is tracked (a variable assigned from `Foo::new()` in this body), `walk::resolve_from_env` learns that case and nothing else
//! moves.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "RS4 step 4's calls.rs builds the Env for each walk"
    )
)]

pub(crate) struct Env;
