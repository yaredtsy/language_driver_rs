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
}

#[cfg(not(test))]
pub(crate) use dead::{requeued, section, site_walk, value_read};
#[cfg(test)]
pub(crate) use live::{requeued, section, site_walk, value_read};

#[cfg(not(test))]
mod dead {
    use super::Section;
    use crate::book::DeclId;
    use crate::key::Key;

    pub(crate) struct Guard;

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
}

#[cfg(test)]
mod live {
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    use super::Section;
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
        pub(crate) site_ctxs: usize,
        /// The contexts that were read for their value and walked: what a reading that gave both would save.
        pub(crate) both: usize,
        /// Exclusive times: what is left, outside every section, is the solver's.
        pub(crate) solver: Duration,
        pub(crate) keys: Duration,
        pub(crate) value: Duration,
        pub(crate) sites: Duration,
        pub(crate) worklist: Duration,
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

    /// A body was read for its value; `again` when its context had been read before.
    pub(crate) fn value_read(def: DeclId, key: &Key, again: bool) {
        STATE.with_borrow_mut(|state| {
            if state.on {
                state.report.value_reads += 1;
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

    fn ms(d: Duration) -> f64 {
        d.as_secs_f64() * 1000.0
    }

    /// V10a: per book of the sweep, under `Precise`, how many times each body is read and where the time goes:
    /// `BEND_CLONE=../language_clone/bend cargo test -p bend-analysis --release --lib probe -- --ignored --nocapture --test-threads=1`.
    #[test]
    #[ignore = "needs BEND_CLONE; run with --release, one at a time"]
    fn where_precise_spends_its_time_on_the_demos() {
        const RUNS: u32 = 20;
        let given = std::env::var_os("BEND_CLONE").expect("BEND_CLONE names the bend clone");
        let clone = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../..")
            .join(given);
        let entries = sweep_entries(&clone);
        assert_eq!(entries.len(), 16, "the sweep's 16 books");
        println!(
            "book | ctxs walked | walks | value ctxs | value reads (again; queued, from data) | both | plain ms | default ms | probed ms: keys, value, sites, worklist, solver"
        );
        let (mut plain_all, mut default_all) = (0.0, 0.0);
        let mut sum = [Duration::ZERO; 6];
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
            let plain = time(Precision::Precise);
            let default = time(Precision::Default);
            // the probed runs: the counts are those of the first, the times the mean
            let mut first = None;
            let mut spent = [Duration::ZERO; 5];
            for _ in 0..RUNS {
                reset();
                std::hint::black_box(ask(Precision::Precise));
                let report = take();
                spent[0] += report.keys;
                spent[1] += report.value;
                spent[2] += report.sites;
                spent[3] += report.worklist;
                spent[4] += report.solver;
                first.get_or_insert(report);
            }
            let report = first.expect("a run");
            let mean = |d: Duration| ms(d) / f64::from(RUNS);
            let probed: f64 = spent.iter().map(|d| mean(*d)).sum();
            println!(
                "{name} | {} | {} | {} | {} ({}; {}, {}) | {} | {:.2} | {:.2} | {probed:.2}: {:.2}, {:.2}, {:.2}, {:.2}, {:.2}",
                report.site_ctxs,
                report.site_walks,
                report.value_ctxs,
                report.value_reads,
                report.value_rereads,
                report.requeued,
                report.requeued_from_data,
                report.both,
                ms(plain),
                ms(default),
                mean(spent[0]),
                mean(spent[1]),
                mean(spent[2]),
                mean(spent[3]),
                mean(spent[4]),
            );
            plain_all += ms(plain);
            default_all += ms(default);
            for (at, d) in spent.iter().enumerate() {
                sum[at] += *d;
            }
        }
        let probed_all: f64 = sum.iter().take(5).map(|d| ms(*d) / f64::from(RUNS)).sum();
        println!(
            "all | plain {plain_all:.2} ms | default {default_all:.2} ms | probed {probed_all:.2} ms: keys {:.2}, value {:.2}, sites {:.2}, worklist {:.2}, solver {:.2}",
            ms(sum[0]) / f64::from(RUNS),
            ms(sum[1]) / f64::from(RUNS),
            ms(sum[2]) / f64::from(RUNS),
            ms(sum[3]) / f64::from(RUNS),
            ms(sum[4]) / f64::from(RUNS),
        );
    }
}
