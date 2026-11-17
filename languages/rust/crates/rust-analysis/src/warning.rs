//! What a load could not do as asked (RS3 §6, D37): data for the driver to show, never a failure. Each has a stable `code()` and a sentence.

use std::fmt;

/// Why a toolchain other than the project's was used.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Why {
    /// The project names a toolchain (`rust-toolchain.toml`) that is not installed.
    NotInstalled,
    /// The project's toolchain is older than the measured minimum.
    TooOld,
}

/// A piece the load went without. The load still succeeded; the tree it gives has holes, and this says why.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Warning {
    /// No `rust-src`: loaded without the standard library. `toolchain` is the name of the toolchain's folder (`1.99.1-aarch64-apple-darwin`).
    NoStdSource { toolchain: String },
    /// Older than the measured minimum, and nothing newer installed.
    OldToolchain { found: String, minimum: String },
    /// Read with another toolchain than the project's.
    ToolchainSubstituted {
        wanted: String,
        used: String,
        why: Why,
    },
    /// `cargo metadata` failed (offline, a dependency that cannot be fetched): loaded with `--no-deps`. `message` is cargo's.
    DependenciesMissing { message: String },
    /// Cargo wrote a `Cargo.lock` into the project, because the toolchain does not honour the redirect (§5, D38).
    LockfileWritten,
    /// The build scripts were asked for and the build run failed (a `build.rs` that panics): loaded as if they had not run, so code they generate
    /// and derives are not shown. `message` is cargo's.
    BuildScriptsFailed { message: String },
    /// No `cargo`: the crate graph was read from `Cargo.toml` by this crate (the ladder, rung 3).
    NoToolchain,
    /// Cargo and this crate's reader both failed on a `Cargo.toml` (rung 4).
    ManifestUnreadable { message: String },
    /// No `Cargo.toml` up to the root: the file alone (rung 4).
    NoManifest,
}

impl Warning {
    /// A name that does not change, for the driver and for `Stats.language` (`warning_<code>`).
    pub fn code(&self) -> &'static str {
        match self {
            Warning::NoStdSource { .. } => "no_std_source",
            Warning::OldToolchain { .. } => "old_toolchain",
            Warning::ToolchainSubstituted { .. } => "toolchain_substituted",
            Warning::DependenciesMissing { .. } => "dependencies_missing",
            Warning::LockfileWritten => "lockfile_written",
            Warning::BuildScriptsFailed { .. } => "build_scripts_failed",
            Warning::NoToolchain => "no_toolchain",
            Warning::ManifestUnreadable { .. } => "manifest_unreadable",
            Warning::NoManifest => "no_manifest",
        }
    }
}

impl fmt::Display for Warning {
    /// The sentence of RS3 §6's table, for the user.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::NoStdSource { toolchain } => write!(
                f,
                "install the Rust source (`rustup component add rust-src`) to see all calls (toolchain {toolchain})"
            ),
            Warning::OldToolchain { found, minimum } => write!(
                f,
                "Rust {found} is older than {minimum}: some calls may be missing"
            ),
            Warning::ToolchainSubstituted {
                wanted,
                used,
                why: Why::TooOld,
            } => write!(
                f,
                "your project uses Rust {wanted}; calls were read with {used}"
            ),
            Warning::ToolchainSubstituted {
                wanted,
                used,
                why: Why::NotInstalled,
            } => write!(
                f,
                "Rust {wanted} is not installed; calls were read with {used}"
            ),
            Warning::DependenciesMissing { message } => write!(
                f,
                "the dependencies could not be loaded, so calls into them and between the workspace's own crates are not shown: {message}"
            ),
            Warning::LockfileWritten => write!(f, "a Cargo.lock was created in your project"),
            Warning::BuildScriptsFailed { message } => write!(
                f,
                "the build scripts failed, so code they generate and derives are not shown: {message}"
            ),
            Warning::NoToolchain => write!(
                f,
                "Rust is not installed: only calls that do not go through the standard library are shown"
            ),
            Warning::ManifestUnreadable { message } => write!(
                f,
                "a Cargo.toml could not be read ({message}): calls into other files of the package are not shown"
            ),
            Warning::NoManifest => write!(
                f,
                "no Cargo.toml was found: calls into other files of the package that this file does not reach by `mod` are not shown"
            ),
        }
    }
}

/// What `sem` read about a load that is worth a warning. All empty (`Facts::default()`) is a complete load.
#[derive(Debug, Default)]
pub(crate) struct Facts {
    /// The toolchain (folder name) whose standard library source is missing.
    pub(crate) no_std_source: Option<String>,
    /// Cargo's message when `cargo metadata` failed and the engine went on with `--no-deps`.
    pub(crate) dependencies_error: Option<String>,
    /// A `Cargo.lock` appeared in a project that had none.
    pub(crate) wrote_lockfile: bool,
    /// The message of a build run that failed, when build scripts were asked for.
    pub(crate) build_error: Option<String>,
    /// `ToolchainSubstituted` or `OldToolchain`: the load did not use the project's toolchain as it was.
    pub(crate) toolchain: Option<Warning>,
}

/// The warnings of a load, from its facts: sorted by code (then by content), each once.
pub(crate) fn collect(facts: Facts) -> Vec<Warning> {
    let mut warnings = Vec::new();
    if let Some(toolchain) = facts.no_std_source {
        warnings.push(Warning::NoStdSource { toolchain });
    }
    if let Some(message) = facts.dependencies_error {
        warnings.push(Warning::DependenciesMissing { message });
    }
    if facts.wrote_lockfile {
        warnings.push(Warning::LockfileWritten);
    }
    if let Some(message) = facts.build_error {
        warnings.push(Warning::BuildScriptsFailed { message });
    }
    warnings.extend(facts.toolchain);
    warnings.sort_by(|a, b| a.code().cmp(b.code()).then_with(|| a.cmp(b)));
    warnings.dedup();
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_warning_has_its_own_code_and_a_sentence() {
        let all = [
            Warning::NoStdSource {
                toolchain: "1.99.1".into(),
            },
            Warning::OldToolchain {
                found: "1.74".into(),
                minimum: "1.82".into(),
            },
            Warning::ToolchainSubstituted {
                wanted: "1.74".into(),
                used: "1.99".into(),
                why: Why::TooOld,
            },
            Warning::ToolchainSubstituted {
                wanted: "1.50".into(),
                used: "1.99".into(),
                why: Why::NotInstalled,
            },
            Warning::DependenciesMissing {
                message: "offline".into(),
            },
            Warning::LockfileWritten,
            Warning::BuildScriptsFailed {
                message: "boom".into(),
            },
            Warning::NoToolchain,
            Warning::ManifestUnreadable {
                message: "bad".into(),
            },
            Warning::NoManifest,
        ];
        let mut codes: Vec<_> = all.iter().map(Warning::code).collect();
        codes.dedup();
        // the two substitutions share a code: nine codes for ten warnings
        assert_eq!(codes.len(), 9);
        assert!(all.iter().all(|w| !w.to_string().is_empty()));
        assert_eq!(
            all[2].to_string(),
            "your project uses Rust 1.74; calls were read with 1.99"
        );
        assert_eq!(
            all[3].to_string(),
            "Rust 1.50 is not installed; calls were read with 1.99"
        );
        assert_eq!(
            Warning::LockfileWritten.to_string(),
            "a Cargo.lock was created in your project"
        );
        assert_eq!(
            Warning::BuildScriptsFailed {
                message: "boom".into()
            }
            .to_string(),
            "the build scripts failed, so code they generate and derives are not shown: boom"
        );
    }

    #[test]
    fn warnings_are_sorted_by_code_each_once() {
        let got = collect(Facts {
            no_std_source: Some("t".into()),
            dependencies_error: Some("m".into()),
            wrote_lockfile: true,
            build_error: Some("b".into()),
            toolchain: None,
        });
        let codes: Vec<_> = got.iter().map(Warning::code).collect();
        assert_eq!(
            codes,
            [
                "build_scripts_failed",
                "dependencies_missing",
                "lockfile_written",
                "no_std_source"
            ]
        );
        assert_eq!(collect(Facts::default()), []);
    }
}
