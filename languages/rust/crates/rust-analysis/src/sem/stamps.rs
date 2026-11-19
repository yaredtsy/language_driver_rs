//! What the disk said about each path a session watches, so a later look can tell whether anything moved (RS5 §3). Plain `std`: no engine type is here, so
//! the `Session` keeps these beside the project and not inside it.

use std::collections::BTreeMap;
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

/// The paths a session watches, each with its stamp, sorted by path (rule 9), no path twice. A path that is gone is not kept: its folder says when it
/// comes back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stamps(BTreeMap<String, Stamp>);

impl Stamps {
    /// Stamps `path` now and keeps it, unless it is already here (the first stamp is the one taken before anything was read, and stays).
    pub(super) fn add(&mut self, path: &str) {
        if !self.0.contains_key(path) {
            self.0.insert(path.to_owned(), Stamp::of(path));
        }
    }

    pub(super) fn get(&self, path: &str) -> Option<Stamp> {
        self.0.get(path).copied()
    }

    pub(super) fn contains(&self, path: &str) -> bool {
        self.0.contains_key(path)
    }

    /// The paths and their stamps, in path order.
    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, Stamp)> {
        self.0.iter().map(|(path, stamp)| (path.as_str(), *stamp))
    }

    /// Forgets the paths that are not there: a file that was deleted, a folder that was removed.
    pub(super) fn forget_missing(&mut self) {
        self.0.retain(|_, stamp| *stamp != Stamp::Missing);
    }

    /// The same paths, stamped again now: what the disk says at this moment.
    pub(super) fn restamped(&self) -> Stamps {
        Stamps(
            self.0
                .keys()
                .map(|path| (path.clone(), Stamp::of(path)))
                .collect(),
        )
    }

    /// How many files and how many folders are watched.
    #[cfg(test)]
    pub(crate) fn counts(&self) -> (usize, usize) {
        let folders = self.0.values().filter(|stamp| stamp.is_dir()).count();
        (self.0.len() - folders, folders)
    }

    #[cfg(test)]
    pub(crate) fn paths(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
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
}
