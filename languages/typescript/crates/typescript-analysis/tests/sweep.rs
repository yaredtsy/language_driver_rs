//! A sweep anyone can point at a project (H13 (d), `docs/03-typescript/10-review-fixes.md` §6.1): the graph of every def of every file of
//! `Project::files()`, on one `Session`, and what it cost. It only reads the project.
//!
//! ```text
//! TS_SWEEP_ROOT=<dir> TS_SWEEP_FILE=<a file of it, which picks the config> [TS_SWEEP_DEPTH=<n, default 1>] \
//!   cargo test --release -p typescript-analysis --test sweep -- --ignored --nocapture
//! ```
//!
//! `TS_SWEEP_ONLY=<text>` keeps the files whose path contains the text, and `TS_SWEEP_ENTRY=<qualname>` the defs with that qualname (every occurrence): a run of
//! one file, or of one entry in its own run (the project is still opened whole).
//!
//! `TS_SWEEP_FILE_CODE=1` also sweeps the file's own code (`Entry::File`) after the defs of each file, outside a sample run (its dump line says `FILE` and `0`, as in a
//! sample): `export const s = new S()` is file code.
//!
//! `TS_SWEEP_PRECISE=1` turns the context layer on (`Precision::Precise`); its dump is comparable with one made without it.
//!
//! `TS_SWEEP_COMPARE=1` checks "never fewer edges" (`common::context::never_fewer_edges`) entry by entry: a second `Session` is opened on the same project with
//! `Precision::Default`, every entry the sweep builds (the sample lines, the defs, the file entries under `TS_SWEEP_FILE_CODE`) is also built by it with the same budget and
//! cancel, and the pair is compared. An entry that one session gives a graph for and the other does not is a problem too. At the end it prints
//! `compare: {n} entries, {m} with other edges, {p} problems` and the first 20 problems, and the test fails when there is one. It needs `TS_SWEEP_PRECISE=1` (alone it
//! panics). Times are not a measure in this mode: two sessions are open, and the memory and the clock are shared.
//!
//! `TS_SWEEP_DUMP=<file>` writes one line per def, `<file>\t<qualname>\t<occurrence>\t<hash>`, sorted, where the hash is 16 hex digits of the graph's `Debug` text (or
//! `none` when there is no graph). Two dumps are comparable only when both builds were made with the same toolchain (`DefaultHasher`'s algorithm may change between
//! Rust releases); a line that differs is re-run alone with `TS_SWEEP_ENTRY`.
//!
//! `TS_SWEEP_SAMPLE=<file>` runs only the entries of that file instead of every def: one per line, fields separated by a tab, blank lines and lines starting with
//! `#` skipped. `<file>` is the file's own code, `<file>\t<qualname>` that def (occurrence 0) and `<file>\t<qualname>\t<n>` its n-th occurrence; the file is relative to
//! `TS_SWEEP_ROOT`. In a sample run the depth is `TS_SWEEP_DEPTH` when that is set and the whole graph when it is not (outside a sample run the default is 1),
//! `TS_SWEEP_ONLY` and `TS_SWEEP_ENTRY` are ignored, and a line whose graph is `None` is a failure. In the dump, a file entry's qualname is `FILE` and its occurrence `0`.
//!
//! `TS_SWEEP_SITES=<file>` writes the site measure (see "The site measure" below), one line per site open in some context, in key order:
//! `<file>\t<qualname>\t<occurrence>\t<start>\t<end>\t<reasons joined by ,>\t<contexts that reach it>\t<contexts it is open in>`.
//! The measure runs only when this or `TS_SWEEP_BASELINE` is set, because it keeps every site of the run in memory.
//!
//! `TS_SWEEP_BASELINE=<file>` reads a file written that way (its first five fields name the site, the sixth is its reasons) and says, for each of its sites, whether this
//! run leaves it open in every context that reaches it, in some, in none, or does not reach it; and how many sites open in this run the file does not have.
//!
//! Depth 1 resolves every site of the project exactly once, which is what finds a disagreement between the engine and the outline; a larger depth is for the
//! speed runs. It fails when a graph counted `decl_not_in_outline` or `engine_kind_unhandled` (the first 20 entries of each are named), and a panic in any graph is a failure by itself: nothing
//! here catches one.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;
use std::path::Path;
use std::time::{Duration, Instant};

use typescript_analysis::testing::session_graph_and_emptied;
use typescript_analysis::{
    Budget, Cancel, ContextId, EdgeTo, Entry, Graph, MarkerStyle, Options, Placement, Precision,
    Session, Site, StableDef,
};
use typescript_syntax::Dialect;

mod common;
use common::context::never_fewer_edges;

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

/// `Session::graph`, and the sites the layer emptied in it (none when there is no graph): what `Comparison::entry` allows as lost.
fn graph_and_emptied(
    session: &Session,
    file: &str,
    entry: &Entry<'_>,
    budget: &Budget,
    cancel: &Cancel,
) -> (Option<Graph>, Vec<(StableDef, Site)>) {
    match session_graph_and_emptied(session, file, entry, budget, cancel) {
        Some((graph, emptied)) => (Some(graph), emptied),
        None => (None, Vec::new()),
    }
}

fn rss_mb() -> f64 {
    let usage = memory_stats::memory_stats().expect("this platform reports memory");
    usage.physical_mem as f64 / (1024.0 * 1024.0)
}

// ---- The site measure ------------------------------------------------------------------------------------------------------------------------------------
//
// `docs/03-typescript/12-context-layer/04-tc0-counts-and-spikes.md` §3.2. With contexts, one call site gives one edge per context that reaches it, so counting edges
// cannot say how many sites a phase answers. This counts sites.
//
// A **site** is named by what does not move between runs and phases: the def it is in (file, qualname, occurrence, from `Graph::defs`) and its `Site` (relative to the
// def). A context **reaches** a site when it has an edge there (every site has one: an unknown target is an `Unresolved` edge, never dropped). A site is **open in** a
// context when that context's edge there is `Unresolved`. Contexts are told apart by `ContextId`, over every graph of the run, so a context that two entries both reach
// is one. Edges handed over and not called (`passed`) are not sites.

/// A call site, by what does not move between runs and phases: its def (file, qualname, occurrence) and its `Site` (relative to the def).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SiteKey {
    file: String,
    qualname: String,
    occurrence: u16,
    start: u32,
    end: u32,
}

/// What the run saw at one site: the contexts with an edge there, those with an `Unresolved` edge there, and the reasons.
#[derive(Default, Debug)]
struct SiteSeen {
    reaching: BTreeSet<ContextId>,
    open_in: BTreeSet<ContextId>,
    reasons: BTreeSet<String>,
}

impl SiteSeen {
    fn reasons_text(&self) -> String {
        self.reasons
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Adds every call edge of `graph` (not the `passed` ones) to `sites`.
fn add_sites(sites: &mut BTreeMap<SiteKey, SiteSeen>, graph: &Graph) {
    for edge in graph.edges.iter().filter(|edge| !edge.passed) {
        let def = graph
            .def(&edge.from.func)
            .unwrap_or_else(|| panic!("a broken graph: no def row for {:?}", edge.from.func));
        let key = SiteKey {
            file: def.file.clone(),
            qualname: def.qualname.clone(),
            occurrence: def.occurrence,
            start: edge.site.start,
            end: edge.site.end,
        };
        let seen = sites.entry(key).or_default();
        seen.reaching.insert(edge.from.clone());
        if let EdgeTo::Unresolved(reason) = &edge.to {
            seen.open_in.insert(edge.from.clone());
            seen.reasons.insert(reason.as_str().to_owned());
        }
    }
}

/// How many sites are open, and in how many of those the site is open in every context that reaches it and in some.
#[derive(Default, Debug, PartialEq, Eq)]
struct OpenCounts {
    total: u64,
    every: u64,
    some: u64,
}

/// The counts over the sites open somewhere, in total and per reason set (the reasons joined by `,`), sorted by that text.
fn open_counts(sites: &BTreeMap<SiteKey, SiteSeen>) -> (OpenCounts, BTreeMap<String, OpenCounts>) {
    let mut all = OpenCounts::default();
    let mut by_reason: BTreeMap<String, OpenCounts> = BTreeMap::new();
    for seen in sites.values().filter(|seen| !seen.open_in.is_empty()) {
        let every = seen.open_in == seen.reaching;
        for counts in [&mut all, by_reason.entry(seen.reasons_text()).or_default()] {
            counts.total += 1;
            if every {
                counts.every += 1;
            } else {
                counts.some += 1;
            }
        }
    }
    (all, by_reason)
}

/// The text of `TS_SWEEP_SITES`: one line per site open in some context, in key order.
fn sites_text(sites: &BTreeMap<SiteKey, SiteSeen>) -> String {
    let mut text = String::new();
    for (key, seen) in sites.iter().filter(|(_, seen)| !seen.open_in.is_empty()) {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            key.file,
            key.qualname,
            key.occurrence,
            key.start,
            key.end,
            seen.reasons_text(),
            seen.reaching.len(),
            seen.open_in.len()
        ));
    }
    text
}

/// What a run says about a site of a baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SiteState {
    /// Open in every context that reaches it.
    Every,
    /// Open in some of them.
    Some,
    /// Reached, and open in none.
    None,
    /// No context of this run has an edge there.
    NotReached,
}

impl SiteState {
    const ALL: [SiteState; 4] = [
        SiteState::Every,
        SiteState::Some,
        SiteState::None,
        SiteState::NotReached,
    ];

    fn label(self) -> &'static str {
        match self {
            SiteState::Every => "every",
            SiteState::Some => "some",
            SiteState::None => "none",
            SiteState::NotReached => "not reached",
        }
    }
}

/// One site of a baseline, and its state in this run.
#[derive(Debug, PartialEq, Eq)]
struct Compared {
    key: SiteKey,
    /// The baseline's reasons for the site: the sixth field of its line.
    reasons: String,
    state: SiteState,
}

/// Each site of `baseline` (the text of an earlier `TS_SWEEP_SITES`), in its order, with its state in `sites`; and the number of sites open in `sites` that the
/// baseline does not have.
fn compare_with_baseline(
    sites: &BTreeMap<SiteKey, SiteSeen>,
    baseline: &str,
) -> (Vec<Compared>, usize) {
    let mut compared = Vec::new();
    let mut known: BTreeSet<SiteKey> = BTreeSet::new();
    for line in baseline.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        assert!(
            fields.len() >= 6,
            "a baseline line with too few fields: {line:?}"
        );
        let number = |at: usize| -> u32 {
            fields[at]
                .parse()
                .unwrap_or_else(|_| panic!("a baseline line with a bad number: {line:?}"))
        };
        let key = SiteKey {
            file: fields[0].to_owned(),
            qualname: fields[1].to_owned(),
            occurrence: u16::try_from(number(2)).expect("an occurrence fits u16"),
            start: number(3),
            end: number(4),
        };
        let state = match sites.get(&key) {
            None => SiteState::NotReached,
            Some(seen) if seen.open_in.is_empty() => SiteState::None,
            Some(seen) if seen.open_in == seen.reaching => SiteState::Every,
            Some(_) => SiteState::Some,
        };
        known.insert(key.clone());
        compared.push(Compared {
            key,
            reasons: fields[5].to_owned(),
            state,
        });
    }
    let new_open = sites
        .iter()
        .filter(|(key, seen)| !seen.open_in.is_empty() && !known.contains(*key))
        .count();
    (compared, new_open)
}

fn print_measure(sites: &BTreeMap<SiteKey, SiteSeen>) {
    let (all, by_reason) = open_counts(sites);
    eprintln!(
        "open sites: {} ({} open in every context that reaches them, {} in some)",
        all.total, all.every, all.some
    );
    for (reasons, counts) in &by_reason {
        eprintln!(
            "  {reasons}: {} ({} every, {} some)",
            counts.total, counts.every, counts.some
        );
    }
}

fn print_baseline(sites: &BTreeMap<SiteKey, SiteSeen>, baseline: &str) {
    let (compared, new_open) = compare_with_baseline(sites, baseline);
    let tally = |compared: &[&Compared]| {
        SiteState::ALL.map(|state| compared.iter().filter(|site| site.state == state).count())
    };
    let line = |name: &str, counts: [usize; 4]| {
        let states: Vec<String> = SiteState::ALL
            .iter()
            .zip(counts)
            .map(|(state, count)| format!("{count} {}", state.label()))
            .collect();
        eprintln!("  {name}: {}", states.join(", "));
    };
    eprintln!("against the baseline ({} sites):", compared.len());
    line("total", tally(&compared.iter().collect::<Vec<_>>()));
    let mut by_reason: BTreeMap<&str, Vec<&Compared>> = BTreeMap::new();
    for site in &compared {
        by_reason.entry(&site.reasons).or_default().push(site);
    }
    for (reasons, group) in &by_reason {
        line(reasons, tally(group));
    }
    eprintln!("new open sites (open in this run, not in the baseline): {new_open}");
}

// ---- The sample --------------------------------------------------------------------------------------------------------------------------------------------

/// One entry of a sample file.
#[derive(Debug, PartialEq, Eq)]
struct SampleEntry {
    file: String,
    /// `None`: the file's own code.
    def: Option<(String, u16)>,
}

fn parse_sample(text: &str) -> Vec<SampleEntry> {
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let def = match fields[..] {
                [_] => None,
                [_, qualname] => Some((qualname.to_owned(), 0)),
                [_, qualname, occurrence] => Some((
                    qualname.to_owned(),
                    occurrence.parse().unwrap_or_else(|_| {
                        panic!("a sample line with a bad occurrence: {line:?}")
                    }),
                )),
                _ => panic!("a sample line with too many fields: {line:?}"),
            };
            SampleEntry {
                file: fields[0].to_owned(),
                def,
            }
        })
        .collect()
}

// ---- The run -----------------------------------------------------------------------------------------------------------------------------------------------

/// One graph that was made.
struct Made {
    file: String,
    qualname: String,
    time: Duration,
    contexts: u64,
}

/// What the graphs of a run add up to.
#[derive(Default)]
struct Totals {
    dump: Option<Vec<String>>,
    defs: u64,
    no_graph: u64,
    made: Vec<Made>,
    counts: BTreeMap<String, u64>,
    gaps: Vec<String>,
    unhandled: Vec<String>,
    /// `Some` only when the site measure is asked for: it keeps every site of the run.
    sites: Option<BTreeMap<SiteKey, SiteSeen>>,
}

impl Totals {
    /// Adds the graph of one entry. `time` was taken right after the graph was made, before anything here.
    fn add(
        &mut self,
        file: &str,
        qualname: &str,
        occurrence: u16,
        graph: Option<Graph>,
        time: Duration,
    ) {
        self.defs += 1;
        if let Some(dump) = &mut self.dump {
            let hash = graph.as_ref().map_or_else(
                || "none".to_owned(),
                |graph| {
                    let mut hasher = std::hash::DefaultHasher::new();
                    hasher.write(format!("{graph:?}").as_bytes());
                    format!("{:016x}", hasher.finish())
                },
            );
            dump.push(format!("{file}\t{qualname}\t{occurrence}\t{hash}"));
        }
        let Some(graph) = graph else {
            self.no_graph += 1;
            return;
        };
        if let Some(sites) = &mut self.sites {
            add_sites(sites, &graph);
        }
        let stats = &graph.stats;
        for (key, value) in [
            ("walks", stats.walks),
            ("contexts", stats.contexts),
            ("reuse_hits", stats.reuse_hits),
            ("frontier", stats.frontier),
            ("merged_defs", stats.merged_defs),
            ("unresolved", stats.unresolved),
        ] {
            *self.counts.entry(key.to_owned()).or_default() += value;
        }
        for (cut, value) in &stats.cuts {
            *self.counts.entry(format!("cut {cut:?}")).or_default() += value;
        }
        for (key, value) in &stats.language {
            *self.counts.entry(format!("language {key}")).or_default() += value;
        }
        if stats
            .language
            .get("decl_not_in_outline")
            .copied()
            .unwrap_or(0)
            > 0
        {
            self.gaps.push(format!("{file} {qualname}"));
        }
        if stats
            .language
            .get("engine_kind_unhandled")
            .copied()
            .unwrap_or(0)
            > 0
        {
            self.unhandled.push(format!("{file} {qualname}"));
        }
        self.made.push(Made {
            file: file.to_owned(),
            qualname: qualname.to_owned(),
            time,
            contexts: stats.contexts,
        });
    }
}

/// The comparison of `TS_SWEEP_COMPARE`: the session with `Precision::Default`, and what the pairs of graphs said.
struct Comparison {
    default: Session,
    entries: usize,
    with_other_edges: usize,
    problems: Vec<String>,
}

impl Comparison {
    /// The graph of `entry` from the default session, compared with `precise`, the sweep's own.
    fn entry(
        &mut self,
        file: &str,
        entry: &Entry<'_>,
        (qualname, occurrence): (&str, u16),
        (budget, cancel): (&Budget, &Cancel),
        precise: &Option<Graph>,
        emptied: &[(StableDef, Site)],
    ) {
        self.entries += 1;
        let default = self.default.graph(file, entry, budget, cancel);
        let at = format!("{file} {qualname} {occurrence}");
        match (&default, precise) {
            (Some(default), Some(precise)) => {
                if default.edges != precise.edges {
                    self.with_other_edges += 1;
                }
                for problem in never_fewer_edges(default, precise, emptied) {
                    self.problems.push(format!("{at}: {problem}"));
                }
            }
            (Some(_), None) => self
                .problems
                .push(format!("{at}: a graph with Default, none with Precise")),
            (None, Some(_)) => self
                .problems
                .push(format!("{at}: a graph with Precise, none with Default")),
            (None, None) => {}
        }
    }

    fn print(&self) {
        eprintln!(
            "compare: {} entries, {} with other edges, {} problems",
            self.entries,
            self.with_other_edges,
            self.problems.len()
        );
        for problem in self.problems.iter().take(20) {
            eprintln!("  {problem}");
        }
    }
}

#[test]
#[ignore = "a sweep of a real project, driven by TS_SWEEP_ROOT, TS_SWEEP_FILE and TS_SWEEP_DEPTH"]
fn sweep_every_def_of_a_project() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
    let root = std::path::PathBuf::from(var("TS_SWEEP_ROOT"));
    let file = var("TS_SWEEP_FILE");
    let depth: Option<u32> = std::env::var("TS_SWEEP_DEPTH")
        .ok()
        .map(|text| text.parse().expect("TS_SWEEP_DEPTH is a number"));
    let sample = std::env::var("TS_SWEEP_SAMPLE").ok().map(|path| {
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
        parse_sample(&text)
    });

    let only = std::env::var("TS_SWEEP_ONLY").ok();
    let only_entry = std::env::var("TS_SWEEP_ENTRY").ok();
    let file_code = std::env::var("TS_SWEEP_FILE_CODE").is_ok_and(|value| value == "1");
    let precision = if std::env::var("TS_SWEEP_PRECISE").is_ok_and(|value| value == "1") {
        Precision::Precise
    } else {
        Precision::Default
    };
    let compare = std::env::var("TS_SWEEP_COMPARE").is_ok_and(|value| value == "1");
    assert!(
        !compare || precision == Precision::Precise,
        "TS_SWEEP_COMPARE=1 compares Precise with Default, so it needs TS_SWEEP_PRECISE=1"
    );
    let dump_to = std::env::var("TS_SWEEP_DUMP").ok();
    let sites_to = std::env::var("TS_SWEEP_SITES").ok();
    let baseline_from = std::env::var("TS_SWEEP_BASELINE").ok();
    let mut totals = Totals {
        dump: dump_to.is_some().then(Vec::new),
        sites: (sites_to.is_some() || baseline_from.is_some()).then(BTreeMap::new),
        ..Totals::default()
    };
    let before_open = rss_mb();
    let started = Instant::now();
    let mut options = Options::new(STYLE);
    options.precision = precision;
    let session = Session::open(&root, Path::new(&file), options)
        .unwrap_or_else(|error| panic!("the project does not open: {error:?}"));
    let open_time = started.elapsed();
    let after_open = rss_mb();
    let mut comparison = compare.then(|| {
        let mut options = Options::new(STYLE);
        options.precision = Precision::Default;
        Comparison {
            default: Session::open(&root, Path::new(&file), options)
                .unwrap_or_else(|error| panic!("the project does not open as Default: {error:?}")),
            entries: 0,
            with_other_edges: 0,
            problems: Vec::new(),
        }
    });

    let files = session.project().expect("a session just opened").files();
    // a sample run is the whole graph unless asked otherwise; a sweep of every def is depth 1 unless asked otherwise
    let depth = if sample.is_some() {
        depth
    } else {
        Some(depth.unwrap_or(1))
    };
    let budget = Budget::new(depth, None);
    let cancel = Cancel::new();
    let sweep = Instant::now();
    if let Some(sample) = &sample {
        for line in sample {
            let (entry, qualname, occurrence) = match &line.def {
                None => (Entry::File, "FILE", 0),
                Some((qualname, occurrence)) => (
                    Entry::Def {
                        qualname,
                        occurrence: *occurrence,
                    },
                    qualname.as_str(),
                    *occurrence,
                ),
            };
            let started = Instant::now();
            let (graph, emptied) =
                graph_and_emptied(&session, &line.file, &entry, &budget, &cancel);
            let time = started.elapsed();
            assert!(
                graph.is_some(),
                "no graph for the sample line {:?}",
                (&line.file, qualname, occurrence)
            );
            if let Some(comparison) = &mut comparison {
                comparison.entry(
                    &line.file,
                    &entry,
                    (qualname, occurrence),
                    (&budget, &cancel),
                    &graph,
                    &emptied,
                );
            }
            totals.add(&line.file, qualname, occurrence, graph, time);
        }
    } else {
        for name in &files {
            if only
                .as_ref()
                .is_some_and(|only| !name.contains(only.as_str()))
            {
                continue;
            }
            let Some(dialect) = Dialect::from_path(name) else {
                continue;
            };
            let text = std::fs::read_to_string(root.join(name))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            let outline = typescript_syntax::outline(&text, dialect, STYLE);
            // the entry names a def by its qualname and which of the same-named defs it is, in source order
            let mut seen: BTreeMap<&str, u16> = BTreeMap::new();
            for def in &outline.defs {
                let occurrence = seen.entry(def.qualname.as_str()).or_default();
                let skip = only_entry
                    .as_ref()
                    .is_some_and(|only| *only != def.qualname);
                let this = *occurrence;
                let entry = Entry::Def {
                    qualname: &def.qualname,
                    occurrence: this,
                };
                *occurrence += 1;
                if skip {
                    continue;
                }
                let started = Instant::now();
                let (graph, emptied) = graph_and_emptied(&session, name, &entry, &budget, &cancel);
                let time = started.elapsed();
                if let Some(comparison) = &mut comparison {
                    comparison.entry(
                        name,
                        &entry,
                        (&def.qualname, this),
                        (&budget, &cancel),
                        &graph,
                        &emptied,
                    );
                }
                totals.add(name, &def.qualname, this, graph, time);
            }
            if file_code {
                let started = Instant::now();
                let (graph, emptied) =
                    graph_and_emptied(&session, name, &Entry::File, &budget, &cancel);
                let time = started.elapsed();
                if let Some(comparison) = &mut comparison {
                    comparison.entry(
                        name,
                        &Entry::File,
                        ("FILE", 0),
                        (&budget, &cancel),
                        &graph,
                        &emptied,
                    );
                }
                totals.add(name, "FILE", 0, graph, time);
            }
        }
    }
    let all_graphs = sweep.elapsed();
    if let (Some(path), Some(dump)) = (&dump_to, &mut totals.dump) {
        dump.sort();
        let mut text = dump.join("\n");
        text.push('\n');
        std::fs::write(path, text).unwrap_or_else(|error| panic!("{path}: {error}"));
    }
    if let (Some(path), Some(sites)) = (&sites_to, &totals.sites) {
        std::fs::write(path, sites_text(sites)).unwrap_or_else(|error| panic!("{path}: {error}"));
    }
    let after_last = rss_mb();

    let Totals {
        defs,
        no_graph,
        mut made,
        counts,
        gaps,
        unhandled,
        sites,
        ..
    } = totals;
    let depth_text = depth.map_or_else(|| "the whole graph".to_owned(), |depth| depth.to_string());
    eprintln!("precision: {precision:?}");
    eprintln!(
        "depth {depth_text}: {} files, {defs} {}, {} graphs made ({no_graph} with no graph)",
        files.len(),
        if sample.is_some() {
            "sample entries"
        } else {
            "defs"
        },
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
    if let Some(sites) = &sites {
        print_measure(sites);
        if let Some(path) = &baseline_from {
            let baseline =
                std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));
            print_baseline(sites, &baseline);
        }
    } else {
        eprintln!("site measure: off (set TS_SWEEP_SITES or TS_SWEEP_BASELINE)");
    }
    if let Some(comparison) = &comparison {
        comparison.print();
        assert!(
            comparison.problems.is_empty(),
            "the layer drew fewer edges than Default in {} places; see above",
            comparison.problems.len()
        );
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

#[test]
fn the_site_measure_tells_every_some_none_and_not_reached() {
    use typescript_analysis::DefKind;
    use typescript_analysis::{
        Basis, Context, DefInfo, Edge, Reason, Site, Span, StableDef, Stats,
    };

    let def = |name: &str, range: Span| {
        DefInfo::new(
            StableDef::Path {
                module: "a.ts".to_owned(),
                qname: name.to_owned(),
                nth: 0,
            },
            name,
            name,
            0,
            DefKind::Function,
            "a.ts",
            range,
        )
    };
    let func = |name: &str| StableDef::Path {
        module: "a.ts".to_owned(),
        qname: name.to_owned(),
        nth: 0,
    };
    let context = |name: &str, digest: u8| ContextId {
        func: func(name),
        digest: [digest; 16],
    };
    let (f0, f1, g0) = (context("f", 0), context("f", 1), context("g", 0));
    let open = |reason: &str| EdgeTo::Unresolved(Reason::new(reason));
    let calls_g = || EdgeTo::Context(g0.clone(), Basis::Type);
    let edges = vec![
        Edge::new(f0.clone(), Site::new(10, 20), open("function_typed_value")),
        Edge::new(f1.clone(), Site::new(10, 20), open("function_typed_value")),
        Edge::new(f0.clone(), Site::new(30, 40), open("any_or_untyped")),
        Edge::new(f1.clone(), Site::new(30, 40), calls_g()),
        Edge::new(f0.clone(), Site::new(50, 60), calls_g()),
        Edge::new(f1.clone(), Site::new(50, 60), calls_g()),
    ];
    let graph = Graph::new(
        vec![],
        vec![def("f", Span::new(0, 100)), def("g", Span::new(200, 300))],
        vec![Context::new(f0), Context::new(f1), Context::new(g0.clone())],
        edges,
        Stats::default(),
    );

    let mut sites = BTreeMap::new();
    add_sites(&mut sites, &graph);

    // 1. two open sites: 10..20 in every context, 30..40 in some; 50..60 is a site, open nowhere
    let (all, _) = open_counts(&sites);
    assert_eq!(
        all,
        OpenCounts {
            total: 2,
            every: 1,
            some: 1
        }
    );
    assert_eq!(sites.len(), 3);
    let answered = sites
        .iter()
        .find(|(key, _)| key.start == 50)
        .expect("50..60 is a site");
    assert!(answered.1.open_in.is_empty());

    // 2. the lines of TS_SWEEP_SITES
    let text = sites_text(&sites);
    let wanted = "a.ts\tf\t0\t10\t20\tfunction_typed_value\t2\t2\n\
                  a.ts\tf\t0\t30\t40\tany_or_untyped\t2\t1\n";
    assert_eq!(text, wanted);

    // 3. against a baseline of four sites
    let baseline = format!(
        "{wanted}a.ts\tf\t0\t50\t60\tfunction_typed_value\t1\t1\na.ts\tf\t0\t70\t80\tfunction_typed_value\t1\t1\n"
    );
    let (compared, new_open) = compare_with_baseline(&sites, &baseline);
    let states: Vec<SiteState> = compared.iter().map(|site| site.state).collect();
    assert_eq!(
        states,
        [
            SiteState::Every,
            SiteState::Some,
            SiteState::None,
            SiteState::NotReached
        ]
    );
    assert_eq!(new_open, 0);
}

#[test]
fn a_sample_file_names_files_and_defs() {
    let text = "# a comment\n\nsrc/a.ts\nsrc/a.ts\tf\nsrc/b.ts\tC.m\t2\n";
    assert_eq!(
        parse_sample(text),
        [
            SampleEntry {
                file: "src/a.ts".to_owned(),
                def: None
            },
            SampleEntry {
                file: "src/a.ts".to_owned(),
                def: Some(("f".to_owned(), 0))
            },
            SampleEntry {
                file: "src/b.ts".to_owned(),
                def: Some(("C.m".to_owned(), 2))
            },
        ]
    );
}
