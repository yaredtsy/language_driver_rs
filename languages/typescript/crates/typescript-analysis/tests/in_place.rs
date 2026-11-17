//! H12 (`docs/03-typescript/10-review-fixes.md` §6): two kinds of def written where they are reached without a direct call. (a) `new` of a named class expression is a
//! `Construct`, as `new C()` is; (b) a named function expression through `.call` / `.apply` is counted in `uncovered_call_apply`, as a name is.
//! **The expected trees and counters were written before the first run.**

use std::path::{Path, PathBuf};

use analyzer_testkit::graph::check_invariants;
use typescript_analysis::{
    Cancel, Entry, Graph, MarkerStyle, Options, Placement, Project, RenderOptions, breakdown,
    call_graph, render, to_text,
};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/in-place")
}

fn graph_of(entry: &str) -> Graph {
    let project = Project::open(&root(), Path::new("src/main.ts")).unwrap();
    let entry = Entry::Def {
        qualname: entry,
        occurrence: 0,
    };
    let graph = call_graph(
        &project,
        "src/main.ts",
        &entry,
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .unwrap_or_else(|| panic!("no entry {entry:?}"));
    check_invariants(&graph);
    graph
}

fn tree(graph: &Graph) -> String {
    to_text(&render(graph, &graph.entries[0], &RenderOptions::default()))
}

type Counts = Vec<(String, u64)>;

/// The counters of what no edge covers, and the reasons of the unresolved edges.
fn counters(graph: &Graph) -> (Counts, Counts) {
    let summary = breakdown(graph);
    (
        summary.uncovered.into_iter().collect(),
        summary.reasons.into_iter().collect(),
    )
}

fn pairs(pairs: &[(&str, u64)]) -> Counts {
    pairs.iter().map(|(k, n)| ((*k).to_owned(), *n)).collect()
}

#[test]
fn new_of_a_named_class_expression_is_a_construction_of_it() {
    let graph = graph_of("newNamedClass");
    // the class, with its constructor's `g` under it: the tree of `new N()`
    assert_eq!(
        tree(&graph),
        "newNamedClass\n  newNamedClass.<locals>.N\n    g\n"
    );
    assert_eq!(counters(&graph), (vec![], vec![]));
    assert_eq!(breakdown(&graph).constructs, 1);
    // one edge to the class at the `new`, and the hand-over rule (D22) adds no Passed edge for it
    assert!(graph.edges.iter().all(|edge| !edge.passed));
    assert_eq!(
        graph.edges.len(),
        2,
        "the construction, and the constructor's `g`"
    );
}

#[test]
fn the_construction_counts_the_instance_initializers_as_any_class_does() {
    let graph = graph_of("newNamedClassWithField");
    assert_eq!(
        tree(&graph),
        "newNamedClassWithField\n  newNamedClassWithField.<locals>.M\n"
    );
    assert_eq!(
        counters(&graph),
        (pairs(&[("construct_initializer_sites", 1)]), vec![])
    );
}

#[test]
fn a_type_assertion_is_looked_through() {
    let graph = graph_of("newAssertedClass");
    assert_eq!(
        tree(&graph),
        "newAssertedClass\n  newAssertedClass.<locals>.A\n    g\n"
    );
    assert_eq!(counters(&graph), (vec![], vec![]));
}

#[test]
fn new_of_an_anonymous_class_is_what_it_was() {
    // no def to construct: unresolved, `no_declaration`, and nothing drawn
    let graph = graph_of("newAnonymousClass");
    assert_eq!(tree(&graph), "newAnonymousClass\n");
    assert_eq!(counters(&graph), (vec![], pairs(&[("no_declaration", 1)])));
}

#[test]
fn a_named_function_expression_through_call_or_apply_is_counted() {
    for entry in ["callWrapped", "applyWrapped"] {
        let graph = graph_of(entry);
        // the edge to `Function.call` / `Function.apply` stays (an external, not drawn); `g` of the wrapped function is in no tree here, and is counted
        assert_eq!(tree(&graph), format!("{entry}\n"), "{entry}");
        assert_eq!(breakdown(&graph).externals, 1, "{entry}");
        assert_eq!(
            counters(&graph),
            (pairs(&[("call_apply", 1)]), vec![]),
            "{entry}"
        );
    }
}

#[test]
fn an_anonymous_function_expression_through_call_is_not_counted() {
    // its `g` is this body's, drawn, so nothing is lost
    let graph = graph_of("callAnonymous");
    assert_eq!(tree(&graph), "callAnonymous\n  g\n");
    assert_eq!(counters(&graph), (vec![], vec![]));
}

#[test]
fn a_named_function_through_call_by_name_is_counted_as_before() {
    let graph = graph_of("callNamed");
    assert_eq!(counters(&graph).0, pairs(&[("call_apply", 1)]));
}
