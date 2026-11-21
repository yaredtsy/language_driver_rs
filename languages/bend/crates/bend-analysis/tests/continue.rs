//! `Session::graph_from` (`docs/08-bend/06-context-layer-design.md` §5, BC-D7): a node a request cut is continued under the key it had, so that the pieces make the
//! tree that was asked for whole. The session keeps the canonical key of every digest it has made, and drops them with the book.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

use analyzer_core::graph::StableDef;
use analyzer_testkit::graph::check_invariants;
use bend_analysis::{
    Budget, Cancel, Context, ContextId, Entry, Graph, Precision, RenderOptions, Session, Status,
    render, to_text,
};

mod common;
use common::{Program, options_at};

fn entry(qualname: &str) -> Entry<'_> {
    Entry::Def {
        qualname,
        occurrence: 0,
    }
}

fn open_fixture(folder: &str, precision: Precision) -> Session {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/context")
        .join(folder);
    Session::open(&root, Path::new("main.bend"), options_at(precision)).expect("the fixture opens")
}

fn whole(session: &Session, qualname: &str) -> Graph {
    let graph = session
        .graph(
            "main.bend",
            &entry(qualname),
            &Budget::default(),
            &Cancel::new(),
        )
        .expect("a graph");
    check_invariants(&graph);
    graph
}

fn tree(graph: &Graph) -> String {
    to_text(&render(graph, &graph.entries[0], &RenderOptions::default()))
}

fn frontier(graph: &Graph) -> Vec<ContextId> {
    graph
        .contexts
        .iter()
        .filter(|c| c.status == Status::Frontier)
        .map(|c| c.id.clone())
        .collect()
}

/// The tree asked for whole, built from pieces: `main` cut at depth 1, then every frontier node continued with `graph_from`, repeatedly, until nothing is cut. The pieces
/// are put together by hand: every def, every edge, and each context as `Finished` once any piece walked it.
fn in_pieces(session: &Session) -> Graph {
    let budget = Budget::new(Some(1), None);
    let cancel = Cancel::new();
    let first = session
        .graph("main.bend", &entry("main"), &budget, &cancel)
        .expect("a graph");
    let mut queue: VecDeque<ContextId> = frontier(&first).into();
    let mut asked: Vec<ContextId> = Vec::new();
    let mut pieces = vec![first];
    while let Some(id) = queue.pop_front() {
        if asked.contains(&id) {
            continue;
        }
        let piece = session
            .graph_from(&id, &budget, &cancel)
            .expect("a graph from a context the session listed");
        assert_eq!(
            piece.stats.language.get("context_not_found"),
            None,
            "{id:?} is a context the session listed"
        );
        asked.push(id);
        queue.extend(frontier(&piece));
        pieces.push(piece);
    }
    assert!(pieces.len() > 2, "the cut must have cut something");
    let mut contexts: BTreeMap<ContextId, Status> = BTreeMap::new();
    for piece in &pieces {
        for context in &piece.contexts {
            let status = contexts.entry(context.id.clone()).or_insert(context.status);
            if context.status == Status::Finished {
                *status = Status::Finished;
            }
        }
    }
    let roots = pieces[0].entries.clone();
    Graph::new(
        roots,
        pieces.iter().flat_map(|p| p.defs.clone()).collect(),
        contexts
            .into_iter()
            .map(|(id, status)| {
                let mut context = Context::new(id);
                context.status = status;
                context
            })
            .collect(),
        pieces.iter().flat_map(|p| p.edges.clone()).collect(),
        Default::default(),
    )
}

#[test]
fn the_pieces_make_the_tree_asked_for_whole() {
    for folder in [
        "param-flow",
        "template-flow",
        "param-through-param",
        "recursion-keyed",
        "return-set-argument",
    ] {
        let session = open_fixture(folder, Precision::Precise);
        let asked = whole(&session, "main");
        let pieces = in_pieces(&session);
        assert_eq!(tree(&asked), tree(&pieces), "{folder}: the tree");
        assert_eq!(asked.edges, pieces.edges, "{folder}: the edges");
        assert_eq!(asked.contexts, pieces.contexts, "{folder}: the contexts");
        assert_eq!(asked.defs, pieces.defs, "{folder}: the defs");
        assert!(
            asked.contexts.iter().any(|c| c.id.digest != [0; 16]),
            "{folder}: there are keyed contexts to continue"
        );
    }
}

/// A continued node is drawn under the key it had: `each` asked for by name is the open one, and its `f(x)` has no target; `each` continued from the whole tree has
/// `work` under it.
#[test]
fn a_continued_node_has_its_key_and_the_def_asked_for_by_name_does_not() {
    let session = open_fixture("param-def", Precision::Precise);
    let asked = whole(&session, "main");
    let each = asked
        .contexts
        .iter()
        .map(|c| c.id.clone())
        .find(|id| matches!(&id.func, StableDef::Path { qname, .. } if qname == "each"))
        .expect("a context of each");
    assert_ne!(each.digest, [0; 16]);
    let continued = session
        .graph_from(&each, &Budget::default(), &Cancel::new())
        .expect("a graph");
    assert_eq!(tree(&continued), "each\n  work\n");
    assert_eq!(continued.stats.language.get("context_not_found"), None);
    assert_eq!(tree(&whole(&session, "each")), "each\n");
}

/// A digest the session did not make, and a def that is not in the book.
#[test]
fn an_unknown_digest_counts_context_not_found_and_gives_the_open_key_graph() {
    let session = open_fixture("param-def", Precision::Precise);
    let each = StableDef::Path {
        module: "main.bend".to_owned(),
        qname: "each".to_owned(),
        nth: 0,
    };
    let unknown = ContextId {
        func: each.clone(),
        digest: [7; 16],
    };
    let graph = session
        .graph_from(&unknown, &Budget::default(), &Cancel::new())
        .expect("a graph");
    check_invariants(&graph);
    assert_eq!(graph.stats.language.get("context_not_found"), Some(&1));
    // the graph is the open key's: what `graph` gives for `each`, but for the counter, and the root's digest is zeros
    let open = whole(&session, "each");
    assert_eq!(tree(&graph), tree(&open));
    assert_eq!(graph.edges, open.edges);
    assert_eq!(graph.contexts, open.contexts);
    assert_eq!(graph.entries, open.entries);

    // zeros are the open key, and nothing is missing
    let zeros = session
        .graph_from(&ContextId::of(each), &Budget::default(), &Cancel::new())
        .expect("a graph");
    assert_eq!(zeros, open);

    // no def has the id
    for func in [
        StableDef::Path {
            module: "main.bend".to_owned(),
            qname: "nope".to_owned(),
            nth: 0,
        },
        StableDef::Path {
            module: "elsewhere.bend".to_owned(),
            qname: "each".to_owned(),
            nth: 0,
        },
        StableDef::Marked("no-such-marker".to_owned()),
    ] {
        assert!(
            session
                .graph_from(&ContextId::of(func), &Budget::default(), &Cancel::new())
                .is_none()
        );
    }
}

/// The keys belong to the book: after a reload the session has none, a digest it made before is unknown, and a request that was asked again makes them again.
#[test]
fn after_a_reload_the_kept_keys_are_gone() {
    let mut session = open_fixture("param-def", Precision::Precise);
    let each = whole(&session, "main")
        .contexts
        .iter()
        .map(|c| c.id.clone())
        .find(|id| matches!(&id.func, StableDef::Path { qname, .. } if qname == "each"))
        .expect("a context of each");
    let ask = |session: &Session| {
        session
            .graph_from(&each, &Budget::default(), &Cancel::new())
            .expect("a graph")
    };
    assert_eq!(tree(&ask(&session)), "each\n  work\n");
    assert!(session.reload().reopened);
    let after = ask(&session);
    assert_eq!(after.stats.language.get("context_not_found"), Some(&1));
    assert_eq!(tree(&after), "each\n", "the open key's graph");
    // asking for the whole tree again makes the keys again
    whole(&session, "main");
    assert_eq!(ask(&session).stats.language.get("context_not_found"), None);
}

/// A refresh that finds nothing changed keeps the keys; one that builds the book again drops them.
#[test]
fn a_refresh_keeps_the_keys_until_the_book_changes() {
    let program = Program::new(&[(
        "main.bend",
        "def work(x: U32) -> U32:\n  x\n\ndef each(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n\ndef main() -> U32:\n  each(work, 1)\n",
    )]);
    let mut session = Session::open(
        &program.root,
        Path::new("main.bend"),
        options_at(Precision::Precise),
    )
    .expect("opens");
    let each = whole(&session, "main")
        .contexts
        .iter()
        .map(|c| c.id.clone())
        .find(|id| matches!(&id.func, StableDef::Path { qname, .. } if qname == "each"))
        .expect("a context of each");
    let not_found = |session: &Session| {
        session
            .graph_from(&each, &Budget::default(), &Cancel::new())
            .expect("a graph")
            .stats
            .language
            .get("context_not_found")
            .copied()
    };
    assert!(!session.refresh().reopened);
    assert_eq!(
        not_found(&session),
        None,
        "nothing changed: the key is kept"
    );
    std::fs::write(
        program.root.join("main.bend"),
        "def work(x: U32) -> U32:\n  x\n\ndef each(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n\ndef main() -> U32:\n  each(work, 2)\n",
    )
    .expect("writes");
    assert!(session.refresh().reopened);
    assert_eq!(
        not_found(&session),
        Some(1),
        "the book was built again: the keys went"
    );
}

/// A def named by its marker is found by it, and so is one in a key.
#[test]
fn a_context_of_a_marked_def_is_continued() {
    let program = Program::new(&[(
        "main.bend",
        "# ID: aaaaaaaa-0000-0000-0000-000000000001\ndef work(x: U32) -> U32:\n  x\n\n# ID: aaaaaaaa-0000-0000-0000-000000000002\ndef each(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n\ndef main() -> U32:\n  each(work, 1)\n",
    )]);
    let session = Session::open(
        &program.root,
        Path::new("main.bend"),
        options_at(Precision::Precise),
    )
    .expect("opens");
    let each = whole(&session, "main")
        .contexts
        .iter()
        .map(|c| c.id.clone())
        .find(|id| id.func == StableDef::Marked("aaaaaaaa-0000-0000-0000-000000000002".to_owned()))
        .expect("each is named by its marker");
    assert_ne!(each.digest, [0; 16]);
    let continued = session
        .graph_from(&each, &Budget::default(), &Cancel::new())
        .expect("a graph");
    assert_eq!(tree(&continued), "each\n  work\n");
    assert_eq!(continued.stats.language.get("context_not_found"), None);
}

/// Under `Precision::Default` every digest is zeros, and `graph_from` is `graph` for that def.
#[test]
fn under_default_graph_from_is_graph() {
    for folder in ["param-flow", "template-flow", "two-keys"] {
        let session = open_fixture(folder, Precision::Default);
        let asked = whole(&session, "main");
        for context in &asked.contexts {
            assert_eq!(context.id.digest, [0; 16]);
            let StableDef::Path { qname, .. } = &context.id.func else {
                panic!("no markers here");
            };
            let by_name = whole(&session, qname);
            let continued = session
                .graph_from(&context.id, &Budget::default(), &Cancel::new())
                .expect("a graph");
            assert_eq!(continued, by_name, "{folder}: {qname}");
        }
    }
}
