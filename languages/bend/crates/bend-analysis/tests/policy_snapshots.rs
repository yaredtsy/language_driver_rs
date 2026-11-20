//! The policy table's rows (docs/08-bend/03 §3.3): one fixture folder each under `tests/fixtures/policy/<row>/` (a `main.bend` and what it imports) and a
//! snapshot `tests/snapshots/policy/<row>.txt` in the grammar of `languages/rust/docs/policy.md`: for each entry a heading `== <file> <entry>`, the tree, every
//! edge as `from | callee text | what it is` (by the caller's name, in source order within one def), then `counters: key=n, …`. A def with a doc comment is a def
//! with a doc comment: a site is read from the caller's range start, as the snapshot's text shows.
//!
//! The test that reads `languages/bend/docs/policy.md` and checks every row against its fixture is BD4 step 5; until then the rows are this list.

use std::fs;
use std::path::Path;

use analyzer_testkit::expect_file;
use bend_analysis::Book;

mod common;
use common::render_entry;

const FIXTURES: &str = "tests/fixtures/policy";
const SNAPSHOTS: &str = "tests/snapshots/policy";

/// The 27 rows of 03 §3.3, in the table's order.
const ROWS: [&str; 27] = [
    "direct",
    "gpu",
    "recursion",
    "below",
    "alias",
    "no-shadow",
    "binder-wins",
    "base",
    "hub",
    "foreign",
    "variable",
    "closure-call",
    "template-arg",
    "passed",
    "construct",
    "construct-base",
    "op-project",
    "op-base",
    "index",
    "do-project",
    "do-base",
    "rewrite",
    "type-position",
    "law-call",
    "law-open",
    "unknown",
    "lambda-inline",
];

/// The entries of a row, in the file `main.bend`: `main` unless the row says more.
fn entries(row: &str) -> &'static [&'static str] {
    match row {
        "recursion" => &["count"],
        "do-base" => &["main", "maybe"],
        _ => &["main"],
    }
}

fn folders(of: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(of)
        .unwrap_or_else(|e| panic!("{of}: {e}"))
        .map(|e| {
            let name = e.expect("entry").file_name().to_string_lossy().into_owned();
            name.strip_suffix(".txt")
                .map_or(name.clone(), str::to_owned)
        })
        .collect();
    names.sort();
    names
}

#[test]
fn every_row_has_a_fixture_and_a_snapshot_and_nothing_else_does() {
    let mut rows: Vec<String> = ROWS.iter().map(|r| (*r).to_owned()).collect();
    rows.sort();
    assert_eq!(folders(FIXTURES), rows, "the fixture folders are the rows");
    assert_eq!(folders(SNAPSHOTS), rows, "the snapshots are the rows");
}

#[test]
fn every_row_gives_the_tree_and_edges_of_its_snapshot() {
    for row in ROWS {
        let root = Path::new(FIXTURES).join(row);
        let book = Book::open(&root, Path::new("main.bend"))
            .unwrap_or_else(|| panic!("{row}: the book did not open"));
        let actual: String = entries(row)
            .iter()
            .map(|entry| render_entry(&book, "main.bend", entry))
            .collect();
        expect_file(&Path::new(SNAPSHOTS).join(format!("{row}.txt")), &actual);
    }
}
