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
