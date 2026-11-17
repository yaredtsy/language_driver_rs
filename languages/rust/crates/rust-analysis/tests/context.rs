//! The context workspace (RS4 §8, `tests/fixtures/context/`): the programs layer 2 is about, with their layer-1 trees and `open_*` counts as one reviewed
//! snapshot. Each RC phase changes some of it on purpose; none writes a new program under pressure.

use std::path::Path;

use analyzer_testkit::graph::{assert_deterministic, check_invariants};
use rust_analysis::{
    Cancel, Entry, Graph, LoadOptions, MarkerStyle, Options, Project, RenderOptions, breakdown,
    call_graph, render, to_text,
};

mod common;
use common::copy_of_fixture;

/// (file, qualname, what the program is about). The entries are the callers, and the generics, callbacks and trait objects they reach.
const ENTRIES: [(&str, &str); 14] = [
    // a generic called from two callers
    ("src/generic.rs", "both"),
    ("src/generic.rs", "emit"),
    // a callback: a `fn` pointer, a parameter bounded by `Fn`, a closure
    ("src/callback.rs", "use_callbacks"),
    ("src/callback.rs", "apply"),
    ("src/callback.rs", "apply_generic"),
    // a `dyn` in a local, and as a parameter
    ("src/dyn_local.rs", "dyn_in_a_local"),
    ("src/dyn_param.rs", "call_dyn"),
    // a factory: a trait object, and an `impl Trait` the engine sees through
    ("src/factory.rs", "use_factory"),
    ("src/factory.rs", "use_impl_factory"),
    // a struct holding a `Box<dyn Writer>`
    ("src/holder.rs", "use_holder"),
    ("src/holder.rs", "Holder::run"),
    ("src/holder.rs", "Holder"),
    // a recursive generic
    ("src/recursive.rs", "start"),
    ("src/recursive.rs", "countdown"),
];

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
