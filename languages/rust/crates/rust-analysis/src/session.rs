//! A session: a project kept open for many requests, each with its own budget (`docs/04-rust/04-default-resolver/06-rs5-driver-needs.md` §2), and kept up to
//! date with the disk (§3).
//!
//! The `Project`, the `Options`, and what it takes to know the project is out of date: the stamps of the files and folders it watches. The stamps live here
//! and not in the `Project`, so a session that could not load again would still know what to watch (step 4).
//!
//! A request builds its `Solver` and its `Sem` fresh, inside one `attach_db`, because both borrow the project and a struct cannot borrow from itself. What
//! stays warm between requests is the engine's database, which lives in the `Project`.

use std::path::Path;

use analyzer_core::graph::{Entry, Graph};
use analyzer_core::solve::{Budget, Cancel};

use crate::calls::{Options, graph_within};
use crate::sem::Stamps;
use crate::{LoadOptions, OpenError, Project};

/// A loaded project and how to read it. `Send` and not `Sync`, like the engine's database it holds: it moves to a worker thread, and two threads cannot ask
/// it at once.
pub struct Session {
    project: Project,
    options: Options,
    /// The `.rs` files and folders the last look saw, as the disk was then.
    stamps: Stamps,
}

/// What a [`Session::refresh`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refreshed {
    /// The workspace was loaded again by this call: the next request is as slow as the first. Never yet: a saved file is applied to the database that is there.
    pub reopened: bool,
    /// The session has no project, and why. Never yet: only a reload can fail.
    pub lost: Option<OpenError>,
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
        let project = Project::open(root, file, load)?;
        let stamps = project.loaded().stamp();
        Ok(Session {
            project,
            options,
            stamps,
        })
    }

    /// Brings the session up to date with the disk; call it before each request. A project `.rs` file that was saved is read again, one that was created
    /// or deleted is added or taken away, and the engine's database recomputes only what depended on them. When nothing moved it only compares stamps (no
    /// file is read). Never fails.
    ///
    /// Only saved text is read (D42): a file the editor holds changed and not saved is not seen. Nor is a dependency's source, which is not watched.
    pub fn refresh(&mut self) -> Refreshed {
        if let Some(stamps) = self.project.loaded_mut().follow_disk(&self.stamps) {
            self.stamps = stamps;
        }
        Refreshed {
            reopened: false,
            lost: None,
        }
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

    /// The loaded project: its files, the defs of a file, the environment inputs, the warnings of the load (see [`Project`]). It describes the workspace as it
    /// was loaded, plus every [`refresh`](Session::refresh) since.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// What the last look saw, for the tests of `sem`.
    #[cfg(test)]
    pub(crate) fn stamps(&self) -> &Stamps {
        &self.stamps
    }

    /// The project to write to, for the tests of `sem`.
    #[cfg(test)]
    pub(crate) fn project_mut(&mut self) -> &mut Project {
        &mut self.project
    }
}
