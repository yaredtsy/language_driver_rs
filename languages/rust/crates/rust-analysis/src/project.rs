use std::path::Path;

use crate::sem::{self, Loaded};

/// A loaded Cargo workspace. Slow to make (seconds); asking it things is cheap.
pub struct Project {
    loaded: Loaded,
}

/// Why a project could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenError {
    /// The engine could not load the workspace; its message. (Temporary: RS3 step 5 and 7 replace most of these with a warning on a load that
    /// still succeeds, `docs/04-rust/04-default-resolver/04-rs3-project.md` §6.)
    Load(String),
}

impl Project {
    /// Loads the workspace of the `Cargo.toml` at `manifest` (RS3 step 1: finding the manifest from a file is step 2).
    pub fn open(manifest: &Path) -> Result<Project, OpenError> {
        sem::load(manifest)
            .map(|loaded| Project { loaded })
            .map_err(OpenError::Load)
    }

    /// The project's own `.rs` files under the manifest's folder, relative to it with `/`, sorted.
    pub fn files(&self) -> Vec<String> {
        self.loaded.files()
    }
}
