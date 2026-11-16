//! The toolchain a project's folder selects, as `cargo --version` says. No engine type here: only what a process prints.

use std::path::Path;
use std::process::Command;

/// A cargo version. `release` is false for a pre-release (`1.98.0-nightly`, `1.97.0-beta.1`), which is older than the release it leads up to,
/// as in semver: the field order makes the derived `Ord` say so.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Version {
    major: u32,
    minor: u32,
    patch: u32,
    release: bool,
}

impl Version {
    pub(crate) const fn release(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
            release: true,
        }
    }

    /// The version in the output of `cargo --version`: `cargo 1.99.1 (5f94df478 2026-08-27)`, `cargo 1.98.0-nightly (…)`. `None` for anything else.
    pub(crate) fn parse(output: &str) -> Option<Version> {
        let mut words = output.split_whitespace();
        if words.next()? != "cargo" {
            return None;
        }
        let word = words.next()?;
        let word = word
            .split_once('+')
            .map_or(word, |(version, _build)| version);
        let (numbers, release) = match word.split_once('-') {
            Some((numbers, _suffix)) => (numbers, false),
            None => (word, true),
        };
        let mut parts = numbers.split('.').map(|part| part.parse::<u32>().ok());
        let version = Version {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next()??,
            release,
        };
        parts.next().is_none().then_some(version)
    }
}

/// The version of the `cargo` that runs in `dir` (so a `rust-toolchain.toml` there counts). `RUSTUP_AUTO_INSTALL=0`: a toolchain that is named and
/// not installed gives `None`, it is not downloaded. `None` too when there is no `cargo` or it prints something else.
pub(crate) fn cargo_version(dir: &Path) -> Option<Version> {
    let out = Command::new("cargo")
        .arg("--version")
        .current_dir(dir)
        .env("RUSTUP_AUTO_INSTALL", "0")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Version::parse(std::str::from_utf8(&out.stdout).ok()?)
}

/// Does this toolchain write the lock where `CARGO_RESOLVER_LOCKFILE_PATH` says (RS3 §5, D38: the variable alone from 1.97; 1.98.0 observed to
/// honour it, 1.94.1 to ignore it)? An unreadable version says no: the load then goes on as it is and cargo may write the lock into the project.
pub(crate) fn honours_lockfile_path(version: Option<Version>) -> bool {
    const FROM: Version = Version::release(1, 97, 0);
    version.is_some_and(|version| version >= FROM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_in_cargos_output() {
        let v = Version::parse;
        assert_eq!(
            v("cargo 1.99.1 (5f94df478 2026-08-27)\n"),
            Some(Version::release(1, 99, 1))
        );
        assert_eq!(
            v("cargo 1.74.1 (ecb9851af 2024-02-16)"),
            Some(Version::release(1, 74, 1))
        );
        // a suffix makes it a pre-release of that number; build metadata does not
        assert_eq!(
            v("cargo 1.98.0-nightly (abc123 2026-10-01)"),
            Some(Version {
                release: false,
                ..Version::release(1, 98, 0)
            })
        );
        assert_eq!(
            v("cargo 1.97.0-beta.1 (abc123 2026-10-01)"),
            Some(Version {
                release: false,
                ..Version::release(1, 97, 0)
            })
        );
        assert_eq!(
            v("cargo 1.97.0+local (abc123 2026-10-01)"),
            Some(Version::release(1, 97, 0))
        );
        for unreadable in [
            "",
            "cargo",
            "cargo (abc)",
            "cargo 1.97",
            "cargo 1.97.x",
            "cargo 1.97.0.1",
            "rustc 1.99.1",
            "1.99.1",
            "error: no such command",
        ] {
            assert_eq!(v(unreadable), None, "{unreadable:?}");
        }
    }

    #[test]
    fn the_lockfile_rule() {
        let rule = |output: &str| honours_lockfile_path(Version::parse(output));
        // A (cargo may write the lock into the project): below 1.97, and a version nobody can read
        assert!(!rule("cargo 1.96.0 (x 2026-01-01)"));
        assert!(!rule("cargo 1.96.9 (x 2026-01-01)"));
        assert!(!rule("cargo 1.74.1 (x 2024-02-16)"));
        assert!(
            !rule("cargo 1.97.0-nightly (x 2026-01-01)"),
            "a pre-release of 1.97.0 is not 1.97.0"
        );
        assert!(!rule("something else"));
        assert!(!honours_lockfile_path(None));
        // B (redirect): 1.97.0 and above, a suffix on a later number included
        assert!(rule("cargo 1.97.0 (x 2026-01-01)"));
        assert!(rule("cargo 1.99.1 (x 2026-08-27)"));
        assert!(rule("cargo 1.98.0-nightly (x 2026-01-01)"));
        assert!(rule("cargo 2.0.0 (x 2030-01-01)"));
    }
}
