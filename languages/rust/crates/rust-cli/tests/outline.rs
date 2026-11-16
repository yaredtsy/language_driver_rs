//! `rust-cli outline <file>`: what a person reads. The fixtures are `rust-syntax`'s, so the names are the ones its snapshots pin.

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rust-syntax/tests/fixtures")
        .join(name)
        .to_str()
        .unwrap()
        .to_owned()
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

/// A file of its own, in the folder cargo gives integration tests.
fn scratch(name: &str, text: &str) -> String {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, text).unwrap();
    path.to_str().unwrap().to_owned()
}

#[test]
fn twins_print_their_occurrence_and_the_first_of_a_name_prints_none() {
    let out = cli(&["outline", &fixture("impl_twins.rs")]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out),
        "Method S::m  2:10-2:19
Method S::m #1  3:10-3:19
Method S::m #2  5:10-5:19
Method S<u8>::m  6:14-6:23
Method S<u16>::m  7:15-7:24
"
    );
}

#[test]
fn a_const_and_a_module_of_one_name_are_told_apart_and_children_are_indented() {
    let out = cli(&["outline", &fixture("body_collisions.rs")]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.starts_with(
            "Other(\"const\") C  3:1-3:36
  Function C::helper  3:17-3:31
Module C #1  4:1-4:25
  Function C::helper #1  4:9-4:23
"
        ),
        "{text}"
    );
}

#[test]
fn a_broken_file_flags_the_def_and_lists_the_errors_with_a_position() {
    let out = cli(&["outline", &fixture("impl_broken_local.rs")]);
    assert!(out.status.success(), "a file with errors is still outlined");
    let text = stdout(&out);
    assert!(
        text.starts_with(
            "Function before  2:1-2:15
Function h  3:1-6:2  SYNTAX-ERROR
syntax error 4:14: expected COMMA
"
        ),
        "{text}"
    );
}

#[test]
fn the_edition_changes_what_parses() {
    let file = scratch("edition.rs", "fn async() {}\n");
    let old = cli(&["outline", &file, "--edition", "2015"]);
    assert_eq!(stdout(&old), "Function async  1:1-1:14\n");
    let new = cli(&["outline", &file]);
    assert!(
        stdout(&new).contains("syntax error"),
        "the latest edition is the default: {}",
        stdout(&new)
    );
}

#[test]
fn a_file_of_this_repo_is_outlined_without_a_flag() {
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../core/analyzer-core/src/outline.rs"
    );
    let out = cli(&["outline", file]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("Method Def::new  "), "{text}");
    assert!(
        !text.contains("SYNTAX-ERROR") && !text.contains("syntax error"),
        "{text}"
    );
}

#[test]
fn what_cannot_be_read_is_an_error_and_not_an_empty_outline() {
    let missing = cli(&["outline", "no/such/file.rs"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).starts_with("no/such/file.rs: "));
    assert!(stdout(&missing).is_empty());

    let not_rust = cli(&["outline", "README.md"]);
    assert_eq!(not_rust.status.code(), Some(2));
    assert_eq!(stderr(&not_rust), "README.md: not a .rs file\n");

    let bad_edition = cli(&["outline", "a.rs", "--edition", "2022"]);
    assert_eq!(bad_edition.status.code(), Some(2));
    assert!(stderr(&bad_edition).contains("--edition is one of"));

    let nothing = cli(&[]);
    assert_eq!(nothing.status.code(), Some(2));
    assert!(stderr(&nothing).starts_with("usage: rust-cli outline <file>"));
}
