//! Paths, by text: no engine type, and no file is read except to ask whether a `Cargo.toml` is there.
//!
//! Every path the crate keeps is absolute and clean, and every comparison is made on **engine text**: both sides spelled the way the engine
//! spells the paths it reports (`/` as the separator, no verbatim prefix, an upper-case drive letter). `Path::starts_with` is not used: on
//! Windows a root that came from `canonicalize` (`\\?\D:\repo`) and a path the engine reports (`D:/repo/sub`) have different prefixes, and it
//! calls them different places (TypeScript's bug, commit `8fe3f66`).

use std::io;
use std::path::{Component, Path, PathBuf};

/// `path` made absolute against the working folder when it is relative, and without its `.` and `..` components. By text alone: no symlink is
/// followed. (`std::path::absolute` keeps `..`, and then no loaded file would start with the root.) A verbatim prefix is taken off.
pub(crate) fn absolute(path: &Path) -> io::Result<PathBuf> {
    Ok(normalized(clean(&std::path::absolute(path)?)))
}

/// `path` against `root` (when `path` is relative), made clean, as `absolute` does.
pub(crate) fn absolute_in(root: &Path, path: &Path) -> PathBuf {
    normalized(clean(&root.join(path)))
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

/// The path without a verbatim prefix. Text that is not UTF-8 is left as it is: it has no engine text anyway.
fn normalized(path: PathBuf) -> PathBuf {
    match path.to_str() {
        Some(text) if text.starts_with(r"\\?\") => PathBuf::from(without_verbatim_prefix(text)),
        _ => path,
    }
}

/// On Windows `canonicalize` answers with a verbatim path (`\\?\D:\repo`, `\\?\UNC\server\share`), which the engine never reports. This gives the
/// ordinary spelling back (`D:\repo`, `\\server\share`). Any other text is unchanged.
fn without_verbatim_prefix(path: &str) -> String {
    match path.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(share) => format!(r"\\{share}"),
            None => rest.to_owned(),
        },
        None => path.to_owned(),
    }
}

/// `path` as the engine spells it: `/` separators, no verbatim prefix, the drive letter upper case, no trailing `/` (except a bare root). `None`
/// when it is not UTF-8.
pub(crate) fn engine_text(path: &Path) -> Option<String> {
    let mut text = without_verbatim_prefix(path.to_str()?).replace('\\', "/");
    if text.len() > 1 && text.ends_with('/') {
        text.pop();
    }
    if let [letter, b':', b'/', ..] | [letter, b':'] = text.as_bytes()
        && letter.is_ascii_lowercase()
    {
        text[..1].make_ascii_uppercase();
    }
    Some(text)
}

/// The one place a path becomes root-relative: `path` is `root` or below it, and the answer is what follows `root`, joined with `/` (empty for
/// the root itself). `None` when it is not below, a name only starts like the root's (`/repository` is not in `/repo`), or either is not UTF-8.
/// Whether a path is inside a root is this function's `is_some`.
pub(crate) fn relative(root: &Path, path: &Path) -> Option<String> {
    let root = engine_text(root)?;
    let path = engine_text(path)?;
    let rest = path.strip_prefix(root.as_str())?;
    if rest.is_empty() {
        Some(String::new())
    } else if root.ends_with('/') {
        Some(rest.to_owned())
    } else {
        rest.strip_prefix('/').map(str::to_owned)
    }
}

/// The nearest `Cargo.toml` from the folder of `file` upward, in `root` and no higher (§2). `file` must be below `root`.
pub(crate) fn find_manifest(root: &Path, file: &Path) -> Option<PathBuf> {
    file.parent()?
        .ancestors()
        .take_while(|dir| relative(root, dir).is_some())
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
    fn a_verbatim_prefix_is_taken_off() {
        assert_eq!(without_verbatim_prefix(r"\\?\D:\repo"), r"D:\repo");
        assert_eq!(
            without_verbatim_prefix(r"\\?\UNC\server\share\a"),
            r"\\server\share\a"
        );
        assert_eq!(without_verbatim_prefix(r"D:\repo"), r"D:\repo");
        assert_eq!(without_verbatim_prefix("/repo"), "/repo");
        assert_eq!(
            normalized(PathBuf::from(r"\\?\D:\repo")),
            Path::new(r"D:\repo")
        );
    }

    #[test]
    fn relative_is_below_the_root_with_slashes() {
        let rel = |root: &str, path: &str| relative(Path::new(root), Path::new(path));
        assert_eq!(rel("/r", "/r/a/b.rs").as_deref(), Some("a/b.rs"));
        assert_eq!(rel("/r", "/r").as_deref(), Some(""));
        assert_eq!(rel("/r/", "/r/a.rs").as_deref(), Some("a.rs"));
        assert_eq!(rel("/", "/a/b.rs").as_deref(), Some("a/b.rs"));
        assert_eq!(rel("/r", "/other/a.rs"), None);
        assert_eq!(rel("/r/a", "/r/b.rs"), None, "beside the root");
        assert_eq!(rel("/r/a", "/r"), None, "above the root");
        assert_eq!(
            rel("/repo", "/repository/a.rs"),
            None,
            "a name that only starts like the root's"
        );
    }

    /// Text on both sides, so this runs on every OS: the root as `canonicalize` spells it on Windows, the path as the engine does.
    #[test]
    fn a_path_is_below_the_root_whichever_way_each_is_spelled() {
        let rel = |root: &str, path: &str| relative(Path::new(root), Path::new(path));
        assert_eq!(
            rel(r"\\?\D:\repo", "D:/repo/sub/a.rs").as_deref(),
            Some("sub/a.rs")
        );
        assert_eq!(rel(r"\\?\D:\repo", "D:/repo").as_deref(), Some(""));
        assert_eq!(rel(r"D:\repo", "D:/repo/a/b.rs").as_deref(), Some("a/b.rs"));
        assert_eq!(
            rel(r"D:\repo", r"D:\repo\a\b.rs").as_deref(),
            Some("a/b.rs")
        );
        assert_eq!(
            rel("d:/repo", "D:/repo/a.rs").as_deref(),
            Some("a.rs"),
            "drive letter case"
        );
        assert_eq!(
            rel(r"\\?\UNC\server\share", "//server/share/sub").as_deref(),
            Some("sub")
        );
        assert_eq!(rel(r"\\?\D:\repo", "D:/"), None);
        assert_eq!(rel(r"\\?\D:\repo", "D:/other"), None);
        assert_eq!(rel(r"\\?\D:\repo", "D:/repository"), None);
        assert_eq!(rel(r"D:\repo", "E:/repo/a.rs"), None, "another drive");
    }
}
