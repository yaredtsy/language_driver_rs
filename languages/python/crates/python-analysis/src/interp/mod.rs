pub mod env;

pub mod bind;
pub mod call;
pub mod canon;
pub mod core_graph;
pub mod defer;
pub mod entry;
#[cfg(test)]
mod fixture;
pub mod graph;
pub mod key;
pub mod member;
pub mod relevant;
pub mod solver;
pub mod summary;
pub mod tree;
pub mod value;
pub mod walk;
pub use canon::StableContextId;
pub use graph::{ContextOut, EdgeOut, EdgeTo, GraphOut, Span};
pub use tree::{Mark, Node, to_text};

use std::time::Duration;

use ruff_db::files::File;
use summary::Cut;
use ty_project::ProjectDatabase;

use crate::{
    interp::{graph::graph, key::ContextId, solver::Solver, walk::Walk},
    sem::Sem,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Follow every call into the callee's body (parts 01 and 03).
    Inline,
    /// One walk per call, started from its key. The caller reads the summary.
    Summary,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Cuts {
    pub stack: u32,
    pub contexts: u32,
    pub contexts_per_func: u32,
    pub deadline: u32,
}

impl Cuts {
    pub fn count(&mut self, cut: Cut) {
        match cut {
            Cut::Stack => self.stack += 1,
            Cut::Contexts => self.contexts += 1,
            Cut::ContextsPerFunc => self.contexts_per_func += 1,
            Cut::Deadline => self.deadline += 1,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Stats {
    pub walks: u32,                // bodies walked, the entry included
    pub nodes: u32,                // tree nodes produced
    pub contexts: u32,             // distinct keys interned
    pub reuse_hits: u32,           // calls answered from a Ready slot
    pub rounds_capped: u32,        // group fixpoints stopped by `max_rounds`
    pub cuts: Cuts,                // calls cut by a cost limit, by kind
    pub chain_fanout_hits: u32,    // an attribute hop cut to `max_chain_fanout` values
    pub prologue_walks: u32,       // functions run, and objects built, to start an entry
    pub annotation_fallbacks: u32, // a parameter that started from its annotation
    pub types_dropped: u32,        // a type left out of a key: no one-to-one stable form
    pub lifted_subclass_of: u32,   // `lift` reached a `type[X]` (tests: the arm runs)
}
pub use crate::wire::WireStyle;

#[derive(Clone, Debug)]
pub struct Options {
    pub mode: Mode,
    pub limits: Limits,
    /// Show only functions that carry a marker (and what is under them): the same as core's `RenderOptions::only_marked`.
    /// The harness runs with it off; the request path turns it on.
    pub only_marked: bool,
    /// The prefixes of `target_id` on the wire.
    pub wire: WireStyle,
}

#[derive(Clone, Debug)]
pub struct Limits {
    // precision (rule 3: decided by the key and the code)
    pub max_rounds: u32, // a group's fixpoint stops here, marked incomplete
    pub fp_depth: u8, // steps from a parameter (attribute, receiver, free value, default), in keys and in summaries
    // cost (each one cuts, is marked, and is never reused)
    pub max_chain_fanout: usize, // 8: values carried per attribute hop
    pub max_stack: usize,        // 256
    pub max_contexts: u32,       // 50_000
    pub contexts_per_func: u32,  // 64, counted in this request only
    pub deadline: Option<Duration>,
    // the view: what the tree shows
    pub render_nodes: u32, // 100_000: the tree stops here, and says where
    pub render_depth: Option<u32>, // no depth limit by default: the budget is the guard
    // checks
    pub reuse: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_rounds: 32,
            fp_depth: 2,
            max_chain_fanout: 8,
            max_stack: 256,
            max_contexts: 50_000,
            contexts_per_func: 64,
            deadline: Some(Duration::from_secs(10)),
            render_nodes: 100_000,
            render_depth: None,
            reuse: true,
        }
    }
}

impl Options {
    /// The tree's own options, as the renderer in core takes them: the node budget, the depth, and the ID filter.
    pub fn render_options(&self) -> analyzer_core::render::RenderOptions {
        let mut render = analyzer_core::render::RenderOptions::default();
        render.max_nodes = self.limits.render_nodes;
        render.max_depth = self.limits.render_depth;
        render.only_marked = self.only_marked;
        render
    }
}

impl Default for Options {
    fn default() -> Self {
        Options {
            mode: Mode::Inline,
            limits: Limits::default(),
            only_marked: false, // the harness; requests set it
            wire: WireStyle::default(),
        }
    }
}

/// The call tree under `entry`, a function in `file`. `None` when there is no
/// such function.
pub fn call_tree(
    db: &ProjectDatabase,
    file: File,
    entry: &str,
    opts: &Options,
) -> Option<(Node, Stats)> {
    match opts.mode {
        Mode::Inline => inline_tree(db, file, entry, opts),
        Mode::Summary => summary_tree(db, file, entry, opts),
    }
}

/// Summary mode's tree for the harness: the graph, drawn by core's renderer, as this crate's `Node` (names and marks: what the
/// `# expect:` blocks say). Only the three marks the default options leave on are carried over.
fn summary_tree(
    db: &ProjectDatabase,
    file: File,
    entry: &str,
    opts: &Options,
) -> Option<(Node, Stats)> {
    use analyzer_core::render::{Mark as CoreMark, Node as CoreNode, render};
    fn convert(n: &CoreNode) -> Node {
        Node {
            qname: n.qname.clone(),
            site: None,
            marks: n
                .marks
                .iter()
                .filter_map(|m| match m {
                    CoreMark::Recursive => Some(Mark::Recursive),
                    CoreMark::Cut => Some(Mark::Cut),
                    CoreMark::Incomplete => Some(Mark::Incomplete),
                    _ => None,
                })
                .collect(),
            children: n.children.iter().map(convert).collect(),
        }
    }
    fn below(n: &Node) -> u32 {
        n.children.iter().map(|c| 1 + below(c)).sum()
    }
    let (mut s, root) = solve(db, file, entry, opts)?;
    s.stats.contexts = s.contexts.len() as u32;
    let graph = core_graph::core_graph(&s, &[root]);
    let tree = convert(&render(
        &graph,
        graph.entries.first()?,
        &opts.render_options(),
    ));
    s.stats.nodes = below(&tree);
    Some((tree, s.stats))
}

/// The context graph of `entry`: what the server's `resolve_calls` returns as `graph`.
pub fn call_graph(
    db: &ProjectDatabase,
    file: File,
    entry: &str,
    opts: &Options,
) -> Option<GraphOut> {
    let (mut s, root) = solve(db, file, entry, opts)?;
    s.stats.contexts = s.contexts.len() as u32;
    Some(graph(&s, &[root]))
}

/// The context graph of `entry` in core's types: contexts, a table of definitions, one edge per call site and target. The tree and the
/// wire are views of it.
pub fn call_core_graph(
    db: &ProjectDatabase,
    file: File,
    entry: &str,
    opts: &Options,
) -> Option<analyzer_core::graph::Graph> {
    let (mut s, root) = solve(db, file, entry, opts)?;
    s.stats.contexts = s.contexts.len() as u32;
    Some(core_graph::core_graph(&s, &[root]))
}

/// The wire of `entry`, a view of its graph: `graph -> render -> Node -> Frame`. `merged` joins siblings with one target and counts them.
pub fn call_frame(
    db: &ProjectDatabase,
    file: File,
    entry: &str,
    opts: &Options,
    merged: bool,
) -> Option<(crate::wire::Frame, analyzer_core::graph::Stats)> {
    let graph = call_core_graph(db, file, entry, opts)?;
    let root = graph.entries.first()?.clone();
    let wire_opts = crate::wire::WireOptions::new(opts.wire.clone(), merged, opts.render_options());
    let frame = crate::wire::wire(&graph, &root, &crate::sem::DbSources(db), &wire_opts);
    Some((frame, graph.stats.clone()))
}

/// Summary mode, stopped before rendering: the solver, with the entry solved.
pub fn solve<'db>(
    db: &'db ProjectDatabase,
    file: File,
    entry: &str,
    opts: &Options,
) -> Option<(Solver<'db>, ContextId)> {
    let sem = Sem::new(db);
    let def = sem.entry(file, entry)?;
    let parsed = sem.parsed(def);
    let node = sem.function_node(def, &parsed)?;
    let mut s = Solver::new(sem, opts.clone());
    // Called with no arguments (step 01.11). A walk of the entry's own frame
    // does the binding, exactly as step 02 did.
    let mut w = Walk::new(&s, def);
    let bound = w.bind_parameters(
        &mut s,
        def,
        node,
        None,
        &bind::EvaluatedArgs::default(),
        None,
    );
    let root_ctx = s.key(&w, def, &bound, None, &env::Env::default());
    s.get(root_ctx);
    Some((s, root_ctx))
}

fn inline_tree(
    db: &ProjectDatabase,
    file: File,
    entry: &str,
    opts: &Options,
) -> Option<(Node, Stats)> {
    let sem = Sem::new(db);
    let def = sem.entry(file, entry)?;
    let parsed = sem.parsed(def);
    let node = sem.function_node(def, &parsed)?;
    let mut s = Solver::new(sem, opts.clone());
    let qname = s.sem.qname(def);

    // What Python runs to reach the entry: kept in effect, never shown.
    let mut w = Walk::new(&s, def);
    let mut objects = env::Env::default();
    let start = w.prologue(&mut s, def, &mut objects);
    s.path.push(qname.clone()); // after the prologue: a clean path
    s.stats.walks += 1; // the entry

    // Called with no arguments (step 01.11), and keyed like any call.
    let args = bind::EvaluatedArgs::default();
    let bound = w.bind_parameters(&mut s, def, node, start.receiver, &args, start.enclosing);
    s.key(&w, def, &bound, start.enclosing, &objects);
    let mut env = w.env_from_bound(node, &bound);
    env.scope = Some(s.sem.body_scope(def, node));
    env.enclosing = start.enclosing;
    env.attrs = objects.attrs; // the objects' state
    w.walk_stmts(&mut s, &node.body, env);

    s.stats.contexts = s.contexts.len() as u32;
    let mut root = Node::root(qname);
    root.children = w.frame.children;
    Some((root, s.stats))
}
