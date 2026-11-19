//! What the disk said about each path a session watches, so a later look can tell whether anything moved (RS5 §3). Plain `std`: no engine type is here, so
//! the `Session` keeps these beside the project and not inside it.

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

/// What `std::fs::metadata` says of one path. It follows symlinks, as the engine's own load does (`follow_links(true)`), and any error is `Missing`: a
/// path that cannot be read is a path the load could not read either.
///
/// A file saved again within the clock's resolution with the same length has the same stamp: the stamp cannot see it. The modified time of a file system
/// is as fine as a few milliseconds to a second, and a person saving a file is slower than that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stamp {
    Missing,
    File {
        len: u64,
        modified: Option<SystemTime>,
    },
    /// A folder's modified time moves when an entry is created, deleted or renamed in it: this is how a file that appeared is seen.
    Dir {
        modified: Option<SystemTime>,
    },
}

impl Stamp {
    /// The stamp of `path` now (engine text: `/` separators, absolute, which the OS accepts on every platform we run on).
    pub(crate) fn of(path: &str) -> Stamp {
        match std::fs::metadata(path) {
            Ok(meta) if meta.is_dir() => Stamp::Dir {
                modified: meta.modified().ok(),
            },
            Ok(meta) => Stamp::File {
                len: meta.len(),
                modified: meta.modified().ok(),
            },
            Err(_) => Stamp::Missing,
        }
    }

    pub(crate) fn is_dir(self) -> bool {
        matches!(self, Stamp::Dir { .. })
    }
}

/// The paths a session watches, each with its stamp, sorted by path (rule 9), no path twice.
///
/// Kinds of path, by what happens to them:
/// - a **watched** path (a project `.rs` file, a folder above one): when it is gone, it is forgotten, and its folder says when it comes back;
/// - a **kept** path (an environment input, the file the session was opened from): stamped as missing and kept, so that its creation is seen;
/// - a **scout** (a folder from the project root down to a package root: when it changes, its new folders are walked for an input, because a package
///   created there is a crate the load did not know);
/// - an **input** (kept, and the engine reads it to build the crate graph: a manifest, a lock, a toolchain or cargo config file, a `build.rs` when
///   build scripts ran): when it changes the workspace is loaded again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stamps {
    seen: BTreeMap<String, Stamp>,
    kept: BTreeSet<String>,
    inputs: BTreeSet<String>,
    scouts: BTreeSet<String>,
}

impl Stamps {
    /// Stamps `path` now and keeps it, unless it is already here (the first stamp is the one taken before anything was read, and stays).
    pub(super) fn add(&mut self, path: &str) {
        if !self.seen.contains_key(path) {
            self.seen.insert(path.to_owned(), Stamp::of(path));
        }
    }

    /// Like `add`, and the path is not forgotten when it is missing.
    pub(crate) fn add_kept(&mut self, path: &str) {
        self.add(path);
        self.kept.insert(path.to_owned());
    }

    /// Like `add_kept`, and a change of the path is a reason to load the workspace again.
    pub(super) fn add_input(&mut self, path: &str) {
        self.add_kept(path);
        self.inputs.insert(path.to_owned());
    }

    /// Like `add`, and a change of the folder is a reason to look in it for a new package.
    pub(super) fn add_scout(&mut self, path: &str) {
        self.add(path);
        self.scouts.insert(path.to_owned());
    }

    pub(super) fn is_scout(&self, path: &str) -> bool {
        self.scouts.contains(path)
    }

    pub(super) fn get(&self, path: &str) -> Option<Stamp> {
        self.seen.get(path).copied()
    }

    pub(super) fn contains(&self, path: &str) -> bool {
        self.seen.contains_key(path)
    }

    pub(super) fn is_input(&self, path: &str) -> bool {
        self.inputs.contains(path)
    }

    /// The paths and their stamps, in path order.
    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, Stamp)> {
        self.seen
            .iter()
            .map(|(path, stamp)| (path.as_str(), *stamp))
    }

    /// Forgets the watched paths that are not there: a file that was deleted, a folder that was removed. A kept path stays.
    pub(super) fn forget_missing(&mut self) {
        let kept = &self.kept;
        self.seen
            .retain(|path, stamp| *stamp != Stamp::Missing || kept.contains(path));
        let seen = &self.seen;
        self.scouts.retain(|path| seen.contains_key(path));
    }

    /// The same paths, stamped again now: what the disk says at this moment.
    pub(crate) fn restamped(&self) -> Stamps {
        Stamps {
            seen: self
                .seen
                .keys()
                .map(|path| (path.clone(), Stamp::of(path)))
                .collect(),
            kept: self.kept.clone(),
            inputs: self.inputs.clone(),
            scouts: self.scouts.clone(),
        }
    }

    /// Whether `path` is there in one of `self` and `now` and not in the other (it was created, or it is gone).
    pub(crate) fn existence_moved(&self, now: &Stamps, path: &str) -> bool {
        let missing = |stamps: &Stamps| stamps.get(path) == Some(Stamp::Missing);
        missing(self) != missing(now)
    }

    /// `self` is what was stamped just after a load; `before` is what was stamped just before it. A path `before` already had keeps its stamp from
    /// before: a file saved while the load ran then differs from it at the next look, and the next look reads it again. Stamping it after the load would
    /// describe the new text while the database may hold the old, and nothing would notice. A path `before` did not have is new, and keeps the stamp
    /// it has.
    pub(super) fn keeping(mut self, before: &Stamps) -> Stamps {
        for (path, stamp) in &mut self.seen {
            if let Some(old) = before.seen.get(path) {
                *stamp = *old;
            }
        }
        self
    }

    /// How many files and how many folders are watched.
    #[cfg(test)]
    pub(crate) fn counts(&self) -> (usize, usize) {
        let folders = self.seen.values().filter(|stamp| stamp.is_dir()).count();
        (self.seen.len() - folders, folders)
    }

    #[cfg(test)]
    pub(crate) fn paths(&self) -> impl Iterator<Item = &str> {
        self.seen.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sem::common::{copy_of_fixture, tmp_root, write};

    /// A file, a folder and a path that is not there are told apart, and a file's stamp moves with its length.
    #[test]
    fn a_stamp_tells_file_folder_and_missing_apart() {
        let root = tmp_root().join("stamps_unit");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("a.rs");
        write(&file, "one");
        let text = |path: &std::path::Path| path.to_str().unwrap().to_owned();

        let mut stamps = Stamps::default();
        stamps.add(&text(&root));
        stamps.add(&text(&file));
        stamps.add(&text(&root.join("none.rs")));
        assert_eq!(stamps.counts(), (2, 1), "the missing path is a file here");
        assert!(stamps.get(&text(&root)).unwrap().is_dir());
        assert_eq!(
            stamps.get(&text(&root.join("none.rs"))),
            Some(Stamp::Missing)
        );
        assert_eq!(stamps, stamps.restamped());

        write(&file, "three");
        assert_ne!(stamps, stamps.restamped(), "the length moved");
    }

    /// A path already stamped keeps its first stamp: `add` is for what is new.
    #[test]
    fn adding_a_path_again_keeps_the_stamp_taken_first() {
        let root = copy_of_fixture("lone", "stamps_keep_first");
        let file = root.join("main.rs");
        let path = file.to_str().unwrap().to_owned();
        let mut stamps = Stamps::default();
        stamps.add(&path);
        let first = stamps.get(&path);
        let mut text = std::fs::read_to_string(&file).unwrap();
        text.push_str("\n// longer\n");
        write(&file, &text);
        stamps.add(&path);
        assert_eq!(stamps.get(&path), first);
        assert_ne!(Stamp::of(&path), first.unwrap());
    }

    /// A kept path is stamped as missing and stays; a watched one is forgotten. A path of `before` keeps its older stamp across a load.
    #[test]
    fn a_kept_path_survives_being_missing_and_a_known_path_keeps_its_old_stamp() {
        let root = tmp_root().join("stamps_kept");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (watched, kept) = (root.join("a.rs"), root.join("Cargo.lock"));
        let text = |path: &std::path::Path| path.to_str().unwrap().to_owned();

        let mut stamps = Stamps::default();
        stamps.add(&text(&watched));
        stamps.add_input(&text(&kept));
        assert_eq!(stamps.get(&text(&kept)), Some(Stamp::Missing));
        assert!(stamps.is_input(&text(&kept)) && !stamps.is_input(&text(&watched)));
        stamps.forget_missing();
        assert!(
            !stamps.contains(&text(&watched)),
            "a watched path is forgotten"
        );
        assert!(stamps.contains(&text(&kept)), "a kept one stays");

        write(&kept, "version = 4\n");
        let before = stamps.restamped();
        assert_ne!(before, stamps, "its creation is seen");
        write(&kept, "version = 4\n# longer\n");
        let mut after = Stamps::default();
        after.add_input(&text(&kept));
        let after = after.keeping(&before);
        assert_eq!(after.get(&text(&kept)), before.get(&text(&kept)));
        assert_ne!(
            after,
            after.restamped(),
            "saved during the load: seen at the next look"
        );
    }
}
