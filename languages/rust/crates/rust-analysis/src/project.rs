use std::fmt;
use std::path::Path;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::outline::DefKind;
use analyzer_core::text::Span;
use analyzer_core::wire::Sources;

use crate::paths;
use crate::sem::{self, Loaded, Rung, attached, text_of};
use crate::warning::{self, Facts, Warning};

/// Config, manifest and lock files whose change means "load again". Each is relative to the root, so it starts with `**/`.
const ENVIRONMENT_GLOBS: [&str; 5] = [
    "**/Cargo.toml",
    "**/Cargo.lock",
    "**/rust-toolchain",
    "**/rust-toolchain.toml",
    "**/.cargo/config.toml",
];

/// Added when build scripts ran: a `build.rs` is code that changes what the load sees.
const BUILD_SCRIPT_GLOB: &str = "**/build.rs";

/// A loaded Cargo workspace. Slow to make (seconds); asking it things is cheap.
pub struct Project {
    loaded: Loaded,
    manifest: String,
    warnings: Vec<Warning>,
    /// Build scripts ran (they were asked for, or proc macros were: those are built by the same run).
    build_scripts_ran: bool,
}

/// A def found by qualname: which of the same-named defs it is, what it is, where, and its marker (D15).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DefMatch {
    /// The number `Entry::Def::occurrence` takes: 0-based, in source order.
    pub occurrence: u16,
    pub kind: DefKind,
    /// The whole def, in bytes of the file.
    pub range: Span,
    pub marker: Option<String>,
}

/// What a load may do beyond reading the workspace (RS3 §3). Both are off by default: each runs the project's own code (a `build.rs`, a proc
/// macro) and costs a build. What stays missing while they are off is counted in [`Project::skipped`].
///
/// The struct is `#[non_exhaustive]`: make it with `LoadOptions::default()` and set the fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LoadOptions {
    /// Run the workspace's build scripts: code under `include!(concat!(env!("OUT_DIR"), …))` and the `cfg`s a `build.rs` sets are then seen.
    pub build_scripts: bool,
    /// Start the proc-macro server: derives and attribute macros then expand. They are built by the build run, so this runs the build scripts
    /// too, and `build_scripts_skipped` is then not counted.
    pub proc_macros: bool,
}

/// Why a project could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenError {
    /// The file does not exist, cannot be read, or is not inside `root` (or `root` cannot be used): there is nothing to load.
    NotUnderRoot,
    /// What no rung of the ladder can get past (RS3 §6): a path the engine cannot be given (not UTF-8), or the engine's own load failing after the
    /// project description was accepted. The message says which. A missing `Cargo.toml`, a manifest nobody can read, a missing `cargo`, a toolchain
    /// that is not installed: none of these is an error, each is a warning on a load that still succeeds.
    Load(String),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::NotUnderRoot => {
                write!(
                    f,
                    "the file does not exist, cannot be read, or is not inside the root: there is nothing to load"
                )
            }
            OpenError::Load(message) => write!(f, "the project could not be loaded: {message}"),
        }
    }
}

impl std::error::Error for OpenError {}

impl Project {
    /// The Cargo workspace that owns `file` (absolute, or relative to `root`), searching for its `Cargo.toml` no higher than `root` (§2): the
    /// nearest one from the file's folder upward. Cargo itself finds the workspace root, which may be above `root`: it is loaded, and it is not
    /// project code, so `files` lists only what is under `root`.
    pub fn open(root: &Path, file: &Path, opts: &LoadOptions) -> Result<Project, OpenError> {
        let root = paths::absolute(root).map_err(|_| OpenError::NotUnderRoot)?;
        let file = paths::absolute_in(&root, file);
        if !file.is_file() || paths::relative(&root, &file).is_none() {
            return Err(OpenError::NotUnderRoot);
        }
        // No `Cargo.toml` between the file and the root is not an error: the file stands alone (RS3 §6, rung 4)
        let manifest = paths::find_manifest(&root, &file);
        let name = match &manifest {
            Some(manifest) => paths::relative(&root, manifest)
                .ok_or_else(|| OpenError::Load(format!("{}: not UTF-8", manifest.display())))?,
            None => String::new(),
        };
        let loaded = sem::load(&root, manifest.as_deref(), &file, opts).map_err(OpenError::Load)?;
        let loaded_by_cargo = loaded.rung() == Rung::Cargo;
        let warnings = warning::collect(Facts {
            no_std_source: loaded.no_std_source().map(str::to_owned),
            dependencies_error: loaded.dependencies_error().map(str::to_owned),
            wrote_lockfile: loaded.wrote_lockfile(),
            build_error: loaded.build_error().map(str::to_owned),
            toolchain: loaded.toolchain_warning().cloned(),
            no_toolchain: loaded.no_toolchain(),
            cargo_failed: loaded.cargo_failed().map(str::to_owned),
            no_manifest: loaded.no_manifest(),
            manifest_unreadable: loaded.manifest_unreadable().map(str::to_owned),
        });
        Ok(Project {
            loaded,
            manifest: name,
            warnings,
            // only cargo runs a build: the reader's and the lone file's loads never did
            build_scripts_ran: (opts.build_scripts || opts.proc_macros) && loaded_by_cargo,
        })
    }

    /// The project's own `.rs` files: those the engine loaded that are under the root, relative to it with `/`, sorted.
    pub fn files(&self) -> Vec<String> {
        self.loaded.files()
    }

    /// The defs of `file` (relative to the root, as `files` lists it) that the outline names `qualname`, in source order: a qualname is not unique in a
    /// file (D15: two `impl S` blocks with one method, `#[cfg]` twins), so a tool lists the matches and never picks the first. `None` unless the file is
    /// project code: one of `files()`. Every def the outline lists is offered, including those that cannot start a graph (a trait, a module): asking
    /// `call_graph` for one says so with `None`.
    pub fn defs_named(
        &self,
        file: &str,
        qualname: &str,
        marker: MarkerStyle<'_>,
    ) -> Option<Vec<DefMatch>> {
        attached(&self.loaded, marker, |sem| sem.defs_named(file, qualname)).map(|found| {
            found
                .into_iter()
                .map(|def| DefMatch {
                    occurrence: def.occurrence,
                    kind: def.kind,
                    range: def.range,
                    marker: def.marker,
                })
                .collect()
        })
    }

    /// The `Cargo.toml` that was found for the file, relative to the root with `/`: never a path with `..`, even when cargo's workspace root is
    /// above the root.
    pub fn manifest(&self) -> &str {
        &self.manifest
    }

    /// What, if it changes, means "load this project again": globs for the manifests, locks and toolchain and cargo config files (and
    /// `**/build.rs` when build scripts ran), then the exact files that fed the load and no glob covers: the manifests of the workspace's own
    /// packages, and its root manifest and lock, when they are outside the root. An entry is relative to the root with `/` (a glob), or, for a
    /// file outside the root, the engine's absolute path with `/`.
    pub fn environment_inputs(&self) -> Vec<String> {
        let mut inputs: Vec<String> = ENVIRONMENT_GLOBS.iter().map(|g| (*g).to_owned()).collect();
        if self.build_scripts_ran {
            inputs.push(BUILD_SCRIPT_GLOB.to_owned());
        }
        inputs.extend(self.loaded.outside_inputs().iter().cloned());
        inputs
    }

    /// The engine's load, for the call graph (`calls.rs`): it builds its `Sem` inside one `attach_db` per request.
    pub(crate) fn loaded(&self) -> &Loaded {
        &self.loaded
    }

    /// The engine's load, to write to: a session follows the disk with it (`refresh`), and nothing else changes a loaded project.
    pub(crate) fn loaded_mut(&mut self) -> &mut Loaded {
        &mut self.loaded
    }

    /// What this load did not do, by name, sorted (§3): `build_scripts_skipped`, the local packages (members and path dependencies) that have a build script, when the
    /// scripts did not run; `proc_macros_skipped`, 1 when a crate of the dependency graph is a proc macro and the server is off. A switch that
    /// is on, or that cuts nothing here, is not listed. Copied into every graph's `Stats.language` from RS4 on, so a tree that lacks a call says why.
    pub fn skipped(&self) -> &[(&'static str, u64)] {
        self.loaded.skipped()
    }

    /// What this load could not do as asked, for the driver to show the user (§6, D37): sorted by code, each once. Empty for a complete,
    /// recent toolchain. Each is also meant for `Stats.language` as `warning_<code>: 1` (RS4).
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
}

/// The wire needs the text of a file to turn a call site (bytes relative to its function) into a line and a column. A project has it: the text of the file
/// **as the engine loaded it**, by its path relative to the root, not a fresh read of the disk, so the wire's offsets are the graph's.
impl Sources for Project {
    fn text(&self, file: &str) -> Option<String> {
        text_of(&self.loaded, file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_error_says_what_it_is_and_is_an_error() {
        let not_under = OpenError::NotUnderRoot.to_string();
        assert!(not_under.contains("not inside the root"), "{not_under}");
        assert_eq!(
            OpenError::Load("no Cargo.toml".into()).to_string(),
            "the project could not be loaded: no Cargo.toml"
        );
        // usable where a `Box<dyn Error>` is: the `?` of a caller that returns one
        let boxed: Box<dyn std::error::Error> = Box::new(OpenError::NotUnderRoot);
        assert_eq!(boxed.to_string(), not_under);
    }
}
