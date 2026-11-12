//! `python-cli` as a person runs it: the built binary, real arguments, and what it prints and returns.
//!
//! The files are the fixture projects of `python-analysis`.

use std::path::PathBuf;
use std::process::{Command, Output};

/// A fixture file of `python-analysis`, as a path the binary can open.
fn fixture(project: &str, file: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "python-analysis",
        "tests",
        "fixtures",
        project,
        "python",
        file,
    ]
    .iter()
    .collect();
    path.to_str().expect("a fixture path is UTF-8").to_owned()
}

/// Run the binary with these arguments.
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_python-cli"))
        .args(args)
        .output()
        .expect("the binary starts")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr is UTF-8")
}

/// Written from the file, not from the program: each line is `<Kind> <qualname>  <line>:<col>-<line>:<col>`, a def's range runs from its
/// `def` or `class` keyword to the end of its last body token, children are indented two spaces under their parent, and the two
/// branches of the `if` give two defs of the same name (D15).
const TWINS: &str = "\
Function a  4:1-5:9
Function b  8:1-9:9
Function f  14:5-15:12
Class Impl  17:5-19:16
  Method Impl.run  18:9-19:16
Function f  23:5-24:12
Class Impl  26:5-28:16
  Method Impl.run  27:9-28:16
";

#[test]
fn outline_prints_one_line_per_def_children_indented() {
    let out = run(&["outline", &fixture("08-entries", "twins.py")]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), TWINS);
    assert_eq!(stderr(&out), "");
}

#[test]
fn outline_label_is_a_parameter() {
    let file = fixture("06-wire", "ids_and_classes.py");

    let default = stdout(&run(&["outline", &file]));
    let marked = default
        .lines()
        .find(|l| l.starts_with("Class Report"))
        .expect("the class is listed");
    assert!(
        marked.ends_with("  ID: 06000000-0001-0000-0000-000000000000"),
        "{marked}"
    );
    assert!(default.contains("Function main"), "{default}");
    assert_eq!(default.matches("06000000-").count(), 3, "{default}");

    // under another label the same docstrings carry no marker
    let other = run(&["outline", &file, "--label", "Other"]);
    assert_eq!(other.status.code(), Some(0));
    let other = stdout(&other);
    assert!(!other.contains("06000000"), "{other}");
    assert!(!other.contains("ID:"), "{other}");
    assert!(other.contains("Function main"), "{other}");
}

#[test]
fn outline_of_a_missing_file_is_exit_1_with_a_message_and_no_output() {
    let out = run(&["outline", "no/such/file.py"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("no/such/file.py"), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn a_wrong_command_line_is_exit_2_with_the_usage() {
    for args in [&[][..], &["frobnicate"][..], &["outline"][..]] {
        let out = run(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(
            stderr(&out).contains("usage:"),
            "{args:?}: {}",
            stderr(&out)
        );
        assert_eq!(stdout(&out), "", "{args:?}");
    }
}
