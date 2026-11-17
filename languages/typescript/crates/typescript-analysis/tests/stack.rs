//! H7 (`docs/03-typescript/10-review-fixes.md` §7): a stack overflow is not a panic, it aborts the process, so a driver that overflows on one file is gone for
//! every file. A `+` chain of 10 000 terms and a method chain of 10 000 calls nest 10 000 levels deep in the parser's tree and in every walk over it. `outline` and
//! a call graph of each run **on a thread of the size the driver is told to use** (`DRIVER_STACK`), as a driver runs them, and not on the test harness's own
//! 2 MiB thread. The files are written when the test runs (`CARGO_TARGET_TMPDIR`), not kept.
//!
//! `TS_STACK_KB=<n>` runs the same tests on a thread of that many KiB, to measure the smallest size that passes (an overflow aborts the process, so one
//! size is one process): `cargo test --release -p typescript-analysis --test stack -- <test name>`, from a loop over sizes.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;

use typescript_analysis::{Cancel, Entry, MarkerStyle, Options, Placement, Project, call_graph};
use typescript_syntax::{Dialect, outline};

/// What the driver is told to give every thread that calls an analyzer function. Measured in a release build at `DEPTH` levels (H7): a `+` chain passes at
/// 4 MiB and aborts at 3.5, a method chain passes at 8 MiB and aborts at 7. Four times the larger is 32 MiB: reserved address space, not memory used, until the
/// stack grows (tsrs gives each of its own threads 512 MiB the same way).
const DRIVER_STACK: usize = 32 * 1024 * 1024;

/// Levels of nesting: what `ENGINE.md` measured aborting at 2 MiB.
const DEPTH: usize = 10_000;

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

fn stack_size() -> usize {
    std::env::var("TS_STACK_KB")
        .ok()
        .and_then(|kb| kb.parse::<usize>().ok())
        .map_or(DRIVER_STACK, |kb| kb * 1024)
}

/// Runs `work` on a thread with the driver's stack, and gives back what it returned. A panic in it is the test's failure; an overflow ends the process.
fn on_the_drivers_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    thread::Builder::new()
        .stack_size(stack_size())
        .spawn(work)
        .unwrap()
        .join()
        .unwrap()
}

/// The project folder, written once: `src/plus.ts` (`sum`: `f() + f() + …`, `DEPTH` terms) and `src/chain.ts` (`chain`: `new B().m().m()…`, `DEPTH` calls).
fn root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("deep-nesting");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("tsconfig.json"),
            r#"{ "compilerOptions": { "strict": true }, "include": ["src"] }"#,
        )
        .unwrap();
        let terms = vec!["  f()"; DEPTH].join(" +\n");
        std::fs::write(
            root.join("src/plus.ts"),
            format!("function f(): number {{\n  return 1;\n}}\nexport function sum(): number {{\n  return\n{terms};\n}}\n"),
        )
        .unwrap();
        let calls = vec!["    .m()"; DEPTH].join("\n");
        std::fs::write(
            root.join("src/chain.ts"),
            format!("class B {{\n  m(): B {{\n    return this;\n  }}\n}}\nexport function chain(): B {{\n  return new B()\n{calls};\n}}\n"),
        )
        .unwrap();
        root
    })
}

fn outline_of(file: &'static str) -> usize {
    let text = std::fs::read_to_string(root().join(file)).unwrap();
    on_the_drivers_thread(move || {
        outline(&text, Dialect::from_path(file).unwrap(), STYLE)
            .defs
            .len()
    })
}

fn edges_of(file: &'static str, entry: &'static str) -> usize {
    on_the_drivers_thread(move || {
        let project = Project::open(root(), Path::new(file)).unwrap();
        let entry = Entry::Def {
            qualname: entry,
            occurrence: 0,
        };
        call_graph(&project, file, &entry, &Options::new(STYLE), &Cancel::new())
            .expect("the entry is there")
            .edges
            .len()
    })
}

#[test]
fn the_outline_of_a_long_plus_chain() {
    // `f` and `sum`
    assert_eq!(outline_of("src/plus.ts"), 2);
}

#[test]
fn the_outline_of_a_long_method_chain() {
    // the class, its method, `chain`
    assert_eq!(outline_of("src/chain.ts"), 3);
}

#[test]
fn the_call_graph_of_a_long_plus_chain() {
    // one call of `f` for each term
    assert_eq!(edges_of("src/plus.ts", "sum"), DEPTH);
}

#[test]
fn the_call_graph_of_a_long_method_chain() {
    // the construction of `B`, and one call of `m` for each link
    assert_eq!(edges_of("src/chain.ts", "chain"), DEPTH + 1);
}
