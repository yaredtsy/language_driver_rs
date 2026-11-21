//! Folders the in-crate tests write books to (`CARGO_TARGET_TMPDIR` does not exist for unit tests, so the temp dir).
//!
//! The folders are all made once, together, before any test asks for one, and a test only empties its own: tests run in parallel, and none of them creates or
//! removes a folder beside another's.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Every folder a test may ask `scratch` for.
const NAMES: [&str; 7] = [
    "key-canon",
    "key-order",
    "key-relevant",
    "stamps-kinds",
    "stamps-twice",
    "stamps-moves",
    "rebuild-count",
];

/// The folder of `name`, empty, and canonical (the OS may spell the temp dir through a link: `/var` is `/private/var` on macOS, and the book's paths are the
/// real ones).
pub(crate) fn scratch(name: &str) -> PathBuf {
    static MADE: OnceLock<PathBuf> = OnceLock::new();
    let parent = MADE.get_or_init(|| {
        let parent = std::env::temp_dir().join("bend-analysis-unit");
        for name in NAMES {
            std::fs::create_dir_all(parent.join(name)).unwrap();
        }
        parent.canonicalize().unwrap()
    });
    assert!(NAMES.contains(&name), "`{name}` is not in scratch::NAMES");
    let dir = parent.join(name);
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            std::fs::remove_dir_all(path).unwrap();
        } else {
            std::fs::remove_file(path).unwrap();
        }
    }
    dir
}

/// Writes `text` to `path`. The one place the in-crate tests write a file, so that `scripts/check-all.sh` (rule 2: a library never writes) has one test-only file to
/// leave out and not every `#[cfg(test)]` block of `src`.
pub(crate) fn write(path: impl AsRef<Path>, text: &str) {
    std::fs::write(path, text).unwrap();
}
