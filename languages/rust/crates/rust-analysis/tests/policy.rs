//! The policy table (`languages/rust/docs/policy.md`) is a test: every row names its fixture in the grammar of "Where a row is checked", and the trees and
//! edges those entries give are in `snapshots/policy/<row>.txt`. A row with no fixture, a workspace or file that is not there, a file with no entry, an id
//! used twice, a row without its snapshot and a snapshot of no row each fail, one sentence each.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use analyzer_testkit::expect_file;
use analyzer_testkit::graph::check_invariants;
use rust_analysis::{
    Cancel, EdgeTo, Entry, Graph, LoadOptions, MarkerStyle, Options, Project, RenderOptions,
    StableDef, call_graph, render, to_text,
};

mod common;
use common::{Cell, Group, Row, copy_of_fixture, one_load, rows, shared, tmp_root};

const TABLE: &str = include_str!("../../../docs/policy.md");

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn snapshot_path(id: &str) -> PathBuf {
    tests_dir()
        .join("snapshots/policy")
        .join(format!("{id}.txt"))
}

/// What is wrong with a table, one sentence for each. `tests` is the folder that holds `fixtures/` and `snapshots/policy/`.
fn problems(table: &str, tests: &Path) -> Vec<String> {
    let rows = rows(table);
    let fixtures = tests.join("fixtures");
    let snapshots = tests.join("snapshots/policy");
    let mut found = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for row in &rows {
        let id = &row.id;
        if id.is_empty() || seen.contains(&id.as_str()) {
            found.push(format!("row `{id}`: the id is empty or used twice"));
        }
        seen.push(id);
        match &row.cell {
            Cell::Broken(why) => found.push(format!("row `{id}`: {why}")),
            Cell::Folder(folder) => {
                if !fixtures.join(folder).is_dir() {
                    found.push(format!("row `{id}`: the folder `{folder}` is not there"));
                }
            }
            Cell::Groups(groups) => {
                for group in groups {
                    let root = fixtures.join(&group.workspace);
                    if !root.is_dir() {
                        found.push(format!(
                            "row `{id}`: the workspace `{}` is not there",
                            group.workspace
                        ));
                    } else if !root.join(&group.file).is_file() {
                        found.push(format!(
                            "row `{id}`: the file `{}` is not in `{}`",
                            group.file, group.workspace
                        ));
                    }
                    if group.entries.is_empty() {
                        found.push(format!(
                            "row `{id}`: the file `{}` has no entry",
                            group.file
                        ));
                    }
                }
                if !snapshots.join(format!("{id}.txt")).is_file() {
                    found.push(format!("row `{id}`: no snapshot"));
                }
            }
        }
    }
    if let Ok(dir) = std::fs::read_dir(&snapshots) {
        let mut names: Vec<String> = dir
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            let id = name.trim_end_matches(".txt");
            let has_row = rows
                .iter()
                .any(|row| row.id == id && matches!(row.cell, Cell::Groups(_)));
            if !has_row {
                found.push(format!("snapshot `{name}`: no row of the table builds it"));
            }
        }
    }
    found
}

fn options() -> Options {
    Options::new(MarkerStyle::docstring("ID"))
}

/// The two workspaces that most rows use are loaded once for this test program, each used by one test at a time (`common::shared`).
fn policy() -> MutexGuard<'static, Project> {
    static POLICY: OnceLock<Mutex<Project>> = OnceLock::new();
    shared(&POLICY, || shared_load("policy"))
}

fn context() -> MutexGuard<'static, Project> {
    static CONTEXT: OnceLock<Mutex<Project>> = OnceLock::new();
    shared(&CONTEXT, || shared_load("context"))
}

fn shared_load(fixture: &str) -> Project {
    let copy = copy_of_fixture(fixture, &format!("policy_table_{fixture}"));
    Project::open(&copy, Path::new("src/lib.rs"), &LoadOptions::default())
        .unwrap_or_else(|error| panic!("{fixture}: {error:?}"))
}

/// A def as the snapshot names it: its qualname, and `#n` for the n-th of one name in its file.
fn label(graph: &Graph, id: &StableDef) -> String {
    let def = graph.def(id).expect("a def of the graph");
    match def.occurrence {
        0 => def.qualname.clone(),
        n => format!("{}#{n}", def.qualname),
    }
}

/// Every edge of `graph` as `from | callee text | what it is`: grouped by the def they leave (sorted by its name), in site order inside a def.
fn edges_as_text(graph: &Graph, root: &Path) -> Vec<String> {
    let mut lines: Vec<(String, String)> = graph
        .edges
        .iter()
        .map(|edge| {
            let from = graph.def(&edge.from.func).expect("the caller is a def");
            let text = std::fs::read(root.join(&from.file)).expect("the caller's file");
            let (start, end) = (
                (from.range.start + edge.site.start) as usize,
                (from.range.start + edge.site.end) as usize,
            );
            let callee = text.get(start..end).unwrap_or_else(|| {
                panic!("{}: the site {start}..{end} is outside the file", from.file)
            });
            let to = match &edge.to {
                EdgeTo::Context(id, _) if edge.passed => {
                    format!("pass {}", label(graph, &id.func))
                }
                EdgeTo::Context(id, _) => format!("call {}", label(graph, &id.func)),
                EdgeTo::Construct { class, init, .. } => match init {
                    Some(init) => format!(
                        "construct {} init {}",
                        label(graph, class),
                        label(graph, &init.func)
                    ),
                    None => format!("construct {} no init", label(graph, class)),
                },
                EdgeTo::External(external, _) => {
                    format!("external {} {}", external.package, external.name)
                }
                EdgeTo::Unresolved(reason) => format!("unresolved {}", reason.as_str()),
                _ => unreachable!("an edge kind this test does not know"),
            };
            let from = label(graph, &edge.from.func);
            let line = format!("{from} | {} | {to}", String::from_utf8_lossy(callee));
            (from, line)
        })
        .collect();
    // stable: the edges of one def stay in site order
    lines.sort_by(|a, b| a.0.cmp(&b.0));
    lines.into_iter().map(|(_, line)| line).collect()
}

/// What one entry gives: the tree (the project's defs only: an external, an unresolved site and a count are not in it), every edge of the whole graph,
/// and the counters above 0. An entry that is no def says so.
fn render_entry(project: &Project, root: &Path, group: &Group, entry: &str) -> String {
    let (qualname, occurrence) = match entry.rsplit_once('#') {
        Some((name, n)) if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => {
            (name, n.parse().expect("an occurrence"))
        }
        _ => (entry, 0),
    };
    let mut out = format!("== {} {entry}\n", group.file);
    let entry = Entry::Def {
        qualname,
        occurrence,
    };
    let Some(graph) = call_graph(project, &group.file, &entry, &options(), &Cancel::new()) else {
        out.push_str("no def\n");
        return out;
    };
    check_invariants(&graph);
    out.push_str(&to_text(&render(
        &graph,
        &graph.entries[0],
        &RenderOptions::default(),
    )));
    let edges = edges_as_text(&graph, root);
    if edges.is_empty() {
        out.push_str("edges: none\n");
    } else {
        out.push_str("edges:\n");
        for line in edges {
            out.push_str(&format!("{line}\n"));
        }
    }
    let counters: Vec<String> = graph
        .stats
        .language
        .iter()
        .filter(|(_, count)| **count > 0)
        .map(|(key, count)| format!("{key}={count}"))
        .collect();
    if !counters.is_empty() {
        out.push_str(&format!("counters: {}\n", counters.join(", ")));
    }
    out
}

/// What a row gives, group by group. `policy` and `context` are the shared loads; any other workspace is a copy of its fixture, loaded for the group (one
/// load of its own at a time, `common::one_load`) and opened at its folder: `owner/ws/app` is the copy of `owner`, opened at `ws/app`.
fn render_row(row: &Row, groups: &[Group]) -> String {
    let mut out = String::new();
    for group in groups {
        let entries = |project: &Project, root: &Path| {
            group
                .entries
                .iter()
                .map(|entry| render_entry(project, root, group, entry))
                .collect::<String>()
        };
        out.push_str(&match group.workspace.as_str() {
            name @ ("policy" | "context") => {
                let project = if name == "policy" {
                    policy()
                } else {
                    context()
                };
                let root = tmp_root().join(format!("policy_table_{name}"));
                entries(&project, &root)
            }
            workspace => {
                let _load = one_load();
                let (top, below) = workspace.split_once('/').unwrap_or((workspace, ""));
                let copy = copy_of_fixture(top, &format!("policy_table_{top}"));
                let root = copy.join(below);
                let project = Project::open(&root, Path::new(&group.file), &LoadOptions::default())
                    .unwrap_or_else(|error| panic!("row `{}`: {error:?}", row.id));
                entries(&project, &root)
            }
        });
    }
    out
}

#[test]
fn every_row_of_the_table_has_a_fixture_and_a_snapshot() {
    let found = problems(TABLE, &tests_dir());
    assert!(found.is_empty(), "{}", found.join("\n"));
    assert!(rows(TABLE).len() > 40, "the table was not read");
}

#[test]
fn the_check_finds_every_kind_of_problem() {
    let tests = tests_dir();
    let table = |rows: &str| {
        format!(
            "| Row | Feature | Policy | Changed by | Fixture |\n|---|---|---|---|---|\n{rows}\n"
        )
    };
    let one = |row: &str| problems(&table(row), &tests);
    let said = |found: Vec<String>, sentence: &str| {
        assert!(
            found.iter().any(|p| p == sentence),
            "`{sentence}` is not in {found:?}"
        );
    };
    // a row with no fixture word
    said(
        one("| `site` | text | | | nothing written |"),
        "row `site`: no fixture",
    );
    // a workspace that is not there, a file that is not there, a folder alone that is not there
    said(
        one("| `site` | text | | | `gone` `src/calls.rs` `plain_calls` |"),
        "row `site`: the workspace `gone` is not there",
    );
    said(
        one("| `site` | text | | | `policy` `src/gone.rs` `plain_calls` |"),
        "row `site`: the file `src/gone.rs` is not in `policy`",
    );
    said(
        one("| `def` | text | | | `gone/fixtures` |"),
        "row `def`: the folder `gone/fixtures` is not there",
    );
    // a file with no entry
    said(
        one("| `site` | text | | | `policy` `src/calls.rs` |"),
        "row `site`: the file `src/calls.rs` has no entry",
    );
    // a file before any workspace, and a workspace with no file
    said(
        one("| `site` | text | | | `src/calls.rs` `plain_calls` |"),
        "row `site`: the file `src/calls.rs` has no workspace",
    );
    said(
        one("| `site` | text | | | `policy` `src/calls.rs` `plain_calls`; `context` |"),
        "row `site`: the workspace `context` has no file",
    );
    // a row that has no snapshot
    said(
        one("| `no-such-row` | text | | | `policy` `src/calls.rs` `plain_calls` |"),
        "row `no-such-row`: no snapshot",
    );
    // the same id twice
    said(
        one(
            "| `site` | a | | | `policy` `src/calls.rs` `plain_calls` |\n| `site` | b | | | `policy` `src/calls.rs` `plain_calls` |",
        ),
        "row `site`: the id is empty or used twice",
    );
    // every snapshot of the real table is stale in a table that has one row only
    said(
        one("| `site` | text | | | `policy` `src/calls.rs` `plain_calls` |"),
        "snapshot `branches.txt`: no row of the table builds it",
    );
    // a group that begins with a file keeps the workspace of the group before it, and a row that is right has no problem of its own
    let right = one(
        "| `site` | text | | | `policy` `src/calls.rs` `plain_calls`; `src/shapes.rs` `Rec::mk`; `context` `src/generic.rs` `both` |",
    );
    assert!(
        !right.iter().any(|p| p.starts_with("row `site`")),
        "{right:?}"
    );
    // a table is read from its `| Row |` header to the first line that is not a row: the lines before and after are not rows
    let framed = format!(
        "| `ignored` | before the table |\n{}\nprose, and `a` `b`\n| `after` | | | | `policy` |\n",
        table("| `site` | text | | | `policy` `src/calls.rs` `plain_calls` |").trim_end()
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
            expect_file(&snapshot_path(&row.id), &render_row(&row, groups));
        });
        if compared.is_err() {
            differ.push(row.id.clone());
        }
    }
    assert!(differ.is_empty(), "rows that differ: {differ:?}");
}
