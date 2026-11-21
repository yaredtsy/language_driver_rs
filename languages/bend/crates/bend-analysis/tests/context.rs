//! The context layer (`docs/08-bend/06-context-layer-design.md`), step 2: what a key does before `resolve_from_env` answers anything. The rows of the precise table
//! and their snapshots come with step 3; these are plain assertions on the graph's contexts.

use std::path::Path;

use analyzer_core::graph::StableDef;
use analyzer_testkit::graph::{assert_deterministic, check_invariants};
use bend_analysis::{
    Book, Cancel, ContextId, EdgeTo, Graph, Limits, MarkerStyle, Options, Precision, call_graph,
};

mod common;
use common::Program;

fn graph(book: &Book, entry: &str, precision: Precision) -> Graph {
    let options = Options::new(MarkerStyle::above("ID")).with_precision(precision);
    let graph = call_graph(
        book,
        Path::new("main.bend"),
        entry,
        &options,
        &Limits::default(),
        &Cancel::new(),
    )
    .expect("a graph");
    check_invariants(&graph);
    graph
}

fn fixture(name: &str) -> Book {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/context")
        .join(name);
    Book::open(&root, Path::new("main.bend")).expect("the book opens")
}

fn named(qualname: &str) -> impl Fn(&&ContextId) -> bool + '_ {
    move |id| matches!(&id.func, StableDef::Path { qname, .. } if qname == qualname)
}

/// The contexts of the def named `qualname`.
fn contexts_of(graph: &Graph, qualname: &str) -> Vec<ContextId> {
    graph
        .contexts
        .iter()
        .map(|c| &c.id)
        .filter(named(qualname))
        .cloned()
        .collect()
}

/// Every edge as (from def, site, what it reaches by def or kind), with the context digests left out: what "no new edge" compares.
fn edges_without_digests(graph: &Graph) -> Vec<String> {
    graph
        .edges
        .iter()
        .map(|e| {
            let to = match &e.to {
                EdgeTo::Context(id, _) => format!("{:?} passed={}", id.func, e.passed),
                other => format!("{other:?}"),
            };
            format!("{:?} {:?} {to}", e.from.func, e.site)
        })
        .collect()
}

const ZEROS: [u8; 16] = [0; 16];

#[test]
fn two_keys_has_two_contexts_of_each_and_no_new_edge() {
    let book = fixture("two-keys");
    let default = graph(&book, "main", Precision::Default);
    let precise = graph(&book, "main", Precision::Precise);

    let each = contexts_of(&default, "each");
    assert_eq!(each.len(), 1, "layer 1: one context of `each`");
    assert_eq!(each[0].digest, ZEROS);

    let each = contexts_of(&precise, "each");
    assert_eq!(
        each.len(),
        2,
        "one context of `each` for each of `work` and `other`"
    );
    assert_ne!(each[0].digest, each[1].digest);
    assert!(
        each.iter().all(|c| c.digest != ZEROS),
        "a key that names something is not the open one"
    );

    // the three calls of `each` from `main` reach the two contexts: the first and the third the same one
    let reached: Vec<&ContextId> = precise
        .edges
        .iter()
        .filter_map(|e| match &e.to {
            EdgeTo::Context(id, _) if !e.passed && named("each")(&id) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(reached.len(), 3);
    assert_eq!(reached[0], reached[2]);
    assert_ne!(reached[0], reached[1]);

    // `resolve_from_env` answers nothing in this step: the distinct edges are layer 1's (the same sites reaching the same defs, `f(x)` unresolved), and the only
    // difference is that each of the two contexts of `each` has its own copy of its one site
    let mut distinct = edges_without_digests(&precise);
    distinct.dedup();
    assert_eq!(distinct, edges_without_digests(&default));
    assert_eq!(precise.edges.len(), default.edges.len() + 1);
    // the other defs have the one open context they had
    for def in ["main", "work", "other"] {
        let ids = contexts_of(&precise, def);
        assert_eq!(ids.len(), 1, "{def}");
        assert_eq!(ids[0].digest, ZEROS, "{def}");
    }
    assert_deterministic(|| graph(&book, "main", Precision::Precise));
}

#[test]
fn a_parameter_the_def_never_calls_is_not_in_the_key() {
    let book = fixture("not-relevant");
    let precise = graph(&book, "main", Precision::Precise);
    let log = contexts_of(&precise, "log");
    assert_eq!(
        log.len(),
        1,
        "`f` is not relevant: `log(work, 1)` and `log(other, 2)` are one context"
    );
    assert_eq!(log[0].digest, ZEROS);
    assert!(precise.contexts.iter().all(|c| c.id.digest == ZEROS));
    assert_eq!(
        edges_without_digests(&precise),
        edges_without_digests(&graph(&book, "main", Precision::Default))
    );
}

#[test]
fn the_digest_of_one_key_is_the_same_from_two_entries() {
    let program = Program::new(&[(
        "main.bend",
        "def work(x: U32) -> U32:\n  x\n\ndef each(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n\ndef main() -> U32:\n  each(work, 1)\n\ndef second() -> U32:\n  each(work, 2)\n",
    )]);
    let book = program.book("main.bend");
    let from_main = contexts_of(&graph(&book, "main", Precision::Precise), "each");
    let from_second = contexts_of(&graph(&book, "second", Precision::Precise), "each");
    assert_eq!((from_main.len(), from_second.len()), (1, 1));
    assert_ne!(from_main[0].digest, ZEROS);
    assert_eq!(from_main, from_second);
}

/// The key is the callee's relevant parameters the caller named, and nothing else (BC-D2): `f` is passed to a project def, so it is relevant; the constructor field
/// and Base's `U32.show` are not.
#[test]
fn a_parameter_is_relevant_when_called_or_passed_to_a_project_def() {
    let program = Program::new(&[(
        "main.bend",
        concat!(
            "def work(x: U32) -> U32:\n  x\n\n",
            "def each(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n\n",
            "def relay(f: U32 -> U32, x: U32) -> U32:\n  each(f, x)\n\n",
            "def boxed(f: U32 -> U32, x: U32) -> U32:\n  Box{f}\n\n",
            "def main() -> U32:\n  a = relay(work, 1)\n  boxed(work, 2)\n",
        ),
    )]);
    let book = program.book("main.bend");
    let precise = graph(&book, "main", Precision::Precise);
    // `relay` passes `f` along to `each`: relevant, so keyed; `boxed` hands it to a constructor: not
    assert_eq!(contexts_of(&precise, "relay").len(), 1);
    assert_ne!(contexts_of(&precise, "relay")[0].digest, ZEROS);
    assert_eq!(contexts_of(&precise, "boxed")[0].digest, ZEROS);
}
