//! `rust-cli calls` (RS4 step 4): the tree and the breakdown, a qualname of several defs listed and never picked (D15), and the usage errors.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A copy of the policy fixture of `rust-analysis` under this crate's scratch folder: a load may write into a project, and the repo is not written.
fn policy_copy() -> PathBuf {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
    }
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../rust-analysis/tests/fixtures/policy");
    let to = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cli_policy");
    let _ = std::fs::remove_dir_all(&to);
    copy(&from, &to);
    to
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_tree_then_the_breakdown() {
    let dir = policy_copy();
    let out = cli(&[
        "calls",
        dir.to_str().unwrap(),
        "src/calls.rs",
        "--entry",
        "plain_calls",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.starts_with("plain_calls\n  twice\n  helper\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("breakdown:\n  edges: 2 call, 0 construct, 0 external, 0 unresolved"),
        "{stdout}"
    );
}

/// D15: two defs, one qualname: both are listed on stderr, the exit code is 2, and the first is not picked. `--occurrence` and `--id` pick one.
#[test]
fn a_qualname_of_several_defs_is_listed_and_none_is_picked() {
    let dir = policy_copy();
    let dir = dir.to_str().unwrap();
    let out = cli(&["calls", dir, "src/shapes.rs", "--entry", "Rec::get"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("matches 2 defs"), "{stderr}");
    assert!(stderr.contains("#0") && stderr.contains("#1"), "{stderr}");
    assert!(text(&out.stdout).is_empty());

    let out = cli(&[
        "calls",
        dir,
        "src/shapes.rs",
        "--entry",
        "Rec::get",
        "--occurrence",
        "1",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let out = cli(&[
        "calls",
        dir,
        "src/lib.rs",
        "--entry",
        "twin_a",
        "--id",
        "twin",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
}

#[test]
fn what_cannot_start_a_graph_and_bad_usage_are_said() {
    let dir = policy_copy();
    let dir = dir.to_str().unwrap();
    // a trait cannot be an entry: exit 1 with the reason
    let out = cli(&["calls", dir, "src/lib.rs", "--entry", "Writer"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("cannot start a graph"),
        "{}",
        text(&out.stderr)
    );
    // a file the load does not hold
    let out = cli(&["calls", dir, "src/stray.rs", "--entry", "stray"]);
    assert_eq!(out.status.code(), Some(1));
    // no `--entry`: nothing runs at the top of a Rust file, so there is no file entry
    let out = cli(&["calls", dir, "src/lib.rs"]);
    assert_eq!(out.status.code(), Some(2));
    // two ways to pick a def
    let out = cli(&[
        "calls",
        dir,
        "src/lib.rs",
        "--entry",
        "plain",
        "--id",
        "x",
        "--occurrence",
        "0",
    ]);
    assert_eq!(out.status.code(), Some(2));
}
