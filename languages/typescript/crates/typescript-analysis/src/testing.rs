//! Not API. What a graph's walk knew that the graph does not say: the sites the layer emptied (a callee that holds only `undefined` has no edge).
//! The crate's own tests read it to make "never fewer edges" exact (`tests/common/context.rs`, `08-tc4` §0, step 0b); the public call functions still
//! return a `Graph` and nothing else (rule 7). Turned on by the feature `__testing`, which the crate's tests enable through a dev-dependency on itself.

use analyzer_core::graph::{Entry, Graph, Site, StableDef};
use analyzer_core::solve::{Budget, Cancel};

use crate::calls::graph_and_emptied;
use crate::{Options, Project, Session};

/// `call_graph`, and the sites the layer emptied in it (sorted): a callee that holds only `undefined` has no edge.
pub fn call_graph_and_emptied(
    project: &Project,
    file: &str,
    entry: &Entry<'_>,
    opts: &Options,
    cancel: &Cancel,
) -> Option<(Graph, Vec<(StableDef, Site)>)> {
    graph_and_emptied(project, file, entry, opts, &Budget::default(), cancel)
}

/// `Session::graph`, and the sites the layer emptied in it.
pub fn session_graph_and_emptied(
    session: &Session,
    file: &str,
    entry: &Entry<'_>,
    budget: &Budget,
    cancel: &Cancel,
) -> Option<(Graph, Vec<(StableDef, Site)>)> {
    graph_and_emptied(
        session.project()?,
        file,
        entry,
        session.options(),
        budget,
        cancel,
    )
}
