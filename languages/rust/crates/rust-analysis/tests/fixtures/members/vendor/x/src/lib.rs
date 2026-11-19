//! A crate the workspace excludes (fix 4c, `tests/refresh.rs`): it is under the root, has a manifest, and is no package's of the load. It is there before
//! the session opens, so it is no new folder when the root changes.

pub fn x() {}
