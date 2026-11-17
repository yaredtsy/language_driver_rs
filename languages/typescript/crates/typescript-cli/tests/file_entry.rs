//! `typescript-cli calls <dir> <file>` with no `--entry` is the file's own code (`Entry::File`, D32, M step 5).

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../typescript-analysis/tests/fixtures")
        .join(name)
}

fn calls(fixture_name: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_typescript-cli"))
        .arg("calls")
        .arg(fixture(fixture_name))
        .args(args)
        .output()
        .unwrap()
}

fn wire_of(out: &Output) -> serde_json::Value {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    serde_json::from_slice(&out.stdout).expect("the wire is JSON alone on stdout")
}

#[test]
fn no_entry_prints_the_tree_of_the_files_own_code() {
    let out = calls("file-entry", &["src/iife.ts"]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // the tree, a blank line, then the breakdown: the same shape as for a def entry
    assert!(
        stdout.starts_with("src/iife.ts\n  h\n\nbreakdown:\n"),
        "{stdout}"
    );
}

#[test]
fn no_entry_with_wire_has_a_module_as_its_root() {
    let frame = wire_of(&calls("file-entry", &["src/iife.ts", "--wire"]));
    assert_eq!(frame["target_kind"], "Module");
    assert_eq!(frame["target_qualname"], "");
    assert_eq!(frame["target_id"], "");
    assert_eq!(frame["target_file"], "src/iife.ts");
    assert_eq!(frame["children"][0]["target_qualname"], "h");
}

#[test]
fn no_entry_with_merged_wire_and_prefixes() {
    let frame = wire_of(&calls(
        "file-entry",
        &[
            "src/twice.ts",
            "--wire",
            "--merged",
            "--function-prefix",
            "F/",
            "--class-prefix",
            "C/",
        ],
    ));
    assert_eq!(frame["target_kind"], "Module");
    assert_eq!(frame["children"][0]["target_qualname"], "h");
    // two calls of `h` at the top are one node that says it stands for one more
    assert_eq!(frame["children"].as_array().unwrap().len(), 1);
    assert_eq!(frame["children"][0]["call_count"], 1);
}

#[test]
fn no_entry_with_graph_prints_the_graph() {
    let out = calls("file-entry", &["src/iife.ts", "--graph"]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.starts_with("Graph {"), "{stdout}");
}

#[test]
fn id_without_entry_is_a_usage_error() {
    let out = calls("file-entry", &["src/iife.ts", "--id", "abc"]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("--id picks among the defs that --entry names"),
        "{stderr}"
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn a_file_with_no_file_entry_says_what_a_def_entry_says() {
    // in node_modules: not project code
    let out = calls("node-modules-source", &["node_modules/lib/index.ts"]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("not project code"), "{stderr}");
    assert!(out.stdout.is_empty());

    // outside the root: the error `Project::open` gives, as for a def entry
    let out = calls("outside-root/app", &["../shared/lib.ts"]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("the file is outside the root"), "{stderr}");
    assert!(out.stdout.is_empty());
}
