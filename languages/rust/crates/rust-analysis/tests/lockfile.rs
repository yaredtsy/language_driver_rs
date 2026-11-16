//! A load must not write into the project (RS3 §5, D38). The fixture `fixtures/lock/` is an `app` with a path dependency `helper`, and neither has a
//! `Cargo.lock`: none is committed, because the case is "cargo has to make one". Each test loads a copy under `target/`, so a load that does write
//! one leaves nothing in the repo, and the copy sits inside the repo's folder tree, so `rust-toolchain.toml` still selects the toolchain.

use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use rust_analysis::{LoadOptions, Project};

mod common;
use common::{copy_of_fixture, snapshot};

/// The two tests run in one process, so they share a process id and a temporary folder: while one has its redirect, the other must not look for
/// leftovers. Each holds this for its whole length. (A test that panics poisons the mutex; the next one takes it anyway.)
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn one_at_a_time() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
    let _one = one_at_a_time();
    let copy = copy_of_fixture("lock", "lock_none");
    assert_toolchain_honours_the_redirect(&copy.join("app"));
    let before = snapshot(&copy);
    assert!(
        before.keys().all(|path| !path.ends_with("Cargo.lock")),
        "the fixture must start with no lock: {:?}",
        before.keys()
    );

    let project = Project::open(
        &copy.join("app"),
        &copy.join("app/src/lib.rs"),
        &LoadOptions::default(),
    )
    .expect("app loads");
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
    let _one = one_at_a_time();
    let copy = copy_of_fixture("lock", "lock_stale");
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
    let project =
        Project::open(&app, &app.join("src/lib.rs"), &LoadOptions::default()).expect("app loads");
    assert_eq!(project.files(), ["src/lib.rs"]);

    assert_eq!(
        snapshot(&copy),
        before,
        "the load changed the project's folder"
    );
    assert_no_redirect_left();
}
