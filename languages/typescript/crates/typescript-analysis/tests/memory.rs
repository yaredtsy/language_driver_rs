//! A long-running process must not grow: opening a project and dropping it has to give the memory back. The engine's `Program` and its
//! processed files are `Box::leak`ed outside any region, so only `free_unshared_program` (called by `Sem::drop`) returns them. Checked by
//! hand: with that call removed, RSS grows from 20 MB to 349 MB over 60 opens and this test fails; with it, 19 MB at round 10 and 24 MB at 60.
//! Early is round 10, and the peak is read over the later rounds, not the last value: the OS takes freed pages back whenever it likes.

use std::path::Path;

use typescript_analysis::Project;

fn rss_mb() -> f64 {
    let usage = memory_stats::memory_stats().expect("this platform reports memory");
    usage.physical_mem as f64 / (1024.0 * 1024.0)
}

#[test]
fn opening_and_dropping_a_project_100_times_does_not_grow_the_process() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/three-files");
    let mut early = 0.0;
    let mut peak: f64 = 0.0;
    for round in 1..=100 {
        let project = Project::open(&root, Path::new("src/main.ts")).unwrap();
        assert_eq!(project.files().len(), 3);
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
