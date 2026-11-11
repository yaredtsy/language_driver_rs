//! SLOW: project loading, call sites, call graph. `sem.rs` is the only file that imports the checker.

mod sem;

use std::fmt;
use std::path::Path;

/// A loaded TypeScript / JavaScript program. Dropping it frees the program.
pub struct Project {
    sem: sem::Sem,
}

impl Project {
    /// Loads the project that owns `file` (relative to `root`, or absolute): the nearest `tsconfig.json` or `jsconfig.json` above it,
    /// searching no higher than `root`. A project that loads with type errors is not an error.
    pub fn open(root: &Path, file: &Path) -> Result<Project, OpenError> {
        Ok(Project {
            sem: sem::Sem::open(root, file)?,
        })
    }

    /// The files of the loaded program that live under the root, relative to it, sorted. The bundled libraries are not listed.
    pub fn files(&self) -> Vec<String> {
        self.sem.files()
    }
}

/// Why a project did not load.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenError {
    /// The root or the file does not exist, or the file is not a file.
    NoSuchFile,
    /// No config was found, or the config could not be read. The text is for a person.
    ConfigError(String),
    /// The config is valid and was loaded, but its program does not hold the file (its `include` / `exclude` leave it out). Holds the
    /// config's path relative to the root. A solution config's `references` are not followed yet (TS3 step 6).
    NotInProject(String),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::NoSuchFile => f.write_str("no such file"),
            OpenError::ConfigError(reason) => write!(f, "config error: {reason}"),
            OpenError::NotInProject(config) => write!(f, "{config} does not include the file"),
        }
    }
}

impl std::error::Error for OpenError {}
