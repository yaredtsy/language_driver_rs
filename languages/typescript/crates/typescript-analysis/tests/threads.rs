//! A `Project` is `Send + Sync` today (`ENGINE.md`: nothing pins it, the decision is TS4's first checker query). What it must survive in the meantime
//! is the case a driver hits: opened on one thread, dropped on another. `Sem::drop` enters the program's region and frees it, and tsrs enters
//! regions per thread, so this is the test that the free does not depend on the thread that made the program.

use std::path::{Path, PathBuf};
use std::thread;

use typescript_analysis::Project;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/three-files")
}

fn open() -> Project {
    Project::open(&root(), Path::new("src/main.ts")).unwrap()
}

#[test]
fn a_project_opened_on_one_thread_is_used_and_dropped_on_another() {
    for _ in 0..20 {
        let project = thread::spawn(open).join().unwrap();
        let files = thread::spawn(move || {
            let files = project.files();
            drop(project); // dropped here, not where it was opened
            files
        })
        .join()
        .unwrap();
        assert_eq!(files.len(), 3);
    }
}

#[test]
fn a_project_is_dropped_on_the_main_thread_after_a_spawned_thread_opened_it() {
    let projects: Vec<Project> = (0..4)
        .map(|_| thread::spawn(open).join().unwrap())
        .collect();
    // all four alive at once, opened on four different threads, freed here in reverse
    for project in projects.into_iter().rev() {
        assert_eq!(project.files().len(), 3);
    }
}

#[test]
fn projects_opened_at_the_same_time_on_several_threads_do_not_disturb_each_other() {
    let handles: Vec<_> = (0..4)
        .map(|_| {
            thread::spawn(|| {
                for _ in 0..10 {
                    assert_eq!(open().files().len(), 3);
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
}

fn rss_mb() -> f64 {
    let usage = memory_stats::memory_stats().expect("this platform reports memory");
    usage.physical_mem as f64 / (1024.0 * 1024.0)
}

#[test]
fn opening_on_one_thread_and_dropping_on_another_100_times_does_not_grow_the_process() {
    // as `tests/memory.rs`, but each project is freed on a different thread from the one that made it
    let mut early = 0.0;
    let mut peak: f64 = 0.0;
    for round in 1..=100 {
        let project = thread::spawn(open).join().unwrap();
        drop(project);
        if round == 10 {
            early = rss_mb();
        } else if round > 10 {
            peak = peak.max(rss_mb());
        }
    }
    assert!(
        peak - early < 30.0,
        "memory grew from {early:.1} MB (round 10) to a peak of {peak:.1} MB (rounds 11 to 100)"
    );
}
