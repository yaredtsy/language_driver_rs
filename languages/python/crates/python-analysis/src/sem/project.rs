use ruff_db::files::{File, Files, system_path_to_file};
use ruff_db::source::source_text;
use ruff_db::system::{OsSystem, System, SystemPath, SystemPathBuf};

use ty_project::{Db as _, ProjectDatabase, ProjectMetadata};
#[cfg(test)]
use ty_python_core::ProgramFile;
#[cfg(test)]
use ty_python_semantic::Db as _;
use ty_python_semantic::PythonEnvironment;

/// The text of the project's files, for the wire: a call site is bytes relative to its function, and the wire wants a line and a column.
pub(crate) struct DbSources<'db>(pub &'db ProjectDatabase);

impl crate::wire::Sources for DbSources<'_> {
    fn text(&self, file: &str) -> Option<String> {
        let path = self.0.project().root(self.0).join(file);
        let file = system_path_to_file(self.0, &path).ok()?;
        Some(source_text(self.0, file).as_str().to_owned())
    }
}

/// A loaded project: the one handle of the public API, so no engine type is in it. Requests read from it and it is not changed by them;
/// [`Project::refresh`] is the one thing that changes it.
pub struct Project {
    db: ProjectDatabase,
    /// The environment inputs (see [`input_list`]) as they were when the project was opened or last reopened.
    inputs: Vec<String>,
    /// The state of those inputs at that time: `refresh` compares against it.
    stamps: Stamps,
}

/// What a refresh did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refreshed {
    /// An environment input changed (a config, the Python version, the virtual environment, a lock file), so the project was opened again
    /// from the same root: the next request is as slow as the first one.
    pub reopened: bool,
}

/// The files that decide the search paths, the Python version or the rules, relative to the root, whatever the machine has.
/// `requirements*.txt` is a pattern: [`stamps`] reads the root folder for it. `.venv/pyvenv.cfg` is here even when no environment uses
/// it yet: creating a `.venv` later changes what the engine picks when nothing else names one.
const FIXED_INPUTS: [&str; 7] = [
    "pyproject.toml",
    "ty.toml",
    ".venv/pyvenv.cfg",
    "uv.lock",
    "poetry.lock",
    "Pipfile.lock",
    "requirements*.txt",
];

/// The inputs of a project rooted at `root` whose `sys.prefix` is `environment` (`None`: no environment with a `pyvenv.cfg`): the fixed
/// entries and `<environment>/pyvenv.cfg`. An entry is relative to `root` when it is under it, and absolute otherwise; either way it is
/// written with `/` on every OS (`C:/envs/x/pyvenv.cfg`, `//server/share/x/pyvenv.cfg`). Sorted and without duplicates (rule 9).
///
/// It takes plain paths and asks the engine nothing, so it is tested with any made-up root and environment: the part that asks the
/// engine is [`environment_prefix`].
fn input_list(root: &SystemPath, environment: Option<&SystemPath>) -> Vec<String> {
    let mut list: Vec<String> = FIXED_INPUTS.iter().map(|e| (*e).to_owned()).collect();
    if let Some(environment) = environment {
        let cfg = environment.join("pyvenv.cfg");
        list.push(match cfg.strip_prefix(root) {
            Ok(relative) => relative
                .components()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join("/"),
            Err(_) => with_slashes(cfg.as_str()),
        });
    }
    list.sort();
    list.dedup();
    list
}

/// `path` with `/` for every separator. Only on Windows: there `\` is a separator and `/` is one too, so nothing is lost. On Unix a `\`
/// is a character of a file name, and the engine's canonical paths may hold one, so the path is kept as it is.
fn with_slashes(path: &str) -> String {
    if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    }
}

/// The `sys.prefix` of the environment the engine uses for `metadata`, found with the engine's own functions in its order
/// (`to_program_settings`): the `environment.python` of the config, else `PythonEnvironment::discover`.
///
/// When the config names one, the answer is its `sys.prefix` if the engine can open it, and the configured path itself (absolute, as
/// the engine makes it) if not: the config names it even when it is broken, and the user may fix it. When it is discovered, the answer is
/// there only if the environment has a `pyvenv.cfg`: a system Python has none to watch. A failure is `None`, never an error (rule 8).
fn environment_prefix(
    metadata: &ProjectMetadata,
    root: &SystemPath,
    system: &dyn System,
) -> Option<SystemPathBuf> {
    let merged = metadata.to_merged_options();
    let configured = merged
        .options()
        .environment
        .as_ref()
        .and_then(|environment| environment.python.as_ref());
    if let Some(python) = configured {
        return Some(match merged.python_environment(system) {
            Ok(Some(environment)) => environment.sys_prefix().to_path_buf(),
            _ => python.absolute(root, system),
        });
    }
    let environment = PythonEnvironment::discover(Some(root), system).ok()??;
    let prefix = environment.sys_prefix().to_path_buf();
    system.is_file(&prefix.join("pyvenv.cfg")).then_some(prefix)
}

/// For each input that exists: its path (as the list has it, or the file's name for a match of a pattern), its size and its modified
/// time. Sorted by path (rule 9).
type Stamps = Vec<(String, u64, Option<std::time::SystemTime>)>;

/// The stamps of `inputs` under `root`. An entry with a `*` is a pattern for the names in the root folder; an absolute entry is
/// stamped as it is. Reading metadata does not write (rule 2). A file that cannot be read is absent, never an error.
fn stamps(root: &std::path::Path, inputs: &[String]) -> Stamps {
    let mut names = Vec::new();
    for input in inputs {
        match input.split_once('*') {
            Some((prefix, suffix)) => {
                if let Ok(entries) = std::fs::read_dir(root) {
                    names.extend(
                        entries
                            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                            .filter(|name| name.starts_with(prefix) && name.ends_with(suffix)),
                    );
                }
            }
            None => names.push(input.clone()),
        }
    }
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter_map(|name| {
            // `join` of an absolute path is that path
            let meta = std::fs::metadata(root.join(&name)).ok()?;
            meta.is_file()
                .then(|| (name, meta.len(), meta.modified().ok()))
        })
        .collect()
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
        let (db, inputs) = open_with_inputs(root).map_err(|_| not())?;
        let stamps = stamps(&absolute, &inputs);
        Ok(Project { db, inputs, stamps })
    }

    /// Brings the project up to date with the disk; call it before each request, because a request answers from what the project knows
    /// and does not look at the disk. It never fails.
    ///
    /// The environment inputs ([`Project::environment_inputs`]) are compared with how they were at open. If one changed, appeared or
    /// went away, the project is opened again from the same root (D26: the root is never rediscovered) and `reopened` is true: the
    /// next request is as slow as the first. Otherwise the engine re-reads the metadata
    /// of the files it has seen, and only what read a changed file is recomputed; this is cheap (milliseconds), and a file created
    /// after the first request is found too. If the project cannot be opened again (the root is gone), the old one is kept, synced, and
    /// `reopened` is false: requests then answer `None`.
    ///
    /// It takes `&mut self`, so no request can be running: the compiler checks that.
    pub fn refresh(&mut self) -> Refreshed {
        let root = self.db.project().root(&self.db).to_path_buf();
        let current = stamps(root.as_std_path(), &self.inputs);
        let mut reopened = false;
        if current != self.stamps
            && root.as_std_path().is_dir()
            && let Ok((db, inputs)) = open_with_inputs(&root)
        {
            self.stamps = stamps(root.as_std_path(), &inputs);
            self.inputs = inputs;
            self.db = db;
            reopened = true;
        }
        if !reopened {
            // Never `ChangeEvent::Rescan`: it discovers the project again from the closest config and can take a parent's (D26).
            Files::sync_all(&mut self.db);
        }
        Refreshed { reopened }
    }

    /// The files whose change means "answers may differ": the host hashes them and drops what it cached when the hash changes. An entry is a
    /// path, or the glob `requirements*.txt`, relative to the root, or an absolute path when the environment the engine uses lives
    /// outside the root. Always written with `/`, on every OS, the absolute one too (`C:/envs/x/pyvenv.cfg`). The list was computed when the project was opened or last reopened by [`Project::refresh`], not on each call:
    /// finding the environment looks at `PATH`. It is sorted.
    pub fn environment_inputs(&self) -> Vec<String> {
        self.inputs.clone()
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

/// The metadata of the project rooted at `dir` (D26). `ProjectMetadata::discover` walks up through the parent folders and lets a config file
/// there move the root, so its answer is used only when the root is `dir` itself. When it is a parent, `dir` gets the engine's own "no
/// config" project, which is what `discover` returns when no folder has a config.
fn metadata_for(dir: &SystemPath, system: &OsSystem) -> ProjectMetadata {
    let here = || ProjectMetadata::new(dir.file_name().unwrap_or("root"), dir.to_path_buf());
    match ProjectMetadata::discover(dir, system) {
        // The root is `dir` or one of its parents, cut from the same text, so it is spelled as `dir` is: no `canonicalize` needed.
        Ok(m) if m.root() == dir => m,
        Ok(_) => here(),
        Err(_) => {
            // Broken config must never take the analyser down (quirk 13).
            here()
        }
    }
}

/// The project rooted at `dir` (D26), for the tests that need only the database.
#[cfg(test)]
pub(crate) fn open_project(dir: &SystemPath) -> anyhow::Result<ProjectDatabase> {
    let system = OsSystem::new(dir);
    let metadata = metadata_for(dir, &system);
    Ok(ProjectDatabase::use_defaults(metadata, system))
}

/// [`open_project`], and the environment inputs of the project it opened. Only the public [`Project`] needs them, so the tests that open
/// a database do not look for a Python on `PATH`.
fn open_with_inputs(dir: &SystemPath) -> anyhow::Result<(ProjectDatabase, Vec<String>)> {
    let system = OsSystem::new(dir);
    let metadata = metadata_for(dir, &system);
    let prefix = environment_prefix(&metadata, dir, &system);
    let inputs = input_list(dir, prefix.as_deref());
    Ok((ProjectDatabase::use_defaults(metadata, system), inputs))
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

#[cfg(test)]
mod input_lists {
    use super::input_list;
    use ruff_db::system::SystemPath;

    /// The entries every list has, sorted.
    const FIXED: [&str; 7] = [
        ".venv/pyvenv.cfg",
        "Pipfile.lock",
        "poetry.lock",
        "pyproject.toml",
        "requirements*.txt",
        "ty.toml",
        "uv.lock",
    ];

    fn list(environment: Option<&str>) -> Vec<String> {
        input_list(SystemPath::new("/proj"), environment.map(SystemPath::new))
    }

    fn sorted(list: &[String]) -> bool {
        list.windows(2).all(|w| w[0] <= w[1])
    }

    #[test]
    fn without_an_environment_it_is_the_fixed_entries() {
        let l = list(None);
        assert_eq!(l, FIXED);
        assert!(sorted(&l));
    }

    #[test]
    fn an_environment_under_the_root_is_relative() {
        let l = list(Some("/proj/env"));
        let mut expected: Vec<String> = FIXED.iter().map(|s| (*s).to_owned()).collect();
        expected.push("env/pyvenv.cfg".to_owned());
        expected.sort();
        assert_eq!(l, expected);
        assert!(sorted(&l));
    }

    #[test]
    fn the_project_s_own_venv_is_listed_once() {
        let l = list(Some("/proj/.venv"));
        assert_eq!(l, FIXED);
    }

    #[test]
    fn an_environment_outside_the_root_is_absolute() {
        let l = list(Some("/elsewhere/env"));
        assert!(l.contains(&"/elsewhere/env/pyvenv.cfg".to_owned()), "{l:?}");
        assert_eq!(l.len(), FIXED.len() + 1);
        assert!(sorted(&l));
    }

    /// Windows CI only: the engine's own spelling there has `\`.
    #[cfg(windows)]
    #[test]
    fn a_drive_path_outside_the_root_is_written_with_slashes() {
        let l = input_list(
            SystemPath::new(r"C:\proj"),
            Some(SystemPath::new(r"C:\elsewhere\env")),
        );
        assert!(
            l.contains(&"C:/elsewhere/env/pyvenv.cfg".to_owned()),
            "{l:?}"
        );
        assert!(l.iter().all(|e| !e.contains('\\')), "{l:?}");
        assert!(sorted(&l));
    }

    #[cfg(windows)]
    #[test]
    fn a_unc_path_outside_the_root_is_written_with_slashes() {
        let l = input_list(
            SystemPath::new(r"C:\proj"),
            Some(SystemPath::new(r"\\server\share\env")),
        );
        assert!(
            l.contains(&"//server/share/env/pyvenv.cfg".to_owned()),
            "{l:?}"
        );
        assert!(l.iter().all(|e| !e.contains('\\')), "{l:?}");
        assert!(sorted(&l));
    }

    /// Windows CI only: `stamps` hands an entry to `Path::join`, which keeps the right side only if it is absolute. A `//server/share/x`
    /// entry must be, or a change to a UNC environment is never seen.
    #[cfg(windows)]
    #[test]
    #[allow(clippy::join_absolute_paths)] // the join replacing the root is what this test checks
    fn an_entry_written_with_slashes_is_still_absolute() {
        assert!(std::path::Path::new("C:/elsewhere/env/pyvenv.cfg").is_absolute());
        assert!(std::path::Path::new("//server/share/env/pyvenv.cfg").is_absolute());
        let root = std::path::Path::new(r"C:\proj");
        assert_eq!(
            root.join("//server/share/env/pyvenv.cfg"),
            std::path::PathBuf::from("//server/share/env/pyvenv.cfg")
        );
    }
}
