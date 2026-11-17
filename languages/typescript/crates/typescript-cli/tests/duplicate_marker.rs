//! H9: `calls … --entry <qualname> --id <marker>` picks the def that **owns** the marker (D34: the first in the file); a def that only carries the same text is
//! not picked, and the message says which def owns it.

use std::path::PathBuf;
use std::process::{Command, Output};

fn calls(args: &[&str]) -> Output {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../typescript-analysis/tests/fixtures/duplicate-marker");
    Command::new(env!("CARGO_BIN_EXE_typescript-cli"))
        .arg("calls")
        .arg(root)
        .arg("src/main.ts")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn id_picks_the_def_that_owns_the_marker() {
    let out = calls(&["--entry", "a", "--id", "SAME"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .starts_with("a\n  x\n"),
    );
}

#[test]
fn id_does_not_pick_a_def_that_only_carries_the_marker() {
    // `b` carries `ID: SAME`, and `a` (line 2) owns it: `b`'s id is where it is, so `--id SAME` does not name `b`
    let out = calls(&["--entry", "b", "--id", "SAME"]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(out.stdout.is_empty());
    assert!(stderr.contains("no def `b` owns ID: SAME"), "{stderr}");
    assert!(stderr.contains("`a` at 2:1 owns it"), "{stderr}");
}

#[test]
fn a_marker_nobody_carries_is_still_no_def_that_carries_it() {
    let out = calls(&["--entry", "a", "--id", "OTHER"]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("no def `a` carries ID: OTHER"), "{stderr}");
}
