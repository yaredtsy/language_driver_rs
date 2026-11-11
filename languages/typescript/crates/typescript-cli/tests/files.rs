//! `typescript-cli files <dir> <file>` on the analysis crate's fixture projects: the output is what a person reads to see which config was chosen.

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../typescript-analysis/tests/fixtures")
        .join(name)
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_typescript-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn files(fixture_name: &str, file: &str) -> Output {
    cli(&["files", fixture(fixture_name).to_str().unwrap(), file])
}

const WATCHED: &str = "  **/tsconfig*.json
  **/jsconfig.json
  **/package.json
  **/package-lock.json
  **/npm-shrinkwrap.json
  **/yarn.lock
  **/pnpm-lock.yaml
  **/bun.lock
  **/bun.lockb
";

#[test]
fn a_solution_config_shows_the_referenced_config_that_was_chosen() {
    let out = files("solution", "src/main.ts");
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!(
            "config: tsconfig.app.json
files (2):
  src/main.ts
  src/util.ts
declaration files (0):
environment inputs (9):
{WATCHED}"
        )
    );
}

#[test]
fn a_folder_with_no_config_says_it_is_inferred() {
    let out = files("plain-js", "index.js");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.starts_with("config: (inferred project)\nfiles (2):\n  index.js\n  lib/util.js\n"),
        "{stdout}"
    );
}

#[test]
fn declaration_files_and_an_extended_config_are_listed() {
    let out = files("with-node-modules", "src/main.ts");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("declaration files (1):\n  src/types.d.ts\n"),
        "{stdout}"
    );
    let out = files("extends", "src/main.ts");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.ends_with("  config/shared.json\n"), "{stdout}");
}

#[test]
fn a_file_no_config_holds_says_which_config_left_it_out_and_exits_1() {
    let out = files("excluded-file", "b.ts");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "b.ts: tsconfig.json does not include the file\n"
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn a_missing_file_exits_1_and_wrong_arguments_exit_2() {
    assert_eq!(files("solution", "src/gone.ts").status.code(), Some(1));
    assert_eq!(cli(&["files", "only-one-argument"]).status.code(), Some(2));
    assert_eq!(cli(&[]).status.code(), Some(2));
}

#[test]
fn outline_still_works() {
    let file = fixture("three-files").join("src/util.ts");
    let out = cli(&["outline", file.to_str().unwrap()]);
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout).unwrap().contains("double"));
}

fn scenario(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../scenarios")
        .join(name)
        .join("typescript")
}

fn calls(dir: &std::path::Path, file: &str, rest: &[&str]) -> Output {
    let mut args = vec!["calls", dir.to_str().unwrap(), file];
    args.extend_from_slice(rest);
    cli(&args)
}

#[test]
fn calls_prints_the_tree_and_then_the_breakdown() {
    let out = calls(&scenario("straight-line"), "main.ts", &["--entry", "main"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "main
  fetch
    parse
  transform
  store

breakdown:
  edges: 4 call, 0 construct, 0 external, 0 unresolved
  contexts 5, bodies walked 5, summaries reused 0
  cuts: none
"
    );
}

#[test]
fn calls_says_why_edges_are_unresolved_in_the_breakdown() {
    let out = calls(&fixture("resolve"), "src/main.ts", &["--entry", "callers"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(
            "  unresolved by reason:
    any_or_untyped: 2
    computed_name: 1
    function_typed_value: 1
"
        ),
        "{stdout}"
    );
}

#[test]
fn an_entry_that_matches_several_defs_lists_them_and_exits_2_and_never_picks_the_first() {
    let out = calls(&fixture("entries"), "src/main.ts", &["--entry", "Box.size"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("`Box.size` matches 2 defs"), "{stderr}");
    assert!(stderr.contains("#0  Method  14:3"), "{stderr}");
    assert!(stderr.contains("#1  Method  17:3"), "{stderr}");
    assert!(stderr.contains("--id"), "{stderr}");
}

#[test]
fn id_picks_the_def_that_carries_the_marker() {
    let dir = fixture("resolve");
    let out = calls(
        &dir,
        "src/lib.ts",
        &["--entry", "plain", "--id", "PLAIN-0001"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .starts_with("plain\n")
    );
    // a marker the def does not carry is not found, not guessed
    let wrong = calls(&dir, "src/lib.ts", &["--entry", "plain", "--id", "OTHER"]);
    assert_eq!(wrong.status.code(), Some(1));
    assert!(
        String::from_utf8(wrong.stderr)
            .unwrap()
            .contains("no def `plain` carries ID: OTHER")
    );
}

#[test]
fn calls_exits_1_for_a_def_that_is_not_there_and_2_for_wrong_arguments() {
    let dir = fixture("resolve");
    assert_eq!(
        calls(&dir, "src/main.ts", &["--entry", "nope"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        calls(&dir, "src/gone.ts", &["--entry", "callers"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(calls(&dir, "src/main.ts", &[]).status.code(), Some(2));
}

#[test]
fn graph_prints_the_graph_and_keeps_the_breakdown_off_stdout() {
    let out = calls(
        &scenario("straight-line"),
        "main.ts",
        &["--entry", "main", "--graph"],
    );
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("Graph {"), "{stdout}");
    assert!(!stdout.contains("breakdown:"));
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("breakdown:")
    );
}
