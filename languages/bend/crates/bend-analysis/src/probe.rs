//! Counting for the cost of `Precision::Precise` (`docs/08-bend/07-value-layer-design.md` §15, V10): how many times a body is read, for what, and where the time goes.
//!
//! In a normal build every function here is empty and the guard has no size, so the places that call them cost nothing; in the tests they fill in a thread-local
//! record, once `reset` has turned it on. Nothing in it changes what the analysis answers.
//!
//! 🦀 A *guard* is a value whose `Drop` runs when it goes out of scope: `let _time = probe::section(Section::Keys);` starts the clock for the keys and stops it at the
//! end of the block, however the block is left (an early `return` or `?` included). The section it interrupts is paused meanwhile, so each microsecond is counted in
//! exactly one section (the times are *exclusive*).

/// Where a stretch of time is spent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    /// Building a key, its digest and its canonical form.
    Keys,
    /// A body read for its value, in the worklist.
    Value,
    /// The solver's walk of a context: reading its body for its sites.
    Sites,
    /// The worklist itself: its queue, its cells, its memo.
    Worklist,
    /// Working out which parameters of a def a key may hold (`key::relevant`), once per def.
    Relevance,
    /// A key as a canonical key (`key::canon`).
    Canon,
    /// A canonical key as bytes, and their hash.
    Hash,
}

/// Something that happened, counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Count {
    /// A key was made (`key::key_with` at `Precise`), and how many of them hold a slot.
    KeyMade,
    KeyClosed,
    /// A digest was asked of a closed key, and how many had to be made (the others were found).
    DigestAsked,
    DigestMade,
    /// A key was written out as a canonical key.
    Canon,
    /// A def's relevant parameters were worked out.
    Relevance,
}

#[cfg(test)]
const COUNTS: usize = 6;

#[cfg(not(test))]
pub(crate) use dead::{count, kept_used, requeued, section, site_walk, value_read};
#[cfg(test)]
pub(crate) use live::{count, kept_used, requeued, section, site_walk, value_read};

#[cfg(not(test))]
mod dead {
    use super::{Count, Section};
    use crate::book::DeclId;
    use crate::key::Key;

    pub(crate) struct Guard;

    #[inline(always)]
    pub(crate) fn count(_: Count) {}

    #[inline(always)]
    pub(crate) fn section(_: Section) -> Guard {
        Guard
    }

    #[inline(always)]
    pub(crate) fn value_read(_: DeclId, _: &Key, _: bool) {}

    #[inline(always)]
    pub(crate) fn site_walk(_: DeclId, _: &Key) {}

    #[inline(always)]
    pub(crate) fn requeued(_: bool, _: u64) {}

    #[inline(always)]
    pub(crate) fn kept_used() {}
}

#[cfg(test)]
mod live {
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    use std::collections::HashMap;

    use super::{COUNTS, Count, Section};
    use crate::book::DeclId;
    use crate::key::Key;

    /// What one request cost.
    #[derive(Debug, Default)]
    pub(crate) struct Report {
        /// Bodies read for their value, and how many of those were a context read again because a value it read had grown.
        pub(crate) value_reads: u64,
        pub(crate) value_rereads: u64,
        /// The contexts read for their value.
        pub(crate) value_ctxs: usize,
        /// Contexts put back in the queue because a value they read grew, and how many of those it grew from "nothing yet" (data).
        pub(crate) requeued: u64,
        pub(crate) requeued_from_data: u64,
        /// The solver's walks of a context, and the contexts walked.
        pub(crate) site_walks: u64,
        /// Of those, the walks that took their sites from the evaluation that gave the value.
        pub(crate) site_reused: u64,
        pub(crate) site_ctxs: usize,
        /// The contexts that were read for their value and walked: what a reading that gave both would save.
        pub(crate) both: usize,
        /// Exclusive times: what is left, outside every section, is the solver's.
        pub(crate) solver: Duration,
        pub(crate) keys: Duration,
        pub(crate) value: Duration,
        pub(crate) sites: Duration,
        pub(crate) worklist: Duration,
        pub(crate) relevance: Duration,
        pub(crate) canon: Duration,
        pub(crate) hash: Duration,
        pub(crate) counts: [u64; COUNTS],
        /// How many times the body of each def was read for its value.
        pub(crate) value_by_def: HashMap<DeclId, u64>,
    }

    struct State {
        on: bool,
        stack: Vec<Section>,
        last: Instant,
        report: Report,
        value_ctxs: HashSet<(DeclId, Key)>,
        site_ctxs: HashSet<(DeclId, Key)>,
    }

    thread_local! {
        static STATE: RefCell<State> = RefCell::new(State {
            on: false,
            stack: Vec::new(),
            last: Instant::now(),
            report: Report::default(),
            value_ctxs: HashSet::new(),
            site_ctxs: HashSet::new(),
        });
    }

    impl State {
        /// The time since the last change of section goes to the section on top, or to the solver when there is none.
        fn charge(&mut self) {
            let now = Instant::now();
            let spent = now - self.last;
            self.last = now;
            let report = &mut self.report;
            match self.stack.last() {
                None => report.solver += spent,
                Some(Section::Keys) => report.keys += spent,
                Some(Section::Value) => report.value += spent,
                Some(Section::Sites) => report.sites += spent,
                Some(Section::Worklist) => report.worklist += spent,
                Some(Section::Relevance) => report.relevance += spent,
                Some(Section::Canon) => report.canon += spent,
                Some(Section::Hash) => report.hash += spent,
            }
        }
    }

    /// Starts counting, from nothing.
    pub(crate) fn reset() {
        STATE.with_borrow_mut(|state| {
            state.on = true;
            state.stack.clear();
            state.report = Report::default();
            state.value_ctxs.clear();
            state.site_ctxs.clear();
            state.last = Instant::now();
        });
    }

    /// Stops counting and gives what was counted.
    pub(crate) fn take() -> Report {
        STATE.with_borrow_mut(|state| {
            state.charge();
            state.on = false;
            let mut report = std::mem::take(&mut state.report);
            report.value_ctxs = state.value_ctxs.len();
            report.site_ctxs = state.site_ctxs.len();
            report.both = state.value_ctxs.intersection(&state.site_ctxs).count();
            report
        })
    }

    pub(crate) struct Guard(bool);

    pub(crate) fn section(section: Section) -> Guard {
        STATE.with_borrow_mut(|state| {
            if !state.on {
                return Guard(false);
            }
            state.charge();
            state.stack.push(section);
            Guard(true)
        })
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if self.0 {
                STATE.with_borrow_mut(|state| {
                    state.charge();
                    state.stack.pop();
                });
            }
        }
    }

    pub(crate) fn count(what: Count) {
        STATE.with_borrow_mut(|state| {
            if state.on {
                state.report.counts[what as usize] += 1;
            }
        });
    }

    /// A body was read for its value; `again` when its context had been read before.
    pub(crate) fn value_read(def: DeclId, key: &Key, again: bool) {
        STATE.with_borrow_mut(|state| {
            if state.on {
                state.report.value_reads += 1;
                *state.report.value_by_def.entry(def).or_default() += 1;
                state.report.value_rereads += u64::from(again);
                if !again {
                    state.value_ctxs.insert((def, key.clone()));
                }
            }
        });
    }

    /// A value grew and `readers` contexts that read it were queued again; `from_data` when it grew from "nothing yet".
    pub(crate) fn requeued(from_data: bool, readers: u64) {
        STATE.with_borrow_mut(|state| {
            if state.on {
                state.report.requeued += readers;
                state.report.requeued_from_data += if from_data { readers } else { 0 };
            }
        });
    }

    /// A walk took the sites of a context from the evaluation that gave its value.
    pub(crate) fn kept_used() {
        STATE.with_borrow_mut(|state| {
            if state.on {
                state.report.site_reused += 1;
            }
        });
    }

    /// The solver walked a context.
    pub(crate) fn site_walk(def: DeclId, key: &Key) {
        STATE.with_borrow_mut(|state| {
            if state.on {
                state.report.site_walks += 1;
                state.site_ctxs.insert((def, key.clone()));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use analyzer_core::edit::MarkerStyle;
    use analyzer_core::solve::{Cancel, Limits};

    use super::Count;
    use super::live::{reset, take};
    use crate::book::Book;
    use crate::calls::{Options, Precision, call_graph};

    /// The entry of each of the 16 books of the sweep: a demo's `main.bend`, or its `walkers_test.bend` when it has no `main.bend`.
    fn sweep_entries(clone: &Path) -> Vec<(String, PathBuf)> {
        let mut dirs: Vec<PathBuf> = fs::read_dir(clone.join("demos"))
            .expect("the demos")
            .map(|e| e.expect("a demo").path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        dirs.into_iter()
            .map(|dir| {
                let name = dir
                    .file_name()
                    .expect("a name")
                    .to_string_lossy()
                    .into_owned();
                let entry = ["main.bend", "walkers_test.bend"]
                    .into_iter()
                    .map(|f| dir.join(f))
                    .find(|f| f.is_file())
                    .expect("a main.bend or a walkers_test.bend");
                (name, entry)
            })
            .collect()
    }

    /// How many contexts of `graph` are on a cycle of calls (or hand-overs): they reach themselves.
    fn on_a_cycle(graph: &analyzer_core::graph::Graph) -> usize {
        use analyzer_core::graph::{ContextId, EdgeTo};
        use std::collections::{HashMap, HashSet};
        let mut next: HashMap<&ContextId, Vec<&ContextId>> = HashMap::new();
        for edge in &graph.edges {
            if let EdgeTo::Context(to, _) = &edge.to {
                next.entry(&edge.from).or_default().push(to);
            }
        }
        graph
            .contexts
            .iter()
            .filter(|context| {
                let mut seen: HashSet<&ContextId> = HashSet::new();
                let mut stack: Vec<&ContextId> = next.get(&context.id).cloned().unwrap_or_default();
                while let Some(at) = stack.pop() {
                    if at == &context.id {
                        return true;
                    }
                    if seen.insert(at) {
                        stack.extend(next.get(at).into_iter().flatten().copied());
                    }
                }
                false
            })
            .count()
    }

    fn ms(d: Duration) -> f64 {
        d.as_secs_f64() * 1000.0
    }

    /// The return type a def's header writes (`def f(x: U32) -> List<U32>:` is `List<U32>`), as text; `None` when it writes none. Read from the text for the measurement
    /// only: nothing in the analysis reads a type.
    fn written_return(book: &Book, id: crate::book::DeclId) -> Option<String> {
        let (file, def) = (book.file(id.file())?, book.def(id)?);
        let text = file
            .text
            .get(def.range.start as usize..def.range.end as usize)?;
        // the range starts at the comment block above the def
        let header = text
            .match_indices("def ")
            .find(|(at, _)| *at == 0 || text.as_bytes()[at - 1] == b'\n')?
            .0;
        let text = &text[header..];
        let open = text.find('(')?;
        let (mut depth, mut close) = (0_i32, None);
        for (at, c) in text[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + at);
                        break;
                    }
                }
                _ => {}
            }
        }
        let rest = text[close? + 1..].trim_start().strip_prefix("->")?;
        let end = rest.find(":\n").or_else(|| rest.rfind(':'))?;
        Some(rest[..end].trim().to_owned())
    }

    /// What a written return type is, for V10c: a Base scalar (a name of a Base `type` that has no parameter and no function in it), a type of the project, a function type,
    /// or anything else (a Base container, a type variable, a hub's type); and nothing written.
    fn kind_of_return(book: &Book, id: crate::book::DeclId) -> &'static str {
        use crate::book::Resolved;
        const SCALARS: [&str; 9] = [
            "U32", "F32", "Nat", "Bool", "Char", "String", "Unit", "Empty", "Cmp",
        ];
        let Some(written) = written_return(book, id) else {
            return "none written";
        };
        if SCALARS.contains(&written.as_str()) {
            return "Base scalar";
        }
        if written.contains("->") {
            return "function type";
        }
        match book.resolve(id.file(), &written) {
            Resolved::Project(ty)
                if book
                    .decl(ty)
                    .is_some_and(|d| d.kind == crate::DefKind::Enum) =>
            {
                "project type"
            }
            _ => "other",
        }
    }

    /// V10a: per book of the sweep, under `Precise`, how many times each body is read and where the time goes:
    /// `BEND_CLONE=../language_clone/bend cargo test -p bend-analysis --release --lib probe -- --ignored --nocapture --test-threads=1`.
    #[test]
    #[ignore = "needs BEND_CLONE; run with --release, one at a time"]
    fn where_precise_spends_its_time_on_the_demos() {
        use std::collections::BTreeMap;
        const RUNS: u32 = 20;
        const SECTIONS: [&str; 8] = [
            "keys",
            "value",
            "sites",
            "worklist",
            "solver",
            "relevance",
            "canon",
            "hash",
        ];
        let given = std::env::var_os("BEND_CLONE").expect("BEND_CLONE names the bend clone");
        let clone = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../..")
            .join(given);
        let entries = sweep_entries(&clone);
        assert_eq!(entries.len(), 16, "the sweep's 16 books");
        println!(
            "book | ctxs walked | walks (reused) | value ctxs | value reads (again; queued, from data) | both | on a cycle | plain ms | default ms | probed ms: {}",
            SECTIONS.join(", ")
        );
        let (mut plain_all, mut default_all) = (0.0, 0.0);
        let (mut cold_all, mut cold_relevance, mut cold_relevance_defs) = (0.0, 0.0, 0_u64);
        let mut sum = [Duration::ZERO; 8];
        let mut counts = [0_u64; super::COUNTS];
        let mut by_return: BTreeMap<&str, (u64, usize)> = BTreeMap::new();
        for (name, entry) in &entries {
            let book = Book::open(
                entry.parent().unwrap(),
                Path::new(entry.file_name().unwrap()),
            )
            .expect("opens");
            let file = entry.file_name().unwrap().to_string_lossy().into_owned();
            let ask = |precision: Precision| {
                let options = Options::new(MarkerStyle::above("ID")).with_precision(precision);
                call_graph(
                    &book,
                    Path::new(&file),
                    "main",
                    &options,
                    &Limits::default(),
                    &Cancel::new(),
                )
                .expect("a graph of main")
            };
            let time = |precision: Precision| {
                let mut times: Vec<Duration> = (0..RUNS)
                    .map(|_| {
                        let start = Instant::now();
                        std::hint::black_box(ask(precision));
                        start.elapsed()
                    })
                    .collect();
                times.sort();
                times[times.len() / 2]
            };
            // the first request of a freshly opened book: what is kept with the book (the walks, the relevance) is worked out here, and only here
            reset();
            std::hint::black_box(ask(Precision::Precise));
            let cold = take();
            cold_all += ms(cold.keys
                + cold.value
                + cold.sites
                + cold.worklist
                + cold.solver
                + cold.relevance
                + cold.canon
                + cold.hash);
            cold_relevance += ms(cold.relevance);
            cold_relevance_defs += cold.counts[Count::Relevance as usize];
            let plain = time(Precision::Precise);
            let default = time(Precision::Default);
            // the probed runs: the counts are those of the first, the times the mean
            let cyclic = on_a_cycle(&ask(Precision::Precise));
            let mut first = None;
            let mut spent = [Duration::ZERO; 8];
            for _ in 0..RUNS {
                reset();
                std::hint::black_box(ask(Precision::Precise));
                let report = take();
                let times = [
                    report.keys,
                    report.value,
                    report.sites,
                    report.worklist,
                    report.solver,
                    report.relevance,
                    report.canon,
                    report.hash,
                ];
                for (at, d) in times.into_iter().enumerate() {
                    spent[at] += d;
                }
                first.get_or_insert(report);
            }
            let report = first.expect("a run");
            let mean = |d: Duration| ms(d) / f64::from(RUNS);
            let probed: f64 = spent.iter().map(|d| mean(*d)).sum();
            let each: Vec<String> = spent.iter().map(|d| format!("{:.2}", mean(*d))).collect();
            println!(
                "{name} | {} | {} ({}) | {} | {} ({}; {}, {}) | {} | {cyclic} | {:.2} | {:.2} | {probed:.2}: {}",
                report.site_ctxs,
                report.site_walks,
                report.site_reused,
                report.value_ctxs,
                report.value_reads,
                report.value_rereads,
                report.requeued,
                report.requeued_from_data,
                report.both,
                ms(plain),
                ms(default),
                each.join(", "),
            );
            plain_all += ms(plain);
            default_all += ms(default);
            for (at, d) in spent.iter().enumerate() {
                sum[at] += *d;
            }
            for (at, n) in report.counts.iter().enumerate() {
                counts[at] += n;
            }
            for (def, reads) in &report.value_by_def {
                let entry = by_return.entry(kind_of_return(&book, *def)).or_default();
                entry.0 += reads;
                entry.1 += 1;
            }
        }
        let probed_all: f64 = sum.iter().map(|d| ms(*d) / f64::from(RUNS)).sum();
        let each: Vec<String> = SECTIONS
            .iter()
            .zip(&sum)
            .map(|(name, d)| format!("{name} {:.2}", ms(*d) / f64::from(RUNS)))
            .collect();
        println!(
            "all | plain {plain_all:.2} ms | default {default_all:.2} ms | probed {probed_all:.2} ms: {}",
            each.join(", ")
        );
        println!(
            "first request of each book, cold: {cold_all:.2} ms, of which relevance {cold_relevance:.2} ms for {cold_relevance_defs} defs (the rows above are warm: the median of {RUNS} requests on the same book)"
        );
        println!(
            "counts over the 16 books: keys made {}, closed {}; digests asked {}, made {}; canonical keys written {}",
            counts[Count::KeyMade as usize],
            counts[Count::KeyClosed as usize],
            counts[Count::DigestAsked as usize],
            counts[Count::DigestMade as usize],
            counts[Count::Canon as usize],
        );
        let reads: u64 = by_return.values().map(|(reads, _)| reads).sum();
        for (kind, (n, defs)) in &by_return {
            println!(
                "value reads of defs whose written return type is: {kind}: {n} of {reads} ({:.1}%), {defs} defs",
                *n as f64 * 100.0 / reads as f64
            );
        }
    }
}
