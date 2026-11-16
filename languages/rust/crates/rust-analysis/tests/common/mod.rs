//! Helpers shared by the test files of this crate. Each file under `tests/` is its own program, so each says `mod common;`.
#![allow(dead_code, reason = "each test program uses some of these")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every file under `dir` with its text (lossy: all of them are text, and a failed comparison is then readable), by path relative to `dir`
/// with `/`.
pub(crate) fn snapshot(dir: &Path) -> BTreeMap<String, String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let parts: Vec<_> = path
                    .strip_prefix(base)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_str().unwrap())
                    .collect();
                out.insert(
                    parts.join("/"),
                    String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

pub(crate) fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A fresh copy of `tests/fixtures/<fixture>` under `target/`, named `name`. A load that writes into the project then leaves nothing in the
/// repo, and the copy is inside the repo's folder tree, so `rust-toolchain.toml` still selects the toolchain.
pub(crate) fn copy_of_fixture(fixture: &str, name: &str) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let to = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&to);
    copy_dir(&from, &to);
    to
}

/// Every `.rs` file under `dir`, relative to `base` with `/`, found by walking the disk (not through the engine), skipping `target/`.
pub(crate) fn rs_files(base: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                rs_files(base, &path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            let parts: Vec<_> = path
                .strip_prefix(base)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_str().unwrap())
                .collect();
            out.push(parts.join("/"));
        }
    }
}
