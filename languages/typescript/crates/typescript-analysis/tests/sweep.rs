//! A sweep anyone can point at a project (H13 (d), `docs/03-typescript/10-review-fixes.md` §6.1): the graph of every def of every file of
//! `Project::files()`, on one `Session`, and what it cost. It only reads the project.
//!
//! ```text
//! TS_SWEEP_ROOT=<dir> TS_SWEEP_FILE=<a file of it, which picks the config> [TS_SWEEP_DEPTH=<n, default 1>] \
//!   cargo test --release -p typescript-analysis --test sweep -- --ignored --nocapture
//! ```
//!
//! Depth 1 resolves every site of the project exactly once, which is what finds a disagreement between the engine and the outline; a larger depth is for the
//! speed runs. It fails when a graph counted `decl_not_in_outline` or `engine_kind_unhandled` (the first 20 entries of each are named), and a panic in any graph is a failure by itself: nothing
//! here catches one.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use typescript_analysis::{Budget, Cancel, Entry, MarkerStyle, Options, Placement, Session};
use typescript_syntax::Dialect;

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

fn rss_mb() -> f64 {
    let usage = memory_stats::memory_stats().expect("this platform reports memory");
    usage.physical_mem as f64 / (1024.0 * 1024.0)
}

/// One graph that was made.
struct Made {
    file: String,
    qualname: String,
    time: Duration,
    contexts: u64,
}

#[test]
#[ignore = "a sweep of a real project, driven by TS_SWEEP_ROOT, TS_SWEEP_FILE and TS_SWEEP_DEPTH"]
fn sweep_every_def_of_a_project() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
    let root = std::path::PathBuf::from(var("TS_SWEEP_ROOT"));
    let file = var("TS_SWEEP_FILE");
    let depth: u32 = std::env::var("TS_SWEEP_DEPTH")
        .map(|text| text.parse().expect("TS_SWEEP_DEPTH is a number"))
        .unwrap_or(1);

    let before_open = rss_mb();
    let started = Instant::now();
    let session = Session::open(&root, Path::new(&file), Options::new(STYLE))
        .unwrap_or_else(|error| panic!("the project does not open: {error:?}"));
    let open_time = started.elapsed();
    let after_open = rss_mb();

    let files = session.project().expect("a session just opened").files();
    let budget = Budget::new(Some(depth), None);
    let cancel = Cancel::new();
    let mut defs = 0;
    let mut no_graph = 0;
    let mut made: Vec<Made> = Vec::new();
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut gaps: Vec<String> = Vec::new();
    let mut unhandled: Vec<String> = Vec::new();
    let sweep = Instant::now();
    for name in &files {
        let Some(dialect) = Dialect::from_path(name) else {
            continue;
        };
        let text = std::fs::read_to_string(root.join(name))
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let outline = typescript_syntax::outline(&text, dialect, STYLE);
        // the entry names a def by its qualname and which of the same-named defs it is, in source order
        let mut seen: BTreeMap<&str, u16> = BTreeMap::new();
        for def in &outline.defs {
            defs += 1;
            let occurrence = seen.entry(def.qualname.as_str()).or_default();
            let entry = Entry::Def {
                qualname: &def.qualname,
                occurrence: *occurrence,
            };
            *occurrence += 1;
            let started = Instant::now();
            let Some(graph) = session.graph(name, &entry, &budget, &cancel) else {
                no_graph += 1;
                continue;
            };
            let time = started.elapsed();
            let stats = &graph.stats;
            for (key, value) in [
                ("walks", stats.walks),
                ("contexts", stats.contexts),
                ("reuse_hits", stats.reuse_hits),
                ("frontier", stats.frontier),
                ("merged_defs", stats.merged_defs),
                ("unresolved", stats.unresolved),
            ] {
                *counts.entry(key.to_owned()).or_default() += value;
            }
            for (cut, value) in &stats.cuts {
                *counts.entry(format!("cut {cut:?}")).or_default() += value;
            }
            for (key, value) in &stats.language {
                *counts.entry(format!("language {key}")).or_default() += value;
            }
            if stats
                .language
                .get("decl_not_in_outline")
                .copied()
                .unwrap_or(0)
                > 0
            {
                gaps.push(format!("{name} {}", def.qualname));
            }
            if stats
                .language
                .get("engine_kind_unhandled")
                .copied()
                .unwrap_or(0)
                > 0
            {
                unhandled.push(format!("{name} {}", def.qualname));
            }
            made.push(Made {
                file: name.clone(),
                qualname: def.qualname.clone(),
                time,
                contexts: stats.contexts,
            });
        }
    }
    let all_graphs = sweep.elapsed();
    let after_last = rss_mb();

    eprintln!(
        "depth {depth}: {} files, {defs} defs, {} graphs made ({no_graph} defs with no graph)",
        files.len(),
        made.len()
    );
    eprintln!("open: {open_time:?}");
    eprintln!(
        "resident memory: {before_open:.1} MB before open, {after_open:.1} MB after open, {after_last:.1} MB after the last graph"
    );
    eprintln!("all the graphs: {all_graphs:?}");
    made.sort_by_key(|made| std::cmp::Reverse(made.time));
    eprintln!("the 10 slowest entries:");
    for slow in made.iter().take(10) {
        eprintln!(
            "  {:?}  {} {}  ({} contexts)",
            slow.time, slow.file, slow.qualname, slow.contexts
        );
    }
    eprintln!(
        "decl_not_in_outline: {} entries; engine_kind_unhandled: {} entries",
        gaps.len(),
        unhandled.len()
    );
    eprintln!("the sum of every count over all the graphs:");
    for (key, value) in &counts {
        eprintln!("  {key}: {value}");
    }
    assert!(
        gaps.is_empty(),
        "decl_not_in_outline is counted by {} entries; the first 20: {:#?}",
        gaps.len(),
        &gaps[..gaps.len().min(20)]
    );
    assert!(
        unhandled.is_empty(),
        "engine_kind_unhandled is counted by {} entries; the first 20: {:#?}",
        unhandled.len(),
        &unhandled[..unhandled.len().min(20)]
    );
}
