use std::path::Path;

use crate::paths;
use crate::sem::{self, Loaded};

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
    /// Build scripts ran (they were asked for, or proc macros were: those are built by the same run).
    build_scripts_ran: bool,
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
    /// The file is not there or not inside the root, no `Cargo.toml` lies between it and the root, or the engine could not load the workspace;
    /// the message says which. (Temporary: RS3 steps 5 and 7 turn most of these into a warning on a load that still succeeds, and keep one
    /// variant for a file that is not there, `docs/04-rust/04-default-resolver/04-rs3-project.md` §6.)
    Load(String),
}

impl Project {
    /// The Cargo workspace that owns `file` (absolute, or relative to `root`), searching for its `Cargo.toml` no higher than `root` (§2): the
    /// nearest one from the file's folder upward. Cargo itself finds the workspace root, which may be above `root`: it is loaded, and it is not
    /// project code, so `files` lists only what is under `root`.
    pub fn open(root: &Path, file: &Path, opts: &LoadOptions) -> Result<Project, OpenError> {
        let root = paths::absolute(root)
            .map_err(|e| OpenError::Load(format!("{}: {e}", root.display())))?;
        let file = paths::absolute_in(&root, file);
        if !file.is_file() {
            return Err(OpenError::Load(format!("{}: not a file", file.display())));
        }
        if paths::relative(&root, &file).is_none() {
            return Err(OpenError::Load(format!(
                "{}: not inside the root {}",
                file.display(),
                root.display()
            )));
        }
        let manifest = paths::find_manifest(&root, &file).ok_or_else(|| {
            OpenError::Load(format!(
                "no Cargo.toml between {} and the root {}",
                file.display(),
                root.display()
            ))
        })?;
        let name = paths::relative(&root, &manifest)
            .ok_or_else(|| OpenError::Load(format!("{}: not UTF-8", manifest.display())))?;
        let loaded = sem::load(&root, &manifest, opts).map_err(OpenError::Load)?;
        Ok(Project {
            loaded,
            manifest: name,
            build_scripts_ran: opts.build_scripts || opts.proc_macros,
        })
    }

    /// The project's own `.rs` files: those the engine loaded that are under the root, relative to it with `/`, sorted.
    pub fn files(&self) -> Vec<String> {
        self.loaded.files()
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

    /// What this load did not do, by name, sorted (§3): `build_scripts_skipped`, the workspace's packages that have a build script, when the
    /// scripts did not run; `proc_macros_skipped`, 1 when a crate of the dependency graph is a proc macro and the server is off. A switch that
    /// is on, or that cuts nothing here, is not listed. Copied into every graph's `Stats.language` from RS4 on, so a tree that lacks a call says why.
    pub fn skipped(&self) -> &[(&'static str, u64)] {
        self.loaded.skipped()
    }
}
