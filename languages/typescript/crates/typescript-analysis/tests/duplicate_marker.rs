//! D34 rule 1: in one file the first def in source order owns a marker text; a later def with the same text is named by where it is, and counted.

use std::path::{Path, PathBuf};

mod common;

use common::edges_as_text;
use typescript_analysis::{
    Cancel, DefInfo, EdgeTo, Entry, Frame, Graph, MarkerStyle, Options, Placement, Project,
    RenderOptions, StableDef, WireOptions, WireStyle, breakdown, call_graph, render, to_text, wire,
};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/duplicate-marker")
}

fn project() -> Project {
    Project::open(&root(), Path::new("src/main.ts")).unwrap()
}

fn graph_of(entry: &str) -> Graph {
    let entry = Entry::Def {
        qualname: entry,
        occurrence: 0,
    };
    call_graph(
        &project(),
        "src/main.ts",
        &entry,
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .unwrap()
}

fn tree(entry: &str) -> String {
    let graph = graph_of(entry);
    to_text(&render(
        &graph,
        &graph.entries[0],
        &RenderOptions::default(),
    ))
}

fn frame(entry: &str, merged: bool) -> Frame {
    let graph = graph_of(entry);
    let style = WireStyle {
        function_prefix: "F/".into(),
        class_prefix: "C/".into(),
    };
    wire(
        &graph,
        &graph.entries[0],
        &project(),
        &WireOptions::new(style, merged, RenderOptions::default()),
    )
}

fn duplicates(graph: &Graph) -> Option<u64> {
    graph.stats.language.get("duplicate_marker").copied()
}

fn def_named<'g>(graph: &'g Graph, qname: &str) -> &'g DefInfo {
    graph.defs.iter().find(|def| def.qname == qname).unwrap()
}

#[test]
fn the_second_def_with_a_marker_is_drawn_as_itself() {
    // `a` calls `x` and `b` calls `y`: each shows its own calls, once (today `b` is drawn as `a`, and `a` as the one that comes first)
    assert_eq!(
        edges_as_text(&graph_of("main"), &root()),
        [
            "a | x | call x",
            "b | y | call y",
            "main | a | call a",
            "main | b | call b",
        ]
    );
    assert_eq!(tree("main"), "main\n  a\n    x\n  b\n    y\n");
}

#[test]
fn the_merged_wire_has_two_nodes_and_counts_each_once() {
    let main = frame("main", true);
    let names: Vec<&str> = main
        .children
        .iter()
        .map(|child| child.target_qualname.as_str())
        .collect();
    assert_eq!(names, ["a", "b"]);
    assert!(main.children.iter().all(|child| child.call_count == 0));
}

#[test]
fn the_first_keeps_the_marker_and_the_second_is_named_by_where_it_is() {
    let graph = graph_of("main");
    assert_eq!(
        def_named(&graph, "a").id,
        StableDef::Marked("SAME".to_owned())
    );
    assert_eq!(
        def_named(&graph, "b").id,
        StableDef::Path {
            module: "src/main.ts".into(),
            qname: "b".into(),
            nth: 0
        }
    );
    let wire = frame("main", false);
    assert_eq!(wire.children[0].target_qualname, "a");
    assert_eq!(wire.children[1].target_qualname, "b");
    assert_eq!(wire.children[1].target_id, "");
    assert_ne!(wire.children[0].target_id, "");
}

#[test]
fn the_counter_is_once_per_def_in_the_graph() {
    // `b` is called twice and is one def
    assert_eq!(duplicates(&graph_of("twice")), Some(1));
    assert_eq!(duplicates(&graph_of("main")), Some(1));
    let summary = breakdown(&graph_of("main"));
    assert_eq!(summary.duplicate_marker, 1);
    assert!(
        summary
            .to_string()
            .contains("because a def of the same file owns their marker: 1"),
        "{summary}"
    );
}

#[test]
fn three_carriers_of_one_marker_count_two() {
    let graph = graph_of("three");
    assert_eq!(duplicates(&graph), Some(2));
    let calls = graph
        .edges
        .iter()
        .filter(|edge| matches!(edge.to, EdgeTo::Context(..)))
        .count();
    // `three` calls p, q and r, and each calls `x`
    assert_eq!(calls, 6);
}

#[test]
fn a_class_and_a_function_with_one_marker_are_two_defs() {
    let graph = graph_of("mixed");
    assert_eq!(duplicates(&graph), Some(1));
    assert_eq!(
        def_named(&graph, "K").id,
        StableDef::Marked("KF".to_owned())
    );
    assert!(matches!(def_named(&graph, "f").id, StableDef::Path { .. }));
}

#[test]
fn a_def_asked_for_alone_has_the_id_it_has_under_main() {
    // the reason the rule stops at the file: an id must not depend on what the request reaches
    let under_main = graph_of("main");
    let alone = graph_of("b");
    assert_eq!(def_named(&alone, "b").id, def_named(&under_main, "b").id);
    let alone_a = graph_of("a");
    assert_eq!(def_named(&alone_a, "a").id, def_named(&under_main, "a").id);
    // `b` alone is the one def of its graph that is demoted
    assert_eq!(duplicates(&alone), Some(1));
    assert_eq!(duplicates(&alone_a), None);
}

#[test]
fn a_copy_above_the_original_owns_the_marker_known_and_accepted() {
    let graph = graph_of("above");
    assert_eq!(
        def_named(&graph, "copy").id,
        StableDef::Marked("UP".to_owned())
    );
    assert!(matches!(
        def_named(&graph, "original").id,
        StableDef::Path { .. }
    ));
    assert_eq!(duplicates(&graph), Some(1));
}

#[test]
fn the_outline_still_reports_the_marker_on_both() {
    let project = project();
    for name in ["a", "b"] {
        let found = project.defs_named("src/main.ts", name, STYLE).unwrap();
        assert_eq!(found[0].marker.as_deref(), Some("SAME"), "{name}");
    }
}

#[test]
fn a_graph_with_no_duplicate_has_no_counter_and_no_line() {
    let none = Project::open(&root().join("../three-files"), Path::new("src/main.ts")).unwrap();
    let entry = Entry::Def {
        qualname: "run",
        occurrence: 0,
    };
    let graph = call_graph(
        &none,
        "src/main.ts",
        &entry,
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .unwrap();
    assert_eq!(duplicates(&graph), None);
    assert!(!breakdown(&graph).to_string().contains("owns"));
}

/// The known limit of rule 1: it reads one file. The same marker in a second file is one node, and core counts the def it dropped (D2).
#[test]
fn the_same_marker_in_a_second_file_is_one_node_and_is_counted() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/limits/duplicate-marker-across");
    let project = Project::open(&root, Path::new("src/main.ts")).unwrap();
    let entry = Entry::Def {
        qualname: "main",
        occurrence: 0,
    };
    let graph = call_graph(
        &project,
        "src/main.ts",
        &entry,
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .unwrap();
    assert_eq!(graph.stats.merged_defs, 1);
    assert!(!graph.stats.language.contains_key("duplicate_marker"));
    let summary = breakdown(&graph);
    assert_eq!(summary.merged_defs, 1);
    assert!(
        summary
            .to_string()
            .contains("their calls are drawn under it): 1"),
        "{summary}"
    );
    // one node for both: `b` is drawn as `a`, and each shows the other's calls (the known limit, pinned; `merged_defs` is the count)
    let drawn = to_text(&render(
        &graph,
        &graph.entries[0],
        &RenderOptions::default(),
    ));
    assert_eq!(drawn, "main\n  a\n    x\n    y\n  a\n    x\n    y\n");
    // a graph with no merged def says nothing
    let none = breakdown(&graph_of("main"));
    assert_eq!(none.merged_defs, 0);
    assert!(!none.to_string().contains("dropped because"));
}
