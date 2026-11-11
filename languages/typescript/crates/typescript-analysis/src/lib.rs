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
    /// searching no higher than `root`. With none, an inferred project: `file` alone is the root and its imports pull in the rest. A
    /// project that loads with type errors is not an error.
    pub fn open(root: &Path, file: &Path) -> Result<Project, OpenError> {
        Ok(Project {
            sem: sem::Sem::open(root, file)?,
        })
    }

    /// The project's own code that lives under the root, relative to it with `/`, sorted. Left out: `node_modules`, declaration files
    /// (`.d.ts`), the bundled libraries, and project code outside the root.
    pub fn files(&self) -> Vec<String> {
        let mut files: Vec<String> = self
            .sem
            .files()
            .filter(|&file| self.sem.is_project_code(file))
            .filter_map(|file| self.sem.relative_path(file))
            .collect();
        files.sort();
        files
    }
}

#[cfg(feature = "__proof")]
#[doc(hidden)]
pub mod proof {
    //! Not API. The real `Sem` and `File`, public only so a doctest (a separate crate) can name them in the proof that a `File<'p>` cannot
    //! outlive its `Sem`. `Sem::drop` frees the program with `unsafe`, and its `SAFETY` comment rests on this. A doctest on a wrapper would
    //! prove only the wrapper's own signature, so it uses the real types.
    //!
    //! ```compile_fail,E0505
    //! use std::path::Path;
    //! use typescript_analysis::proof::Sem;
    //!
    //! let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/three-files");
    //! let sem = Sem::open(&root, Path::new("src/main.ts")).unwrap();
    //! let file = sem.file("src/main.ts").unwrap();
    //! drop(sem);
    //! let _still_used = file;
    //! ```
    //!
    //! The twin below differs only in the order of its last two lines, and must compile and run: a typo or a renamed function cannot hide
    //! behind the failure above, because it would break the twin too.
    //!
    //! ```
    //! use std::path::Path;
    //! use typescript_analysis::proof::Sem;
    //!
    //! let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/three-files");
    //! let sem = Sem::open(&root, Path::new("src/main.ts")).unwrap();
    //! let file = sem.file("src/main.ts").unwrap();
    //! let _still_used = file;
    //! drop(sem);
    //! ```
    //!
    //! The error code is pinned for the reader: stable rustdoc ignores it (checked: pinning `E0499` still passes), and `RUSTC_BOOTSTRAP=1
    //! RUSTDOCFLAGS=-Zunstable-options cargo test --doc` enforces it (`E0505`, "cannot move out of `sem` because it is borrowed").
    pub use crate::sem::{File, Sem};
}

/// Why a project did not load.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenError {
    /// The root or the file does not exist, or the file is not a file.
    NoSuchFile,
    /// The config could not be read, or has a syntax error. The text is for a person.
    ConfigError(String),
    /// The config is valid and was loaded, but its program does not hold the file (its `include` / `exclude` leave it out). Holds the
    /// config's path relative to the root, or `(inferred project)` when there was none (the file is not TypeScript or JavaScript). A solution config's `references` are not followed yet (TS3 step 6).
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
