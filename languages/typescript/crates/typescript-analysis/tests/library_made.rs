//! A function a library made is named, not unresolved (D56, policy row `library-made`, `docs/03-typescript/12-context-layer/11-later.md` §4.1): an `External`
//! named after the call that made it, counted `library_made_function` beside the edge.
//!
//! **Written before it was run:** the names are the ones the design says.

mod common;

use std::path::Path;

use common::{STYLE, call_graph, edges_as_text, tests_dir};
use typescript_analysis::{Cancel, Entry, Options, Project, breakdown};

fn graph() -> typescript_analysis::Graph {
    let root = tests_dir().join("policy");
    let project = Project::open(&root, Path::new("src/library_made.tsx")).unwrap();
    call_graph(
        &project,
        "src/library_made.tsx",
        &Entry::Def {
            qualname: "library_made",
            occurrence: 0,
        },
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .expect("the def is there")
}

#[test]
fn the_four_functions_a_library_made_are_externals_named_after_their_maker() {
    let graph = graph();
    let edges = edges_as_text(&graph, &tests_dir().join("policy"));
    let from_it: Vec<&str> = edges
        .iter()
        .filter_map(|edge| edge.strip_prefix("library_made | "))
        .collect();
    assert_eq!(
        from_it,
        [
            "useState | external hooks useState",
            "setOpen | external hooks useState()[1]",
            "useMutation | external hooks useMutation",
            "mutate | external hooks useMutation().mutate",
            "useNavigate | external hooks useNavigate",
            "navigate | external hooks useNavigate()",
            "useStore | external hooks create()()",
            "useNavigate | external hooks useNavigate",
            "later | unresolved function_typed_value",
            "useMutation | external hooks useMutation",
            "go | unresolved function_typed_value",
            "localMaker | call localMaker",
            "made | unresolved function_typed_value",
        ]
    );
}

#[test]
fn it_is_counted_beside_the_edge_and_leaves_the_function_typed_counts() {
    let graph = graph();
    let language = |key: &str| graph.stats.language.get(key).copied().unwrap_or(0);
    assert_eq!(language("library_made_function"), 4);
    assert_eq!(language("function_typed_value"), 3);
    assert_eq!(language("function_typed_value_from_call_result"), 3);
}

#[test]
fn the_breakdown_says_how_many() {
    let text = breakdown(&graph()).to_string();
    assert!(
        text.contains("  library-made functions (named after the call that made them): 4\n"),
        "{text}"
    );
}
