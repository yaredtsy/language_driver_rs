//! A session: a book kept open for many requests, each with its own budget (`docs/08-bend/03-phase-2-defs-and-calls.md` §4), and kept up to date with the disk
//! the way TypeScript's and Rust's are (D41, D46; `docs/03-typescript/11-step6-text-and-disk.md`).
//!
//! The `Book`, the `Options`, what it takes to open the book again (the root and the entry, as `open` was given them), and what it takes to know the book is out
//! of date: the stamps of every path the last load depended on. The stamps live here and not in the `Book`, so a session whose rebuild failed (no book) still
//! knows what to watch and comes back by itself.
//!
//! A book is a few megabytes of text read in milliseconds, so a change is answered by **building it again**, not by patching it. There is no database to keep
//! warm and no engine to ask: nothing here is cleverer than "did a stamp move?".
//!
//! A request builds its `Solver` fresh, because a `Solver` borrows the book and a struct cannot borrow from itself.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use analyzer_core::graph::{ContextId, Entry, Graph};
use analyzer_core::solve::{Budget, Cancel};

use crate::book::Book;
use crate::calls::{Keys, Options, continue_from, graph_within};
use crate::stamps::Stamps;

/// Why a book could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenError {
    /// The root or the entry file is not there, or cannot be read: there is nothing to load.
    NoSuchFile,
    /// The entry file is not under the root (D33): project code is under the root, so a file beside it or above it has no tree.
    OutsideRoot,
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::NoSuchFile => write!(
                f,
                "the root or the entry file does not exist or cannot be read: there is nothing to load"
            ),
            OpenError::OutsideRoot => write!(f, "the entry file is not under the root"),
        }
    }
}

impl std::error::Error for OpenError {}

/// A loaded book and how to read it. `Send` and `Sync` (a book is plain data), but a rebuild takes `&mut self`: it cannot run while a request reads.
pub struct Session {
    /// As `open` was given them, the root made absolute once: a rebuild opens from the same two, whatever the working folder is by then.
    root: PathBuf,
    entry: PathBuf,
    options: Options,
    /// `None` while the last rebuild failed: `lost` says why.
    book: Option<Book>,
    lost: Option<OpenError>,
    /// The paths the last load depended on, as the disk was when each was asked for.
    stamps: Stamps,
    /// For every context of every graph this book has answered: its digest and the canonical key it stands for (`docs/08-bend/06-context-layer-design.md` §5), which
    /// is what lets a node that a request cut be continued under its key. Dropped with the book. Only looked up, never iterated (rule 9). Under `Precision::Default`
    /// every digest is zeros and nothing is kept.
    ///
    /// A `Mutex` and not a `RefCell` or `&mut self`: `graph` takes `&self` (many requests may read one session) and a `Session` is `Send` and `Sync`, which a `RefCell`
    /// would end. The lock is held only to look a key up or to add a request's keys, never while a graph is built.
    keys: Mutex<Keys>,
}

/// What a [`Session::refresh`] or [`Session::reload`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refreshed {
    /// The book was built again by this call: the next request reads the files again.
    pub reopened: bool,
    /// The session has no book, and why (the entry file is gone, or its folder). Every request answers `None`. Reported by **every** `refresh` and `reload`
    /// while it lasts, not only the first.
    pub lost: Option<OpenError>,
}

impl Session {
    /// Loads the book of `entry`, a file under `root` (see [`Book::open`]). `options` are the limits and the marker style of every request. A first load that
    /// fails gives no session: open again when asked again. Only a session that was open once can lose its book and get it back.
    pub fn open(root: &Path, entry: &Path, options: Options) -> Result<Session, OpenError> {
        let root = std::path::absolute(root).map_err(|_| OpenError::NoSuchFile)?;
        let (built, stamps) = Book::load(&root, entry);
        Ok(Session {
            root,
            entry: entry.to_path_buf(),
            options,
            book: Some(built?),
            lost: None,
            stamps,
            keys: Mutex::new(HashMap::new()),
        })
    }

    /// Brings the session up to date with the disk; call it before each request (the driver calls it, `graph` does not: D41 (c)). Builds the book again when
    /// a path the last load depended on changed: a file it read saved, an imported file created where an import looked for it, or deleted. Otherwise it only
    /// compares stamps (no file is read). Never fails: a rebuild that cannot be done leaves the session with no book, and `lost` says why on every call until
    /// one works. A file that does not parse is not lost: it is in the book with its errors.
    ///
    /// Only saved text is read (D42): a file the editor holds changed and not saved is not seen. A file saved within the clock's tick of the last look, to
    /// the same length, is not seen either (a stamp is the length and the modified time): [`Session::reload`] sees it.
    pub fn refresh(&mut self) -> Refreshed {
        if self.stamps.restamped() == self.stamps {
            return self.report(false);
        }
        self.rebuild()
    }

    /// Builds the book again whatever the stamps say: for a change [`Session::refresh`] does not see.
    pub fn reload(&mut self) -> Refreshed {
        self.rebuild()
    }

    fn rebuild(&mut self) -> Refreshed {
        // The old book is freed first, so two are never alive at once. The keys go with it: they name defs of that book.
        self.book = None;
        self.keys
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        // Every path is stamped by the load itself, just before it is read: a file saved while the load runs keeps a stamp older than its text, and the next
        // look builds again. When the load fails, what it stamped is the entry's path, which is what brings the session back.
        let (built, stamps) = Book::load(&self.root, &self.entry);
        self.stamps = stamps;
        match built {
            Ok(book) => {
                self.book = Some(book);
                self.lost = None;
                self.report(true)
            }
            Err(error) => {
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

    /// One request: the graph under `entry`, a def in `file` (relative to the root, or absolute), walked no further than `budget`. A context past the budget
    /// is listed and not walked (`Status::Frontier`), and its node is drawn `Mark::Cut`: asking for that def as a new entry continues from there. `None`
    /// when there is no such def (no such file in the book, no such qualname, fewer than `occurrence + 1` defs with it, or [`Entry::File`]: a Bend file has
    /// no code of its own), and while the session has no book. A request whose `cancel` was set before it started is a graph like any other cancelled one.
    /// With `Budget::default()` it is [`call_graph`](crate::call_graph)'s graph.
    pub fn graph(
        &self,
        file: &str,
        entry: &Entry<'_>,
        budget: &Budget,
        cancel: &Cancel,
    ) -> Option<Graph> {
        let (graph, keys) = graph_within(
            self.book.as_ref()?,
            file,
            entry,
            &self.options,
            budget,
            cancel,
        )?;
        self.remember(keys);
        Some(graph)
    }

    /// Continues a context: the graph that starts at `from`, which a request listed (and cut, as a frontier, or did not). Asking for the def alone would start it
    /// under the open key and give a subtree that differs from the whole tree's; this starts it under the key it had, so that the subtree is the one the whole tree
    /// has (BC-D7). `from.func` names the def, as the ids of a graph do (by marker, or by file, qualname and which of the name); the key is the one this session kept
    /// for `from.digest`. Zeros are the open key. A digest this session has not made (another session, a book that was loaded again since), or a def of the key that
    /// is gone, gives the graph under the open key with `context_not_found` counted in `Stats.language`. `None` when no def of the book has that id, and while the
    /// session has no book. Under `Precision::Default` every digest is zeros and this is [`Session::graph`] for that def.
    pub fn graph_from(&self, from: &ContextId, budget: &Budget, cancel: &Cancel) -> Option<Graph> {
        let book = self.book.as_ref()?;
        let known = self
            .keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&from.digest)
            .cloned();
        let (graph, keys) =
            continue_from(book, from, known.as_ref(), &self.options, budget, cancel)?;
        self.remember(keys);
        Some(graph)
    }

    fn remember(&self, keys: Keys) {
        if !keys.is_empty() {
            self.keys
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend(keys);
        }
    }

    /// Why the session has no book, while it has none: what the last `open`, `refresh` or `reload` reported. `None` when it has one.
    pub fn lost(&self) -> Option<&OpenError> {
        self.lost.as_ref()
    }

    /// The loaded book, or `None` while the session has no book (see [`Refreshed::lost`]).
    pub fn book(&self) -> Option<&Book> {
        self.book.as_ref()
    }
}

#[cfg(test)]
mod tests {
    //! What needs a hook inside the crate: a count of loads.

    use std::path::Path;

    use analyzer_core::solve::{Budget, Cancel};
    use bend_syntax::MarkerStyle;

    use super::*;
    use crate::book::BUILDS;
    use crate::calls::Precision;
    use crate::scratch::{scratch, write};

    fn builds() -> u32 {
        BUILDS.with(std::cell::Cell::get)
    }

    /// Nothing changed: nothing is loaded. A change is one load, a `reload` is one more whatever the stamps say, and a lost session loads nothing until
    /// a stamp moves.
    #[test]
    fn a_refresh_loads_only_when_a_stamp_moved() {
        let root = scratch("rebuild-count");
        write(root.join("main.bend"), "def main() -> U32:\n  1\n");
        let mut session = Session::open(
            &root,
            Path::new("main.bend"),
            Options::new(MarkerStyle::above("ID")).with_precision(Precision::Default),
        )
        .unwrap();
        let opened = builds();
        for _ in 0..3 {
            assert!(!session.refresh().reopened);
        }
        assert_eq!(builds(), opened, "nothing changed: nothing loaded");

        write(root.join("main.bend"), "def main() -> U32:\n  22\n");
        assert!(session.refresh().reopened);
        assert_eq!(builds(), opened + 1);
        assert!(!session.refresh().reopened);
        assert_eq!(builds(), opened + 1, "and now nothing changed again");

        assert!(session.reload().reopened);
        assert_eq!(builds(), opened + 2, "reload loads whatever the stamps say");

        std::fs::remove_file(root.join("main.bend")).unwrap();
        assert_eq!(session.refresh().lost, Some(OpenError::NoSuchFile));
        assert_eq!(builds(), opened + 3, "the deletion is a change: one load");
        for _ in 0..3 {
            assert_eq!(session.refresh().lost, Some(OpenError::NoSuchFile));
        }
        assert_eq!(
            builds(),
            opened + 3,
            "lost and nothing changed: nothing loaded"
        );
        assert!(
            session
                .graph(
                    "main.bend",
                    &Entry::File,
                    &Budget::default(),
                    &Cancel::new()
                )
                .is_none()
        );
    }
}
