//! Paths, by text: no engine type, and no file is read except to ask whether a `Cargo.toml` is there.
//!
//! Every path the crate keeps is absolute and clean, spelled as the engine spells the paths it reports. `std::path::absolute` is used and
//! never `canonicalize`: on Windows the latter adds a `\\?\` prefix that no path the engine reports has.

use std::io;
use std::path::{Component, Path, PathBuf};

/// `path` made absolute against the working folder when it is relative, and without its `.` and `..` components. By text alone: no symlink is
/// followed. (`std::path::absolute` keeps `..`, and then no loaded file would start with the root.)
pub(crate) fn absolute(path: &Path) -> io::Result<PathBuf> {
    Ok(clean(&std::path::absolute(path)?))
}

/// `path` against `root` (when `path` is relative), made clean.
pub(crate) fn absolute_in(root: &Path, path: &Path) -> PathBuf {
    clean(&root.join(path))
}

fn clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The one place a path becomes root-relative: `path` below `root`, components joined with `/`. `None` when it is not below, or not UTF-8.
pub(crate) fn relative(root: &Path, path: &Path) -> Option<String> {
    let below = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = below.components().map(|c| c.as_os_str().to_str()).collect();
    Some(parts?.join("/"))
}

/// The nearest `Cargo.toml` from the folder of `file` upward, in `root` and no higher (§2). `file` must be below `root`.
pub(crate) fn find_manifest(root: &Path, file: &Path) -> Option<PathBuf> {
    file.parent()?
        .ancestors()
        .take_while(|dir| dir.starts_with(root))
        .map(|dir| dir.join("Cargo.toml"))
        .find(|manifest| manifest.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_are_taken_out_by_text() {
        assert_eq!(clean(Path::new("/a/b/../c/./d")), Path::new("/a/c/d"));
        assert_eq!(
            absolute_in(Path::new("/r"), Path::new("x/../y.rs")),
            Path::new("/r/y.rs")
        );
        assert_eq!(
            absolute_in(Path::new("/r"), Path::new("/elsewhere/y.rs")),
            Path::new("/elsewhere/y.rs")
        );
    }

    #[test]
    fn relative_is_below_the_root_with_slashes() {
        assert_eq!(
            relative(Path::new("/r"), Path::new("/r/a/b.rs")).as_deref(),
            Some("a/b.rs")
        );
        assert_eq!(
            relative(Path::new("/r"), Path::new("/r")).as_deref(),
            Some("")
        );
        assert_eq!(relative(Path::new("/r"), Path::new("/other/a.rs")), None);
        assert_eq!(
            relative(Path::new("/r/a"), Path::new("/r/b.rs")),
            None,
            "above or beside the root is not below it"
        );
    }
}
