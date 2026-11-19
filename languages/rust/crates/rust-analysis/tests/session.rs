//! `Session::graph` (RS5 §7 step 2): with no budget it is `call_graph`, and with a budget a request's frontier nodes are `Cut`, and asking for each one's def as a
//! new request gives, stitched, the tree of one request with no budget (D31: a request for the cut def stands in for `expand`).
//!
//! The same proof as TypeScript's (`languages/typescript/crates/typescript-analysis/tests/session.rs`), over every entry of the policy and context workspaces.
//! A request rooted at a cut def does not know the defs above it, so where it draws one of them again the stitcher draws what the unbounded renderer draws
//! there: a `Recursive` leaf. Without that rule a cycle never ends. Nothing is asserted about the counters of a request: a def two requests share is counted
//! once in each.

use std::path::{Path, PathBuf};

use analyzer_testkit::graph::check_invariants;
use rust_analysis::{
    Budget, Cancel, Entry, Graph, LoadOptions, Mark, MarkerStyle, Node, Options, RenderOptions,
    Session, StableDef, Status, call_graph, render, to_text,
};

mod common;
use common::{CONTEXT_ENTRIES, POLICY_ENTRIES, copy_of_fixture};

fn options() -> Options {
    Options::new(MarkerStyle::docstring("ID"))
}

/// One workspace, opened once, and the entries to ask of it: (file, qualname, occurrence).
struct Case {
    name: &'static str,
    root: PathBuf,
    session: Session,
    entries: Vec<(&'static str, &'static str, u16)>,
}

fn open(
    name: &'static str,
    fixture: &str,
    entries: Vec<(&'static str, &'static str, u16)>,
) -> Case {
    let root = copy_of_fixture(fixture, &format!("session_{name}"));
    let session = Session::open(
        &root,
        Path::new("src/lib.rs"),
        &LoadOptions::default(),
        options(),
    )
    .expect("the fixture loads");
    Case {
        name,
        root,
        session,
        entries,
    }
}

fn cases() -> Vec<Case> {
    vec![
        open("policy", "policy", POLICY_ENTRIES.to_vec()),
        open(
            "context",
            "context",
            CONTEXT_ENTRIES
                .iter()
                .map(|&(file, qualname)| (file, qualname, 0))
                .collect(),
        ),
    ]
}

fn request(
    session: &Session,
    file: &str,
    qualname: &str,
    occurrence: u16,
    budget: &Budget,
) -> Graph {
    let entry = Entry::Def {
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

/// What stitching did, for the checks that the test is not empty.
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

/// How deep a stitched tree may go before the stitching is said not to end. A cycle that is not recognized grows by one def for each request.
const MAX_PATH: usize = 200;

struct Stitcher<'a> {
    session: &'a Session,
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
            grafted = request(
                self.session,
                &info.file,
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

    /// What the renderer draws for a def already on the path: the node's site, its `passed` and contexts, the marks `Recursive` and the edge's `Likely`,
    /// and no children.
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
    entry: (&str, &str, u16),
    budget: &Budget,
    ancestors: bool,
) -> Result<(Node, Report), String> {
    let (file, qualname, occurrence) = entry;
    let first = request(session, file, qualname, occurrence, budget);
    let tree = tree_of(&first);
    let mut stitcher = Stitcher {
        session,
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

fn unbounded(session: &Session, entry: (&str, &str, u16)) -> Node {
    let (file, qualname, occurrence) = entry;
    tree_of(&request(
        session,
        file,
        qualname,
        occurrence,
        &Budget::default(),
    ))
}

#[test]
fn with_no_budget_a_session_gives_the_graph_call_graph_gives() {
    let mut asked = 0;
    for case in cases() {
        for &(file, qualname, occurrence) in &case.entries {
            let entry = Entry::Def {
                qualname,
                occurrence,
            };
            let expected = call_graph(
                case.session.project(),
                file,
                &entry,
                &options(),
                &Cancel::new(),
            );
            let graph = case
                .session
                .graph(file, &entry, &Budget::default(), &Cancel::new());
            let name = format!("{} {file}:{qualname}", case.name);
            assert_eq!(graph, expected, "{name}");
            if let Some(graph) = graph {
                assert_eq!(graph.stats.frontier, 0, "{name}");
                assert_eq!(tree_of(&graph), tree_of(&expected.unwrap()), "{name}");
                asked += 1;
            }
        }
    }
    assert!(asked > 50, "the entries were not found: {asked}");
}

#[test]
fn a_budget_changes_the_graph() {
    // the check above is empty if a session ignores its budget: `recurse_a` has a callee, and at depth 0 nothing of it is walked
    let case = open("budget", "policy", Vec::new());
    let none = request(
        &case.session,
        "src/calls.rs",
        "recurse_a",
        0,
        &Budget::default(),
    );
    let zero = request(
        &case.session,
        "src/calls.rs",
        "recurse_a",
        0,
        &Budget::new(Some(0), None),
    );
    assert_ne!(none, zero);
    assert_eq!(zero.stats.frontier, 1);
    assert_eq!(to_text(&tree_of(&zero)), "recurse_a [Cut]\n");
    // `max_contexts` goes through the same way
    let two = request(
        &case.session,
        "src/calls.rs",
        "plain_calls",
        0,
        &Budget::new(None, Some(1)),
    );
    assert!(two.stats.frontier > 0);
}

#[test]
fn a_request_for_each_cut_def_stitches_to_the_tree_of_one_request_with_no_budget() {
    let mut redrawn: Vec<String> = Vec::new();
    let mut frontier_at = [0u32; 3];
    for case in cases() {
        for &(file, qualname, occurrence) in &case.entries {
            let entry = (file, qualname, occurrence);
            let name = format!("{} {file}:{qualname}", case.name);
            let Some(_) = case.session.graph(
                file,
                &Entry::Def {
                    qualname,
                    occurrence,
                },
                &Budget::default(),
                &Cancel::new(),
            ) else {
                continue;
            };
            let expected = unbounded(&case.session, entry);
            for depth in 1..=3u32 {
                let budget = Budget::new(Some(depth), None);
                let (tree, report) = stitched(&case.session, entry, &budget, true)
                    .unwrap_or_else(|error| panic!("{name}, depth {depth}: {error}"));
                assert!(
                    tree == expected,
                    "{name}, depth {depth}\nstitched:\n{}unbounded:\n{}",
                    to_text(&tree),
                    to_text(&expected)
                );
                assert_eq!(report.renderer_cuts, 0, "{name}, depth {depth}");
                assert_eq!(
                    u64::from(report.first_frontier) > 0,
                    report.first_stats_frontier > 0,
                    "{name}, depth {depth}: the tree and the counter disagree on whether there is a frontier"
                );
                frontier_at[depth as usize - 1] += report.first_frontier;
                if report.ancestors_redrawn > 0 {
                    redrawn.push(format!("{name}, depth {depth}"));
                }
            }
        }
    }
    eprintln!(
        "frontier nodes of the first request, over every entry, at depth 1, 2, 3: {frontier_at:?}"
    );
    eprintln!(
        "the recursion exception fired in {} runs: {redrawn:?}",
        redrawn.len()
    );
    // a test that never meets a frontier, or never the exception, proves nothing about either
    assert!(frontier_at.iter().all(|&n| n > 0), "{frontier_at:?}");
    assert!(
        redrawn.iter().any(|name| name.contains("recurse_a")),
        "{redrawn:?}"
    );
}

#[test]
fn without_the_ancestor_rule_a_cycle_is_never_finished() {
    // the recursion exception is needed, not a courtesy: `recurse_a` cut at `recurse_b`, `recurse_b` asked for alone draws `recurse_a` expanded, which is
    // cut again
    let case = open("cycle", "policy", Vec::new());
    let entry = ("src/calls.rs", "recurse_a", 0);
    let budget = Budget::new(Some(1), None);
    let error = stitched(&case.session, entry, &budget, false).unwrap_err();
    assert!(error.contains("does not end"), "{error}");
    stitched(&case.session, entry, &budget, true).unwrap();
    // a root the session was opened on stays where `copy_of_fixture` put it
    assert!(case.root.is_dir());
}
