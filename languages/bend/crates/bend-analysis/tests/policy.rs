//! The policy table (`languages/bend/docs/policy.md`) is a test: every row names its fixture in the grammar of "Where a row is checked", and the trees and edges
//! those entries give are in `snapshots/policy/<row>.txt`. A row with no fixture, a folder or file that is not there, a file with no entry, a row without its
//! snapshot and a snapshot of no row each fail, one sentence each. Rust's `rust-analysis/tests/policy.rs` is the model.
//!
//! The grammar, read from the doc: **groups** separated by `; `; a group is a fixture folder under `tests/fixtures/policy/` (also the root the book is
//! opened at), one file of it (it ends in `.bend`), then the entries (qualnames; `name#1` is a second def of one name); a group that begins with a file keeps
//! the folder of the group before it. One reading beyond the words: a group of **entries only** (`do-base`: `` `main.bend` `main`; `maybe` ``) is more entries of
//! the file of the group before it, since nothing else in the grammar could give it a meaning.

use std::path::{Path, PathBuf};

use analyzer_testkit::expect_file;
use bend_analysis::Book;

mod common;
use common::table::{self, Cell, Group, Row, entry_name};
use common::{Program, render_entry};

const TABLE: &str = include_str!("../../../docs/policy.md");

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn snapshots_dir() -> PathBuf {
    tests_dir().join("snapshots/policy")
}

const HEADER: &str = "| Row |";

fn rows(table: &str) -> Vec<Row> {
    table::rows(table, HEADER)
}

fn problems(table: &str, fixtures: &Path, snapshots: &Path) -> Vec<String> {
    table::problems(table, HEADER, fixtures, snapshots, &[])
}

/// What a row gives, group by group: the fixture folder opened as the root with the group's file as the entry of the book.
fn render_row(row: &Row, groups: &[Group]) -> String {
    let mut out = String::new();
    for group in groups {
        let root = tests_dir().join("fixtures/policy").join(&group.folder);
        let book = Book::open(&root, Path::new(&group.file))
            .unwrap_or_else(|| panic!("row `{}`: the book did not open", row.id));
        for entry in &group.entries {
            out.push_str(&render_entry(&book, &group.file, entry_name(entry).0));
        }
    }
    out
}

#[test]
fn every_row_of_the_table_has_a_fixture_and_a_snapshot() {
    let found = problems(
        TABLE,
        &tests_dir().join("fixtures/policy"),
        &snapshots_dir(),
    );
    assert!(found.is_empty(), "{}", found.join("\n"));
    assert_eq!(rows(TABLE).len(), 28, "the table was not read");
}

#[test]
fn the_check_finds_every_kind_of_problem() {
    let scratch = Program::new(&[
        ("fixtures/f/main.bend", "def main() -> U32:\n  1\n"),
        ("snapshots/site.txt", "== main.bend main\n"),
        ("snapshots/stale.txt", "== a def that is gone\n"),
    ]);
    let fixtures = scratch.root.join("fixtures");
    let snapshots = scratch.root.join("snapshots");
    let table = |rows: &str| {
        format!(
            "| Row | Bend | Edge | Counter | Where it is checked |\n|---|---|---|---|---|\n{rows}\n"
        )
    };
    let one = |row: &str| problems(&table(row), &fixtures, &snapshots);
    let said = |found: Vec<String>, sentence: &str| {
        assert!(
            found.iter().any(|p| p == sentence),
            "`{sentence}` is not in {found:?}"
        );
    };
    said(
        one("| `site` | x | | | nothing written |"),
        "row `site`: no fixture",
    );
    said(
        one("| `site` | x | | | `gone` `main.bend` `main` |"),
        "row `site`: the folder `gone` is not there",
    );
    said(
        one("| `site` | x | | | `f` `gone.bend` `main` |"),
        "row `site`: the file `gone.bend` is not in `f`",
    );
    said(
        one("| `site` | x | | | `f` `main.bend` |"),
        "row `site`: the file `main.bend` has no entry",
    );
    said(
        one("| `site` | x | | | `main.bend` `main` |"),
        "row `site`: the file `main.bend` has no folder",
    );
    said(
        one("| `site` | x | | | `f` |"),
        "row `site`: the folder `f` has no file",
    );
    said(
        one("| `no-such-row` | x | | | `f` `main.bend` `main` |"),
        "row `no-such-row`: no snapshot",
    );
    said(
        one(
            "| `site` | x | | | `f` `main.bend` `main` |\n| `site` | y | | | `f` `main.bend` `main` |",
        ),
        "row `site`: the id is empty or used twice",
    );
    said(
        one("| `site` | x | | | `f` `main.bend` `main` |"),
        "snapshot `stale.txt`: no row of the table builds it",
    );
    said(
        one("| `site` | x | | | `f` `main.bend` `main#1` |"),
        "row `site`: the entry `main#1` is not the def `call_graph` enters for that name",
    );
    // a group that begins with a file keeps the folder, and a group of entries only is more entries of the file before it
    let right = rows(&table(
        "| `site` | x | | | `f` `main.bend` `main`; `other.bend` `a`; `b` |",
    ));
    let Cell::Groups(groups) = &right[0].cell else {
        panic!("a row that is right is read");
    };
    let read: Vec<(&str, &str, &[String])> = groups
        .iter()
        .map(|g| (g.folder.as_str(), g.file.as_str(), g.entries.as_slice()))
        .collect();
    assert_eq!(
        read,
        [
            ("f", "main.bend", &["main".to_owned()][..]),
            ("f", "other.bend", &["a".to_owned(), "b".to_owned()][..]),
        ]
    );
    // a table is read from its `| Row |` header to the first line that is not a row
    let framed = format!(
        "| `ignored` | before the table |\n{}\nprose, and `a` `b`\n| `after` | | | | `f` |\n",
        table("| `site` | x | | | `f` `main.bend` `main` |").trim_end()
    );
    let ids: Vec<String> = rows(&framed).into_iter().map(|row| row.id).collect();
    assert_eq!(ids, ["site"]);
}

#[test]
fn every_row_gives_the_tree_and_edges_of_its_snapshot() {
    // every row is compared, and the ones that differ are named together at the end (each diff is printed as it is found)
    let mut differ = Vec::new();
    for row in rows(TABLE) {
        let Cell::Groups(groups) = &row.cell else {
            continue;
        };
        let compared = std::panic::catch_unwind(|| {
            expect_file(
                &snapshots_dir().join(format!("{}.txt", row.id)),
                &render_row(&row, groups),
            );
        });
        if compared.is_err() {
            differ.push(row.id.clone());
        }
    }
    assert!(differ.is_empty(), "rows that differ: {differ:?}");
}
