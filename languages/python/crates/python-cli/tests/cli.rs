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

/// A fixture project of `python-analysis`: the folder `calls` is given.
fn project(name: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "python-analysis",
        "tests",
        "fixtures",
        name,
        "python",
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

/// The file is `python-syntax`'s `syntax_error_mid_file.py`: `def broken(:` is line 5, and the parser reports the `:` (column 12) and the
/// newline after it (column 13). The def that holds an error is flagged, its neighbours are not, and each error is its own line after the
/// defs.
#[test]
fn outline_flags_a_def_with_a_syntax_error_and_lists_the_errors() {
    let file: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "python-syntax",
        "tests",
        "fixtures",
        "syntax_error_mid_file.py",
    ]
    .iter()
    .collect();
    let out = run(&["outline", file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        "\
Function before  1:1-2:9
Function broken  5:1-6:9  SYNTAX-ERROR
Function after  9:1-10:9
syntax error 5:12: Expected a parameter or the end of the parameter list
syntax error 5:13: Expected `)`, found newline
"
    );
}

// `calls`: the tree (C2)

/// What `tests/api.rs` of `python-analysis` pins for `main` of `ids_and_classes.py`.
const MAIN_TREE: &str = "\
ids_and_classes.main
  ids_and_classes.Report
    ids_and_classes.Report.prepare
  ids_and_classes.marked
    ids_and_classes.unmarked
  ids_and_classes.unmarked
  ids_and_classes.unmarked
";

/// The tree and the counters, split at the blank line between them.
fn tree_and_counters(out: &Output) -> (String, String) {
    let text = stdout(out);
    let (tree, counters) = text
        .split_once("\n\n")
        .unwrap_or_else(|| panic!("no blank line between the tree and the counters: {text:?}"));
    (format!("{tree}\n"), counters.to_owned())
}

#[test]
fn calls_prints_the_tree_a_blank_line_and_the_counters() {
    let dir = project("06-wire");
    let out = run(&["calls", &dir, "ids_and_classes.py", "--entry", "main"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (tree, counters) = tree_and_counters(&out);
    assert_eq!(tree, MAIN_TREE);
    // `main`, `Report.__init__`, `Report.prepare`, `marked`, `unmarked`: five bodies, each walked once; the second and third call of
    // `unmarked`, and the one inside `marked`, ask for a summary that is known, which is two of the three (the first is the walk)
    let mut lines = counters.lines();
    assert_eq!(
        lines.next(),
        Some("contexts 5, walks 5, reuse hits 2, unresolved 0")
    );
    // then a line per cut and per language counter, `<key>: <n>`, in the order of the maps (sorted by key: rule 9). The keys are the
    // analyzer's own and change with it, so the test pins their form and their order, not the list
    let rest: Vec<(&str, u64)> = lines
        .map(|l| {
            let (key, n) = l.split_once(": ").unwrap_or_else(|| panic!("{l:?}"));
            (key, n.parse().unwrap_or_else(|_| panic!("{l:?}")))
        })
        .collect();
    assert!(
        !rest.is_empty(),
        "the language's counters are listed, zeros too"
    );
    let cuts = rest
        .iter()
        .take_while(|(k, _)| k.starts_with("cut "))
        .count();
    for part in [&rest[..cuts], &rest[cuts..]] {
        assert!(part.windows(2).all(|w| w[0].0 < w[1].0), "{part:?}");
    }
    assert_eq!(stderr(&out), "");
}

#[test]
fn calls_only_marked_hides_what_has_no_marker_and_what_is_under_it() {
    let dir = project("06-wire");
    let all = run(&["calls", &dir, "ids_and_classes.py", "--entry", "main"]);
    let marked = run(&[
        "calls",
        &dir,
        "ids_and_classes.py",
        "--entry",
        "main",
        "--only-marked",
    ]);
    assert_eq!(marked.status.code(), Some(0), "{}", stderr(&marked));
    let (tree, _) = tree_and_counters(&marked);
    // from the rule (core's rule 9): a function with no marker is hidden with everything under it. `Report` and `marked` carry an ID;
    // `Report.prepare` and `unmarked` do not
    assert_eq!(
        tree,
        "ids_and_classes.main\n  ids_and_classes.Report\n  ids_and_classes.marked\n"
    );
    assert_ne!(tree, tree_and_counters(&all).0);
}

#[test]
fn calls_with_two_defs_of_one_name_lists_them_and_picks_none() {
    let dir = project("08-entries");
    let out = run(&["calls", &dir, "twins.py", "--entry", "f"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "", "the first is never picked");
    let err = stderr(&out);
    assert!(err.contains("#0  Function  14:5  no marker"), "{err}");
    assert!(err.contains("#1  Function  23:5  no marker"), "{err}");
}

#[test]
fn calls_occurrence_picks_the_nth_def_of_that_name() {
    let dir = project("08-entries");
    let out = run(&[
        "calls",
        &dir,
        "twins.py",
        "--entry",
        "f",
        "--occurrence",
        "1",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (tree, _) = tree_and_counters(&out);
    assert_eq!(
        tree, "twins.f\n  twins.b\n",
        "the second `f` calls `b`, not `a`"
    );
    let first = run(&[
        "calls",
        &dir,
        "twins.py",
        "--entry",
        "f",
        "--occurrence",
        "0",
    ]);
    assert_eq!(tree_and_counters(&first).0, "twins.f\n  twins.a\n");
}

#[test]
fn calls_occurrence_past_the_last_def_is_exit_1() {
    let dir = project("08-entries");
    let out = run(&[
        "calls",
        &dir,
        "twins.py",
        "--entry",
        "f",
        "--occurrence",
        "2",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert!(stderr(&out).contains("twins.py"), "{}", stderr(&out));
}

#[test]
fn calls_a_class_as_the_entry_has_the_class_as_its_root() {
    let dir = project("08-entries");
    let out = run(&["calls", &dir, "inherit_a.py", "--entry", "Child"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    // `Child` has no `__init__`: the constructor is `Base`'s, in `inherit_b.py`, and it calls `helper`
    assert_eq!(
        tree_and_counters(&out).0,
        "inherit_a.Child\n  inherit_b.helper\n"
    );
}

#[test]
fn calls_a_nested_def_is_named_the_way_the_outline_names_it() {
    let dir = project("08-entries");
    let out = run(&["calls", &dir, "nested.py", "--entry", "top.<locals>.inner"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    // `--entry` takes the outline's qualname, which has `<locals>`; the tree draws the name without it
    assert_eq!(
        tree_and_counters(&out).0,
        "nested.top.inner\n  nested.leaf\n"
    );
}

#[test]
fn calls_with_nothing_to_find_is_exit_1_and_says_what() {
    let dir = project("08-entries");
    let no_def = run(&["calls", &dir, "twins.py", "--entry", "no_such"]);
    assert_eq!(no_def.status.code(), Some(1));
    assert_eq!(stdout(&no_def), "");
    assert!(stderr(&no_def).contains("no_such"), "{}", stderr(&no_def));

    let no_file = run(&["calls", &dir, "nope.py", "--entry", "f"]);
    assert_eq!(no_file.status.code(), Some(1));
    assert!(stderr(&no_file).contains("nope.py"), "{}", stderr(&no_file));

    let no_dir = run(&["calls", "no/such/dir", "twins.py", "--entry", "f"]);
    assert_eq!(no_dir.status.code(), Some(1));
    assert_eq!(stdout(&no_dir), "");
    assert!(
        stderr(&no_dir).contains("no/such/dir"),
        "{}",
        stderr(&no_dir)
    );
}

#[test]
fn calls_with_a_wrong_command_line_is_exit_2() {
    let dir = project("08-entries");
    for args in [
        // `--occurrence` and `--id` name a def two ways
        vec![
            "calls",
            &dir,
            "twins.py",
            "--entry",
            "f",
            "--occurrence",
            "0",
            "--id",
            "x",
        ],
        // an occurrence is a number
        vec![
            "calls",
            &dir,
            "twins.py",
            "--entry",
            "f",
            "--occurrence",
            "first",
        ],
        // a flag with its value missing
        vec!["calls", &dir, "twins.py", "--entry"],
        // no entry
        vec!["calls", &dir, "twins.py"],
        // no file
        vec!["calls", &dir, "--entry", "f"],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("usage:"),
            "{args:?}: {}",
            stderr(&out)
        );
        assert_eq!(stdout(&out), "", "{args:?}");
    }
}

#[test]
fn calls_id_names_the_def_by_its_marker() {
    let dir = project("06-wire");
    let right = run(&[
        "calls",
        &dir,
        "ids_and_classes.py",
        "--entry",
        "marked",
        "--id",
        "06000000-0002-0000-0000-000000000000",
    ]);
    assert_eq!(right.status.code(), Some(0), "{}", stderr(&right));
    assert_eq!(
        tree_and_counters(&right).0,
        "ids_and_classes.marked\n  ids_and_classes.unmarked\n"
    );
    let wrong = run(&[
        "calls",
        &dir,
        "ids_and_classes.py",
        "--entry",
        "marked",
        "--id",
        "06000000-ffff-0000-0000-000000000000",
    ]);
    assert_eq!(wrong.status.code(), Some(1));
    assert_eq!(stdout(&wrong), "");
    assert!(
        stderr(&wrong).contains("06000000-ffff"),
        "{}",
        stderr(&wrong)
    );
}

#[test]
fn calls_label_is_the_marker_label_the_ids_are_read_under() {
    let dir = project("06-wire");
    // under another label no def carries a marker, so the id finds nothing
    let out = run(&[
        "calls",
        &dir,
        "ids_and_classes.py",
        "--entry",
        "marked",
        "--id",
        "06000000-0002-0000-0000-000000000000",
        "--label",
        "Other",
    ]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn calls_timing_goes_to_stderr_and_does_not_change_stdout() {
    let dir = project("06-wire");
    let args = ["calls", &dir, "ids_and_classes.py", "--entry", "main"];
    let plain = run(&args);
    let timed = run(&[&args[..], &["--timing"][..]].concat());
    assert_eq!(timed.status.code(), Some(0));
    assert_eq!(stdout(&timed), stdout(&plain));
    let err = stderr(&timed);
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(
        err.starts_with("open ") && err.contains(" ms, solve ") && err.trim_end().ends_with(" ms"),
        "{err}"
    );
}

#[test]
fn calls_twice_prints_the_same_bytes() {
    let dir = project("06-wire");
    let args = ["calls", &dir, "ids_and_classes.py", "--entry", "main"];
    assert_eq!(run(&args).stdout, run(&args).stdout);
}
