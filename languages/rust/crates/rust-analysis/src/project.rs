use std::path::Path;

use crate::paths;
use crate::sem::{self, Loaded};

/// A loaded Cargo workspace. Slow to make (seconds); asking it things is cheap.
pub struct Project {
    loaded: Loaded,
    manifest: String,
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
    pub fn open(root: &Path, file: &Path) -> Result<Project, OpenError> {
        let root = paths::absolute(root)
            .map_err(|e| OpenError::Load(format!("{}: {e}", root.display())))?;
        let file = paths::absolute_in(&root, file);
        if !file.is_file() {
            return Err(OpenError::Load(format!("{}: not a file", file.display())));
        }
        if !file.starts_with(&root) {
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
        let loaded = sem::load(&root, &manifest).map_err(OpenError::Load)?;
        Ok(Project {
            loaded,
            manifest: name,
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
}
