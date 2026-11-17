//! The toolchain a project's folder selects, as `cargo --version` says. No engine type here: only what a process prints.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::sem;
use crate::warning::{Warning, Why};

/// A cargo version. `release` is false for a pre-release (`1.98.0-nightly`, `1.97.0-beta.1`), which is older than the release it leads up to,
/// as in semver: the field order makes the derived `Ord` say so.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Version {
    major: u32,
    minor: u32,
    patch: u32,
    release: bool,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if self.release {
            Ok(())
        } else {
            write!(f, "-pre")
        }
    }
}

/// The oldest toolchain whose loads are trusted as they are (RS3 §6). 1.82 is the first cargo that copies the lock for `cargo metadata`, so
/// below it a load can rewrite the user's stale `Cargo.lock`. The measurement of 1.74.1, 1.94.1, 1.98.0 and 1.99.1 found nothing else lost in
/// the project's own calls: `languages/rust/ENGINE.md`, "Old toolchains", and RS3 §6.
pub(crate) const MINIMUM: Version = Version::release(1, 82, 0);

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

/// What `cargo --version` says in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Probe {
    Version(Version),
    /// rustup says the toolchain the folder selects (a `rust-toolchain.toml`) is not installed: its name, as rustup spells it.
    NotInstalled(String),
    /// No `cargo`, or it said something else.
    Unreadable,
}

/// A tool of the toolchain this crate starts. It is found the way the engine finds it (`sem::tool_path`), never by the bare name: a `PATH` that lacks
/// `~/.cargo/bin` must not give a load with the engine's `cargo` and no `std`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    Cargo,
    Rustc,
    Rustup,
}

impl Tool {
    /// The program to start.
    pub(crate) fn path(self) -> PathBuf {
        sem::tool_path(self)
    }
}

/// A `Command` for a tool of the toolchain, in `dir`. `RUSTUP_AUTO_INSTALL=0`: a toolchain that is named and not installed must fail, not be
/// downloaded. `toolchain` is the one this crate chose in place of the project's (`decide`): `RUSTUP_TOOLCHAIN` beats every file and override.
pub(crate) fn command(tool: Tool, dir: &Path, toolchain: Option<&str>) -> Command {
    let mut command = Command::new(tool.path());
    command.current_dir(dir).env("RUSTUP_AUTO_INSTALL", "0");
    if let Some(toolchain) = toolchain {
        command.env("RUSTUP_TOOLCHAIN", toolchain);
    }
    command
}

/// `cargo --version` in `dir` (so a `rust-toolchain.toml` there counts), with the toolchain this crate chose, if it chose one.
pub(crate) fn probe(dir: &Path, toolchain: Option<&str>) -> Probe {
    let Ok(out) = command(Tool::Cargo, dir, toolchain)
        .arg("--version")
        .output()
    else {
        return Probe::Unreadable;
    };
    if out.status.success() {
        let text = String::from_utf8_lossy(&out.stdout);
        return Version::parse(&text).map_or(Probe::Unreadable, Probe::Version);
    }
    match not_installed_toolchain(&String::from_utf8_lossy(&out.stderr)) {
        Some(name) => Probe::NotInstalled(name),
        None => Probe::Unreadable,
    }
}

/// Can `cargo` be started at all in `dir`? Not whether it works: a `cargo` that fails on a manifest is present.
pub(crate) fn cargo_present(dir: &Path, toolchain: Option<&str>) -> bool {
    command(Tool::Cargo, dir, toolchain)
        .arg("--version")
        .output()
        .is_ok()
}

/// The version of the `cargo` that runs in `dir`; `None` when there is none, or it is not installed, or it prints something else.
pub(crate) fn cargo_version(dir: &Path, toolchain: Option<&str>) -> Option<Version> {
    match probe(dir, toolchain) {
        Probe::Version(version) => Some(version),
        Probe::NotInstalled(_) | Probe::Unreadable => None,
    }
}

/// The toolchain in rustup's "not installed" error: `error: toolchain '1.50.0-aarch64-apple-darwin' is not installed`.
fn not_installed_toolchain(stderr: &str) -> Option<String> {
    let after = stderr.split_once("toolchain '")?.1;
    let (name, rest) = after.split_once('\'')?;
    rest.trim_start()
        .starts_with("is not installed")
        .then(|| name.to_owned())
}

/// A toolchain name without its host triple (`1.50.0-aarch64-apple-darwin` is Rust `1.50.0`), for a sentence. A name that has none is returned as it is.
fn without_host(name: &str) -> &str {
    const ARCHES: [&str; 8] = [
        "aarch64",
        "x86_64",
        "i686",
        "i586",
        "armv7",
        "powerpc64le",
        "riscv64gc",
        "s390x",
    ];
    for arch in ARCHES {
        if let Some(at) = name.find(&format!("-{arch}-")) {
            return &name[..at];
        }
    }
    name
}

/// One line of `rustup toolchain list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) name: String,
    pub(crate) default: bool,
}

/// `stable-aarch64-apple-darwin (default)`, `1.74.1-aarch64-apple-darwin`, `nightly-x (active)`, `stable-x (active, default)`: the name is the first
/// word, and `default` among the words in the parentheses marks the toolchain used when nothing selects one.
fn parse_toolchain_list(text: &str) -> Vec<Listed> {
    text.lines()
        .filter_map(|line| {
            let name = line.split_whitespace().next()?;
            let flags = line.split_once('(').map_or("", |(_, flags)| flags);
            let default = flags
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| word == "default");
            Some(Listed {
                name: name.to_owned(),
                default,
            })
        })
        .collect()
}

/// The toolchains `rustup toolchain list` knows, each with its version; `None` when there is no `rustup` (nothing can then be substituted, and
/// this crate never installs anything). The version is read with that toolchain's own `cargo`.
pub(crate) fn installed(dir: &Path) -> Option<Vec<Candidate>> {
    let out = Command::new(Tool::Rustup.path())
        .args(["toolchain", "list"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let listed = parse_toolchain_list(&String::from_utf8_lossy(&out.stdout));
    Some(
        listed
            .into_iter()
            .map(|Listed { name, default }| Candidate {
                version: cargo_version(dir, Some(&name)),
                name,
                default,
            })
            .collect(),
    )
}

/// An installed toolchain, as far as a choice needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub(crate) name: String,
    pub(crate) default: bool,
    pub(crate) version: Option<Version>,
}

/// What the load does about the toolchain of the project (RS3 §6).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Decision {
    /// The toolchain to put in `RUSTUP_TOOLCHAIN` for every tool of the load, when this crate chose another than the project's.
    pub(crate) toolchain: Option<String>,
    pub(crate) warning: Option<Warning>,
}

/// The rows of §6 that are about the toolchain, as a choice (pure: `installed` is read only when a row needs it).
///
/// - the project's toolchain is new enough, or unreadable: no change;
/// - older than `minimum`: the newest installed toolchain that is new enough replaces it (`ToolchainSubstituted { TooOld }`), and when there is
///   none, or no `rustup`, it loads as it is (`OldToolchain`);
/// - not installed: the default replaces it (`ToolchainSubstituted { NotInstalled }`); with no `rustup` or no default there is nothing to
///   replace it with, and the load fails as it always did.
pub(crate) fn decide(
    probe: Probe,
    minimum: Version,
    installed: impl FnOnce() -> Option<Vec<Candidate>>,
) -> Decision {
    match probe {
        Probe::Unreadable => Decision::default(),
        Probe::Version(found) if found >= minimum => Decision::default(),
        Probe::Version(found) => {
            let newest = installed().and_then(|list| {
                list.into_iter()
                    .filter(|c| c.version.is_some_and(|v| v >= minimum))
                    // the highest version; among equals the smaller name, so the answer does not depend on the order of rustup's list
                    .max_by(|a, b| a.version.cmp(&b.version).then_with(|| b.name.cmp(&a.name)))
            });
            match newest {
                Some(Candidate {
                    name,
                    version: Some(used),
                    ..
                }) => Decision {
                    warning: Some(Warning::ToolchainSubstituted {
                        wanted: found.to_string(),
                        used: used.to_string(),
                        why: Why::TooOld,
                    }),
                    toolchain: Some(name),
                },
                _ => Decision {
                    toolchain: None,
                    warning: Some(Warning::OldToolchain {
                        found: found.to_string(),
                        minimum: minimum.to_string(),
                    }),
                },
            }
        }
        Probe::NotInstalled(wanted) => {
            let default = installed()
                .and_then(|list| list.into_iter().find(|c| c.default && c.version.is_some()));
            match default {
                Some(Candidate {
                    name,
                    version: Some(used),
                    ..
                }) => Decision {
                    warning: Some(Warning::ToolchainSubstituted {
                        wanted: without_host(&wanted).to_owned(),
                        used: used.to_string(),
                        why: Why::NotInstalled,
                    }),
                    toolchain: Some(name),
                },
                _ => Decision::default(),
            }
        }
    }
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

    fn v(minor: u32, patch: u32) -> Version {
        Version::release(1, minor, patch)
    }

    fn candidate(name: &str, default: bool, version: Option<Version>) -> Candidate {
        Candidate {
            name: name.to_owned(),
            default,
            version,
        }
    }

    /// An installed set like this machine's: old ones, the one the project wants, and a default.
    fn installed() -> Option<Vec<Candidate>> {
        Some(vec![
            candidate("stable-x", true, Some(v(99, 1))),
            candidate("1.74.1-x", false, Some(v(74, 1))),
            candidate("1.94.1-x", false, Some(v(94, 1))),
            candidate("1.99.1-x", false, Some(v(99, 1))),
            candidate("broken-x", false, None),
        ])
    }

    fn substituted(wanted: &str, used: &str, why: Why) -> Option<Warning> {
        Some(Warning::ToolchainSubstituted {
            wanted: wanted.into(),
            used: used.into(),
            why,
        })
    }

    #[test]
    fn the_choice_of_a_toolchain_row_by_row() {
        let min = v(82, 0);
        // new enough, exactly the minimum, or unreadable: the project's own toolchain, nothing to say
        assert_eq!(
            decide(Probe::Version(v(99, 1)), min, installed),
            Decision::default()
        );
        assert_eq!(
            decide(Probe::Version(v(82, 0)), min, installed),
            Decision::default()
        );
        assert_eq!(
            decide(Probe::Unreadable, min, installed),
            Decision::default()
        );
        // the list is not even read when the toolchain is fine
        assert_eq!(
            decide(Probe::Version(v(99, 1)), min, || unreachable!(
                "no need to read rustup"
            )),
            Decision::default()
        );

        // too old and a newer one installed: the newest that is new enough; of equal versions the smaller name, whatever the list's order
        let want = Decision {
            toolchain: Some("1.99.1-x".into()),
            warning: substituted("1.74.1", "1.99.1", Why::TooOld),
        };
        assert_eq!(decide(Probe::Version(v(74, 1)), min, installed), want);
        let reversed = || {
            installed().map(|mut list| {
                list.reverse();
                list
            })
        };
        assert_eq!(decide(Probe::Version(v(74, 1)), min, reversed), want);
        // 1.81.9 is below 1.82.0
        let just_below = Decision {
            warning: substituted("1.81.9", "1.99.1", Why::TooOld),
            ..want
        };
        assert_eq!(decide(Probe::Version(v(81, 9)), min, installed), just_below);

        // too old and nothing new enough installed, or no rustup at all: it loads as it is, and says so
        let old = Decision {
            toolchain: None,
            warning: Some(Warning::OldToolchain {
                found: "1.74.1".into(),
                minimum: "1.82.0".into(),
            }),
        };
        let only_old = || {
            Some(vec![
                candidate("1.74.1-x", true, Some(v(74, 1))),
                candidate("1.80.0-x", false, Some(v(80, 0))),
            ])
        };
        assert_eq!(decide(Probe::Version(v(74, 1)), min, only_old), old);
        assert_eq!(decide(Probe::Version(v(74, 1)), min, || None), old);
        // a candidate whose version could not be read never counts
        let unreadable = || Some(vec![candidate("odd-x", true, None)]);
        assert_eq!(decide(Probe::Version(v(74, 1)), min, unreadable), old);

        // named and not installed: the default replaces it, with the name as the user wrote it (no host triple)
        let missing = Probe::NotInstalled("1.50.0-aarch64-apple-darwin".into());
        let by_default = Decision {
            toolchain: Some("stable-x".into()),
            warning: substituted("1.50.0", "1.99.1", Why::NotInstalled),
        };
        assert_eq!(decide(missing.clone(), min, installed), by_default);
        // no rustup, no default, or a default nobody can read: nothing to replace it with
        assert_eq!(decide(missing.clone(), min, || None), Decision::default());
        let no_default = || Some(vec![candidate("1.99.1-x", false, Some(v(99, 1)))]);
        assert_eq!(
            decide(missing.clone(), min, no_default),
            Decision::default()
        );
        assert_eq!(decide(missing, min, unreadable), Decision::default());
    }

    #[test]
    fn what_rustup_prints() {
        let error = "error: toolchain '1.50.0-aarch64-apple-darwin' is not installed\nhelp: run `rustup toolchain install` to install it\n";
        assert_eq!(
            not_installed_toolchain(error).as_deref(),
            Some("1.50.0-aarch64-apple-darwin")
        );
        assert_eq!(
            not_installed_toolchain("error: no such command: `foo`"),
            None
        );
        assert_eq!(not_installed_toolchain("toolchain 'x' is broken"), None);

        assert_eq!(without_host("1.50.0-aarch64-apple-darwin"), "1.50.0");
        assert_eq!(
            without_host("nightly-2026-01-01-x86_64-unknown-linux-gnu"),
            "nightly-2026-01-01"
        );
        assert_eq!(without_host("my-linked"), "my-linked");

        let list = "stable-aarch64-apple-darwin (default)\n1.74.1-aarch64-apple-darwin\n1.99.1-aarch64-apple-darwin (active)\n\nnosrc\n";
        // the default that is also the active one is printed with both words
        let both = parse_toolchain_list("stable-x (active, default)\n1.74.1-x (override)\n");
        assert_eq!((both[0].default, both[1].default), (true, false));
        let got = parse_toolchain_list(list);
        let names: Vec<_> = got.iter().map(|l| (l.name.as_str(), l.default)).collect();
        assert_eq!(
            names,
            [
                ("stable-aarch64-apple-darwin", true),
                ("1.74.1-aarch64-apple-darwin", false),
                ("1.99.1-aarch64-apple-darwin", false),
                ("nosrc", false)
            ]
        );
        assert_eq!(parse_toolchain_list(""), []);
    }

    #[test]
    fn a_version_reads_back_as_text() {
        assert_eq!(v(74, 1).to_string(), "1.74.1");
        assert_eq!(
            Version::parse("cargo 1.98.0-nightly (x)")
                .unwrap()
                .to_string(),
            "1.98.0-pre"
        );
    }
}
