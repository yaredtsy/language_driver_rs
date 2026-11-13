pub(crate) mod env;

pub(crate) mod bind;
pub(crate) mod call;
pub(crate) mod canon;
pub(crate) mod core_graph;
pub(crate) mod defer;
pub(crate) mod entry;
#[cfg(test)]
mod fixture;
pub(crate) mod key;
pub(crate) mod member;
pub(crate) mod relevant;
pub(crate) mod solver;
pub(crate) mod summary;
pub(crate) mod tree;
pub(crate) mod value;
pub(crate) mod walk;
#[cfg(test)]
pub(crate) use tree::to_text;
pub(crate) use tree::{Mark, Node};

use std::time::Duration;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::Entry;

use ruff_db::files::File;
use summary::Cut;
use ty_project::ProjectDatabase;

use crate::{
    interp::{key::ContextId, solver::Solver, walk::Walk},
    sem::Sem,
};

/// What `solve` started from: the entry's own context, or a class and its constructor's context (D20). `core_graph` turns it into core's `Root`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum EntryRoot<'db> {
    Context(ContextId),
    /// `init` is `None` when the class has no constructor function in project code.
    Construct {
        class: crate::sem::Definition<'db>,
        init: Option<ContextId>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Follow every call into the callee's body (parts 01 and 03).
    Inline,
    /// One walk per call, started from its key. The caller reads the summary.
    Summary,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Cuts {
    pub stack: u32,
    pub contexts: u32,
    pub contexts_per_func: u32,
    pub deadline: u32,
    pub cancelled: u32,
}

impl Cuts {
    pub(crate) fn count(&mut self, cut: Cut) {
        match cut {
            Cut::Stack => self.stack += 1,
            Cut::Contexts => self.contexts += 1,
            Cut::ContextsPerFunc => self.contexts_per_func += 1,
            Cut::Deadline => self.deadline += 1,
            Cut::Cancelled => self.cancelled += 1,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub(crate) struct Stats {
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
    pub effects_applied: u32,      // a callee's write onto an object the caller holds, applied
    pub effects_in_cycle: u32, // a call site whose callee's effects were not carried (a cycle or a replay), until CS8
    pub ref_unresolved: u32,   // a `Ref` or `Obj` no caller resolved (until CS2b: all)
}
pub(crate) use crate::wire::WireStyle;

#[derive(Clone, Debug)]
pub(crate) struct Options {
    pub mode: Mode,
    pub limits: Limits,
    /// Show only functions that carry a marker (and what is under them): the same as core's `RenderOptions::only_marked`.
    /// The harness runs with it off; the request path turns it on.
    pub only_marked: bool,
    /// The prefixes of `target_id` on the wire.
    pub wire: WireStyle,
    /// The label a marker is read under (`ID: …` in a docstring), the same style `python_syntax::outline` takes.
    pub marker: MarkerStyle<'static>,
    /// The caller's request to stop, and its own deadline. Checked before every walk, never inside one.
    pub cancel: analyzer_core::solve::Cancel,
    /// Tests only: fire `cancel` when this many walks have been made, so a test can stop a request at a known place without a thread.
    #[cfg(test)]
    pub cancel_at_walk: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Limits {
    // precision (rule 3: decided by the key and the code)
    pub max_rounds: u32, // a group's fixpoint stops here, marked incomplete
    pub fp_depth: u8, // steps from a parameter (attribute, receiver, free value, default), in keys and in summaries
    /// Attribute steps from a parameter that a summary may name (6; at least `fp_depth`, a smaller value is raised to it). Past it an
    /// object has no name of its own and a write onto it is a counted fallback (`Stats::language["deep_write"]`).
    pub max_path: usize,
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
            max_path: 6,
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

impl Limits {
    /// The longest path an input object is named by: `max_path`, and never less than the key's own depth.
    pub(crate) fn path_cap(&self) -> usize {
        self.max_path.max(usize::from(self.fp_depth))
    }
}

impl Options {
    /// The tree's own options, as the renderer in core takes them: the node budget, the depth, and the ID filter.
    pub(crate) fn render_options(&self) -> analyzer_core::render::RenderOptions {
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
            marker: MarkerStyle::docstring("ID"),
            cancel: analyzer_core::solve::Cancel::default(),
            #[cfg(test)]
            cancel_at_walk: None,
        }
    }
}

/// The call tree under `entry`, a function in `file`. `None` when there is no
/// such function.
#[allow(dead_code)] // inline mode is the fixture suite's cross-check and is not reachable from the public API
pub(crate) fn call_tree(
    db: &ProjectDatabase,
    file: File,
    entry: &Entry<'_>,
    opts: &Options,
) -> Option<(Node, Stats)> {
    match opts.mode {
        Mode::Inline => inline_tree(db, file, entry, opts),
        Mode::Summary => summary_tree(db, file, entry, opts),
    }
}

/// Summary mode's tree for the harness: the graph, drawn by core's renderer, as this crate's `Node` (names and marks: what the
/// `# expect:` blocks say). Only the three marks the default options leave on are carried over.
#[allow(dead_code)] // inline mode is the fixture suite's cross-check and is not reachable from the public API
fn summary_tree(
    db: &ProjectDatabase,
    file: File,
    entry: &Entry<'_>,
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

/// The context graph of `entry` in core's types: contexts, a table of definitions, one edge per call site and target. The tree and the
/// wire are views of it.
pub(crate) fn call_graph(
    db: &ProjectDatabase,
    file: File,
    entry: &Entry<'_>,
    opts: &Options,
) -> Option<analyzer_core::graph::Graph> {
    let (mut s, root) = solve(db, file, entry, opts)?;
    s.stats.contexts = s.contexts.len() as u32;
    Some(core_graph::core_graph(&s, &[root]))
}

/// The wire of `entry`, a view of its graph: `graph -> render -> Node -> Frame`. `merged` joins siblings with one target and counts them.
pub(crate) fn call_frame(
    db: &ProjectDatabase,
    file: File,
    entry: &Entry<'_>,
    opts: &Options,
    merged: bool,
) -> Option<(crate::wire::Frame, analyzer_core::graph::Stats)> {
    let graph = call_graph(db, file, entry, opts)?;
    let root = graph.entries.first()?.clone();
    let wire_opts = crate::wire::WireOptions::new(opts.wire.clone(), merged, opts.render_options());
    let frame = crate::wire::wire(&graph, &root, &crate::sem::DbSources(db), &wire_opts);
    Some((frame, graph.stats.clone()))
}

/// Summary mode, stopped before rendering: the solver, with the entry solved.
pub(crate) fn solve<'db>(
    db: &'db ProjectDatabase,
    file: File,
    entry: &Entry<'_>,
    opts: &Options,
) -> Option<(Solver<'db>, EntryRoot<'db>)> {
    let sem = Sem::new(db, opts.marker);
    let def = sem.entry(file, entry)?;
    if sem.is_class(def) {
        return Some(solve_class(sem, def, opts));
    }
    let parsed = sem.parsed(def);
    let node = sem.function_node(def, &parsed)?;
    let mut s = Solver::new(sem, opts.clone());
    // Called with no arguments (step 01.11). A walk of the entry's own frame
    // does the binding, exactly as step 02 did.
    let mut w = Walk::new(&s, def);
    let mut env = env::Env::default();
    // The one prologue of inline mode starts the entry: the parents run (hidden), a method's object has its `__init__` run on it, and a
    // closure's free values are read from the run it was defined in.
    let start = w.prologue(&mut s, def, &mut env);
    let bound = w.bind_parameters(
        &mut s,
        def,
        node,
        start.receiver,
        &bind::EvaluatedArgs::default(),
        start.enclosing,
    );
    let root_ctx = s.key(&w, def, &bound, start.enclosing, &env);
    s.get(root_ctx);
    Some((s, EntryRoot::Context(root_ctx)))
}

/// A class as the entry: what a `Class()` call site runs, with no arguments and no caller. It is `construct_summary`, the function the call
/// site goes through, so the MRO lookup of `__init__`, the "project code only" filter and the binding of `self` are shared, not copied.
/// `Basis::Type`: the root is not an edge, so it rests on nothing.
fn solve_class<'db>(
    sem: Sem<'db>,
    class: crate::sem::Definition<'db>,
    opts: &Options,
) -> (Solver<'db>, EntryRoot<'db>) {
    let mut s = Solver::new(sem, opts.clone());
    let mut w = Walk::new(&s, class);
    let mut env = env::Env::default();
    let mut targets = smallvec::SmallVec::new();
    let site = s.sem.def_range(class);
    w.construct_summary(
        &mut s,
        class,
        site,
        &bind::EvaluatedArgs::default(),
        analyzer_core::graph::Basis::Type,
        &mut targets,
        &mut env,
    );
    let init = targets.iter().find_map(|t| match t {
        summary::Target::Construct { init, .. } => *init,
        _ => None,
    });
    (s, EntryRoot::Construct { class, init })
}

// A class entry has no function node, so this gives `None` for one: no fixture asks for a class in inline mode.
#[allow(dead_code)] // inline mode is the fixture suite's cross-check and is not reachable from the public API
fn inline_tree(
    db: &ProjectDatabase,
    file: File,
    entry: &Entry<'_>,
    opts: &Options,
) -> Option<(Node, Stats)> {
    let sem = Sem::new(db, opts.marker);
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
