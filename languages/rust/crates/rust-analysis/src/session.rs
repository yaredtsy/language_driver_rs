//! A session: a project kept open for many requests, each with its own budget (`docs/04-rust/04-default-resolver/06-rs5-driver-needs.md` §2), and kept up to
//! date with the disk (§3, §6).
//!
//! The `Project`, the `Options`, what it takes to open the project again (the root, the file and the `LoadOptions`, as `open` was given them), and what it
//! takes to know the project is out of date: the stamps of what it watches. The stamps live here and not in the `Project`, so a session whose reload failed
//! (no project) still knows what to watch and comes back by itself.
//!
//! A request builds its `Solver` and its `Sem` fresh, inside one `attach_db`, because both borrow the project and a struct cannot borrow from itself. What
//! stays warm between requests is the engine's database, which lives in the `Project`.

use std::path::{Path, PathBuf};

use analyzer_core::graph::{Entry, Graph};
use analyzer_core::solve::{Budget, Cancel};

use crate::calls::{Options, graph_within};
use crate::sem::{Disk, Stamps};
use crate::{LoadOptions, OpenError, Project, paths};

/// A loaded project and how to read it. `Send` and not `Sync`, like the engine's database it holds: it moves to a worker thread, and two threads cannot ask
/// it at once.
pub struct Session {
    /// As `open` was given them, made absolute and clean once: a reload opens from the same three, whatever the working folder is by then.
    root: PathBuf,
    file: PathBuf,
    load: LoadOptions,
    options: Options,
    /// `None` while the last reload failed: `lost` says why.
    project: Option<Project>,
    lost: Option<OpenError>,
    /// What the last look saw, as the disk was then.
    stamps: Stamps,
}

/// What a [`Session::refresh`] or [`Session::reload`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refreshed {
    /// The workspace was loaded again by this call: the next request is as slow as the first. A saved, created or deleted `.rs` file that is not where cargo
    /// finds a target is applied to the database that is there, and is not this.
    pub reopened: bool,
    /// The session has no project, and why (the file it was opened from is gone, or the load failed). Every request answers `None`. Reported by **every**
    /// `refresh` and `reload` while it lasts, not only the first.
    pub lost: Option<OpenError>,
}

impl Session {
    /// Loads the workspace that owns `file` (see [`Project::open`]). `load` says what the load may run; `options` are the limits and the marker style of
    /// every request. A first load that fails gives no session: open again when asked again. Only a session that was open once can lose its project and get
    /// it back.
    pub fn open(
        root: &Path,
        file: &Path,
        load: &LoadOptions,
        options: Options,
    ) -> Result<Session, OpenError> {
        let root = paths::absolute(root).map_err(|_| OpenError::NotUnderRoot)?;
        let file = paths::absolute_in(&root, file);
        let mut project = Project::open(&root, &file, load)?;
        let stamps = project.watch(&Stamps::default(), &opened(&file));
        Ok(Session {
            root,
            file,
            load: load.clone(),
            options,
            project: Some(project),
            lost: None,
            stamps,
        })
    }

    /// Brings the session up to date with the disk; call it before each request. Never fails.
    ///
    /// - A project `.rs` file that was saved is read again, one that was created or deleted is added or taken away, and the engine's database recomputes
    ///   only what depended on them. When nothing moved it only compares stamps (no file is read).
    /// - The workspace is loaded again (`reopened`) when an environment input moved (a manifest, a lock, a toolchain or cargo config file, a `build.rs`
    ///   when build scripts ran), or a `.rs` file was created or deleted where cargo finds a target by itself (`src/main.rs`, `src/bin/*`, ...).
    /// - A reload that cannot be done leaves the session with no project, and `lost` says why on every call until one works. A lost session loads again by
    ///   itself when a path it watches moved (the file it was opened from written back, say).
    ///
    /// Only saved text is read (D42): a file the editor holds changed and not saved is not seen. Nor is a dependency's source, which is not watched:
    /// [`Session::reload`] sees that one.
    pub fn refresh(&mut self) -> Refreshed {
        // stamp first: what the disk says before anything is read or loaded
        let mut now = self.stamps.restamped();
        if now == self.stamps {
            return self.report(false);
        }
        if let Some(project) = self.project.as_mut() {
            // the file the session was opened from is created or gone: no workspace is loaded around a file that is not there
            let opened_moved = self.stamps.existence_moved(&now, &opened(&self.file));
            let inputs = project.environment_inputs();
            if !opened_moved
                && let Disk::Applied =
                    project
                        .loaded_mut()
                        .follow_disk(&self.stamps, &mut now, &inputs)
            {
                self.stamps = now;
                return self.report(false);
            }
        }
        self.rebuild(now)
    }

    /// Loads the workspace again whatever the stamps say: for a change [`Session::refresh`] does not see (a dependency's source edited by hand).
    pub fn reload(&mut self) -> Refreshed {
        let before = self.stamps.restamped();
        self.rebuild(before)
    }

    /// `before` is what the disk said of the session's paths just now, before anything is loaded.
    fn rebuild(&mut self, before: Stamps) -> Refreshed {
        // The old project is freed first, so two databases are never alive at once.
        self.project = None;
        match Project::open(&self.root, &self.file, &self.load) {
            Ok(mut project) => {
                // a path the session knew keeps the stamp from before the load; a new one is stamped now and read once more
                self.stamps = project.watch(&before, &opened(&self.file));
                self.project = Some(project);
                self.lost = None;
                self.report(true)
            }
            Err(error) => {
                // keep watching what was watched, and the file it was opened from, so that a change brings the session back
                self.stamps = before;
                self.stamps.add_kept(&opened(&self.file));
                self.lost = Some(error);
                self.report(false)
            }
        }
    }

    fn report(&self, reopened: bool) -> Refreshed {
        Refreshed {
            reopened,
            lost: self.lost.clone(),
        }
    }

    /// One request: the graph under `entry`, a def in `file` (relative to the root), walked no further than `budget`. A context past the budget is
    /// listed and not walked (`Status::Frontier`), and its node is drawn `Mark::Cut`: asking for that def as a new entry continues from there. `None`
    /// when there is no such def (see [`call_graph`](crate::call_graph)), and while the session has no project. With `Budget::default()` it is
    /// `call_graph`'s graph.
    pub fn graph(
        &self,
        file: &str,
        entry: &Entry<'_>,
        budget: &Budget,
        cancel: &Cancel,
    ) -> Option<Graph> {
        graph_within(
            self.project.as_ref()?,
            file,
            entry,
            &self.options,
            budget,
            cancel,
        )
    }

    /// The loaded project: its files, the defs of a file, the environment inputs, the warnings of the load (see [`Project`]). It describes the workspace as it
    /// was loaded, plus every [`refresh`](Session::refresh) since. `None` while the session has no project (see [`Refreshed::lost`]).
    pub fn project(&self) -> Option<&Project> {
        self.project.as_ref()
    }

    /// What the last look saw, for the tests of `sem`.
    #[cfg(test)]
    pub(crate) fn stamps(&self) -> &Stamps {
        &self.stamps
    }

    /// The project to write to, for the tests of `sem`.
    #[cfg(test)]
    pub(crate) fn project_mut(&mut self) -> &mut Project {
        self.project.as_mut().expect("the session has a project")
    }
}

/// The file the session was opened from, as engine text (a path the session keeps is made absolute and clean by `open`, so it has one unless it is not
/// UTF-8, and then it cannot be stamped and the empty text is stamped as a path that is not there).
fn opened(file: &Path) -> String {
    paths::engine_text(file).unwrap_or_default()
}
