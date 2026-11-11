//! The shared tests: fixture runner, marker round trip, never panics, scenario runner, graph invariants.
//!
//! A dev-dependency only. A language's test file is a constant and a few one-line tests; the checks live here once.
//!
//! Expected files (outline snapshots, `expected.*.tree`) are compared, never rewritten, unless the environment variable
//! `ANALYZER_BLESS=1` is set. Blessing is for the person writing a test: read `git diff` before you commit it.

pub mod graph;
pub mod scenarios;
pub mod syntax;

use std::fs;
use std::path::Path;

/// Compare `actual` with the file at `path`. A mismatch panics with the first differing line.
pub(crate) fn expect_file(path: &Path, actual: &str) {
    if std::env::var_os("ANALYZER_BLESS").is_some_and(|v| v == "1") {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).expect("create the directory of an expected file");
        }
        fs::write(path, actual).expect("write an expected file");
        return;
    }
    let expected = fs::read_to_string(path).unwrap_or_else(|_| {
        panic!(
            "no expected file {}\n  run with ANALYZER_BLESS=1 to write it, then review it.\n--- actual ---\n{actual}",
            path.display()
        )
    });
    if expected != actual {
        panic!(
            "{} differs ({})\n  run with ANALYZER_BLESS=1 to rewrite it, then review the diff.\n--- expected ---\n{expected}--- actual ---\n{actual}",
            path.display(),
            first_difference(&expected, actual)
        );
    }
}

fn first_difference(expected: &str, actual: &str) -> String {
    for (n, (e, a)) in expected.lines().zip(actual.lines()).enumerate() {
        if e != a {
            return format!("line {}: expected {e:?}, got {a:?}", n + 1);
        }
    }
    format!(
        "expected {} lines, got {}",
        expected.lines().count(),
        actual.lines().count()
    )
}

#[cfg(test)]
mod toy;
