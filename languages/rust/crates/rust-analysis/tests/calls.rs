//! `call_graph` through the public API (RS4 §8, §9 step 4): the first trees of the policy workspace, the invariants and determinism of its graphs, the
//! entries that are `None`, and what a load that could not do as asked says in the graph's `Stats.language`.

use std::path::{Path, PathBuf};

use analyzer_testkit::graph::{assert_deterministic, check_invariants};
use rust_analysis::{
    Cancel, DefKind, Entry, Graph, LoadOptions, MarkerStyle, Options, Project, RenderOptions,
    breakdown, call_graph, render, to_text,
};

mod common;
use common::copy_of_fixture;

fn options() -> Options {
    Options::new(MarkerStyle::docstring("ID"))
}

fn open(root: &Path, file: &str) -> Project {
    Project::open(root, Path::new(file), &LoadOptions::default()).expect("the fixture loads")
}

fn graph(project: &Project, file: &str, qualname: &str, occurrence: u16) -> Option<Graph> {
    call_graph(
        project,
        file,
        &Entry::Def {
            qualname,
            occurrence,
        },
        &options(),
        &Cancel::new(),
    )
}

fn policy(name: &str) -> (PathBuf, Project) {
    let copy = copy_of_fixture("policy", name);
    let project = open(&copy, "src/lib.rs");
    (copy, project)
}

/// Every entry of the first-trees snapshot: a function, a method, a const's initializer, a struct and an enum (a construction with nothing to enter),
/// recursion, a project macro defined elsewhere, an `impl Trait` the engine sees through, one it does not, a built-in derive, a std call, a duplicate marker.
const ENTRIES: [(&str, &str, u16); 21] = [
    ("src/calls.rs", "plain_calls", 0),
    ("src/calls.rs", "method_chain", 0),
    ("src/calls.rs", "constructions", 0),
    ("src/calls.rs", "closures", 0),
    ("src/calls.rs", "nested_items", 0),
    ("src/calls.rs", "macro_from_another_module", 0),
    ("src/calls.rs", "recurse_a", 0),
    ("src/calls.rs", "FROM_CONST", 0),
    ("src/calls.rs", "Wrapper", 0),
    ("src/macros.rs", "std_arguments", 0),
    ("src/macros.rs", "project_wrapping_external", 0),
    ("src/dispatch.rs", "generic_by_value", 0),
    ("src/dispatch.rs", "impl_return", 0),
    ("src/dispatch.rs", "impl_return_of_a_parameter", 0),
    ("src/dispatch.rs", "fn_pointer", 0),
    ("src/shapes.rs", "derived", 0),
    ("src/shapes.rs", "E", 0),
    ("src/lib.rs", "Writer::twice", 0),
    ("src/lib.rs", "uses_the_twins", 0),
    ("src/external.rs", "std_calls", 0),
    ("src/external.rs", "std_naming", 0),
];

fn text(graph: &Graph) -> String {
    to_text(&render(graph, &graph.entries[0], &RenderOptions::default()))
}

/// The first trees, each with its breakdown, as one reviewed snapshot. The invariants hold for every graph, and the same request gives the same graph.
#[test]
fn the_first_trees_of_the_policy_workspace() {
    let (_copy, project) = policy("calls_trees");
    let mut out = String::new();
    for (file, qualname, occurrence) in ENTRIES {
        let graph = graph(&project, file, qualname, occurrence)
            .unwrap_or_else(|| panic!("{file}:{qualname} is an entry"));
        check_invariants(&graph);
        out.push_str(&format!("=== {file} {qualname}\n{}\n", text(&graph)));
        out.push_str(&breakdown(&graph).to_string());
        out.push('\n');
    }
    let snapshot = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/trees/policy.txt");
    analyzer_testkit::expect_file(&snapshot, &out);
}

#[test]
fn the_same_request_gives_the_same_graph() {
    let (_copy, project) = policy("calls_deterministic");
    for (file, qualname, occurrence) in [
        ("src/calls.rs", "closures", 0),
        ("src/dispatch.rs", "impl_return_of_a_parameter", 0),
        ("src/external.rs", "std_calls", 0),
    ] {
        assert_deterministic(|| graph(&project, file, qualname, occurrence).unwrap());
    }
}

/// What is not an entry is `None`, and so is everything the plan says: a file the load does not hold, a kind that cannot start a graph, code a `cfg`
/// turns off, an occurrence that is not there, and `Entry::File` (nothing runs at the top of a Rust file).
#[test]
fn what_is_not_an_entry_is_none() {
    let (_copy, project) = policy("calls_none");
    let none = |file: &str, qualname: &str, occurrence: u16| {
        assert!(
            graph(&project, file, qualname, occurrence).is_none(),
            "{file}:{qualname}:{occurrence}"
        );
    };
    none("src/stray.rs", "stray", 0);
    none("src/missing.rs", "plain", 0);
    none("src/lib.rs", "Writer", 0);
    none("src/lib.rs", "tests", 0);
    none("src/lib.rs", "make_fn!", 0);
    none("src/shapes.rs", "Alias", 0);
    none("src/lib.rs", "disabled", 0);
    none("src/lib.rs", "plain", 1);
    assert!(
        call_graph(
            &project,
            "src/lib.rs",
            &Entry::File,
            &options(),
            &Cancel::new()
        )
        .is_none()
    );
    // and one that is: the first of two `Rec::get`, the second, and a def written inside a body
    assert!(graph(&project, "src/shapes.rs", "Rec::get", 1).is_some());
    assert!(graph(&project, "src/calls.rs", "nested_items::{fn}::inner", 0).is_some());
}

/// `defs_named`: a qualname is not unique (D15), and the list is what a tool shows. `None` for a file the load does not hold.
#[test]
fn defs_named_lists_the_matches_and_picks_none() {
    let (_copy, project) = policy("calls_defs_named");
    let style = MarkerStyle::docstring("ID");
    let found = project
        .defs_named("src/shapes.rs", "Rec::get", style)
        .unwrap();
    assert_eq!(
        found
            .iter()
            .map(|d| (d.occurrence, d.kind.clone()))
            .collect::<Vec<_>>(),
        [(0, DefKind::Method), (1, DefKind::Method)]
    );
    assert!(found[0].range.start < found[1].range.start);
    // a marker is reported; a def with none says so
    let marked = project.defs_named("src/lib.rs", "marked", style).unwrap();
    assert_eq!(marked[0].marker.as_deref(), Some("marked-one"));
    assert_eq!(
        project.defs_named("src/lib.rs", "plain", style).unwrap()[0].marker,
        None
    );
    // no def of that name is an empty list, a file the load does not hold is `None`
    assert_eq!(
        project.defs_named("src/lib.rs", "nope", style),
        Some(vec![])
    );
    assert_eq!(project.defs_named("src/stray.rs", "stray", style), None);
}

/// The load's warnings and what it skipped are in the graph's `Stats.language`, as counts, and the breakdown says them first.
#[test]
fn a_graph_of_a_warned_load_carries_its_warning() {
    // cargo refuses the manifest, our reader reads it: no `std`, no dependencies, and the layout the reader could not follow is counted
    let copy = copy_of_fixture("cargo_fails", "calls_cargo_fails");
    let project = open(&copy, "src/lib.rs");
    let graph = graph(&project, "src/lib.rs", "go", 0).unwrap();
    check_invariants(&graph);
    assert_eq!(graph.stats.language["warning_cargo_failed"], 1);
    assert_eq!(graph.stats.language["layout_skipped"], 1);
    let summary = breakdown(&graph);
    assert_eq!(summary.warnings, ["cargo_failed"]);
    assert_eq!(summary.skipped["layout_skipped"], 1);
    let shown = summary.to_string();
    let first = shown.lines().nth(1).unwrap();
    assert_eq!(first, "  warnings: cargo_failed", "{shown}");
    // the project's own calls still resolve without `std`
    assert_eq!(summary.calls, 2, "{shown}");

    // a file with no manifest: rung 4
    let lone = copy_of_fixture("lone", "calls_lone");
    let project = open(&lone, "main.rs");
    let graph = graph_of(&project, "main.rs", "run");
    assert_eq!(graph.stats.language["warning_no_manifest"], 1);
    assert_eq!(breakdown(&graph).calls, 1);

    // a load that did not run the proc macro: its derive's method is not there, which is counted, and the switches left off are counted too
    let derive = copy_of_fixture("derive", "calls_derive");
    let project = Project::open(
        &derive,
        Path::new("app/src/lib.rs"),
        &LoadOptions::default(),
    )
    .unwrap();
    let graph = graph_of(&project, "app/src/lib.rs", "use_it");
    assert_eq!(graph.stats.language["unknown_method"], 1);
    assert_eq!(graph.stats.language["build_scripts_skipped"], 1);
    assert_eq!(graph.stats.language["proc_macros_skipped"], 1);
    let shown = breakdown(&graph).to_string();
    assert!(
        shown.contains("skipped on purpose: build_scripts_skipped 1, proc_macros_skipped 1"),
        "{shown}"
    );
}

/// The bridge's three ways a def of a local crate is still external (RS4 §5) are counted in the graph: a crate beside the root (`outside_root`, one per
/// call), and a def a macro made (`macro_generated_target`). The tree names them and does not enter them.
#[test]
fn a_call_into_a_local_crate_that_is_not_entered_is_counted() {
    let copy = copy_of_fixture("owner", "calls_owner");
    // the root is the package `app`: `member` (a workspace member) and `inside` (a path dependency) are local crates whose files are not under it
    let project = open(&copy.join("ws/app"), "src/lib.rs");
    let graph = graph_of(&project, "src/lib.rs", "run");
    check_invariants(&graph);
    assert_eq!(graph.stats.language["outside_root"], 2);
    assert_eq!(graph.stats.language["macro_generated_target"], 1);
    let shown = breakdown(&graph).to_string();
    assert!(
        shown.contains("calls into source outside the root, not walked: 2"),
        "{shown}"
    );
    assert!(
        shown.contains("calls to a def a macro or a derive made, not entered: 1"),
        "{shown}"
    );

    // with the root over the whole fixture the same two calls are project code, and are walked
    let project = open(&copy, "ws/app/src/lib.rs");
    let graph = graph_of(&project, "ws/app/src/lib.rs", "run");
    assert!(!graph.stats.language.contains_key("outside_root"));
    assert_eq!(graph.stats.language["macro_generated_target"], 1);
}

/// A graph of a complete load has no `warning_*` and no `*_skipped`: the keys are absent when 0.
#[test]
fn a_clean_load_adds_nothing() {
    let (_copy, project) = policy("calls_clean");
    let graph = graph(&project, "src/calls.rs", "plain_calls", 0).unwrap();
    assert!(
        graph
            .stats
            .language
            .keys()
            .all(|key| !key.starts_with("warning_") && !key.ends_with("_skipped")),
        "{:?}",
        graph.stats.language
    );
}

fn graph_of(project: &Project, file: &str, qualname: &str) -> Graph {
    graph(project, file, qualname, 0).unwrap_or_else(|| panic!("{file}:{qualname}"))
}
