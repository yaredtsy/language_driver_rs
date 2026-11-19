//! A session: a project kept open for many requests, each with its own budget (`docs/04-rust/04-default-resolver/06-rs5-driver-needs.md` §2).
//!
//! Nothing here but the `Project` and the `Options`. A request builds its `Solver` and its `Sem` fresh, inside one `attach_db`, because both borrow
//! the project and a struct cannot borrow from itself. What stays warm between requests is the engine's database, which lives in the `Project`.

use std::path::Path;

use analyzer_core::graph::{Entry, Graph};
use analyzer_core::solve::{Budget, Cancel};

use crate::calls::{Options, graph_within};
use crate::{LoadOptions, OpenError, Project};

/// A loaded project and how to read it.
pub struct Session {
    project: Project,
    options: Options,
}

impl Session {
    /// Loads the workspace that owns `file` (see [`Project::open`]). `load` says what the load may run; `options` are the limits and the marker style of
    /// every request.
    pub fn open(
        root: &Path,
        file: &Path,
        load: &LoadOptions,
        options: Options,
    ) -> Result<Session, OpenError> {
        Ok(Session {
            project: Project::open(root, file, load)?,
            options,
        })
    }

    /// One request: the graph under `entry`, a def in `file` (relative to the root), walked no further than `budget`. A context past the budget is
    /// listed and not walked (`Status::Frontier`), and its node is drawn `Mark::Cut`: asking for that def as a new entry continues from there. `None`
    /// when there is no such def (see [`call_graph`](crate::call_graph)). With `Budget::default()` it is `call_graph`'s graph.
    pub fn graph(
        &self,
        file: &str,
        entry: &Entry<'_>,
        budget: &Budget,
        cancel: &Cancel,
    ) -> Option<Graph> {
        graph_within(&self.project, file, entry, &self.options, budget, cancel)
    }

    pub fn project(&self) -> &Project {
        &self.project
    }
}
