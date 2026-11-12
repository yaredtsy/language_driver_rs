//! The policy table (`languages/typescript/docs/policy.md`): every row names a fixture and the entries to build, and has the trees and edges those give, in
//! `snapshots/policy/<row>.txt`. A row without a fixture, a fixture that is not there, a row without a snapshot and a snapshot without a row each fail.

mod common;

use std::path::{Path, PathBuf};

use analyzer_testkit::expect_file;
use analyzer_testkit::graph::check_invariants;
use common::edges_as_text;
use typescript_analysis::{
    Cancel, Entry, MarkerStyle, Options, Placement, Project, RenderOptions, breakdown, call_graph,
    render, to_text,
};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

const TABLE: &str = include_str!("../../../docs/policy.md");

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

/// One row of the table: its id, and the backticked words of its last cell. A row with one word names a folder that must exist (the outline's fixtures, a
/// rule of the outline and not of the calls). A row with more names the project's folder, a file of it, and the entries (qualnames) built from that file.
struct Row {
    id: String,
    words: Vec<String>,
}

/// The text between pairs of backticks, in order.
fn backticked(text: &str) -> Vec<String> {
    text.split('`')
        .enumerate()
        .filter(|(n, _)| n % 2 == 1)
        .map(|(_, word)| word.to_owned())
        .collect()
}

/// The rows of a table: the lines that begin with `` | `id` | ``. The fixture is the last cell.
fn rows(table: &str) -> Vec<Row> {
    table
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            let id = backticked(line).into_iter().next().unwrap_or_default();
            let last = line
                .trim_end()
                .trim_end_matches('|')
                .rsplit_once(" | ")
                .map_or("", |(_, cell)| cell);
            Row {
                id,
                words: backticked(last),
            }
        })
        .collect()
}

fn snapshot_path(id: &str) -> PathBuf {
    tests_dir()
        .join("snapshots/policy")
        .join(format!("{id}.txt"))
}

/// What is wrong with a table, one sentence for each: a row with no fixture, a fixture that is not there, no snapshot, a snapshot of no row.
fn problems(table: &str, tests: &Path) -> Vec<String> {
    let rows = rows(table);
    let mut found = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for row in &rows {
        if row.id.is_empty() || seen.contains(&row.id.as_str()) {
            found.push(format!("row `{}`: the id is empty or used twice", row.id));
        }
        seen.push(&row.id);
        let Some(root) = row.words.first() else {
            found.push(format!("row `{}`: no fixture", row.id));
            continue;
        };
        if !tests.join(root).exists() {
            found.push(format!(
                "row `{}`: the fixture `{root}` is not there",
                row.id
            ));
            continue;
        }
        match row.words.len() {
            1 => continue,
            2 => {
                found.push(format!("row `{}`: a file and no entry", row.id));
                continue;
            }
            _ => {}
        }
        if !tests.join(root).join(&row.words[1]).is_file() {
            found.push(format!(
                "row `{}`: the file `{}` is not in `{root}`",
                row.id, row.words[1]
            ));
        }
        if !tests
            .join("snapshots/policy")
            .join(format!("{}.txt", row.id))
            .is_file()
        {
            found.push(format!("row `{}`: no snapshot", row.id));
        }
    }
    if let Ok(dir) = std::fs::read_dir(tests.join("snapshots/policy")) {
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let id = name.trim_end_matches(".txt");
            let has_row = rows.iter().any(|row| row.id == id && row.words.len() > 2);
            if !has_row {
                found.push(format!("snapshot `{name}`: no row of the table builds it"));
            }
        }
    }
    found
}

/// What a row gives: for each entry, the tree, the edges of the whole graph (the tree shows only the defs of the project: an external, an unresolved site
/// and a count are not in it) grouped by the def they leave, and the counters of what no edge covers when any is above 0.
fn render_row(row: &Row) -> String {
    let root = tests_dir().join(&row.words[0]);
    let file = &row.words[1];
    let project = Project::open(&root, Path::new(file))
        .unwrap_or_else(|error| panic!("row `{}`: {error:?}", row.id));
    let mut out = String::new();
    for entry in &row.words[2..] {
        let graph = call_graph(
            &project,
            file,
            &Entry {
                qualname: entry,
                occurrence: 0,
            },
            &Options::new(STYLE),
            &Cancel::new(),
        )
        .unwrap_or_else(|| panic!("row `{}`: no def `{entry}` in {file}", row.id));
        check_invariants(&graph);
        out.push_str(&format!("== {entry}\n"));
        out.push_str(&to_text(&render(
            &graph,
            &graph.entries[0],
            &RenderOptions::default(),
        )));
        let mut edges = edges_as_text(&graph, &root);
        // stable: the edges of one def stay in source order
        edges.sort_by_key(|line| line.split(" | ").next().unwrap_or_default().to_owned());
        if edges.is_empty() {
            out.push_str("edges: none\n");
        } else {
            out.push_str("edges:\n");
            for line in edges {
                out.push_str(&format!("{line}\n"));
            }
        }
        let uncovered: Vec<String> = breakdown(&graph)
            .uncovered
            .iter()
            .map(|(kind, count)| format!("{kind}={count}"))
            .collect();
        if !uncovered.is_empty() {
            out.push_str(&format!("uncovered: {}\n", uncovered.join(", ")));
        }
    }
    out
}

#[test]
fn every_row_of_the_table_has_a_fixture_and_a_snapshot() {
    let found = problems(TABLE, &tests_dir());
    assert!(found.is_empty(), "{}", found.join("\n"));
    assert!(rows(TABLE).len() > 30, "the table was not read");
}

#[test]
fn the_check_finds_a_row_without_a_fixture_a_missing_fixture_and_a_stale_snapshot() {
    let tests = tests_dir();
    let table = |row: &str| format!("| row | policy | fixture |\n|---|---|---|\n{row}\n");
    let one = |row: &str| problems(&table(row), &tests);
    // a row with no fixture word
    assert!(
        one("| `site` | text | nothing written |")
            .iter()
            .any(|p| p.contains("row `site`: no fixture"))
    );
    // a folder that is not there
    assert!(
        one("| `site` | text | `policy-gone` `src/sites.ts` `sites` |")
            .iter()
            .any(|p| p.contains("`policy-gone` is not there"))
    );
    // a file that is not there
    assert!(
        one("| `site` | text | `policy` `src/gone.ts` `sites` |")
            .iter()
            .any(|p| p.contains("`src/gone.ts` is not in `policy`"))
    );
    // a file and no entry
    assert!(
        one("| `site` | text | `policy` `src/sites.ts` |")
            .iter()
            .any(|p| p.contains("a file and no entry"))
    );
    // a row that has no snapshot
    assert!(
        one("| `no-such-row` | text | `policy` `src/sites.ts` `sites` |")
            .iter()
            .any(|p| p.contains("row `no-such-row`: no snapshot"))
    );
    // the same id twice
    assert!(
        problems(
            &table("| `site` | a | `policy` `src/sites.ts` `sites` |\n| `site` | b | `policy` `src/sites.ts` `sites` |"),
            &tests
        )
        .iter()
        .any(|p| p.contains("used twice"))
    );
    // every snapshot of the real table is stale in a table that has one row only
    assert!(
        one("| `site` | text | `policy` `src/sites.ts` `sites` |")
            .iter()
            .any(|p| p.contains("snapshot `branches.txt`: no row of the table builds it"))
    );
    // the one row that is right has no problem of its own
    assert!(
        !one("| `site` | text | `policy` `src/sites.ts` `sites` |")
            .iter()
            .any(|p| p.starts_with("row `site`"))
    );
}

#[test]
fn every_row_gives_the_tree_and_edges_of_its_snapshot() {
    // every row is compared, and the ones that differ are named together at the end (each diff is printed as it is found)
    let mut differ = Vec::new();
    for row in rows(TABLE).iter().filter(|row| row.words.len() > 2) {
        let compared = std::panic::catch_unwind(|| {
            expect_file(&snapshot_path(&row.id), &render_row(row));
        });
        if compared.is_err() {
            differ.push(row.id.clone());
        }
    }
    assert!(differ.is_empty(), "rows that differ: {differ:?}");
}

#[test]
fn every_row_gives_the_same_text_twice() {
    // rule 11 over the whole table: two projects opened one after the other give the same trees, edges and counters
    for row in rows(TABLE).iter().filter(|row| row.words.len() > 2) {
        assert_eq!(render_row(row), render_row(row), "row `{}`", row.id);
    }
}
