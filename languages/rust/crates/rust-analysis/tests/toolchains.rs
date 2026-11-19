//! The rows of RS3 §6 that depend on the toolchain. Most run by hand (`--ignored`), each with the name of an installed toolchain:
//!
//! ```text
//! RS_TOOLCHAIN_WITHOUT_SRC=nosrc  RS_TOOLCHAIN_BEFORE_1_97=1.94.1  RS_TOOLCHAIN_OLD=1.74.1  cargo test -p rust-analysis --test toolchains -- --ignored
//! ```
//!
//! The one that runs everywhere is the toolchain that is not installed: it needs `rustup` and nothing else.
//!
//! `cargo test` itself sets `RUSTUP_TOOLCHAIN`, which beats a `rust-toolchain.toml`, and this crate may not call `set_var` (`unsafe_code` is
//! forbidden). So the test starts a second copy of this program, a *worker*, with the toolchain in the child's environment only; the worker
//! is the test that loads. Run by itself a worker does nothing (it needs `RS_WORKER`).

use std::path::Path;
use std::process::Command;

use rust_analysis::{LoadOptions, Project, Warning, Why};

mod common;
use common::{copy_of_fixture, one_load};

/// Runs the worker `name` of this program with `RUSTUP_TOOLCHAIN=toolchain` (or with none: `cargo test` sets it, and it beats a
/// `rust-toolchain.toml`), and fails with its output when it fails. `CARGO` and `RUSTC` stay as `cargo test` sets them (the toolchain's own binaries, not
/// rustup's proxy): the engine's rule for a load with a sysroot, `prefer_proxy`, finds the proxy first, so a pin is still read. A worker that passes
/// with them set is what proves `sem::tool_path` does not use `Tool::path`.
fn run_worker(name: &str, toolchain: Option<&str>) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", name, "--nocapture"])
        .env("RS_WORKER", "1");
    match toolchain {
        Some(toolchain) => command.env("RUSTUP_TOOLCHAIN", toolchain),
        None => command.env_remove("RUSTUP_TOOLCHAIN"),
    };
    let out = command.output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success() && text.contains("1 passed"),
        "the worker failed:\n{text}"
    );
}

fn is_worker() -> bool {
    std::env::var_os("RS_WORKER").is_some()
}

fn open(root: &Path) -> Project {
    Project::open(root, &root.join("src/lib.rs"), &LoadOptions::default())
        .expect("the fixture loads")
}

#[test]
#[ignore = "by hand: RS_TOOLCHAIN_WITHOUT_SRC=<an installed toolchain that has no rust-src>"]
fn a_toolchain_without_rust_src_loads_with_a_warning() {
    let toolchain =
        std::env::var("RS_TOOLCHAIN_WITHOUT_SRC").expect("set RS_TOOLCHAIN_WITHOUT_SRC");
    run_worker("worker_without_rust_src", Some(&toolchain));
}

#[test]
#[ignore = "run by a_toolchain_without_rust_src_loads_with_a_warning"]
fn worker_without_rust_src() {
    let _load = one_load();
    if !is_worker() {
        return;
    }
    let copy = copy_of_fixture("lock", "toolchains_nosrc");
    let app = copy.join("app");
    let project = open(&app);
    println!("warnings: {:?}", project.warnings());
    println!("{}", project.warnings()[0]);
    let [Warning::NoStdSource { toolchain }] = project.warnings() else {
        panic!("{:?}", project.warnings());
    };
    assert!(!toolchain.is_empty());
}

#[test]
#[ignore = "by hand: RS_TOOLCHAIN_BEFORE_1_97=<an installed toolchain older than 1.97, newer than 1.82, such as 1.94.1>"]
fn a_toolchain_that_ignores_the_redirect_writes_a_lock_and_says_so() {
    let toolchain =
        std::env::var("RS_TOOLCHAIN_BEFORE_1_97").expect("set RS_TOOLCHAIN_BEFORE_1_97");
    run_worker("worker_lockfile_written", Some(&toolchain));
}

#[test]
#[ignore = "run by a_toolchain_that_ignores_the_redirect_writes_a_lock_and_says_so"]
fn worker_lockfile_written() {
    let _load = one_load();
    if !is_worker() {
        return;
    }
    let copy = copy_of_fixture("lock", "toolchains_lock");
    let app = copy.join("app");
    assert!(!app.join("Cargo.lock").exists());
    let project = open(&app);
    println!("warnings: {:?}", project.warnings());
    assert!(
        app.join("Cargo.lock").exists(),
        "this toolchain was expected to write the lock"
    );
    assert!(
        project.warnings().contains(&Warning::LockfileWritten),
        "{:?}",
        project.warnings()
    );
}

/// `rustup toolchain list`, or `None` without rustup. From a neutral folder and with `RUSTUP_AUTO_INSTALL=0`, as the crate runs it: this is the test that
/// says "nothing was installed", and it must not install anything itself.
fn rustup_list() -> Option<String> {
    let out = Command::new("rustup")
        .args(["toolchain", "list"])
        .current_dir(std::env::temp_dir())
        .env("RUSTUP_AUTO_INSTALL", "0")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn a_toolchain_that_is_not_installed_is_replaced_by_the_default_and_nothing_is_installed() {
    let Some(before) = rustup_list() else {
        eprintln!("no rustup here: there is nothing to substitute, and nothing to test");
        return;
    };
    run_worker("worker_not_installed", None);
    assert_eq!(
        rustup_list().unwrap(),
        before,
        "the load installed a toolchain"
    );
}

#[test]
#[ignore = "run by a_toolchain_that_is_not_installed_is_replaced_by_the_default_and_nothing_is_installed"]
fn worker_not_installed() {
    let _load = one_load();
    if !is_worker() {
        return;
    }
    let copy = copy_of_fixture("lock", "toolchains_missing");
    let app = copy.join("app");
    // the project asks for a toolchain nobody has: rustup answers "toolchain '1.50.0-<host>' is not installed"
    std::fs::write(
        app.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.50.0\"\n",
    )
    .unwrap();
    let project = open(&app);
    println!("warnings: {:?}", project.warnings());
    println!("{}", project.warnings()[0]);

    let substituted: Vec<_> = project
        .warnings()
        .iter()
        .filter(|w| w.code() == "toolchain_substituted")
        .collect();
    let [Warning::ToolchainSubstituted { wanted, used, why }] = substituted[..] else {
        panic!("{:?}", project.warnings());
    };
    assert_eq!((wanted.as_str(), why), ("1.50.0", &Why::NotInstalled));
    assert!(used.starts_with("1."), "{used}");
    assert!(project.files().iter().any(|f| f == "src/lib.rs"));
    // the default's cargo did the reading: if it is older than 1.97 it ignores the lock redirect, and then the lock is written and said (§5); if it is
    // newer there is no lock. Either way the file and the warning agree.
    assert_eq!(
        app.join("Cargo.lock").exists(),
        project.warnings().contains(&Warning::LockfileWritten),
        "{:?}",
        project.warnings()
    );
}

#[test]
#[ignore = "by hand: RS_TOOLCHAIN_OLD=<an installed toolchain older than 1.82, such as 1.74.1, with a newer one installed>"]
fn a_toolchain_below_the_minimum_is_replaced_by_a_newer_one() {
    let toolchain = std::env::var("RS_TOOLCHAIN_OLD").expect("set RS_TOOLCHAIN_OLD");
    run_worker("worker_too_old", Some(&toolchain));
}

#[test]
#[ignore = "run by a_toolchain_below_the_minimum_is_replaced_by_a_newer_one"]
fn worker_too_old() {
    let _load = one_load();
    if !is_worker() {
        return;
    }
    let copy = copy_of_fixture("lock", "toolchains_old");
    let app = copy.join("app");
    let project = open(&app);
    println!("warnings: {:?}", project.warnings());
    let substituted: Vec<_> = project
        .warnings()
        .iter()
        .filter(|w| w.code() == "toolchain_substituted")
        .collect();
    let [Warning::ToolchainSubstituted { wanted, used, why }] = substituted[..] else {
        panic!("{:?}", project.warnings());
    };
    assert_eq!(*why, Why::TooOld);
    println!("{wanted} -> {used}");
    assert_ne!(wanted, used);
    // the newer toolchain honours the lock redirect, which the old one (below 1.97) does not
    assert!(!app.join("Cargo.lock").exists());
    assert!(project.files().iter().any(|f| f == "src/lib.rs"));
}
