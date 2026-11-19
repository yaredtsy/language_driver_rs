//! RS5 §4's measurement: is the flag, looked at between sites, enough to stop a request? The solver looks at the `Cancel` before each body and the walk before
//! each site, and two engine calls in between cannot be interrupted: `sem.sites(f)`, which infers a whole body, and each `resolve`. This times them in a
//! cold first request, finds the longest stretch between two looks, and sets the flag from another thread at 100 ms, 500 ms and 1 s into a cold request to see
//! how long the request takes to return.
//!
//! Ignored, run by hand on `RS_MEASURE_ROOT` / `RS_MEASURE_FILE` / `RS_MEASURE_ENTRY` (see `testing::measured`):
//!
//! ```text
//! cargo test -p rust-analysis --lib -j2 -- --ignored --nocapture --test-threads=2 cancel::measure
//! ```

use std::time::{Duration, Instant};

use super::common::one_load;
use super::testing::{copy_of_workspace, measured, open_measured, peak_footprint, timed};
use crate::walk::probe::{self, Call};
use crate::{Budget, Cancel, Entry, Graph, Session, Status};

fn ask(session: &Session, file: &str, qualname: &str, cancel: &Cancel) -> Graph {
    let entry = Entry::Def {
        qualname,
        occurrence: 0,
    };
    session
        .graph(file, &entry, &Budget::default(), cancel)
        .unwrap_or_else(|| panic!("no def `{qualname}` in {file}"))
}

fn incomplete(graph: &Graph) -> usize {
    graph
        .contexts
        .iter()
        .filter(|context| matches!(context.status, Status::Incomplete(_)))
        .count()
}

/// The five longest of `calls`, printed with the def and the file (and the site of a `resolve`).
fn print_longest(what: &str, calls: &[Call]) {
    let total: Duration = calls.iter().map(|call| call.took).sum();
    println!(
        "MEASURE {what}: {} calls, {total:?} in all; the longest five:",
        calls.len()
    );
    let mut longest: Vec<&Call> = calls.iter().collect();
    longest.sort_by_key(|call| std::cmp::Reverse(call.took));
    for call in longest.into_iter().take(5) {
        let site = call
            .site
            .map(|site| format!(" site {site:?}"))
            .unwrap_or_default();
        println!(
            "MEASURE   {:>12?}  {} in {}{site}",
            call.took, call.def, call.file
        );
    }
}

#[test]
#[ignore = "a measurement: run by hand with --ignored --nocapture"]
fn measure_cancel() {
    let _load = one_load();
    let measured = measured();
    let (file, qualname) = (measured.file.as_str(), measured.qualname.as_str());
    let root = copy_of_workspace(&measured.from, "cancel_measure");

    // the engine calls of a cold first request, one by one
    let (session, opened) = open_measured(&root, &measured.open_file);
    println!("MEASURE open {opened:?}; request for `{qualname}` in {file}");
    probe::start();
    let (graph, request) = timed(|| ask(&session, file, qualname, &Cancel::new()));
    let log = probe::finish();
    println!(
        "MEASURE cold first request: {request:?}, {} contexts, {} incomplete",
        graph.contexts.len(),
        incomplete(&graph)
    );
    print_longest("sites(f)", &log.sites);
    print_longest("resolve", &log.resolves);

    let started = log.checks[0].at;
    println!("MEASURE where the request goes, from its start:");
    for (at, what) in &log.marks {
        println!("MEASURE   {:>12?}  {what}", at.duration_since(started));
    }
    if let Some(first) = log.checks.get(1) {
        println!(
            "MEASURE   {:>12?}  the first look at the cancel (before the first body)",
            first.at.duration_since(started)
        );
    }

    // the stretches between two looks at the cancel (the solver's, just before a walk, and the walk's, before each site)
    let mut gaps: Vec<(Duration, usize)> = log
        .checks
        .windows(2)
        .enumerate()
        .map(|(at, pair)| (pair[1].at.duration_since(pair[0].at), at))
        .collect();
    gaps.sort_by_key(|(gap, _)| std::cmp::Reverse(*gap));
    println!(
        "MEASURE {} looks at the cancel; the longest five stretches between two:",
        log.checks.len()
    );
    for (gap, at) in gaps.into_iter().take(5) {
        let after = &log.checks[at];
        println!(
            "MEASURE   {gap:>12?}  after the look in {} ({})",
            after.def,
            if after.file.is_empty() {
                "-"
            } else {
                &after.file
            }
        );
    }
    drop(session);

    // the flag set from another thread, into a cold first request
    for after_ms in [100u64, 500, 1000] {
        let (session, _) = open_measured(&root, &measured.open_file);
        let cancel = Cancel::new();
        let flag = cancel.clone();
        let setter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(after_ms));
            let at = Instant::now();
            flag.cancel();
            at
        });
        let started = Instant::now();
        let graph = ask(&session, file, qualname, &cancel);
        let returned = Instant::now();
        let set_at = setter.join().expect("the thread that sets the flag");
        if returned < set_at {
            println!(
                "MEASURE flag at {after_ms} ms: the request had already returned, after {:?}; {} incomplete",
                returned - started,
                incomplete(&graph)
            );
        } else {
            println!(
                "MEASURE flag at {after_ms} ms (set {:?} into the request): returned {:?} after it was set; {} of {} contexts incomplete",
                set_at - started,
                returned - set_at,
                incomplete(&graph),
                graph.contexts.len()
            );
        }
    }
    println!("MEASURE process {}", peak_footprint());
}
