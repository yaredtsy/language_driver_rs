//! The command line, run as a program: what it prints, and that a bad request is an exit code and a sentence on stderr, never a panic.

use std::process::{Command, Output};

const POLICY: &str = "../bend-analysis/tests/fixtures/policy";

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bend-cli"))
        .args(args)
        .output()
        .expect("the program runs")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn outline_lists_the_defs_with_their_markers_and_places() {
    let o = run(&["outline", "../bend-syntax/tests/fixtures/marked_def.bend"]);
    assert_eq!(o.status.code(), Some(0));
    let text = out(&o);
    assert!(text.contains("Function "), "{text}");
    assert!(text.contains("  ID: "), "a marker is printed: {text}");
    // `--label` is the caller's: another label reads no marker
    let other = out(&run(&[
        "outline",
        "../bend-syntax/tests/fixtures/marked_def.bend",
        "--label",
        "TAG",
    ]));
    assert!(!other.contains("  TAG: "), "{other}");
}

#[test]
fn calls_prints_the_tree_and_the_summary() {
    let root = format!("{POLICY}/template-arg");
    let o = run(&["calls", &root, "main.bend", "--entry", "main"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let text = out(&o);
    assert!(
        text.starts_with("main\n  twice\n  f\n  twice\n  g [Passed]\n"),
        "{text}"
    );
    assert!(
        text.contains("edges: 3 call, 0 construct, 0 external, 2 unresolved, 1 passed"),
        "{text}"
    );
    assert!(
        text.contains("counters: function_typed_value=2, type_position=8"),
        "{text}"
    );
}

#[test]
fn the_wire_is_json_alone_on_stdout_and_the_summary_goes_to_stderr() {
    let root = format!("{POLICY}/passed");
    let o = run(&[
        "calls",
        &root,
        "main.bend",
        "--entry",
        "main",
        "--wire",
        "--merged",
        "--function-prefix",
        "fn:",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let frame: serde_json::Value =
        serde_json::from_str(&out(&o)).expect("stdout is one JSON value");
    assert_eq!(frame["target_qname"], "main");
    assert_eq!(frame["children"].as_array().map(Vec::len), Some(4));
    assert_eq!(frame["children"][1]["passed"], true);
    assert!(err(&o).contains("edges: "), "{}", err(&o));
}

#[test]
fn the_limits_cut_and_say_so() {
    let root = format!("{POLICY}/template-arg");
    let o = run(&[
        "calls",
        &root,
        "main.bend",
        "--entry",
        "main",
        "--max-depth",
        "0",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).contains("cuts {MaxDepth"), "{}", out(&o));
}

#[test]
fn all_enters_every_def_of_the_book_once() {
    let root = format!("{POLICY}/template-arg");
    let o = run(&["calls", &root, "main.bend", "--all"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let text = out(&o);
    assert!(
        text.contains("files 1, defs 4\nentered 4, skipped 0"),
        "{text}"
    );
    // each body once: `twice` has two calls through `~h`, `main` none
    assert!(
        text.contains("unresolved: function_typed_value 2"),
        "{text}"
    );
}

#[test]
fn a_bad_request_is_an_exit_code_and_a_sentence() {
    let root = format!("{POLICY}/direct");
    for (args, code) in [
        (vec!["calls", &root, "nope.bend", "--entry", "main"], 1),
        (vec!["calls", &root, "main.bend", "--entry", "nope"], 1),
        (
            vec!["calls", "no-such-folder", "main.bend", "--entry", "main"],
            1,
        ),
        (vec!["outline", "no-such-file.bend"], 1),
        (vec!["calls", &root, "main.bend"], 2),
        (
            vec!["calls", &root, "main.bend", "--entry", "main", "--all"],
            2,
        ),
        (
            vec![
                "calls",
                &root,
                "main.bend",
                "--entry",
                "main",
                "--graph",
                "--wire",
            ],
            2,
        ),
        (
            vec!["calls", &root, "main.bend", "--entry", "main", "--merged"],
            2,
        ),
        (
            vec![
                "calls",
                &root,
                "main.bend",
                "--entry",
                "main",
                "--max-depth",
                "x",
            ],
            2,
        ),
        (
            vec![
                "calls",
                &root,
                "main.bend",
                "--entry",
                "main",
                "--max-contexts",
            ],
            2,
        ),
        (vec!["nonsense"], 2),
        (vec![], 2),
    ] {
        let o = run(&args);
        assert_eq!(o.status.code(), Some(code), "{args:?}: {}", err(&o));
        assert!(!err(&o).is_empty(), "{args:?}: says why");
        assert!(!err(&o).contains("panicked"), "{args:?}: {}", err(&o));
        assert!(out(&o).is_empty(), "{args:?}: nothing on stdout");
    }
}
