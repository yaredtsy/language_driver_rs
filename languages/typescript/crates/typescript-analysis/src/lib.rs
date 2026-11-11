//! SLOW: project loading, call sites, call graph. `sem.rs` is the only file that imports the checker.

mod sem;

use std::fmt;
use std::path::Path;

/// Config, package and lock files whose change means "load again". The last two are bun's text and binary lock files.
const ENVIRONMENT_GLOBS: [&str; 9] = [
    "**/tsconfig*.json",
    "**/jsconfig.json",
    "**/package.json",
    "**/package-lock.json",
    "**/npm-shrinkwrap.json",
    "**/yarn.lock",
    "**/pnpm-lock.yaml",
    "**/bun.lock",
    "**/bun.lockb",
];

/// Whether `ENVIRONMENT_GLOBS` already covers the file at `path`. Only the file name matters: every glob starts with `**/`.
fn matches_a_glob(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    (name.starts_with("tsconfig") && name.ends_with(".json"))
        || [
            "jsconfig.json",
            "package.json",
            "package-lock.json",
            "npm-shrinkwrap.json",
            "yarn.lock",
            "pnpm-lock.yaml",
            "bun.lock",
            "bun.lockb",
        ]
        .contains(&name)
}

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

    /// The config this program was built from, relative to the root (the one that holds the file, after following references and climbing),
    /// or `None` for an inferred project.
    pub fn config_file(&self) -> Option<&str> {
        self.sem.config_file()
    }

    /// What, if it changes, means "load this project again": globs for the config, package and lock files, then the exact config files that fed
    /// this program and no glob covers (an `extends` target such as `config/shared.json`, or a config skipped for a syntax error). Relative to the
    /// root. The repo's own `.d.ts` files are not here: they are [`Project::declaration_files`].
    pub fn environment_inputs(&self) -> Vec<String> {
        let mut inputs: Vec<String> = ENVIRONMENT_GLOBS
            .iter()
            .map(|glob| (*glob).to_owned())
            .collect();
        inputs.extend(
            self.sem
                .config_files()
                .iter()
                .filter(|name| !matches_a_glob(name))
                .cloned(),
        );
        inputs
    }

    /// The repo's own declaration files (`.d.ts`) in the program, relative to the root, sorted. They are not in [`Project::files`], because nothing
    /// in them is analyzed, but a call into one resolves to the declaration and not to a def, so editing one must reload the project.
    pub fn declaration_files(&self) -> Vec<String> {
        let mut files: Vec<String> = self
            .sem
            .files()
            .filter(|&file| self.sem.is_repo_declaration_file(file))
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
    //! A handle must not cross threads: tsrs enters its regions per thread and one checker answers one question at a time, so nothing here
    //! claims a `File` is safe to move or share. (Whether that should change is TS5's question, when the driver asks for parallelism.)
    //!
    //! ```compile_fail,E0277
    //! use std::path::Path;
    //! use typescript_analysis::proof::Sem;
    //!
    //! let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/three-files");
    //! let sem = Sem::open(&root, Path::new("src/main.ts")).unwrap();
    //! let file = sem.file("src/main.ts").unwrap();
    //! std::thread::scope(|scope| {
    //!     scope.spawn(move || drop(file));
    //! });
    //! ```
    //!
    //! ```
    //! use std::path::Path;
    //! use typescript_analysis::proof::Sem;
    //!
    //! let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/three-files");
    //! let sem = Sem::open(&root, Path::new("src/main.ts")).unwrap();
    //! let file = sem.file("src/main.ts").unwrap();
    //! (move || drop(file))();
    //! ```
    //!
    //! Nor may two threads share one (`&File` crossing needs `File: Sync`):
    //!
    //! ```compile_fail,E0277
    //! use std::path::Path;
    //! use typescript_analysis::proof::Sem;
    //!
    //! let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/three-files");
    //! let sem = Sem::open(&root, Path::new("src/main.ts")).unwrap();
    //! let file = sem.file("src/main.ts").unwrap();
    //! std::thread::scope(|scope| {
    //!     scope.spawn(|| {
    //!         let _shared = &file;
    //!     });
    //! });
    //! ```
    //!
    //! ```
    //! use std::path::Path;
    //! use typescript_analysis::proof::Sem;
    //!
    //! let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/three-files");
    //! let sem = Sem::open(&root, Path::new("src/main.ts")).unwrap();
    //! let file = sem.file("src/main.ts").unwrap();
    //! (|| {
    //!     let _shared = &file;
    //! })();
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
    /// No config's program holds the file: its `include` / `exclude` leave it out, or it is not TypeScript or JavaScript at all.
    NotInProject {
        /// The nearest config, relative to the root, or `(inferred project)` when there was none.
        config: String,
        /// Configs the search had to skip because they did not parse, as `path: reason`. One of them may be the real owner of the file, so a
        /// person looking at "not in project" should look here first.
        skipped: Vec<String>,
    },
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::NoSuchFile => f.write_str("no such file"),
            OpenError::ConfigError(reason) => write!(f, "config error: {reason}"),
            OpenError::NotInProject { config, skipped } if skipped.is_empty() => {
                write!(f, "{config} does not include the file")
            }
            OpenError::NotInProject { config, skipped } => write!(
                f,
                "{config} does not include the file (configs skipped because they did not parse: {})",
                skipped.join("; ")
            ),
        }
    }
}

impl std::error::Error for OpenError {}
