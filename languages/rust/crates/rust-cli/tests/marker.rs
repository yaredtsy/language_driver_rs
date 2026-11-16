//! `rust-cli marker` (RS2 step 5): a marker planned, written into a scratch copy of a file of this repo, and read back. The tool is the one
//! place that writes a file, so these tests are where "plan, then apply" is shown to work on real source, not only on fixtures.

use std::path::PathBuf;
use std::process::{Command, Output};

use rust_syntax::{Edition, MarkerStyle, header_marker, marker, occurrences, outline};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: rust_syntax::Placement::Docstring,
};

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

fn code(out: &Output) -> i32 {
    out.status.code().unwrap()
}

/// A source file of this repo, by its path from `languages/rust/crates/`.
fn repo_file(path: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(path);
    std::fs::read_to_string(path).unwrap()
}

/// A copy of `text` in the folder cargo gives integration tests, under a name of its own; the path as a string.
fn scratch(name: &str, text: &str) -> String {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, text).unwrap();
    path.to_str().unwrap().to_owned()
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// `(qualname, occurrence, marker)` of every def of `text`.
fn markers(text: &str) -> Vec<(String, u16, Option<String>)> {
    let o = outline(text, Edition::LATEST, STYLE);
    o.defs
        .iter()
        .zip(occurrences(&o))
        .map(|(d, n)| (d.qualname.clone(), n, d.marker.clone()))
        .collect()
}

#[test]
fn a_plan_without_write_prints_the_edit_and_changes_nothing() {
    let original = repo_file("rust-syntax/src/header.rs");
    let path = scratch("plan_only_header.rs", &original);
    let out = cli(&["marker", &path, "--def", "plan", "--text", "T1"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    // the edit goes on the line of `fn plan`, just below the doc comment that is already there
    let line = original
        .lines()
        .position(|l| l.starts_with("pub(crate) fn plan("))
        .unwrap()
        + 1;
    assert_eq!(
        stdout(&out),
        format!("plan #0: insert at {line}:1\n+ ///\n+ /// ID: T1\n")
    );
    assert_eq!(read(&path), original);
}

#[test]
fn a_marker_written_on_a_def_of_this_repo_is_read_back() {
    let original = repo_file("rust-syntax/src/header.rs");
    let path = scratch("write_def_header.rs", &original);
    let out = cli(&["marker", &path, "--def", "plan", "--text", "T1", "--write"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stdout(&out).ends_with("written: ID: T1\n"),
        "{}",
        stdout(&out)
    );

    let written = read(&path);
    // read again by the library, not by the tool: the def has the marker and no other def has changed
    let mut want = markers(&original);
    want.iter_mut().find(|(q, ..)| q == "plan").unwrap().2 = Some("T1".to_owned());
    assert_eq!(markers(&written), want);
    // and the only change is the two lines that were printed
    assert_eq!(written.replacen("///\n/// ID: T1\n", "", 1), original);
    // `outline` shows it too
    let shown = stdout(&cli(&["outline", &path]));
    assert!(
        shown
            .lines()
            .any(|l| l.starts_with("Function plan ") && l.contains("ID: T1")),
        "{shown}"
    );
}

#[test]
fn a_header_written_on_a_file_of_this_repo_is_read_back() {
    let original = repo_file("rust-syntax/src/outline.rs");
    let path = scratch("write_header_outline.rs", &original);
    assert_eq!(header_marker(&original, STYLE), None);
    let out = cli(&["marker", &path, "--header", "--text", "H1", "--write"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stdout(&out).ends_with("written: ID: H1\n"),
        "{}",
        stdout(&out)
    );

    let written = read(&path);
    assert_eq!(header_marker(&written, STYLE).as_deref(), Some("H1"));
    // the file starts with `//!`: the header goes above it, with an empty line after it so that it is not read as crate docs
    assert!(
        written.starts_with("// ID: H1\n\n//!"),
        "{}",
        &written[..80]
    );
    assert_eq!(written.replacen("// ID: H1\n\n", "", 1), original);
    assert_eq!(markers(&written), markers(&original));
}

#[test]
fn a_second_marker_is_a_skip_with_its_reason_and_the_file_is_left_alone() {
    let original = repo_file("rust-syntax/src/header.rs");
    let path = scratch("twice_header.rs", &original);
    assert_eq!(
        code(&cli(&[
            "marker", &path, "--def", "plan", "--text", "T1", "--write"
        ])),
        0
    );
    let once = read(&path);
    let out = cli(&["marker", &path, "--def", "plan", "--text", "T2", "--write"]);
    assert_eq!(code(&out), 3);
    assert!(
        stdout(&out).contains("already has a marker: T1"),
        "{}",
        stdout(&out)
    );
    assert_eq!(read(&path), once);

    assert_eq!(
        code(&cli(&[
            "marker", &path, "--header", "--text", "H1", "--write"
        ])),
        0
    );
    let out = cli(&["marker", &path, "--header", "--text", "H2", "--write"]);
    assert_eq!(code(&out), 3);
    assert!(
        stdout(&out).contains("already has a marker: H1"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_qualname_of_several_defs_is_listed_and_none_is_picked() {
    // `impl_twins.rs` has them on one line each, which takes no marker (`NotOnOwnLine`): so these are on lines of their own
    let twins =
        "impl S {\n    fn m() {}\n}\nimpl S {\n    fn m() {}\n}\nimpl S {\n    fn m() {}\n}\n";
    let path = scratch("twins.rs", twins);
    let out = cli(&["marker", &path, "--def", "S::m", "--text", "T1", "--write"]);
    assert_eq!(code(&out), 2);
    let err = stderr(&out);
    assert!(
        err.contains("matches 3 defs") && err.contains("#0") && err.contains("#2"),
        "{err}"
    );
    assert_eq!(read(&path), twins);

    // the person says which
    let out = cli(&[
        "marker",
        &path,
        "--def",
        "S::m",
        "--occurrence",
        "1",
        "--text",
        "T1",
        "--write",
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let after = markers(&read(&path));
    let marked: Vec<_> = after.iter().filter(|(.., m)| m.is_some()).collect();
    assert_eq!(marked, [&("S::m".to_owned(), 1, Some("T1".to_owned()))]);
    assert_eq!(marker_of(&read(&path), "S::m", 1).as_deref(), Some("T1"));
}

fn marker_of(text: &str, qualname: &str, occurrence: u16) -> Option<String> {
    let o = outline(text, Edition::LATEST, STYLE);
    let (i, _) = occurrences(&o)
        .into_iter()
        .enumerate()
        .find(|&(i, n)| o.defs[i].qualname == qualname && n == occurrence)?;
    marker(text, Edition::LATEST, &o.defs[i], STYLE)
}

#[test]
fn the_label_and_the_line_ending_are_the_files_and_the_callers() {
    let crlf = "// Copyright.\r\n\r\n/// Docs.\r\nfn f() {}\r\n";
    let path = scratch("crlf.rs", crlf);
    assert_eq!(
        code(&cli(&[
            "marker", &path, "--label", "NOTE", "--def", "f", "--text", "n1", "--write"
        ])),
        0
    );
    assert_eq!(
        code(&cli(&[
            "marker", &path, "--label", "NOTE", "--header", "--text", "h1", "--write"
        ])),
        0
    );
    let written = read(&path);
    assert_eq!(
        written,
        "// Copyright.\r\n//\r\n// NOTE: h1\r\n\r\n/// Docs.\r\n///\r\n/// NOTE: n1\r\nfn f() {}\r\n"
    );
    assert!(!written.replace("\r\n", "").contains(['\r', '\n']));
}

#[test]
fn bad_usage_and_a_missing_file_are_told_apart() {
    let path = scratch("usage.rs", "fn f() {}\n");
    for args in [
        vec!["marker", &path, "--def", "f"],
        vec!["marker", &path, "--text", "x"],
        vec!["marker", &path, "--def", "f", "--header", "--text", "x"],
        vec![
            "marker",
            &path,
            "--header",
            "--occurrence",
            "1",
            "--text",
            "x",
        ],
        vec![
            "marker",
            &path,
            "--def",
            "f",
            "--occurrence",
            "one",
            "--text",
            "x",
        ],
        vec!["marker", &path, "--def", "nope", "--text", "x"],
    ] {
        assert_eq!(code(&cli(&args)), 2, "{args:?}");
    }
    assert_eq!(
        code(&cli(&[
            "marker",
            "no/such/file.rs",
            "--header",
            "--text",
            "x"
        ])),
        1
    );
    assert_eq!(read(&path), "fn f() {}\n");
}
