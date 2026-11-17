//! The engine module (rule 4, D35): the only files of this crate that name `ra_ap_*`, and no engine type leaves them. `mod.rs` exports by name.

mod bridge;
mod load;
mod origin;
mod sites;

#[expect(
    unused_imports,
    reason = "RS4's calls.rs and walk.rs use them (steps 3 and 4)"
)]
pub(crate) use bridge::{Bridged, Func, Sem, attached};
pub(crate) use load::{Loaded, Rung, load};
#[expect(unused_imports, reason = "RS4's calls.rs uses them (step 4)")]
pub(crate) use origin::{Cause, Item, Owner};

// the helpers of the integration tests, which are one place for "copy a fixture under target/" (that file says `#![allow(dead_code)]`: each program
// uses some of it). A unit test cannot `use` code from `tests/`, so the file is included by path, once, for every test module of `sem`.
#[cfg(test)]
#[path = "../../tests/common/mod.rs"]
mod common;

#[cfg(test)]
mod testing;
