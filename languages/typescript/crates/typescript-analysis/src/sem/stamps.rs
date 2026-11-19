//! What the disk said about each path a build depended on, so a later look can tell whether anything moved (`docs/03-typescript/11-step6-text-and-disk.md` §4).
//! Plain `std`: no engine type is here, so a `Session` can keep these beside a program, or with no program at all.

use std::time::SystemTime;

/// What `std::fs::metadata` says of one path. It follows symlinks, as the engine's `realpath` does, and any error (the path is not there, or cannot be
/// read) is `Missing`: a path that cannot be read is a path the build could not read either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stamp {
    Missing,
    File {
        len: u64,
        modified: Option<SystemTime>,
    },
    /// A folder's modified time moves when an entry is created, deleted or renamed in it, so this sees a file appear in a folder the config's `include`
    /// listed.
    Dir {
        modified: Option<SystemTime>,
    },
}

impl Stamp {
    /// The stamp of `path` now. The engine spells paths with `/` and absolute, which the OS accepts on every platform we run on (Windows takes `/`).
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
}

/// The paths one build depended on, each with its stamp. Sorted by path, no path twice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stamps(Vec<(String, Stamp)>);

impl Stamps {
    /// The same paths, stamped again now: what the disk says at this moment.
    pub(crate) fn restamped(&self) -> Stamps {
        Stamps(
            self.0
                .iter()
                .map(|(path, _)| (path.clone(), Stamp::of(path)))
                .collect(),
        )
    }

    /// Every path of `paths`, stamped now.
    pub(crate) fn of_all(paths: &[String]) -> Stamps {
        let mut paths = paths.to_vec();
        paths.sort();
        paths.dedup();
        Stamps(
            paths
                .into_iter()
                .map(|path| {
                    let stamp = Stamp::of(&path);
                    (path, stamp)
                })
                .collect(),
        )
    }

    /// The stamps for the paths a build depended on, where `self` is what was stamped **before** that build. A path `self` already has keeps its old
    /// stamp: a file saved while the build ran then differs from it at the next look, and the next look rebuilds. Stamping it now would describe the
    /// new text while the program may hold the old one, and nothing would ever notice. A path `self` does not have is new to the session and is
    /// stamped now.
    pub(crate) fn after_build(&self, paths: &[String]) -> Stamps {
        let mut paths = paths.to_vec();
        paths.sort();
        paths.dedup();
        Stamps(
            paths
                .into_iter()
                .map(|path| {
                    let stamp = match self.0.binary_search_by(|(known, _)| known.cmp(&path)) {
                        Ok(at) => self.0[at].1,
                        Err(_) => Stamp::of(&path),
                    };
                    (path, stamp)
                })
                .collect(),
        )
    }

    #[cfg(test)]
    pub(crate) fn paths(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(path, _)| path.as_str())
    }
}
