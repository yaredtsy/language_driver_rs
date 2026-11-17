//! M step 4 (`docs/03-typescript/09-module-def.md` §3.3): code that runs when a class or an enum is **defined** belongs to the body that declares it, drawn at its
//! source position among that body's other sites. **The expected trees were written by hand before the first run.**

mod common;

use std::path::{Path, PathBuf};

use analyzer_testkit::graph::check_invariants;
use common::{STYLE, edges_as_text};
use typescript_analysis::{
    Cancel, Entry, Graph, Options, Project, RenderOptions, breakdown, call_graph, render, to_text,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/definition-code")
}

fn graph_of(entry: &Entry<'_>) -> Graph {
    let project = Project::open(&root(), Path::new("src/main.ts")).unwrap();
    let graph = call_graph(
        &project,
        "src/main.ts",
        entry,
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .unwrap_or_else(|| panic!("no entry {entry:?}"));
    check_invariants(&graph);
    graph
}

fn def(qualname: &str) -> Entry<'_> {
    Entry::Def {
        qualname,
        occurrence: 0,
    }
}

fn tree(graph: &Graph) -> String {
    to_text(&render(graph, &graph.entries[0], &RenderOptions::default()))
}

fn counters(graph: &Graph) -> Vec<(String, u64)> {
    breakdown(graph).uncovered.into_iter().collect()
}

#[test]
fn a_top_level_class_and_enum_are_drawn_under_the_module_in_source_order() {
    let graph = graph_of(&Entry::File);
    // `@dec`; `@UseGuards(guard)` (the call, and `guard` handed over: a passed edge, D22; the call of its result is unresolved, not drawn); `extends base()`;
    // `static s`; the static block; the computed name; a member's decorator; a parameter's decorator. Not `x = made()`: it runs at construction. Then the
    // enum: `A = make()` and `C = A + make()`. Not `static handler = () => init()`: an arrow held by a field is a def of its own. Last, `use(class { … })`:
    // the call, then `static s = make()` and the instance initializer `x = made()`, which an anonymous class has no def to run at construction (below).
    assert_eq!(
        tree(&graph),
        "src/main.ts
  dec
  UseGuards
  guard [Passed]
  base
  make
  init
  key
  memberDec
  paramDec
  make
  make
  use
  make
  made
"
    );
    // nothing of it is counted: the module does not construct `Top`
    assert_eq!(counters(&graph), []);
}

#[test]
fn a_hand_over_in_a_decorator_is_a_passed_edge_of_the_declaring_body() {
    let graph = graph_of(&Entry::File);
    let edges = edges_as_text(&graph, &root());
    assert!(
        edges.contains(&"src/main.ts | guard | pass guard".to_owned()),
        "{edges:#?}"
    );
    assert!(
        edges.contains(&"src/main.ts | UseGuards(guard) | unresolved decorator_result".to_owned()),
        "{edges:#?}"
    );
}

#[test]
fn a_class_and_an_enum_in_a_function_are_drawn_in_that_function() {
    let graph = graph_of(&def("declares"));
    // `@dec`, `extends base()`, `static s = make()` and the enum's `A = make()`; not `y = made()`
    assert_eq!(tree(&graph), "declares\n  dec\n  base\n  make\n  make\n");
    assert_eq!(counters(&graph), []);
}

#[test]
fn a_class_and_an_enum_in_a_namespace_are_drawn_in_the_namespace() {
    let graph = graph_of(&def("N"));
    assert_eq!(tree(&graph), "N\n  make\n  init\n");
    assert_eq!(counters(&graph), []);
}

#[test]
fn a_construction_counts_the_instance_initializers_and_no_definition_code() {
    // `Top`: its definition code is the module's, not this construction's; `x = made()` runs at construction (D25), so it stays counted
    let graph = graph_of(&def("Top"));
    assert_eq!(tree(&graph), "Top\n");
    assert_eq!(
        counters(&graph),
        [("construct_initializer_sites".to_owned(), 1)]
    );
    let graph = graph_of(&def("declares.<locals>.Local"));
    assert_eq!(tree(&graph), "declares.<locals>.Local\n");
    assert_eq!(
        counters(&graph),
        [("construct_initializer_sites".to_owned(), 1)]
    );
}

#[test]
fn an_enum_entry_draws_nothing_because_its_initializers_are_the_declaring_bodys() {
    let graph = graph_of(&def("E"));
    assert_eq!(tree(&graph), "E\n");
    assert_eq!(counters(&graph), []);
}

#[test]
fn an_anonymous_class_expression_is_part_of_the_body_that_writes_it() {
    // `use(class { static s = make(); x = made(); m() { init(); } })`: the class is no def, so the module draws `static s` and also the instance initializer
    // `x = made()` (there is no class def to be constructed and count it), as it draws the calls in an anonymous function. The method is a def of its own.
    let graph = graph_of(&Entry::File);
    let edges: Vec<String> = edges_as_text(&graph, &root())
        .into_iter()
        .filter(|edge| edge.starts_with("src/main.ts |"))
        .collect();
    assert_eq!(
        &edges[edges.len() - 3..],
        [
            "src/main.ts | use | call use",
            "src/main.ts | make | call make",
            "src/main.ts | made | call made",
        ]
    );
    // nothing is counted for it, and the method's `init` is not a second one of the module's (`Top`'s static block has the only one)
    assert_eq!(counters(&graph), []);
    let inits = edges
        .iter()
        .filter(|edge| edge.contains("| init |"))
        .count();
    assert_eq!(inits, 1, "{edges:#?}");
    // the method keeps its own call
    let method = graph_of(&def("m"));
    assert_eq!(tree(&method), "m\n  init\n");
}
