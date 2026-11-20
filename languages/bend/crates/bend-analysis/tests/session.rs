//! `Session::open` and `Session::graph` (BD5, `docs/08-bend/03-phase-2-defs-and-calls.md` §4): with no budget a session gives the graph `call_graph` gives,
//! with a budget a request's frontier is counted and its nodes are `Cut`, a limit that cuts is counted in `Stats.cuts`, and a cancelled request is a sound
//! graph, marked. Following the disk is `refresh.rs`.

use std::path::Path;

use analyzer_testkit::graph::check_invariants;
use bend_analysis::{
    Budget, Cancel, Cut, Entry, Graph, Limits, MarkerStyle, OpenError, Options, RenderOptions,
    Session, Status, render, to_text,
};

mod common;
use common::{Program, graph_of, options};

/// `main` calls `a`, which calls `b`, which calls `c`: a chain four deep. `add_zero` is a law and its proof, for the occurrence of an entry.
const CHAIN: &str = "\
def c() -> U32:
  1

def b() -> U32:
  c()

def a() -> U32:
  b()

def main() -> U32:
  a()

law add_zero:
  for x: Nat
  {x == x : Nat}

def add_zero(x):
  {==}
";

fn open(program: &Program) -> Session {
    Session::open(&program.root, Path::new("main.bend"), options()).expect("the program opens")
}

fn main_entry() -> Entry<'static> {
    Entry::Def {
        qualname: "main",
        occurrence: 0,
    }
}

fn tree(graph: &Graph) -> String {
    to_text(&render(graph, &graph.entries[0], &RenderOptions::default()))
}

fn chain() -> Program {
    Program::new(&[("main.bend", CHAIN)])
}

#[test]
fn a_session_is_send_and_sync() {
    fn is<T: Send + Sync>() {}
    is::<Session>();
}

#[test]
fn open_gives_the_tree_of_the_book_and_nothing_is_lost() {
    let program = chain();
    let session = open(&program);
    assert_eq!(session.lost(), None);
    assert!(session.book().is_some());
    let graph = session
        .graph(
            "main.bend",
            &main_entry(),
            &Budget::default(),
            &Cancel::new(),
        )
        .expect("main is there");
    check_invariants(&graph);
    assert_eq!(tree(&graph), "main\n  a\n    b\n      c\n");
}

#[test]
fn open_says_why_it_cannot() {
    let program = chain();
    assert!(matches!(
        Session::open(&program.root, Path::new("nope.bend"), options()),
        Err(OpenError::NoSuchFile)
    ));
    let nowhere = program.root.join("not-a-folder");
    assert!(matches!(
        Session::open(&nowhere, Path::new("main.bend"), options()),
        Err(OpenError::NoSuchFile)
    ));
    // a file beside the root is a file, and not the project's (D33)
    let beside = Program::new(&[("app/main.bend", CHAIN), ("other.bend", CHAIN)]);
    assert!(matches!(
        Session::open(
            &beside.root.join("app"),
            Path::new("../other.bend"),
            options()
        ),
        Err(OpenError::OutsideRoot)
    ));
    assert!(
        OpenError::OutsideRoot
            .to_string()
            .contains("not under the root")
    );
}

#[test]
fn with_no_budget_a_session_gives_the_graph_call_graph_gives() {
    let program = chain();
    let session = open(&program);
    let book = program.book("main.bend");
    for name in ["main", "a", "b", "c", "add_zero"] {
        let by_name = graph_of(&book, "main.bend", name).expect("a def");
        let entry = Entry::Def {
            qualname: name,
            // `call_graph` takes the proof of a name; the proof is the second `add_zero`
            occurrence: u16::from(name == "add_zero"),
        };
        let by_session = session
            .graph("main.bend", &entry, &Budget::default(), &Cancel::new())
            .expect("a def");
        assert_eq!(tree(&by_session), tree(&by_name), "{name}");
        assert_eq!(by_session.stats.frontier, 0, "{name}");
    }
}

#[test]
fn an_entry_is_named_by_its_occurrence_and_a_file_has_no_code_of_its_own() {
    let program = chain();
    let session = open(&program);
    let budget = Budget::default();
    let cancel = Cancel::new();
    let at = |qualname, occurrence| {
        session.graph(
            "main.bend",
            &Entry::Def {
                qualname,
                occurrence,
            },
            &budget,
            &cancel,
        )
    };
    // the law is the first `add_zero` and a leaf, the proof the second; a third is nothing
    assert!(at("add_zero", 0).is_some());
    assert!(at("add_zero", 1).is_some());
    assert!(at("add_zero", 2).is_none());
    assert!(at("nope", 0).is_none());
    assert!(
        session
            .graph("other.bend", &main_entry(), &budget, &cancel)
            .is_none()
    );
    assert!(
        session
            .graph("main.bend", &Entry::File, &budget, &cancel)
            .is_none()
    );
}

#[test]
fn a_budget_changes_the_graph_and_the_frontier_is_counted() {
    let program = chain();
    let session = open(&program);
    let at = |budget: &Budget| {
        let graph = session
            .graph("main.bend", &main_entry(), budget, &Cancel::new())
            .expect("main is there");
        check_invariants(&graph);
        graph
    };

    let whole = at(&Budget::default());
    assert_eq!(whole.stats.frontier, 0);

    // `Some(0)` walks nothing: the root is the one context, on the frontier
    let zero = at(&Budget::new(Some(0), None));
    assert_eq!(zero.stats.frontier, 1);
    assert_eq!(tree(&zero), "main [Cut]\n");

    // depth 2: `main` and `a` are walked, `b` is listed and not walked
    let two = at(&Budget::new(Some(2), None));
    assert_eq!(two.stats.frontier, 1);
    assert!(two.contexts.iter().any(|c| c.status == Status::Frontier));
    assert_eq!(tree(&two), "main\n  a\n    b [Cut]\n");

    // a budget of contexts: bodies read in this request, counted as the plan says
    let few = at(&Budget::new(None, Some(2)));
    assert!(few.stats.frontier > 0);
}

/// A request for the def a frontier ended at continues from there: the tree under it is the one the whole request draws under it.
#[test]
fn asking_for_the_def_a_frontier_ended_at_continues_from_there() {
    let program = chain();
    let session = open(&program);
    let b = Entry::Def {
        qualname: "b",
        occurrence: 0,
    };
    let from_b = session
        .graph("main.bend", &b, &Budget::default(), &Cancel::new())
        .expect("b is there");
    assert_eq!(tree(&from_b), "b\n  c\n");
}

/// A limit protects the cost of a request and cuts: the cut is marked and counted in `Stats.cuts`, unlike a budget's frontier.
#[test]
fn a_limit_that_cuts_is_counted_in_the_stats() {
    let program = chain();
    let mut options = Options::new(MarkerStyle::above("ID"));
    options.limits = Limits::new(2, None);
    let session = Session::open(&program.root, Path::new("main.bend"), options).unwrap();
    let graph = session
        .graph(
            "main.bend",
            &main_entry(),
            &Budget::default(),
            &Cancel::new(),
        )
        .expect("main is there");
    check_invariants(&graph);
    assert!(
        graph
            .stats
            .cuts
            .get(&Cut::MaxContexts)
            .copied()
            .unwrap_or(0)
            > 0,
        "{:?}",
        graph.stats.cuts
    );
    assert_eq!(graph.stats.frontier, 0, "a limit is not a budget");
}

#[test]
fn a_request_cancelled_before_it_starts_walks_no_body() {
    let program = chain();
    let session = open(&program);
    let cancel = Cancel::new();
    cancel.cancel();
    let graph = session
        .graph("main.bend", &main_entry(), &Budget::default(), &cancel)
        .expect("a cancelled request is still a graph");
    check_invariants(&graph);
    assert_eq!(graph.stats.walks, 0, "no body was read");
    assert_eq!(graph.stats.cuts.get(&Cut::Cancelled).copied(), Some(1));
    assert!(
        graph
            .contexts
            .iter()
            .any(|c| c.status == Status::Incomplete(Cut::Cancelled))
    );
}

/// Cancel at the n-th place a request looks at its flag, for every n until the request is no longer cut: each is a sound graph, marked, and the last
/// one is the whole tree. No timing: `Cancel::after_checks` counts.
#[test]
fn a_cancel_at_each_check_stops_the_walk_and_leaves_a_sound_graph() {
    let program = chain();
    let session = open(&program);
    let whole = tree(
        &session
            .graph(
                "main.bend",
                &main_entry(),
                &Budget::default(),
                &Cancel::new(),
            )
            .unwrap(),
    );
    let mut cut = 0;
    let mut n = 1;
    loop {
        let graph = session
            .graph(
                "main.bend",
                &main_entry(),
                &Budget::default(),
                &Cancel::after_checks(n),
            )
            .unwrap();
        check_invariants(&graph);
        if graph.stats.cuts.get(&Cut::Cancelled).copied().unwrap_or(0) == 0 {
            assert_eq!(
                tree(&graph),
                whole,
                "past the last check it is the whole tree"
            );
            break;
        }
        cut += 1;
        n += 1;
        assert!(n < 100, "a request of four defs has more than 100 checks");
    }
    assert!(cut > 0, "a cancel that never cut proves nothing");
}
