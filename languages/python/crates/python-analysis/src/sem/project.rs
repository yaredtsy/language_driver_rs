use ruff_db::files::{File, system_path_to_file};
use ruff_db::source::source_text;
use ruff_db::system::{OsSystem, SystemPath};

use ty_project::{Db as _, ProjectDatabase, ProjectMetadata};
#[cfg(test)]
use ty_python_core::ProgramFile;
#[cfg(test)]
use ty_python_semantic::Db as _;

/// The text of the project's files, for the wire: a call site is bytes relative to its function, and the wire wants a line and a column.
pub(crate) struct DbSources<'db>(pub &'db ProjectDatabase);

impl crate::wire::Sources for DbSources<'_> {
    fn text(&self, file: &str) -> Option<String> {
        let path = self.0.project().root(self.0).join(file);
        let file = system_path_to_file(self.0, &path).ok()?;
        Some(source_text(self.0, file).as_str().to_owned())
    }
}

/// A loaded project: the one handle of the public API, so no engine type is in it. Requests read from it and it is not changed by them.
pub struct Project {
    db: ProjectDatabase,
}

/// Why a project could not be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenError {
    /// The path is not a directory we can read.
    NotADirectory(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::NotADirectory(path) => write!(f, "{path} is not a directory"),
        }
    }
}

impl std::error::Error for OpenError {}

/// `path` without the verbatim prefix `std::fs::canonicalize` gives on Windows: `\\?\UNC\server\share\x` becomes `\\server\share\x` and `\\?\C:\x`
/// becomes `C:\x`. The paths the engine hands back for files have none, so with it a root cannot be stripped from them and a file
/// given by an absolute path is not found. Any other path is returned as it is, and so is one that is not valid UTF-8 (rule 8: never
/// guess). It reads the path as text, so it compiles and is tested on every OS.
fn plain(path: std::path::PathBuf) -> std::path::PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    if let Some(share) = text
        .strip_prefix(r"\\?\UNC\")
        .filter(|rest| !rest.is_empty())
    {
        return std::path::PathBuf::from(format!(r"\\{share}"));
    }
    if let Some(drive) = text.strip_prefix(r"\\?\") {
        let b = drive.as_bytes();
        if b.len() >= 2
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && b.get(2).is_none_or(|c| *c == b'\\')
        {
            return std::path::PathBuf::from(drive);
        }
    }
    path
}

impl Project {
    /// Opens the project rooted at `dir`: `dir` is the root, whatever is above it (D26). A `ty.toml` or `pyproject.toml` in `dir` is read; one
    /// in a parent folder is not, and files and modules are named from `dir`. A broken project configuration never fails it (the defaults
    /// are used): only a path that is not a directory does.
    pub fn open(dir: &str) -> Result<Project, OpenError> {
        let not = || OpenError::NotADirectory(dir.to_owned());
        let absolute = plain(std::fs::canonicalize(dir).map_err(|_| not())?);
        let root = SystemPath::from_std_path(&absolute)
            .filter(|_| absolute.is_dir())
            .ok_or_else(not)?;
        let db = open_project(root).map_err(|_| not())?;
        Ok(Project { db })
    }

    pub(crate) fn db(&self) -> &ProjectDatabase {
        &self.db
    }

    /// A file of the project, by an absolute path or one relative to the root. `None` when there is no such file.
    pub(crate) fn file(&self, path: &str) -> Option<File> {
        let given = std::path::Path::new(path);
        let absolute = if given.is_absolute() {
            plain(std::fs::canonicalize(given).ok()?)
        } else {
            let root = self.db.project().root(&self.db);
            std::path::PathBuf::from(root.as_str()).join(given)
        };
        system_path_to_file(&self.db, SystemPath::from_std_path(&absolute)?).ok()
    }
}

/// The project rooted at `dir` (D26). `ProjectMetadata::discover` walks up through the parent folders and lets a config file there
/// move the root, so its answer is used only when the root is `dir` itself. When it is a parent, `dir` gets the engine's own "no
/// config" project, which is what `discover` returns when no folder has a config.
pub(crate) fn open_project(dir: &SystemPath) -> anyhow::Result<ProjectDatabase> {
    let system = OsSystem::new(dir);
    let here = || ProjectMetadata::new(dir.file_name().unwrap_or("root"), dir.to_path_buf());
    let metadata = match ProjectMetadata::discover(dir, &system) {
        // The root is `dir` or one of its parents, cut from the same text, so it is spelled as `dir` is: no `canonicalize` needed.
        Ok(m) if m.root() == dir => m,
        Ok(_) => here(),
        Err(_err) => {
            // Broken config must never take the analyser down (quirk 13).
            // tracing::warn!("project discovery failed: {err}; using defaults");
            here()
        }
    };
    Ok(ProjectDatabase::use_defaults(metadata, system))
}

/// Open `path`, returning both handles.
#[cfg(test)]
pub(crate) fn open<'db>(
    db: &'db ProjectDatabase,
    path: &SystemPath,
) -> anyhow::Result<(File, ProgramFile<'db>)> {
    let file = system_path_to_file(db, path)?;
    Ok((file, db.program_file(file)))
}

#[cfg(test)]
mod plain_paths {
    use super::plain;
    use std::path::PathBuf;

    fn plain_of(path: &str) -> String {
        plain(PathBuf::from(path)).to_str().unwrap().to_owned()
    }

    #[test]
    fn a_verbatim_unc_path_is_a_unc_path() {
        assert_eq!(
            plain_of(r"\\?\UNC\server\share\proj\a.py"),
            r"\\server\share\proj\a.py"
        );
    }

    #[test]
    fn a_verbatim_drive_path_is_a_drive_path() {
        assert_eq!(plain_of(r"\\?\C:\proj\a.py"), r"C:\proj\a.py");
        assert_eq!(plain_of(r"\\?\C:"), "C:");
    }

    #[test]
    fn any_other_path_is_unchanged() {
        for path in [
            "/usr/proj/a.py",
            "tests/fixtures",
            r"C:\proj\a.py",
            r"\\server\share\proj",
            r"\\?\Volume{1}\proj", // a verbatim form that is neither of the two: not guessed at
            r"\\?\UNC",            // no server and no share
            "",
        ] {
            assert_eq!(plain_of(path), path);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf_8_is_unchanged() {
        use std::os::unix::ffi::OsStringExt;
        let mut bytes = br"\\?\C:\proj".to_vec();
        bytes.push(0xff);
        let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
        assert_eq!(plain(path.clone()), path);
    }
}
