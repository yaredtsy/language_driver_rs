//! The context workspace (RS4 §8, `tests/fixtures/context/`): the programs layer 2 is about, with their layer-1 trees and `open_*` counts as one reviewed
//! snapshot. Each RC phase changes some of it on purpose; none writes a new program under pressure.

use std::path::Path;

use analyzer_testkit::graph::{assert_deterministic, check_invariants};
use rust_analysis::{
    Cancel, Entry, Graph, LoadOptions, MarkerStyle, Options, Project, RenderOptions, breakdown,
    call_graph, render, to_text,
};

mod common;
use common::{CONTEXT_ENTRIES as ENTRIES, copy_of_fixture};

fn graph(project: &Project, file: &str, qualname: &str) -> Graph {
    call_graph(
        project,
        file,
        &Entry::Def {
            qualname,
            occurrence: 0,
        },
        &Options::new(MarkerStyle::docstring("ID")),
        &Cancel::new(),
    )
    .unwrap_or_else(|| panic!("{file}:{qualname} is an entry"))
}

#[test]
fn the_layer_one_trees_and_open_counts_of_the_context_workspace() {
    let copy = copy_of_fixture("context", "context_trees");
    let project = Project::open(&copy, Path::new("src/lib.rs"), &LoadOptions::default())
        .expect("the fixture loads");
    assert_eq!(project.warnings(), [], "a complete load");
    let mut out = String::new();
    for (file, qualname) in ENTRIES {
        let graph = graph(&project, file, qualname);
        check_invariants(&graph);
        let tree = to_text(&render(
            &graph,
            &graph.entries[0],
            &RenderOptions::default(),
        ));
        out.push_str(&format!(
            "=== {file} {qualname}\n{tree}\n{}\n",
            breakdown(&graph)
        ));
    }
    let snapshot = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/trees/context.txt");
    analyzer_testkit::expect_file(&snapshot, &out);
    // the same request, the same graph
    assert_deterministic(|| graph(&project, "src/generic.rs", "both"));
}
