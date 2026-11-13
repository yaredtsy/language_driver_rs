//! Cancel: a request that is told to stop returns part of the full graph, marks what it gave up, and leaves the session able to answer the next request
//! (`docs/03-typescript/07-ts5-driver-needs.md` §4 and §7 row 5).
//!
//! **Written before it was run.** A cancel fired from another thread lands at a point nobody chose, so no number of runs shows that every point behaves.
//! `Cancel::after_checks(k)` fires on the k-th check, so the main test cancels a request at **every** check it makes, on a session that has never seen a
//! cancel, and compares what comes back with the request that was not cancelled.
//!
//! What a check is, in this crate: the solver looks at the cancel once before it walks each context (after the budget), and `walk::body` once before each
//! site. The engine is never asked (§0 of the plan), so nothing a check can cut is half-made inside it.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use analyzer_testkit::graph::check_invariants;
use common::{Case, cases, open, tests_dir};
use typescript_analysis::{
    Budget, Cancel, Cut, Edge, Entry, Frame, Graph, Mark, Node, RenderOptions, Session, Status,
    WireOptions, WireStyle, render, wire,
};

/// A request that no cancel touches, on a session that has never seen one.
fn uncancelled(case: &Case, budget: &Budget) -> Graph {
    request(&open(case), case, budget, &Cancel::new())
}

fn request(session: &Session, case: &Case, budget: &Budget, cancel: &Cancel) -> Graph {
    let entry = Entry {
        qualname: &case.entry,
        occurrence: 0,
    };
    let graph = session
        .graph(&case.file, &entry, budget, cancel)
        .unwrap_or_else(|| panic!("{}: no def `{}`", case.name, case.entry));
    check_invariants(&graph);
    graph
}

fn tree_of(graph: &Graph) -> Node {
    render(graph, &graph.entries[0], &RenderOptions::default())
}

/// The contexts and marks of every node that is **expanded**. A `Recursive` leaf stands for a context already drawn above it, and is not marked for what
/// the context is: the expanded node above it is.
fn marks_of(node: &Node, out: &mut Vec<(Vec<typescript_analysis::ContextId>, Vec<Mark>)>) {
    if !node.marks.contains(&Mark::Recursive) {
        out.push((node.contexts.clone(), node.marks.clone()));
    }
    for child in &node.children {
        marks_of(child, out);
    }
}

fn frames_with_mark(frame: &Frame, mark: &str) -> usize {
    usize::from(frame.marks.iter().any(|m| m == mark))
        + frame
            .children
            .iter()
            .map(|child| frames_with_mark(child, mark))
            .sum::<usize>()
}

/// The status of the context the request starts at.
fn root_status(graph: &Graph) -> Status {
    let id = graph.entries[0].context().expect("an entry with a context");
    graph.context(id).unwrap().status
}

fn cancelled_count(graph: &Graph) -> usize {
    graph
        .contexts
        .iter()
        .filter(|c| c.status == Status::Incomplete(Cut::Cancelled))
        .count()
}

/// The edges a request cut mid-body keeps: `edges` of the full graph from the same context, without the hand-overs (they are found after the sites), as
/// many as the cut one has. Panics unless the cut one's edges are exactly those, and end between two sites.
fn assert_first_sites(what: &str, cut: &[Edge], full: &[Edge]) {
    assert!(
        cut.iter().all(|edge| !edge.passed),
        "{what}: a passed edge in a cut body"
    );
    let calls: Vec<&Edge> = full.iter().filter(|edge| !edge.passed).collect();
    assert!(
        cut.len() <= calls.len(),
        "{what}: {} edges, the full graph has {} calls",
        cut.len(),
        calls.len()
    );
    for (got, want) in cut.iter().zip(&calls) {
        assert_eq!(got, *want, "{what}: not the first sites of the body");
    }
    if let (Some(last), Some(next)) = (cut.last(), calls.get(cut.len())) {
        assert_ne!(
            last.site, next.site,
            "{what}: the body was cut inside a site (some of its edges, not all)"
        );
    }
}

/// What one case's sweep found.
#[derive(Default, Debug)]
struct Sweep {
    /// Checks the request makes.
    checks: u64,
    /// Cuts that left an `Incomplete` context with at least one edge: a cancel that landed during a body.
    mid_body: u64,
}

/// Cancel `case` at its 1st, 2nd, … check, each on a fresh session, until a countdown outlasts the request. `budget` is the request's (with none, the
/// whole graph).
fn sweep(case: &Case, budget: &Budget) -> Sweep {
    const CAP: u64 = 100_000;
    let full = uncancelled(case, budget);
    assert!(
        full.contexts
            .iter()
            .all(|c| matches!(c.status, Status::Finished | Status::Frontier)),
        "{}: the full graph has a cut context, and nothing below can tell a cancel's cut from it",
        case.name
    );
    let mut found = Sweep::default();
    for k in 1..=CAP {
        let session = open(case);
        let graph = request(&session, case, budget, &Cancel::after_checks(k));
        if graph == full {
            // the countdown outlasted the request: it made k - 1 checks, and each of those was cut
            found.checks = k - 1;
            return found;
        }
        let name = format!("{}, cancelled at check {k}", case.name);

        for context in &graph.contexts {
            let in_full = full
                .context(&context.id)
                .unwrap_or_else(|| panic!("{name}: a context the full graph does not have"));
            let edges = graph.edges_from(&context.id);
            match context.status {
                Status::Finished | Status::Frontier => {
                    assert_eq!(context.status, in_full.status, "{name}");
                    assert_eq!(
                        edges,
                        full.edges_from(&context.id),
                        "{name}: a finished context's edges"
                    );
                }
                Status::Incomplete(Cut::Cancelled) => {
                    assert_first_sites(&name, edges, full.edges_from(&context.id));
                    found.mid_body += u64::from(!edges.is_empty());
                }
                other => panic!("{name}: a context is {other:?}"),
            }
        }
        assert_eq!(
            graph.stats.cuts,
            BTreeMap::from([(Cut::Cancelled, cancelled_count(&graph) as u64)]),
            "{name}: the cuts are the Incomplete(Cancelled) contexts"
        );
        assert!(
            cancelled_count(&graph) > 0,
            "{name}: a cut that cut nothing"
        );
        // `Stats.frontier` is the number of Frontier contexts, cut or not
        assert_eq!(
            graph.stats.frontier,
            graph
                .contexts
                .iter()
                .filter(|c| c.status == Status::Frontier)
                .count() as u64,
            "{name}"
        );

        // the tree and the wire say so
        let tree = tree_of(&graph);
        let mut nodes = Vec::new();
        marks_of(&tree, &mut nodes);
        let mut drawn_cut = std::collections::BTreeSet::new();
        for (contexts, marks) in &nodes {
            let cut = contexts.iter().any(|id| {
                matches!(
                    graph.context(id).map(|c| c.status),
                    Some(Status::Incomplete(_))
                )
            });
            assert_eq!(
                cut,
                marks.contains(&Mark::Incomplete),
                "{name}: {contexts:?} {marks:?}"
            );
            if cut {
                drawn_cut.extend(contexts.iter().cloned());
            }
        }
        let all_cut: std::collections::BTreeSet<_> = graph
            .contexts
            .iter()
            .filter(|c| matches!(c.status, Status::Incomplete(_)))
            .map(|c| c.id.clone())
            .collect();
        assert!(!all_cut.is_empty());
        assert_eq!(
            drawn_cut, all_cut,
            "{name}: every cut context is drawn, marked"
        );
        for merged in [false, true] {
            let frame = wire(
                &graph,
                &graph.entries[0],
                session.project(),
                &WireOptions::new(WireStyle::default(), merged, RenderOptions::default()),
            );
            assert!(
                frames_with_mark(&frame, "incomplete") > 0,
                "{name}: the wire (merged: {merged}) does not carry `incomplete`"
            );
        }

        // the same session, asked again with no cancel, gives what a session that never saw one gives
        let again = request(&session, case, budget, &Cancel::new());
        assert_eq!(again, full, "{name}: the next request on the session");
        assert_eq!(tree_of(&again), tree_of(&full), "{name}: its tree");
    }
    panic!("{}: still cut at check {CAP}", case.name);
}

#[test]
fn cancelled_at_every_check_a_request_returns_part_of_the_full_graph_and_the_session_recovers() {
    let cases = cases();
    assert!(
        cases.len() > 60,
        "the cases were not found: {}",
        cases.len()
    );
    let started = std::time::Instant::now();
    let (mut checks, mut mid_body, mut runs) = (0, 0, 0);
    let mut per_case = Vec::new();
    for case in &cases {
        let found = sweep(case, &Budget::default());
        checks += found.checks;
        mid_body += found.mid_body;
        runs += found.checks + 1;
        per_case.push((case.name.clone(), found.checks));
    }
    let least = per_case.iter().map(|(_, n)| *n).min().unwrap();
    let most = per_case.iter().max_by_key(|(_, n)| *n).unwrap();
    eprintln!(
        "{} cases, {checks} checks in all (per case {least} to {} at `{}`), {mid_body} of the cuts landed inside a body, {runs} requests, {:?}",
        cases.len(),
        most.1,
        most.0,
        started.elapsed()
    );
    for group in ["scenario ", "row ", "recursion "] {
        let ns: Vec<u64> = per_case
            .iter()
            .filter(|(name, _)| name.starts_with(group))
            .map(|(_, n)| *n)
            .collect();
        eprintln!(
            "    {}: {} cases, N from {} to {}",
            group.trim(),
            ns.len(),
            ns.iter().min().unwrap(),
            ns.iter().max().unwrap()
        );
    }
    // a sweep whose every cut lands between two bodies proves nothing about the check before each site
    assert!(mid_body > 0);
}

#[test]
fn a_cancel_from_another_thread_is_seen() {
    // counting cannot show this: the flag is read on the worker and set on another thread
    let case = cases().pop().unwrap();
    let session = open(&case);
    let cancel = Cancel::new();
    let other = cancel.clone();
    std::thread::spawn(move || other.cancel()).join().unwrap();
    let graph = request(&session, &case, &Budget::default(), &cancel);
    assert_eq!(root_status(&graph), Status::Incomplete(Cut::Cancelled));
    assert_eq!(
        graph.contexts.len(),
        1,
        "nothing was walked, so nothing was discovered"
    );
}

#[test]
fn a_deadline_that_has_passed_cuts_with_its_own_reason() {
    let case = cases().pop().unwrap();
    let session = open(&case);
    let cancel = Cancel::new().with_deadline(std::time::Instant::now());
    let graph = request(&session, &case, &Budget::default(), &cancel);
    assert_eq!(root_status(&graph), Status::Incomplete(Cut::Deadline));
    assert_eq!(graph.stats.cuts, BTreeMap::from([(Cut::Deadline, 1)]));
    let wired = wire(
        &graph,
        &graph.entries[0],
        session.project(),
        &WireOptions::new(WireStyle::default(), false, RenderOptions::default()),
    );
    assert_eq!(frames_with_mark(&wired, "incomplete"), 1);
}

/// A node the user can expand (`Cut`, a frontier) must not look like a node the request gave up on (`Incomplete`): the first is a question the request
/// did not ask, the second one it could not answer. A context is one or the other, and the budget is looked at first, so a context past the budget is a
/// frontier even when a cancel would have stopped it too.
#[test]
fn a_cancel_and_a_budget_never_make_one_node_both() {
    let budget = Budget::new(Some(1), None);
    let mut frontier_seen = 0;
    for case in &cases() {
        let found = sweep(case, &budget);
        // the root is at depth 0, inside the budget: cancelled at its check it is `Incomplete`, whatever the budget would say of its callees
        let session = open(case);
        let first = request(&session, case, &budget, &Cancel::after_checks(1));
        // an entry with no context (a class with no constructor) makes no check; every other makes at least one, and it is the root's
        let has_context = first.entries[0].context().is_some();
        assert_eq!(found.checks > 0, has_context, "{}", case.name);
        if has_context {
            assert_eq!(
                root_status(&first),
                Status::Incomplete(Cut::Cancelled),
                "{}",
                case.name
            );
            assert_eq!(
                first.stats.frontier, 0,
                "{}: nothing was walked, so nothing was listed",
                case.name
            );
        }
        // at the last check that still cuts
        let last = request(
            &session,
            case,
            &budget,
            &Cancel::after_checks(found.checks.max(1)),
        );
        for graph in [&first, &last] {
            let tree = tree_of(graph);
            let mut nodes = Vec::new();
            marks_of(&tree, &mut nodes);
            for (contexts, marks) in nodes {
                let statuses: Vec<Status> = contexts
                    .iter()
                    .filter_map(|id| graph.context(id))
                    .map(|c| c.status)
                    .collect();
                if statuses.contains(&Status::Frontier) {
                    frontier_seen += 1;
                    assert!(marks.contains(&Mark::Cut), "{}: {marks:?}", case.name);
                    assert!(
                        !marks.contains(&Mark::Incomplete),
                        "{}: {marks:?}",
                        case.name
                    );
                }
                if statuses.iter().any(|s| matches!(s, Status::Incomplete(_))) {
                    assert!(
                        marks.contains(&Mark::Incomplete),
                        "{}: {marks:?}",
                        case.name
                    );
                    assert!(!marks.contains(&Mark::Cut), "{}: {marks:?}", case.name);
                }
            }
            // below depth 1 every context is a frontier: the only one a cancel can cut is the root
            for context in &graph.contexts {
                if graph.entries[0].context() != Some(&context.id) {
                    assert_eq!(context.status, Status::Frontier, "{}", case.name);
                }
            }
        }
    }
    // the check above is empty if a cancelled request never lists a frontier
    assert!(frontier_seen > 0);
}

/// A class with no constructor has nothing to walk: `walk::construction` notes its counters before the solver runs, and with no `init` the solver has no
/// context to check the cancel at. So a cancel loses nothing, and the answer is the same graph (rule 8: a counter that is noted is not cut).
#[test]
fn a_class_entry_with_no_constructor_is_the_same_under_any_cancel() {
    let class_entry = |entry: &str| Case {
        name: format!("class-entry {entry}"),
        root: tests_dir().join("fixtures/class-entry"),
        file: "src/main.ts".into(),
        entry: entry.into(),
    };
    let case = class_entry("NoConstructor");
    let expected = uncancelled(&case, &Budget::default());
    assert!(
        expected.contexts.is_empty(),
        "the class has a context to cut: {:?}",
        expected.contexts
    );
    for k in 1..=3 {
        let graph = request(
            &open(&case),
            &case,
            &Budget::default(),
            &Cancel::after_checks(k),
        );
        assert_eq!(graph, expected, "cancelled at check {k}");
    }
    let already = Cancel::new();
    already.cancel();
    assert_eq!(
        request(&open(&case), &case, &Budget::default(), &already),
        expected
    );

    // a class with a constructor does have a context to cut, and its counters are the same all the same: they are noted before the solver runs
    let case = class_entry("Rich");
    let full = uncancelled(&case, &Budget::default());
    let cut = request(
        &open(&case),
        &case,
        &Budget::default(),
        &Cancel::after_checks(1),
    );
    assert_ne!(cut, full, "`Rich` is cut at its constructor");
    assert_eq!(cut.stats.language, full.stats.language);
}

/// A real component (a frontend's largest), a smoke test that prints numbers and asserts nothing about time: `TS_CANCEL_ROOT=<dir> TS_CANCEL_FILE=<file of it>
/// TS_CANCEL_ENTRY=<qualname> TS_CANCEL_GOLDEN=<its tree> cargo test --release -p typescript-analysis --test cancel a_real_entry -- --ignored --nocapture`.
/// N is found by cancelling at 1, 2, … until the request is whole; then a cancel at a handful of checks spread over it, each on a fresh session, and the
/// request after it on the same session must be the golden tree.
#[test]
#[ignore = "a check on a real project, driven by TS_CANCEL_ROOT, TS_CANCEL_FILE, TS_CANCEL_ENTRY and TS_CANCEL_GOLDEN"]
fn a_real_entry_cancelled_at_checks_spread_over_it_recovers_to_the_golden_tree() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
    let case = Case {
        name: var("TS_CANCEL_ENTRY"),
        root: Path::new(&var("TS_CANCEL_ROOT")).to_path_buf(),
        file: var("TS_CANCEL_FILE"),
        entry: var("TS_CANCEL_ENTRY"),
    };
    let golden = std::fs::read_to_string(var("TS_CANCEL_GOLDEN")).unwrap();
    let golden = golden
        .split("\nbreakdown:")
        .next()
        .unwrap()
        .trim_end()
        .to_owned();
    let text = |graph: &Graph| {
        typescript_analysis::to_text(&tree_of(graph))
            .trim_end()
            .to_owned()
    };

    let started = std::time::Instant::now();
    let full = uncancelled(&case, &Budget::default());
    eprintln!(
        "cold full request: {:?}, {} contexts",
        started.elapsed(),
        full.contexts.len()
    );
    assert_eq!(
        text(&full),
        golden,
        "the uncancelled request is the golden tree"
    );

    // N, on one session: a cut request is not kept, so each ask is cold for the cancelled part only
    let session = open(&case);
    let mut n = 0;
    for k in 1.. {
        if request(
            &session,
            &case,
            &Budget::default(),
            &Cancel::after_checks(k),
        ) == full
        {
            n = k - 1;
            break;
        }
    }
    eprintln!("N = {n} checks for {} contexts", full.contexts.len());
    let spread: Vec<u64> = [1, 2, 3, n / 8, n / 4, n / 2, n * 3 / 4, n - 1, n]
        .into_iter()
        .filter(|k| (1..=n).contains(k))
        .collect();
    for k in spread {
        let session = open(&case);
        let started = std::time::Instant::now();
        let cut = request(
            &session,
            &case,
            &Budget::default(),
            &Cancel::after_checks(k),
        );
        let cut_time = started.elapsed();
        let incomplete = cancelled_count(&cut);
        assert!(incomplete > 0 && cut.contexts.len() <= full.contexts.len());
        let started = std::time::Instant::now();
        let again = request(&session, &case, &Budget::default(), &Cancel::new());
        eprintln!(
            "cancelled at check {k}: {} contexts, {incomplete} Incomplete, {cut_time:?}; the next request {:?}",
            cut.contexts.len(),
            started.elapsed()
        );
        assert_eq!(text(&again), golden, "after a cancel at check {k}");
        assert_eq!(again, full);
    }
}
