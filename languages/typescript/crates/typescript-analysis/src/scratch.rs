//! Folders the in-crate tests write projects to (`CARGO_TARGET_TMPDIR` does not exist for unit tests, so the temp dir).
//!
//! A session stamps every directory the build looked in, and the directories **above** the project are among them (`sem` `Tracker`): a folder created beside the
//! project, in one of them, moves its stamp. Tests run in parallel, so a test that makes its folder while another waits for "nothing changed" would fail it
//! for nothing. The folders are therefore all made once, together, before any test asks for one, and a test only empties its own.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Every folder a test may ask `scratch` for.
const NAMES: [&str; 4] = ["seen-paths", "stamps", "during-a-build", "lost-no-rebuild"];

/// The folder of `name`, empty, and canonical (the OS may spell the temp dir through a link, `/var` is `/private/var` on macOS, and the engine's paths are the
/// canonical ones).
pub(crate) fn scratch(name: &str) -> PathBuf {
    static MADE: OnceLock<PathBuf> = OnceLock::new();
    let parent = MADE.get_or_init(|| {
        let parent = std::env::temp_dir().join("typescript-analysis-unit");
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
