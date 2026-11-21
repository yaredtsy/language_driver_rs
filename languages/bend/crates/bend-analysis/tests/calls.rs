//! `call_graph` on programs written in the test (docs/08-bend/03 §3): what an entry reaches, the ids, the sites, the cuts. The rows of the policy table have
//! their own fixtures and snapshots (`tests/policy.rs`, step 5), and the scenarios theirs (`tests/scenarios.rs`).

use std::path::Path;

use analyzer_core::graph::StableDef;
use analyzer_testkit::graph::assert_deterministic;
use bend_analysis::{
    Cancel, EdgeTo, Limits, RenderOptions, WalkedKind, call_graph, render, to_text, walked,
};

mod common;
use common::{Program, edges_as_text, graph_of, options};

fn tree(program: &Program, file: &str, entry: &str) -> String {
    let book = program.book(file);
    let graph = graph_of(&book, file, entry).expect("a graph");
    to_text(&render(
        &graph,
        &graph.entries[0],
        &RenderOptions::default(),
    ))
}

#[test]
fn a_call_to_a_def_below_and_one_through_an_alias_are_edges() {
    let program = Program::new(&[
        (
            "main.bend",
            "import ./lib.bend as L\n\ndef main() -> U32:\n  L.two(below())\n\n@unsafe def below() -> U32:\n  1\n",
        ),
        ("lib.bend", "def two(x: U32) -> U32:\n  x\n"),
    ]);
    assert_eq!(
        tree(&program, "main.bend", "main"),
        "main\n  two\n  below\n"
    );
}

#[test]
fn recursion_is_marked_by_the_renderer() {
    let program = Program::new(&[("main.bend", "def count(n: U32) -> U32:\n  count(n)\n")]);
    assert_eq!(
        tree(&program, "main.bend", "count"),
        "count\n  count [Recursive]\n"
    );
}

#[test]
fn an_entry_that_is_not_there_gives_nothing() {
    let program = Program::new(&[("main.bend", "def main() -> U32:\n  1\n")]);
    let book = program.book("main.bend");
    assert!(graph_of(&book, "main.bend", "nope").is_none());
    assert!(graph_of(&book, "other.bend", "main").is_none());
}

#[test]
fn a_site_is_relative_to_the_start_of_the_def_and_the_doc_block_is_in_it() {
    let program = Program::new(&[(
        "main.bend",
        "# Calls f.\n# ID: m-1\ndef main() -> U32:\n  f()\n\ndef f() -> U32:\n  1\n",
    )]);
    let book = program.book("main.bend");
    let graph = graph_of(&book, "main.bend", "main").expect("a graph");
    let from = graph.def(&graph.edges[0].from.func).expect("def");
    assert_eq!(from.range.start, 0, "the range starts at the comment block");
    assert_eq!(edges_as_text(&graph, book.root()), ["main | f | call f"]);
}

#[test]
fn a_marker_is_the_defs_id_and_the_second_def_with_it_is_named_by_where_it_is() {
    let program = Program::new(&[(
        "main.bend",
        "# ID: one\ndef main() -> U32:\n  other()\n\n# ID: one\ndef other() -> U32:\n  1\n\ndef plain() -> U32:\n  1\n",
    )]);
    let book = program.book("main.bend");
    let graph = graph_of(&book, "main.bend", "main").expect("a graph");
    let ids: Vec<&StableDef> = graph.defs.iter().map(|d| &d.id).collect();
    assert!(ids.contains(&&StableDef::Marked("one".into())));
    assert!(ids.contains(&&StableDef::Path {
        module: "main.bend".into(),
        qname: "other".into(),
        nth: 0
    }));
    assert_eq!(graph.stats.language.get("duplicate_marker"), Some(&1));
}

#[test]
fn a_law_and_its_proof_share_a_name_and_the_entry_is_the_one_with_code() {
    let program = Program::new(&[(
        "main.bend",
        "law f:\n  for x: Nat\n  {x == x : Nat}\n\ndef f(x):\n  helper()\n\ndef helper() -> U32:\n  1\n",
    )]);
    assert_eq!(tree(&program, "main.bend", "f"), "f\n  helper\n");
}

#[test]
fn a_call_to_a_law_with_no_proof_is_a_leaf_and_counted() {
    let program = Program::new(&[(
        "main.bend",
        "law f:\n  for x: Nat\n  {x == x : Nat}\n\ndef main() -> U32:\n  f(1)\n",
    )]);
    let book = program.book("main.bend");
    let graph = graph_of(&book, "main.bend", "main").expect("a graph");
    assert_eq!(graph.stats.language.get("open_law"), Some(&1));
    assert_eq!(
        to_text(&render(
            &graph,
            &graph.entries[0],
            &RenderOptions::default()
        )),
        "main\n  f\n"
    );
}

#[test]
fn a_def_outside_the_root_is_an_external_named_by_its_path() {
    let program = Program::new(&[
        (
            "app/main.bend",
            "import ../shared/lib.bend as Lib\n\ndef main() -> U32:\n  Lib.b(Lib.b)\n",
        ),
        ("shared/lib.bend", "def b(f: U32) -> U32:\n  1\n"),
    ]);
    let book = bend_analysis::Book::open(&program.root.join("app"), Path::new("main.bend"))
        .expect("opens");
    let graph = graph_of(&book, "main.bend", "main").expect("a graph");
    let externals: Vec<(String, String)> = graph
        .edges
        .iter()
        .filter_map(|e| match &e.to {
            EdgeTo::External(x, _) => Some((x.package.clone(), x.name.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        externals,
        [("../shared/lib.bend".to_owned(), "b".to_owned())]
    );
    // the call is counted once, and what is handed over is counted, not drawn
    assert_eq!(graph.stats.language.get("outside_root"), Some(&1));
    assert_eq!(graph.stats.language.get("external_passed"), Some(&1));
    assert_eq!(graph.stats.language.get("file_outside_root"), Some(&1));
}

#[test]
fn a_cancel_before_the_walk_gives_a_graph_that_says_so() {
    let program = Program::new(&[(
        "main.bend",
        "def main() -> U32:\n  f()\n\ndef f() -> U32:\n  1\n",
    )]);
    let book = program.book("main.bend");
    let graph = call_graph(
        &book,
        Path::new("main.bend"),
        "main",
        &options(),
        &Limits::default(),
        &Cancel::after_checks(0),
    )
    .expect("a graph");
    analyzer_testkit::graph::check_invariants(&graph);
    assert!(graph.edges.is_empty());
}

#[test]
fn the_same_input_gives_the_same_graph() {
    let program = Program::new(&[
        (
            "main.bend",
            "import ./lib.bend as L\n\ndef main() -> U32:\n  L.a(L.b(1))\n",
        ),
        (
            "lib.bend",
            "def a(x: U32) -> U32:\n  b(x)\n\ndef b(x: U32) -> U32:\n  x\n",
        ),
    ]);
    let book = program.book("main.bend");
    assert_deterministic(|| graph_of(&book, "main.bend", "main").expect("a graph"));
}

/// Two places at one span keep the order layer 1 had, the outer one first: the call of an element, then the `Array.get` it stands on (counted, not a site).
#[test]
fn a_call_and_the_sugar_it_stands_on_come_in_that_order() {
    let program = Program::new(&[(
        "main.bend",
        "import Base\n\ndef g(a: Array, i: U32) -> U32:\n  a[i](1)\n",
    )]);
    let book = program.book("main.bend");
    let g = book
        .decls()
        .iter()
        .find(|d| book.def(d.id).is_some_and(|def| def.qualname == "g"))
        .expect("g")
        .id;
    let text = &book.files()[g.file().index()].text;
    let at: Vec<WalkedKind> = walked(&book, g, &options())
        .expect("a def")
        .into_iter()
        .filter(|w| &text[w.span.start as usize..w.span.end as usize] == "a[i]")
        .map(|w| w.kind)
        .collect();
    assert_eq!(
        at,
        [
            WalkedKind::Unresolved("function_typed_value".to_owned()),
            WalkedKind::Counted("sugar_base".to_owned())
        ]
    );
}
