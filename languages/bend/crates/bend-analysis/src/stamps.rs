//! What the disk said about each path a book depended on, so a later look can tell whether anything moved (D41, `docs/03-typescript/11-step6-text-and-disk.md` §4).
//! Plain `std`: no parser type is here, so a `Session` keeps these beside a book, or with no book at all.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What `fs::metadata` says of one path. It follows symlinks, as the loader's `realpath` does, and any error (the path is not there, or cannot be read) is
/// `Missing`: a path that cannot be read is a path the book could not read either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stamp {
    Missing,
    File {
        len: u64,
        modified: Option<SystemTime>,
    },
    /// A folder's modified time moves when an entry is created, deleted or renamed in it. Only a path an import asked for that turned out to be a folder
    /// has this stamp, and its change is a (harmless) extra rebuild.
    Dir {
        modified: Option<SystemTime>,
    },
}

impl Stamp {
    pub(crate) fn of(path: &Path) -> Stamp {
        match fs::metadata(path) {
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
}

/// The paths one load depended on, each with its stamp: every file it read and every path an import asked for, found or not (a file created where an import
/// looked is then seen). Sorted by path, no path twice.
///
/// A path is stamped **when the loader asks for it**, before the file is read. So a file saved while the load ran has an older stamp than its text and the
/// next look finds it changed: it is read once more, never missed. A stamp taken after the load would describe text the book may not hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stamps(Vec<(PathBuf, Stamp)>);

impl Stamps {
    /// Stamps `path` now and adds it. Call it **before** reading the path.
    pub(crate) fn watch(&mut self, path: &Path) {
        self.0.push((path.to_path_buf(), Stamp::of(path)));
    }

    /// Sorted by path (rule 9: nothing here is in a hash's order), and a path watched twice keeps its first stamp, the earlier one.
    pub(crate) fn finished(mut self) -> Stamps {
        // `sort_by` is stable, so of two equal paths the one watched first stays first, and `dedup_by` keeps the first of a run
        self.0.sort_by(|a, b| a.0.cmp(&b.0));
        self.0.dedup_by(|later, first| later.0 == first.0);
        self
    }

    /// The same paths, stamped again now: what the disk says at this moment.
    pub(crate) fn restamped(&self) -> Stamps {
        Stamps(
            self.0
                .iter()
                .map(|(path, _)| (path.clone(), Stamp::of(path)))
                .collect(),
        )
    }

    #[cfg(test)]
    pub(crate) fn paths(&self) -> impl Iterator<Item = &Path> {
        self.0.iter().map(|(path, _)| path.as_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::{scratch, write};

    #[test]
    fn a_stamp_tells_file_folder_and_missing_apart_and_moves_with_the_length() {
        let root = scratch("stamps-kinds");
        let file = root.join("a.txt");
        write(&file, "one");
        assert!(matches!(Stamp::of(&root), Stamp::Dir { .. }));
        assert!(matches!(Stamp::of(&file), Stamp::File { len: 3, .. }));
        assert_eq!(Stamp::of(&root.join("none.txt")), Stamp::Missing);
    }

    #[test]
    fn a_path_watched_twice_is_kept_once_with_its_first_stamp() {
        let root = scratch("stamps-twice");
        let file = root.join("a.txt");
        let mut stamps = Stamps::default();
        stamps.watch(&file); // missing
        write(&file, "one");
        stamps.watch(&file); // there
        let stamps = stamps.finished();
        assert_eq!(stamps.paths().count(), 1);
        assert_ne!(
            stamps,
            stamps.restamped(),
            "the first stamp, Missing, stays"
        );
    }

    #[test]
    fn restamping_a_file_that_did_not_change_is_equal_and_one_that_did_is_not() {
        let root = scratch("stamps-moves");
        let file = root.join("a.txt");
        write(&file, "one");
        let mut stamps = Stamps::default();
        stamps.watch(&file);
        let stamps = stamps.finished();
        assert_eq!(stamps, stamps.restamped());
        write(&file, "three");
        assert_ne!(stamps, stamps.restamped(), "the length moved");
    }
}
