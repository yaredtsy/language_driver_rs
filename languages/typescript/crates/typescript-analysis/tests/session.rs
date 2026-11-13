//! `Session::graph`: with no budget it is `call_graph`, and with a budget a request's frontier nodes are `Cut`, and asking for each one's def as a new
//! request gives, stitched, the tree of one request with no budget (`docs/03-typescript/07-ts5-driver-needs.md` §7 row 4).
//!
//! **Written before it was run.** The stitched tree equals the unbounded one at `max_depth` 1, 2 and 3, for every scenario, every row of the policy table and
//! the `recursion` fixture. A request rooted at a cut def does not know the defs above it, so where it draws one of them again the stitcher draws what the
//! unbounded renderer draws there: a `Recursive` leaf. Without that rule a cycle never ends. Nothing is asserted about the counters of a request: a def two
//! requests share is counted once in each.

mod common;

use std::path::PathBuf;

use analyzer_testkit::graph::check_invariants;
use common::{Case, STYLE, cases, open, tests_dir};
use typescript_analysis::{
    Budget, Cancel, Entry, Graph, Mark, Node, Options, RenderOptions, Session, StableDef, Status,
    call_graph, render, to_text,
};

fn request(
    session: &Session,
    file: &str,
    qualname: &str,
    occurrence: u16,
    budget: &Budget,
) -> Graph {
    let entry = Entry {
        qualname,
        occurrence,
    };
    let graph = session
        .graph(file, &entry, budget, &Cancel::new())
        .unwrap_or_else(|| panic!("no def `{qualname}` in {file}"));
    check_invariants(&graph);
    graph
}

fn tree_of(graph: &Graph) -> Node {
    render(graph, &graph.entries[0], &RenderOptions::default())
}

/// A node stands for a context the request's budget ended at. (A `Cut` that is not this was the renderer's own node budget.)
fn is_frontier(graph: &Graph, node: &Node) -> bool {
    node.contexts.iter().any(|id| {
        graph
            .context(id)
            .is_some_and(|context| context.status == Status::Frontier)
    })
}

fn frontier_nodes(graph: &Graph, node: &Node) -> u32 {
    u32::from(is_frontier(graph, node))
        + node
            .children
            .iter()
            .map(|child| frontier_nodes(graph, child))
            .sum::<u32>()
}

/// What stitching did, for the reports and for the checks that the test is not empty.
#[derive(Default, Debug)]
struct Report {
    /// New requests made, after the first.
    requests: u32,
    /// Frontier nodes of the first request.
    first_frontier: u32,
    /// `Graph::stats.frontier` of the first request.
    first_stats_frontier: u64,
    /// Nodes a request rooted below the first drew expanded where the unbounded tree has a `Recursive` leaf.
    ancestors_redrawn: u32,
    /// Nodes that were `Cut` and not on a frontier: the renderer's `max_nodes`.
    renderer_cuts: u32,
}

/// How deep a stitched tree may go before the stitching is said not to end. A real tree is a few dozen levels (a real frontend component: 6); a cycle
/// that is not recognized grows by one def for each request, and the stack of the test is the limit after this.
const MAX_PATH: usize = 200;

struct Stitcher<'a> {
    session: &'a Session,
    file_of: &'a str,
    budget: &'a Budget,
    /// A node whose def is on the path above it becomes a `Recursive` leaf. Off only in the test that shows why it is needed.
    ancestors: bool,
    report: Report,
}

impl Stitcher<'_> {
    /// `node`, drawn from `graph`, with every frontier node under it replaced by what a request for its def draws, until none is left. `path` is the defs
    /// above `node`.
    fn complete(
        &mut self,
        graph: &Graph,
        mut node: Node,
        path: &mut Vec<StableDef>,
    ) -> Result<Node, String> {
        let grafted;
        let graph = if is_frontier(graph, &node) {
            if path.len() >= MAX_PATH {
                return Err(format!(
                    "a path of {} defs and still a frontier node: the stitching does not end",
                    path.len()
                ));
            }
            self.report.requests += 1;
            let info = graph.def(&node.func).expect("a node's def has a row");
            let file = if info.file.is_empty() {
                self.file_of
            } else {
                &info.file
            };
            grafted = request(
                self.session,
                file,
                &info.qualname,
                info.occurrence,
                self.budget,
            );
            let mut below = tree_of(&grafted);
            node.marks.retain(|mark| *mark != Mark::Cut);
            node.marks.extend(below.marks.iter().copied());
            node.marks.sort();
            node.marks.dedup();
            node.children = std::mem::take(&mut below.children);
            &grafted
        } else {
            if node.marks.contains(&Mark::Cut) {
                self.report.renderer_cuts += 1;
            }
            graph
        };
        path.push(node.func.clone());
        for child in std::mem::take(&mut node.children) {
            let child = if self.ancestors && path.contains(&child.func) {
                self.recursive_leaf(child)
            } else {
                self.complete(graph, child, path)?
            };
            node.children.push(child);
        }
        path.pop();
        Ok(node)
    }

    /// What the renderer draws for a def already on the path (`render.rs`, `leaf(child, Mark::Recursive)`): the node's site, its `passed` and contexts, the
    /// marks `Recursive` and the edge's `Likely`, and no children.
    fn recursive_leaf(&mut self, mut node: Node) -> Node {
        if !node.marks.contains(&Mark::Recursive) {
            self.report.ancestors_redrawn += 1;
        }
        let likely = node.marks.contains(&Mark::Likely);
        node.marks = vec![Mark::Recursive];
        if likely {
            node.marks.push(Mark::Likely);
        }
        node.children = Vec::new();
        node
    }
}

/// The tree of a request at `budget`, completed by new requests, and what that took.
fn stitched(
    session: &Session,
    case: &Case,
    budget: &Budget,
    ancestors: bool,
) -> Result<(Node, Report), String> {
    let first = request(session, &case.file, &case.entry, 0, budget);
    let tree = tree_of(&first);
    let mut stitcher = Stitcher {
        session,
        file_of: &case.file,
        budget,
        ancestors,
        report: Report {
            first_frontier: frontier_nodes(&first, &tree),
            first_stats_frontier: first.stats.frontier,
            ..Report::default()
        },
    };
    let tree = stitcher.complete(&first, tree, &mut Vec::new())?;
    Ok((tree, stitcher.report))
}

fn unbounded(session: &Session, case: &Case) -> Node {
    tree_of(&request(
        session,
        &case.file,
        &case.entry,
        0,
        &Budget::default(),
    ))
}

#[test]
fn with_no_budget_a_session_gives_the_graph_call_graph_gives() {
    let cases = cases();
    assert!(
        cases.len() > 60,
        "the cases were not found: {}",
        cases.len()
    );
    for case in &cases {
        let session = open(case);
        let entry = Entry {
            qualname: &case.entry,
            occurrence: 0,
        };
        let expected = call_graph(
            session.project(),
            &case.file,
            &entry,
            &Options::new(STYLE),
            &Cancel::new(),
        )
        .unwrap();
        let graph = request(&session, &case.file, &case.entry, 0, &Budget::default());
        assert_eq!(graph, expected, "{}", case.name);
        assert_eq!(graph.stats.frontier, 0, "{}", case.name);
        assert_eq!(tree_of(&graph), tree_of(&expected), "{}", case.name);
    }
}

#[test]
fn a_budget_changes_the_graph() {
    // the check above is empty if a session ignores its budget: `main` of `recursion` has a callee, and at depth 0 nothing of it is walked
    let case = &cases().pop().unwrap();
    let session = open(case);
    let none = request(&session, &case.file, "main", 0, &Budget::default());
    let zero = request(&session, &case.file, "main", 0, &Budget::new(Some(0), None));
    assert_ne!(none, zero);
    assert_eq!(zero.stats.frontier, 1);
    assert_eq!(to_text(&tree_of(&zero)), "main [Cut]\n");
    // `max_contexts` goes through the same way
    let two = request(&session, &case.file, "main", 0, &Budget::new(None, Some(2)));
    assert!(two.stats.frontier > 0);
}

#[test]
fn a_request_for_each_cut_def_stitches_to_the_tree_of_one_request_with_no_budget() {
    let mut redrawn: Vec<String> = Vec::new();
    let mut frontier_at = [0u32; 3];
    for case in &cases() {
        let session = open(case);
        let expected = unbounded(&session, case);
        for depth in 1..=3u32 {
            let budget = Budget::new(Some(depth), None);
            let (tree, report) = stitched(&session, case, &budget, true)
                .unwrap_or_else(|error| panic!("{}, depth {depth}: {error}", case.name));
            assert!(
                tree == expected,
                "{}, depth {depth}\nstitched:\n{}unbounded:\n{}",
                case.name,
                to_text(&tree),
                to_text(&expected)
            );
            assert_eq!(report.renderer_cuts, 0, "{}, depth {depth}", case.name);
            assert_eq!(
                u64::from(report.first_frontier) > 0,
                report.first_stats_frontier > 0,
                "{}, depth {depth}: the tree and the counter disagree on whether there is a frontier",
                case.name
            );
            frontier_at[depth as usize - 1] += report.first_frontier;
            if report.ancestors_redrawn > 0 {
                redrawn.push(format!("{}, depth {depth}", case.name));
            }
        }
    }
    eprintln!(
        "frontier nodes of the first request, over every case, at depth 1, 2, 3: {frontier_at:?}"
    );
    eprintln!(
        "the recursion exception fired in {} runs: {redrawn:?}",
        redrawn.len()
    );
    // a test that never meets a frontier, or never the exception, proves nothing about either
    assert!(frontier_at.iter().all(|&n| n > 0), "{frontier_at:?}");
    assert!(
        redrawn.iter().any(|name| name.starts_with("recursion ")),
        "{redrawn:?}"
    );
}

#[test]
fn without_the_ancestor_rule_a_cycle_is_never_finished() {
    // the recursion exception is needed, not a courtesy: `ping` cut at `pong`, `pong` asked for alone draws `ping` expanded, which is cut again
    let case = Case {
        name: "recursion ping".into(),
        root: tests_dir().join("fixtures/recursion"),
        file: "src/main.ts".into(),
        entry: "ping".into(),
    };
    let session = open(&case);
    let budget = Budget::new(Some(1), None);
    let error = stitched(&session, &case, &budget, false).unwrap_err();
    assert!(error.contains("does not end"), "{error}");
    stitched(&session, &case, &budget, true).unwrap();
}

/// A measurement on a real project: `TS_STITCH_ROOT=<dir> TS_STITCH_FILE=<file of it> TS_STITCH_ENTRY=<qualname> TS_STITCH_GOLDEN=<the tree of one request>
/// cargo test --release -p typescript-analysis --test session stitching_a_real_entry -- --ignored --nocapture`. The stitched tree at depth 1, 2 and 3 must be
/// the golden tree; what the first request of each reports in `Stats.frontier` is printed.
#[test]
#[ignore = "a check on a real project, driven by TS_STITCH_ROOT, TS_STITCH_FILE, TS_STITCH_ENTRY and TS_STITCH_GOLDEN"]
fn stitching_a_real_entry_gives_the_golden_tree() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
    let case = Case {
        name: var("TS_STITCH_ENTRY"),
        root: PathBuf::from(var("TS_STITCH_ROOT")),
        file: var("TS_STITCH_FILE"),
        entry: var("TS_STITCH_ENTRY"),
    };
    // the CLI's `.tree` is the tree, a blank line, and then the breakdown
    let golden = std::fs::read_to_string(var("TS_STITCH_GOLDEN")).unwrap();
    let golden = golden.split("\nbreakdown:").next().unwrap().trim_end();
    let session = open(&case);
    for depth in 1..=3u32 {
        let started = std::time::Instant::now();
        let (tree, report) = stitched(&session, &case, &Budget::new(Some(depth), None), true)
            .unwrap_or_else(|error| panic!("depth {depth}: {error}"));
        eprintln!(
            "depth {depth}: Stats.frontier {} (frontier nodes in the tree {}), {} more requests, {} ancestors redrawn, {} renderer cuts, {:?}",
            report.first_stats_frontier,
            report.first_frontier,
            report.requests,
            report.ancestors_redrawn,
            report.renderer_cuts,
            started.elapsed()
        );
        let text = to_text(&tree);
        let text = text.trim_end();
        if text != golden {
            let first = text.lines().zip(golden.lines()).position(|(a, b)| a != b);
            panic!(
                "depth {depth}: not the golden tree ({} lines, the golden has {}); first difference at line {:?}: {:?} against {:?}",
                text.lines().count(),
                golden.lines().count(),
                first.map(|n| n + 1),
                first.and_then(|n| text.lines().nth(n)),
                first.and_then(|n| golden.lines().nth(n)),
            );
        }
    }
}
