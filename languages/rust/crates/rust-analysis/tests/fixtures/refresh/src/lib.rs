//! The workspace of RS5 step 3 (`tests/refresh.rs`): `entry` calls `callee`, which lives in another file, so a test can rewrite, delete or add the files
//! a graph depends on. `callee` calls `first`; the tests make it call `second`.

mod callee;
mod nested;

pub fn entry() {
    callee::callee();
}
