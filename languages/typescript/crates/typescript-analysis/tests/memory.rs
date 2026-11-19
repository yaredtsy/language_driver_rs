//! A long-running process must not grow: opening a project and dropping it has to give the memory back. The engine's `Program` and its
//! processed files are `Box::leak`ed outside any region, so only `free_unshared_program` (called by `Sem::drop`) returns them. Checked by
//! hand: with that call removed, RSS grows from 20 MB to 349 MB over 60 opens and this test fails; with it, 19 MB at round 10 and 24 MB at 60.
//! Early is round 10, and the peak is read over the later rounds, not the last value: the OS takes freed pages back whenever it likes.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use typescript_analysis::{
    Budget, Cancel, Entry, MarkerStyle, Options, Placement, Project, Session, call_graph,
};

/// Rounds of the graph-building loop: enough to see a leak of a megabyte a round, few enough to stay a test.
const ROUNDS: usize = 100;

/// Both tests read the **process's** memory, so one running beside the other would count the other's growth as its own (H2). Each takes this for its whole
/// run. A test that failed holding it poisons it, and the next one goes on: the lock is for taking turns, not for a value.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn one_at_a_time() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

fn rss_mb() -> f64 {
    let usage = memory_stats::memory_stats().expect("this platform reports memory");
    usage.physical_mem as f64 / (1024.0 * 1024.0)
}

#[test]
fn opening_and_dropping_a_project_100_times_does_not_grow_the_process() {
    let _turn = one_at_a_time();
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

/// Graphs built inside a round: every kind of site the walk asks the checker about (calls through a union, a JSX tag, a decorator, an accessor, a `new`), on two
/// projects, so a checker, a region or a handle that a query keeps alive shows as growth.
fn build_graphs(root: &Path, file: &str, entries: &[&str]) -> usize {
    let project = Project::open(root, Path::new(file)).unwrap();
    let mut edges = 0;
    for entry in entries {
        let graph = call_graph(
            &project,
            file,
            &Entry::Def {
                qualname: entry,
                occurrence: 0,
            },
            &Options::new(MarkerStyle {
                label: "ID",
                placement: Placement::Docstring,
            }),
            &Cancel::new(),
        )
        .unwrap_or_else(|| panic!("no def `{entry}` in {file}"));
        edges += graph.edges.len();
    }
    edges
}

#[test]
fn opening_a_project_building_graphs_and_dropping_it_does_not_grow_the_process() {
    let _turn = one_at_a_time();
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let resolve = tests.join("resolve");
    let kinds = tests.join("site-kinds");
    let mut early = 0.0;
    let mut peak: f64 = 0.0;
    let mut edges_first = 0;
    for round in 1..=ROUNDS {
        let edges = build_graphs(&resolve, "src/main.ts", &["callers", "optional_callbacks"])
            + build_graphs(&kinds, "src/decorators.ts", &["Decorated"])
            + build_graphs(&kinds, "src/tagged.ts", &["tagged"])
            + build_graphs(&kinds, "src/jsx.tsx", &["components"])
            + build_graphs(&kinds, "src/accessors.ts", &["compound", "reads"]);
        // the same work every round, and some of it: a loop that built nothing would pass
        if round == 1 {
            edges_first = edges;
            assert!(edges >= 50, "{edges}");
        }
        assert_eq!(edges, edges_first, "round {round}");
        if round == 10 {
            early = rss_mb();
        } else if round > 10 {
            peak = peak.max(rss_mb());
        }
    }
    assert!(
        peak - early < 30.0,
        "memory grew from {early:.1} MB (round 10) to a peak of {peak:.1} MB (rounds 11 to {ROUNDS})"
    );
}

/// A file rewritten, a `refresh` that builds the program again, and a graph, 50 times on a project the test writes. Each rewrite changes the file's length
/// (no round rests on the clock's resolution), and the callee alternates, so a round that did not read the new text shows. A program the session kept after a
/// rebuild would show as growth.
#[test]
fn rewriting_a_file_refreshing_and_building_a_graph_50_times_does_not_grow_the_process() {
    let _turn = one_at_a_time();
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("memory-refresh");
    std::fs::create_dir_all(root.join("src")).unwrap();
    let write = |relative: &str, text: &str| std::fs::write(root.join(relative), text).unwrap();
    write(
        "tsconfig.json",
        r#"{ "compilerOptions": { "strict": true }, "include": ["src"] }"#,
    );
    write(
        "src/main.ts",
        "import { callee } from \"./lib\";\nexport function main() {\n  callee();\n}\n",
    );
    // `lib.ts` of round `n`: `callee` calls `second` on an odd round and `first` on an even one, and the file is `2n` characters longer than the text it
    // has before, so no two rounds have the same length
    let lib = |round: usize| {
        let calls = if round % 2 == 1 { "second" } else { "first" };
        format!(
            "export function callee() {{\n  {calls}();\n}}\nexport function first() {{}}\nexport function second() {{}}\n// {}\n",
            "x".repeat(2 * round)
        )
    };
    write("src/lib.ts", &lib(0));
    let mut session = Session::open(
        &root,
        Path::new("src/main.ts"),
        Options::new(MarkerStyle {
            label: "ID",
            placement: Placement::Docstring,
        }),
    )
    .unwrap();

    let mut early = 0.0;
    let mut peak: f64 = 0.0;
    for round in 1..=50 {
        write("src/lib.ts", &lib(round));
        let refreshed = session.refresh();
        assert!(refreshed.reopened, "round {round}: the file was rewritten");
        assert_eq!(refreshed.lost, None, "round {round}");
        let entry = Entry::Def {
            qualname: "main",
            occurrence: 0,
        };
        let graph = session
            .graph("src/main.ts", &entry, &Budget::default(), &Cancel::new())
            .unwrap_or_else(|| panic!("round {round}: no graph"));
        let callee: Vec<&str> = graph
            .defs
            .iter()
            .map(|def| def.qname.as_str())
            .filter(|name| *name == "first" || *name == "second")
            .collect();
        let wanted = if round % 2 == 1 { "second" } else { "first" };
        assert_eq!(
            callee,
            [wanted],
            "round {round}: the graph must follow the text"
        );
        if round == 10 {
            early = rss_mb();
        } else if round > 10 {
            peak = peak.max(rss_mb());
        }
    }
    assert!(
        peak - early < 30.0,
        "memory grew from {early:.1} MB (round 10) to a peak of {peak:.1} MB (rounds 11 to 50)"
    );
}
