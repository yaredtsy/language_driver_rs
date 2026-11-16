//! A load must not write into the project (RS3 §5, D38). The fixture `fixtures/lock/` is an `app` with a path dependency `helper`, and neither has a
//! `Cargo.lock`: none is committed, because the case is "cargo has to make one". Each test loads a copy under `target/`, so a load that does write
//! one leaves nothing in the repo, and the copy sits inside the repo's folder tree, so `rust-toolchain.toml` still selects the toolchain.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use rust_analysis::Project;

/// Every file under `dir` with its text (lossy: all of them are text, and a failed comparison is then readable), by path relative to `dir` with `/`.
fn snapshot(dir: &Path) -> BTreeMap<String, String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let parts: Vec<_> = path
                    .strip_prefix(base)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_str().unwrap())
                    .collect();
                out.insert(
                    parts.join("/"),
                    String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A fresh copy of the fixture, named after the test.
fn copy_of_fixture(name: &str) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lock");
    let to = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&to);
    copy_dir(&from, &to);
    to
}

/// The tests are about the redirect, which is for toolchains from 1.97: say so when the toolchain here is older, instead of passing for nothing.
fn assert_toolchain_honours_the_redirect(dir: &Path) {
    let out = Command::new("cargo")
        .arg("--version")
        .current_dir(dir)
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let minor: u32 = text
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.split('.').nth(1))
        .and_then(|m| m.parse().ok())
        .unwrap();
    assert!(
        minor >= 97,
        "this test needs cargo 1.97 or newer (rust-toolchain.toml gives 1.99.1), got {text}"
    );
}

/// No folder of this process's redirects is left in the temporary folder.
fn assert_no_redirect_left() {
    let prefix = format!("rust-analysis-lock-{}-", std::process::id());
    let left: Vec<_> = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|e| e.unwrap().file_name().into_string().ok())
        .filter(|name| name.starts_with(&prefix))
        .collect();
    assert!(left.is_empty(), "a load left {left:?} behind");
}

#[test]
fn a_project_with_no_lock_gets_none() {
    let copy = copy_of_fixture("lock_none");
    assert_toolchain_honours_the_redirect(&copy.join("app"));
    let before = snapshot(&copy);
    assert!(
        before.keys().all(|path| !path.ends_with("Cargo.lock")),
        "the fixture must start with no lock: {:?}",
        before.keys()
    );

    let project = Project::open(&copy.join("app/Cargo.toml")).expect("app loads");
    assert_eq!(project.files(), ["src/lib.rs"]);

    assert_eq!(
        snapshot(&copy),
        before,
        "the load changed the project's folder"
    );
    assert_no_redirect_left();
}

#[test]
fn a_stale_lock_is_left_as_it_is() {
    let copy = copy_of_fixture("lock_stale");
    let app = copy.join("app");
    assert_toolchain_honours_the_redirect(&app);

    // Make the lock with cargo, from a manifest that does not have the dependency yet, then put the dependency back: the lock is behind the manifest.
    let manifest = std::fs::read_to_string(app.join("Cargo.toml")).unwrap();
    let without_dependency: String = manifest
        .lines()
        .filter(|line| !line.starts_with("helper"))
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(app.join("Cargo.toml"), without_dependency).unwrap();
    let made = Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&app)
        .status()
        .unwrap();
    assert!(made.success());
    std::fs::write(app.join("Cargo.toml"), manifest).unwrap();
    let lock = std::fs::read_to_string(app.join("Cargo.lock")).unwrap();
    assert!(!lock.contains("helper"), "the lock must be stale: {lock}");

    let before = snapshot(&copy);
    let project = Project::open(&app.join("Cargo.toml")).expect("app loads");
    assert_eq!(project.files(), ["src/lib.rs"]);

    assert_eq!(
        snapshot(&copy),
        before,
        "the load changed the project's folder"
    );
    assert_no_redirect_left();
}
