//! The rows of RS3 §6 that need a toolchain this machine does not have by default. Run by hand, each with the name of an installed toolchain:
//!
//! ```text
//! RS_TOOLCHAIN_WITHOUT_SRC=nosrc  RS_TOOLCHAIN_BEFORE_1_97=1.94.1  cargo test -p rust-analysis --test toolchains -- --ignored
//! ```
//!
//! `cargo test` itself sets `RUSTUP_TOOLCHAIN`, which beats a `rust-toolchain.toml`, and this crate may not call `set_var` (`unsafe_code` is
//! forbidden). So the test starts a second copy of this program, a *worker*, with the toolchain in the child's environment only; the worker
//! is the test that loads. Run by itself a worker does nothing (it needs `RS_WORKER`).

use std::path::Path;
use std::process::Command;

use rust_analysis::{LoadOptions, Project, Warning};

mod common;
use common::copy_of_fixture;

/// Runs the worker `name` of this program with `RUSTUP_TOOLCHAIN=toolchain`, and fails with its output when it fails.
fn run_worker(name: &str, toolchain: &str) {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", name, "--nocapture"])
        .env("RUSTUP_TOOLCHAIN", toolchain)
        .env("RS_WORKER", "1")
        .output()
        .unwrap();
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
    run_worker("worker_without_rust_src", &toolchain);
}

#[test]
#[ignore = "run by a_toolchain_without_rust_src_loads_with_a_warning"]
fn worker_without_rust_src() {
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
    run_worker("worker_lockfile_written", &toolchain);
}

#[test]
#[ignore = "run by a_toolchain_that_ignores_the_redirect_writes_a_lock_and_says_so"]
fn worker_lockfile_written() {
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
