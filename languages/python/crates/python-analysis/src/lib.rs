//! SLOW: project loading, call sites, the call graph and the wire. The `sem` module (the folder `src/sem/`) is the only code that imports the type checker.
//!
//! Moved from pylspt (`670ae79a15`). The surface is this file's list and nothing else:
//!
//! ```text
//! Project::open(dir)                                     the one handle: no engine type is in it
//! call_graph(&project, file, entry, &options) -> Graph   contexts, a table of definitions, one edge per call site and target
//! call_wire(&project, file, entry, &options, merged)     the wire: `graph -> render -> Node -> Frame`, merged on request
//! render, to_text, Node, Mark, RenderOptions              the tree is drawn from the graph (core's renderer)
//! ```

mod classes;
mod interp;
mod project;
mod sem;
mod stable;
mod wire;

pub use analyzer_core::edit::MarkerStyle;
pub use analyzer_core::graph::{
    Context, ContextId, Edge, EdgeTo, Entry, Graph, Root, StableDef, Stats,
};
pub use analyzer_core::render::{Mark, Node, RenderOptions, render, to_text};
pub use analyzer_core::solve::Cancel;
pub use interp::Limits;
pub use sem::{OpenError, Project, Refreshed};
pub use wire::{Frame, LineSpan, Sources, WireOptions, WireStyle, merge, wire};

/// What a request is asked with.
#[derive(Clone, Debug)]
pub struct Options {
    /// The solver's limits, precision and cost, and the tree's budget. Every one that cuts is marked in the answer.
    pub limits: Limits,
    /// The label a marker is read under (`ID: …` in a docstring): the caller's, the same style `python_syntax::outline` takes.
    pub marker: MarkerStyle<'static>,
    /// Show only functions that carry a marker, and what is under them.
    pub only_marked: bool,
    /// How an id is written on the wire: the prefixes are the caller's.
    pub wire: WireStyle,
    /// A request to stop. The caller fires it with `cancel()` (the host's `$/cancelRequest`), or gives it a deadline with `with_deadline`
    /// (counted from the caller's clock, so it can include the queue). A fired cancel cuts every context not yet walked and the answer says so
    /// (`Cut::Cancelled`, marked `incomplete`). It is checked between walks only: a walk that has begun finishes. The default never fires.
    pub cancel: Cancel,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            limits: Limits::default(),
            marker: MarkerStyle::docstring("ID"),
            only_marked: false,
            wire: WireStyle::default(),
            cancel: Cancel::default(),
        }
    }
}

impl Options {
    fn solver(&self) -> interp::Options {
        interp::Options {
            mode: interp::Mode::Summary,
            limits: self.limits.clone(),
            only_marked: self.only_marked,
            wire: self.wire.clone(),
            marker: self.marker,
            constructor_effects: false,
            cancel: self.cancel.clone(),
            #[cfg(test)]
            cancel_at_walk: None,
        }
    }

    /// The tree's own options, as core's renderer takes them: the node budget, the depth and the marker filter.
    pub fn render_options(&self) -> RenderOptions {
        self.solver().render_options()
    }
}

/// The call graph of `entry`, a def of `file` (a path absolute or relative to the project root), named the way the outline names it:
/// its qualname and which of the defs with that qualname it is (D20). `None` when there is no such file or no such def. Draw a tree from it with [`render`] and [`Options::render_options`].
pub fn call_graph(
    project: &Project,
    file: &str,
    entry: &Entry<'_>,
    opts: &Options,
) -> Option<Graph> {
    interp::call_graph(project.db(), project.file(file)?, entry, &opts.solver())
}

/// The wire of `entry`: the stored tree, one node per call site, or with `merged` the siblings of one target joined and counted.
/// `None` when there is no such file or no such def.
pub fn call_wire(
    project: &Project,
    file: &str,
    entry: &Entry<'_>,
    opts: &Options,
    merged: bool,
) -> Option<Frame> {
    interp::call_frame(
        project.db(),
        project.file(file)?,
        entry,
        &opts.solver(),
        merged,
    )
    .map(|(frame, _)| frame)
}
