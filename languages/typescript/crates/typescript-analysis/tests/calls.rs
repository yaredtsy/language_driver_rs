//! The call graph through the public API: the shared scenarios in TypeScript, the breakdown of why an edge is unresolved, and the properties every graph has.

use std::path::{Path, PathBuf};

use analyzer_testkit::graph::check_invariants;
use analyzer_testkit::scenarios::{Level, Subject, run};
use typescript_analysis::{
    Basis, Cancel, Edge, EdgeTo, Entry, Graph, MarkerStyle, Options, Project, Reason, StableDef,
    breakdown, call_graph,
};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: typescript_analysis::Placement::Docstring,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn graph_of(root: &Path, file: &str, entry: &str) -> Graph {
    let project = Project::open(root, Path::new(file)).unwrap();
    let entry = Entry {
        qualname: entry,
        occurrence: 0,
    };
    call_graph(&project, file, &entry, &Options::new(STYLE), &Cancel::new())
        .unwrap_or_else(|| panic!("no def `{}` in {file}", entry.qualname))
}

fn scenario_graph(program: &Path) -> Option<Graph> {
    let project = Project::open(program, Path::new("main.ts")).ok()?;
    let entry = Entry {
        qualname: "main",
        occurrence: 0,
    };
    call_graph(
        &project,
        "main.ts",
        &entry,
        &Options::new(STYLE),
        &Cancel::new(),
    )
}

#[test]
fn the_shared_scenarios_in_typescript_give_their_trees() {
    let subject = Subject {
        folder: "typescript",
        scenarios: "../../../../scenarios",
        graph: scenario_graph,
    };
    // straight-line, recursion-direct and method-on-class; the others are not written in TypeScript yet (steps 5 and later)
    assert_eq!(run(&subject, Level::Default), 3);
}

#[test]
fn every_resolved_edge_rests_on_the_type_and_every_unresolved_edge_says_why() {
    let graph = graph_of(&fixture("resolve"), "src/main.ts", "callers");
    check_invariants(&graph);
    let mut resolved = 0;
    for Edge { to, .. } in &graph.edges {
        match to {
            EdgeTo::Context(_, basis) | EdgeTo::External(_, basis) => {
                assert_eq!(*basis, Basis::Type);
                resolved += 1;
            }
            EdgeTo::Construct { basis, .. } => assert_eq!(*basis, Basis::Type),
            EdgeTo::Unresolved(reason) => assert!(!reason.as_str().is_empty()),
            _ => unreachable!("an edge kind this test does not know"),
        }
    }
    assert!(resolved > 20);
}

#[test]
fn the_breakdown_says_why_each_unresolved_edge_is_unresolved_and_adds_up() {
    let graph = graph_of(&fixture("resolve"), "src/main.ts", "callers");
    let summary = breakdown(&graph);
    // `cb()` is a function-typed value, two calls are on `any`, one has a computed name (`tests/fixtures/resolve`)
    let reasons: Vec<(&str, u64)> = summary
        .reasons
        .iter()
        .map(|(reason, count)| (reason.as_str(), *count))
        .collect();
    assert_eq!(
        reasons,
        [
            ("any_or_untyped", 2),
            ("computed_name", 1),
            ("function_typed_value", 1)
        ]
    );
    // every unresolved edge has exactly one reason, and the counters in `Stats.language` are the same numbers
    assert_eq!(summary.unresolved, graph.stats.unresolved);
    assert_eq!(
        summary.reasons.values().sum::<u64>(),
        graph.stats.unresolved
    );
    for (reason, count) in &summary.reasons {
        assert_eq!(graph.stats.language.get(reason), Some(count), "{reason}");
    }
    // the four externals: a package, a repo .d.ts, and two bundled-library methods
    assert_eq!(summary.externals, 4);
}

#[test]
fn the_breakdown_of_the_fixture_is_reviewed() {
    let graph = graph_of(&fixture("resolve"), "src/main.ts", "callers");
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/breakdown-resolve.txt"),
        &breakdown(&graph).to_string(),
    );
}

#[test]
fn a_def_with_a_marker_is_named_by_it_in_the_graph() {
    let graph = graph_of(&fixture("resolve"), "src/main.ts", "callers");
    let marked: Vec<&StableDef> = graph
        .defs
        .iter()
        .map(|def| &def.id)
        .filter(|id| matches!(id, StableDef::Marked(_)))
        .collect();
    assert_eq!(marked, [&StableDef::Marked("PLAIN-0001".into())]);
}

#[test]
fn the_same_input_gives_the_same_graph() {
    let root = fixture("resolve");
    assert_eq!(
        graph_of(&root, "src/main.ts", "callers"),
        graph_of(&root, "src/main.ts", "callers")
    );
}

#[test]
fn a_cancelled_request_marks_the_entry_instead_of_inventing_edges() {
    let project = Project::open(&fixture("resolve"), Path::new("src/main.ts")).unwrap();
    let cancel = Cancel::new();
    cancel.cancel();
    let entry = Entry {
        qualname: "callers",
        occurrence: 0,
    };
    let graph = call_graph(
        &project,
        "src/main.ts",
        &entry,
        &Options::new(STYLE),
        &cancel,
    )
    .unwrap();
    check_invariants(&graph);
    assert!(graph.edges.is_empty());
    assert!(!graph.stats.cuts.is_empty());
}

#[test]
fn a_def_that_is_not_there_has_no_graph() {
    let project = Project::open(&fixture("resolve"), Path::new("src/main.ts")).unwrap();
    let opts = Options::new(STYLE);
    let none = |file: &str, qualname: &str, occurrence: u16| {
        call_graph(
            &project,
            file,
            &Entry {
                qualname,
                occurrence,
            },
            &opts,
            &Cancel::new(),
        )
        .is_none()
    };
    assert!(none("src/main.ts", "nope", 0));
    assert!(none("src/main.ts", "callers", 1));
    assert!(none("src/gone.ts", "callers", 0));
}

#[test]
fn an_unresolved_reason_is_a_plain_string_that_survives_the_graph() {
    // `Reason` is the key of the counter, owned, so a graph read back from disk could hold it
    assert_eq!(Reason::new("computed_name").as_str(), "computed_name");
}
