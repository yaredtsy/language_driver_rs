//! RS3 §7: open a project and drop it, twenty times, and memory is flat. Ownership should make this trivially true (a `Project` owns its database and
//! frees it when dropped; nothing is global); this is the one cheap test that says so. Each round also asks `files()`, which makes the engine do real
//! work (it analyses `std`), so what a query keeps is counted too.
//!
//! Slow (twenty loads, each with a first query that takes seconds): `#[ignore]`, run by hand:
//!
//! ```text
//! cargo test -p rust-analysis --test memory -- --ignored --nocapture
//! ```
//!
//! Resident memory is read with `ps` (Unix): this crate may not use `unsafe`, so there is no `getrusage`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use rust_analysis::{LoadOptions, Project};

/// This process's resident memory, in kilobytes, as `ps` says; `None` where there is no `ps`.
fn rss_kb() -> Option<u64> {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn core() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../core")
}

#[test]
#[ignore = "slow: twenty loads of core/ with a first query each; run by hand with --ignored --nocapture"]
fn opening_and_dropping_core_twenty_times_keeps_memory_flat() {
    if rss_kb().is_none() {
        eprintln!("no `ps` here: memory cannot be read, and there is nothing to test");
        return;
    }
    let core = core();
    let mut samples = Vec::new();
    for run in 1..=20 {
        let started = Instant::now();
        let project = Project::open(
            &core,
            &core.join("analyzer-core/src/lib.rs"),
            &LoadOptions::default(),
        )
        .expect("core/ loads");
        let opened = started.elapsed();
        let files = project.files();
        let queried = started.elapsed();
        assert!(files.len() > 5);
        drop(project);
        let rss = rss_kb().unwrap();
        println!(
            "run {run:2}: open {opened:>6.0?}, files() {:>6.0?}, rss after drop {} MB",
            queried - opened,
            rss / 1024
        );
        samples.push(rss);
    }
    // run 2 is the first with every one-off cost paid (allocator arenas, thread stacks); run 20 is the last
    let (second, last) = (samples[1], samples[19]);
    println!(
        "rss at run 2: {} MB, at run 20: {} MB",
        second / 1024,
        last / 1024
    );
    assert!(
        last <= second + second / 4,
        "memory grew from {} MB (run 2) to {} MB (run 20): {samples:?}",
        second / 1024,
        last / 1024
    );
}
