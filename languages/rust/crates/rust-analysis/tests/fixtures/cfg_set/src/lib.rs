//! The `cfg` set a load reads (D48): `debug_assertions` is on, `miri` is off.

fn leaf() {}

#[cfg(debug_assertions)]
fn dbg_only() {
    leaf();
}

#[cfg(not(debug_assertions))]
fn release_only() {
    leaf();
}

#[cfg(miri)]
fn under_miri() {
    leaf();
}

#[cfg(not(miri))]
fn not_miri() {
    leaf();
}

/// Calls all four: the two that are off are no defs, so their calls have no target.
pub fn entry() {
    dbg_only();
    release_only();
    under_miri();
    not_miri();
}
