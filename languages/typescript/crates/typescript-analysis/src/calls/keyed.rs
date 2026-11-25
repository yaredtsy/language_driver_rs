//! The key a call passes (`docs/03-typescript/12-context-layer/06-tc2-functions-through-parameters.md` §5), proved in the crate. `Precision::Precise` turns the
//! layer on with `Caps::BUILT` (`PARAMS`); the tests here turn it on with `with_layer(Some(Caps::PARAMS))` so they name the bit they prove. Since step 5a
//! the layer draws: where the engine says "a function-typed value" and the key says which functions, the site calls each of them. Every other edge is the
//! engine's, and the contexts split by key.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use analyzer_core::graph::{ContextId, EdgeTo, Graph};
use analyzer_core::solve::{Budget, Cancel, Lang, Limits, Solver, Target};

use super::*;
use crate::captures::VALUE_WIDENED_DEPTH;
use crate::env::{
    MODULE_READS_MODULE, VARIABLE_ADDED_TO_ENGINE, VARIABLE_ASSIGNED_MORE_THAN_ONCE,
    VARIABLE_ROUNDS_CAPPED, VARIABLE_WRITE_UNMODELLED,
};
use crate::value::Value;

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: analyzer_core::edit::Placement::Docstring,
};

const ZEROS: [u8; 16] = [0; 16];
const A: [u8; 16] = [
    93, 46, 87, 96, 216, 61, 155, 17, 211, 86, 34, 118, 74, 224, 161, 128,
];
const B: [u8; 16] = [
    172, 21, 36, 62, 234, 108, 51, 149, 129, 38, 137, 144, 112, 95, 31, 68,
];
const HANDLER: [u8; 16] = [
    63, 169, 7, 244, 195, 117, 63, 232, 189, 253, 184, 232, 224, 101, 115, 200,
];
const OTHER: [u8; 16] = [
    182, 165, 153, 246, 90, 12, 30, 220, 138, 89, 219, 215, 130, 124, 131, 66,
];
const UNKNOWN_A: [u8; 16] = [
    64, 110, 214, 97, 245, 134, 207, 177, 242, 253, 50, 72, 57, 215, 243, 92,
];
const ANON: [u8; 16] = [
    227, 59, 111, 114, 81, 178, 31, 219, 156, 77, 31, 52, 186, 66, 183, 107,
];
const UNDEFINED: [u8; 16] = [
    96, 15, 54, 81, 114, 34, 137, 20, 173, 89, 117, 222, 99, 2, 190, 64,
];
const A_OR_B: [u8; 16] = [
    55, 225, 1, 109, 241, 243, 47, 117, 120, 129, 24, 217, 191, 92, 235, 3,
];

fn open(fixture: &str) -> Sem {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture);
    Sem::open(&root, Path::new("src/main.ts")).unwrap()
}

/// What a measured solve asks and keeps (`06-tc2` §10, step 4b). Test-only.
#[derive(Default)]
pub(super) struct Measure {
    /// Every key data `key_of` made, with its callee, after the mask and before interning (so anonymous functions are still told apart).
    pub(super) bound: Vec<(DefId, KeyData)>,
}

impl TypeScript<'_> {
    /// The test-only hook of `walk::key_of`: records `data`, what `bind` made (relevance already applied), and returns it.
    pub(crate) fn measured(&self, callee: DefId, data: KeyData) -> KeyData {
        self.measure.borrow_mut().bound.push((callee, data.clone()));
        data
    }
}

/// The graph under `main`, with the layer as `layer` says.
fn solve<'p>(sem: &'p Sem, layer: Option<Caps>) -> (Solver<TypeScript<'p>>, Graph) {
    let main = sem.entry("src/main.ts", "main", 0, STYLE).unwrap();
    solve_from(sem, main, layer)
}

/// The graph from `func`, with the layer as `layer` says; the solver keeps what it walked and bound.
fn solve_from<'p>(
    sem: &'p Sem,
    func: Func<'p>,
    layer: Option<Caps>,
) -> (Solver<TypeScript<'p>>, Graph) {
    let (solver, graph, _) = solve_timed(sem, func, layer);
    (solver, graph)
}

/// `solve_from`, and how long `graph_within` alone took.
fn solve_timed<'p>(
    sem: &'p Sem,
    func: Func<'p>,
    layer: Option<Caps>,
) -> (Solver<TypeScript<'p>>, Graph, Duration) {
    let lang = TypeScript::new(sem, STYLE).with_layer(layer);
    let start = start_of(&lang, func);
    let mut solver = Solver::new(lang, Limits::default());
    let began = Instant::now();
    let graph = solver.graph_within(&[start], &Budget::default(), &Cancel::new());
    (solver, graph, began.elapsed())
}

fn on() -> Option<Caps> {
    Some(Caps::PARAMS)
}

/// Each def's qualname and the sorted digests of its contexts.
fn contexts(graph: &Graph) -> BTreeMap<String, Vec<[u8; 16]>> {
    let mut found: BTreeMap<String, Vec<[u8; 16]>> = BTreeMap::new();
    for context in &graph.contexts {
        let name = graph.def(&context.id.func).unwrap().qname.clone();
        found.entry(name).or_default().push(context.id.digest);
    }
    for digests in found.values_mut() {
        digests.sort();
    }
    found
}

/// What `contexts` should say, from `(qname, digests)` pairs written as in the design (each list sorted here, as arrays).
fn expected(pairs: &[(&str, &[[u8; 16]])], zeros_for: &[&str]) -> BTreeMap<String, Vec<[u8; 16]>> {
    let mut found: BTreeMap<String, Vec<[u8; 16]>> = BTreeMap::new();
    for (name, digests) in pairs {
        let mut digests = digests.to_vec();
        digests.sort();
        found.insert((*name).to_owned(), digests);
    }
    for name in zeros_for {
        found.insert((*name).to_owned(), vec![ZEROS]);
    }
    found
}

/// The edges of a graph as `from | callee text | target`, digests and keys set aside, once each: what a reader of a fixture sees. The callee text is read
/// from the fixture's file by the site's range, relative to the def (the way `common::edges_as_text` does for the integration tests).
fn readable(fixture: &str, graph: &Graph) -> BTreeSet<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture);
    graph
        .edges
        .iter()
        .map(|edge| {
            let from = graph.def(&edge.from.func).unwrap();
            let text = std::fs::read_to_string(root.join(&from.file)).unwrap();
            let callee = &text[(from.range.start + edge.site.start) as usize
                ..(from.range.start + edge.site.end) as usize];
            let name = |id: &analyzer_core::graph::StableDef| graph.def(id).unwrap().qname.clone();
            let to = match &edge.to {
                EdgeTo::Context(id, _) if edge.passed => format!("pass {}", name(&id.func)),
                EdgeTo::Context(id, _) => format!("call {}", name(&id.func)),
                EdgeTo::Construct { class, init, .. } => format!(
                    "construct {} {}",
                    name(class),
                    init.as_ref()
                        .map_or("no init".to_owned(), |id| name(&id.func))
                ),
                EdgeTo::External(external, _) => {
                    format!("external {} {}", external.package, external.name)
                }
                EdgeTo::Unresolved(reason) => format!("unresolved {}", reason.as_str()),
                other => panic!("an edge this test does not know: {other:?}"),
            };
            format!("{} | {callee} | {to}", name(&edge.from.func))
        })
        .collect()
}

/// `base`, less the lines of `removed` (each must be there) and plus the lines of `added`: the graph with the layer on, written from the graph with it off.
fn changed(base: BTreeSet<String>, removed: &[&str], added: &[&str]) -> BTreeSet<String> {
    let mut found = base;
    for line in removed {
        assert!(
            found.remove(*line),
            "the layer-off graph has no edge `{line}`"
        );
    }
    for line in added {
        assert!(
            found.insert((*line).to_owned()),
            "`{line}` was already there"
        );
    }
    found
}

#[test]
fn two_callers_make_two_contexts_and_each_draws_its_own_callback() {
    let fixture = "tests/fixtures/context/callback-two-callers";
    let sem = open(fixture);
    let (_, with) = solve(&sem, on());
    let (_, without) = solve(&sem, None);
    assert_eq!(
        contexts(&with),
        expected(
            &[("run", &[A, B])],
            &["main", "first", "second", "again", "a", "b"]
        )
    );
    assert_eq!(with.contexts.len(), 8);
    assert_eq!(without.contexts.len(), 7);
    // the one open edge of the graph is the call of `cb`, which is `a` under one key and `b` under the other
    assert_eq!(
        readable(fixture, &with),
        changed(
            readable(fixture, &without),
            &["run | cb | unresolved function_typed_value"],
            &["run | cb | call a", "run | cb | call b"]
        )
    );
    // and each context has its own: `run` under `a` calls `a` only
    for (digest, callee) in [(A, "a"), (B, "b")] {
        let calls: Vec<String> = with
            .edges
            .iter()
            .filter(|edge| {
                edge.from.digest == digest && with.def(&edge.from.func).unwrap().qname == "run"
            })
            .map(|edge| written_edge(&edge.to))
            .collect();
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert!(calls[0].starts_with("call"), "{calls:?}");
        assert!(
            calls[0].contains(&format!("qname: \"{callee}\"")),
            "{calls:?}"
        );
    }
}

#[test]
fn a_key_passes_through_a_parameter() {
    let fixture = "tests/fixtures/context/callback-passed-through";
    let sem = open(fixture);
    let (_, with) = solve(&sem, on());
    let (_, without) = solve(&sem, None);
    assert_eq!(
        contexts(&with),
        expected(
            &[("relay", &[HANDLER, OTHER]), ("each", &[HANDLER, OTHER])],
            &["main", "direct", "handler", "other"]
        )
    );
    assert_eq!(
        readable(fixture, &with),
        changed(
            readable(fixture, &without),
            &["each | f | unresolved function_typed_value"],
            &["each | f | call handler", "each | f | call other"]
        )
    );
}

/// What each context of the `keys` fixture should be, with `ignore`'s as `ignore_digests` says: the one thing relevance changes.
fn kinds(
    found: &BTreeMap<String, Vec<[u8; 16]>>,
    ignore_digests: &[[u8; 16]],
) -> BTreeMap<String, Vec<[u8; 16]>> {
    let mut want = expected(
        &[
            // the spread, the conditional, the two arrows sharing one
            ("run", &[ZEROS, A_OR_B, ANON]),
            ("opt", &[UNDEFINED]),
            ("withDefault", &[UNDEFINED]),
            // a rest parameter
            ("many", &[ZEROS]),
            // a `string` first
            ("second", &[UNKNOWN_A]),
            // a tagged template passes nothing
            ("tagged", &[ZEROS]),
            // `new Box(a)` and `Pair`'s `super(cb)` share `A`; `new Sub(b)` runs the inherited constructor with `B`
            ("Box.constructor", &[A, B]),
            ("Pair.constructor", &[UNKNOWN_A]),
            ("ignore", ignore_digests),
        ],
        &[],
    );
    for name in found.keys() {
        want.entry(name.clone()).or_insert_with(|| vec![ZEROS]);
    }
    want
}

#[test]
fn each_kind_of_argument_makes_its_key() {
    let fixture = "tests/fixtures/keys";
    let sem = open(fixture);
    let (_, with) = solve(&sem, on());
    let (_, without) = solve(&sem, None);
    let found = contexts(&with);
    // a parameter nothing reads still splits, until relevance (step 8)
    assert_eq!(found, kinds(&found, &[A, B]));
    // what the key says about each parameter that is called, written by hand (the edges are a set, so a site under two keys is one line):
    //  - `run`: `a` or `b` (`either`), an arrow (`arrowOne`, `arrowTwo`), and the open key of the spread, which keeps the engine's answer
    //  - `Box.constructor`: `a` (`new Box(a)`, and `Pair`'s `super(cb)`) and `b` (`new Sub(b)`)
    //  - `withDefault`: the default, `a`, for the call that passes nothing
    //  - `second`: `a`, the second parameter, after a `string`
    //  - `opt`: nothing was passed, so there is no callee and no edge
    //  - `many` (a rest parameter) and `tagged` (a template passes no arguments) keep the engine's answer
    assert_eq!(
        readable(fixture, &with),
        changed(
            readable(fixture, &without),
            &[
                "Box.constructor | cb | unresolved function_typed_value",
                "opt | cb | unresolved function_typed_value",
                "second | cb | unresolved function_typed_value",
                "withDefault | cb | unresolved function_typed_value",
            ],
            &[
                "Box.constructor | cb | call a",
                "Box.constructor | cb | call b",
                "run | cb | call a",
                "run | cb | call b",
                "run | cb | unresolved anonymous_function_called",
                "second | cb | call a",
                "withDefault | cb | call a",
            ]
        )
    );
}

/// Every entry of every row of the context table, and the `keys` fixture from its `main`: `(fixture folder, file, entry)`.
fn every_entry() -> Vec<(String, String, String)> {
    let mut entries = context_entries();
    entries.push((
        "tests/fixtures/keys".to_owned(),
        "src/main.ts".to_owned(),
        "main".to_owned(),
    ));
    entries
}

#[test]
fn the_bit_off_is_the_layer_that_knows_nothing() {
    for (fixture, file, entry) in every_entry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(&fixture);
        let sem = Sem::open(&root, Path::new(&file)).unwrap();
        let start = sem
            .entry(&file, &entry, 0, STYLE)
            .unwrap_or_else(|| panic!("{fixture}: no entry `{entry}`"));
        let (_, none) = solve_from(&sem, start, Some(Caps::empty()));
        let (_, off) = solve_from(&sem, start, None);
        assert_eq!(none, off, "{fixture} {entry}");
    }
}

/// With `LOCALS` off the layer is TC2 exactly (`Caps::PARAMS`): every entry of every row, written out whole, in `snapshots/context/locals-off.txt`. Blessed
/// before TC3 step 2 changed any code and never changed since: the proof that the bit off is the layer of TC2.
#[test]
fn the_locals_bit_off_is_tc2() {
    let mut text = String::new();
    for (fixture, file, entry) in every_entry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(&fixture);
        let sem = Sem::open(&root, Path::new(&file)).unwrap();
        let start = sem
            .entry(&file, &entry, 0, STYLE)
            .unwrap_or_else(|| panic!("{fixture}: no entry `{entry}`"));
        let (_, graph) = solve_from(&sem, start, Some(Caps::PARAMS));
        text.push_str(&format!("== {fixture} {entry}\n{graph:?}\n"));
    }
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/context/locals-off.txt"),
        &text,
    );
}

/// With `CAPTURES` off the layer is step 2 exactly (`Caps::PARAMS | Caps::LOCALS`): every entry of every row, written out whole, in
/// `snapshots/context/captures-off.txt`. Blessed before TC3 step 3 part 2 changed any code and never changed since: the proof that the bit off is the layer of
/// step 2.
#[test]
fn the_captures_bit_off_is_step_2() {
    let mut text = String::new();
    for (fixture, file, entry) in every_entry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(&fixture);
        let sem = Sem::open(&root, Path::new(&file)).unwrap();
        let start = sem
            .entry(&file, &entry, 0, STYLE)
            .unwrap_or_else(|| panic!("{fixture}: no entry `{entry}`"));
        let (_, graph) = solve_from(&sem, start, Some(Caps::PARAMS | Caps::LOCALS));
        text.push_str(&format!("== {fixture} {entry}\n{graph:?}\n"));
    }
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/context/captures-off.txt"),
        &text,
    );
}

/// With `MODULES` off the layer is step 3 exactly (`Caps::PARAMS | Caps::LOCALS | Caps::CAPTURES`): every entry of every row, written out whole, in
/// `snapshots/context/modules-off.txt`. Blessed before TC3 step 4 changed any code and never changed since: the proof that the bit off is the layer of
/// step 3.
#[test]
fn the_modules_bit_off_is_step_3() {
    let mut text = String::new();
    for (fixture, file, entry) in every_entry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(&fixture);
        let sem = Sem::open(&root, Path::new(&file)).unwrap();
        let start = sem
            .entry(&file, &entry, 0, STYLE)
            .unwrap_or_else(|| panic!("{fixture}: no entry `{entry}`"));
        let (_, graph) = solve_from(
            &sem,
            start,
            Some(Caps::PARAMS | Caps::LOCALS | Caps::CAPTURES),
        );
        text.push_str(&format!("== {fixture} {entry}\n{graph:?}\n"));
    }
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/context/modules-off.txt"),
        &text,
    );
}

/// With `RELEVANCE` off the layer is step 5 exactly (`Caps::BUILT` without `RELEVANCE`): every entry of every row, written out whole, in
/// `snapshots/context/relevance-off.txt`. Blessed before TC3 step 6 changed any code and never changed since: the proof that the bit off is the layer of
/// step 5.
#[test]
fn the_relevance_bit_off_is_step_5() {
    let mut text = String::new();
    for (fixture, file, entry) in every_entry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(&fixture);
        let sem = Sem::open(&root, Path::new(&file)).unwrap();
        let start = sem
            .entry(&file, &entry, 0, STYLE)
            .unwrap_or_else(|| panic!("{fixture}: no entry `{entry}`"));
        let (_, graph) = solve_from(&sem, start, Some(Caps::BUILT - Caps::RELEVANCE));
        text.push_str(&format!("== {fixture} {entry}\n{graph:?}\n"));
    }
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/context/relevance-off.txt"),
        &text,
    );
}

/// Relevance moves no edge (`07-tc3`, "Relevance"): for every entry of every row, the edges with `RELEVANCE` on and off, each as (from def, site text, call /
/// pass / construct / external / unresolved target) without digests, are the same set, and the bit never makes more contexts. Only contexts merge.
#[test]
fn relevance_changes_no_edge() {
    let (mut fewer, mut same) = (0, 0);
    for (fixture, file, entry) in every_entry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(&fixture);
        let sem = Sem::open(&root, Path::new(&file)).unwrap();
        let start = sem
            .entry(&file, &entry, 0, STYLE)
            .unwrap_or_else(|| panic!("{fixture}: no entry `{entry}`"));
        let (_, without) = solve_from(&sem, start, Some(Caps::BUILT - Caps::RELEVANCE));
        let (_, with) = solve_from(&sem, start, Some(Caps::BUILT));
        assert_eq!(
            readable(&fixture, &with),
            readable(&fixture, &without),
            "{fixture} {entry}: the edges"
        );
        assert!(
            with.contexts.len() <= without.contexts.len(),
            "{fixture} {entry}: {} contexts with the bit, {} without",
            with.contexts.len(),
            without.contexts.len()
        );
        if with.contexts.len() < without.contexts.len() {
            fewer += 1;
        } else {
            same += 1;
        }
    }
    eprintln!("{fewer} entries with fewer contexts, {same} with as many");
    assert!(fewer > 0, "the bit changed nothing anywhere");
}

/// The counters of the variables (`07-tc3` §3), per entry of every row, `(added to the engine, assigned more than once)`; an entry not listed counts 0 of
/// each, and a counter that is 0 is absent from the notes. Also: a write the table has no row for is counted once by the body that has it
/// (`local-write-unmodelled`'s `main`), and a body whose variables did not settle within the rounds' cap is counted too (no row of the table has one). With
/// `LOCALS` off nothing is counted at all. `value_widened_depth` (TC3 step 3) is counted where a closure nests past the depth limit: `capture-self`'s `main`
/// only, and with `CAPTURES` off nowhere. `module_reads_module` (TC3 step 4) is 1 on `module-reads-module`'s `main`, and with `MODULES` off nowhere.
#[test]
fn the_variable_counters_count_where_the_table_says() {
    for (fixture, file, entry) in every_entry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(&fixture);
        let sem = Sem::open(&root, Path::new(&file)).unwrap();
        let start = sem
            .entry(&file, &entry, 0, STYLE)
            .unwrap_or_else(|| panic!("{fixture}: no entry `{entry}`"));
        let unmodelled =
            (fixture.ends_with("/local-write-unmodelled") && entry == "main").then_some(1);
        // what the engine named and the layer added to: `h()` under `const h = flag ? a : b`, and both calls of `let h = a; h(); h = b; h();`
        let (added, more_than_once) = match (fixture.rsplit('/').next().unwrap(), entry.as_str()) {
            ("local-either", "main") => (Some(1), None),
            ("local-assigned-twice", "main") => (Some(2), Some(2)),
            // `cb()` in the one context of `run` that `main` reaches, and in `run` itself: what was passed and one assignment
            ("callback-reassigned", "main" | "run") => (None, Some(1)),
            // `run(cb: typeof a)` called with `b`: the engine names `a`, the key holds `b`, so `main` adds `b`; asked at `run` the key is open and nothing is added
            ("callback-named-by-type", "main") => (Some(1), None),
            // TC3 step 4: `onSave()` under `const onSave = pick ? a : b` at the top of the file: the engine names `a`, the layer adds `b`. The others
            // (`module-let`, `-script`, `-const-imported`, `-reads-module`) have the engine's unresolved edge replaced, or kept, and nothing added
            ("module-const", "main") => (Some(1), None),
            _ => (None, None),
        };
        for (layer, unmodelled, added, more_than_once) in [
            (Caps::BUILT, unmodelled, added, more_than_once),
            (Caps::PARAMS, None, None, None),
        ] {
            let (solver, _) = solve_from(&sem, start, Some(layer));
            let said = |key: &str| solver.lang().notes.borrow().get(key).copied();
            let at = format!("{fixture} {entry} {layer:?}");
            assert_eq!(said(VARIABLE_WRITE_UNMODELLED), unmodelled, "{at}");
            assert_eq!(said(VARIABLE_ROUNDS_CAPPED), None, "{at}");
            // 8 evaluations of `next` past the limit, as TC3 step 3 counted; with `RELEVANCE` 6: `later`'s parameter reaches no call (it has no body), so the
            // argument `later(next)` is not evaluated for the key, and the two evaluations it made are not made
            let widened_depth = (layer.contains(Caps::CAPTURES)
                && fixture.ends_with("/capture-self")
                && entry == "main")
                .then_some(if layer.contains(Caps::RELEVANCE) {
                    6
                } else {
                    8
                });
            assert_eq!(said(VALUE_WIDENED_DEPTH), widened_depth, "{at}");
            assert_eq!(said(VARIABLE_ADDED_TO_ENGINE), added, "{at}");
            // TC3 step 4: `first`'s value reads `second`, a variable of another file, once, in the table of `one.ts`; with `MODULES` off nothing reads it
            let reads_module = (layer.contains(Caps::MODULES)
                && fixture.ends_with("/module-reads-module")
                && entry == "main")
                .then_some(1);
            assert_eq!(said(MODULE_READS_MODULE), reads_module, "{at}");
            assert_eq!(
                said(VARIABLE_ASSIGNED_MORE_THAN_ONCE),
                more_than_once,
                "{at}"
            );
        }
    }
}

/// A target of the graph, written so that a graph's and a summary's can be compared.
fn written_edge(to: &EdgeTo) -> String {
    match to {
        EdgeTo::Context(id, _) => format!("call {:?} {:?}", id.func, id.digest),
        EdgeTo::Construct { class, init, .. } => format!(
            "construct {class:?} {:?}",
            init.as_ref().map(|id| (&id.func, id.digest))
        ),
        EdgeTo::External(external, _) => {
            format!("external {} {}", external.package, external.name)
        }
        EdgeTo::Unresolved(reason) => format!("unresolved {}", reason.as_str()),
        other => panic!("an edge this test does not know: {other:?}"),
    }
}

/// The context table of `docs/policy.md` ("With the context layer"), as `(fixture folder, file, entry)` for every entry of every row: the words in backticks
/// of the last cell of a row that names a fixture under `fixtures/context/`.
fn context_entries() -> Vec<(String, String, String)> {
    const TABLE: &str = include_str!("../../../../docs/policy.md");
    let mut found = Vec::new();
    for line in TABLE
        .lines()
        .filter(|line| line.contains("`fixtures/context/"))
    {
        let cell = line
            .trim_end()
            .trim_end_matches('|')
            .rsplit('|')
            .next()
            .unwrap_or("");
        let words: Vec<&str> = cell.split('`').skip(1).step_by(2).collect();
        assert!(words.len() > 2, "a row with no entry: {line}");
        for entry in &words[2..] {
            found.push((
                format!("tests/{}", words[0]),
                words[1].to_owned(),
                (*entry).to_owned(),
            ));
        }
    }
    found
}

#[test]
fn the_table_of_rows_is_read_whole() {
    let entries = context_entries();
    let of = |folder: &str| entries.iter().filter(|(f, ..)| f.ends_with(folder)).count();
    // `tour`: `main` and `lineA` to `lineI`; the rest as the table lists them
    assert_eq!(of("/tour"), 10);
    assert_eq!(of("callback-two-callers"), 4);
    assert_eq!(of("callback-passed-through"), 3);
    assert_eq!(of("callback-either"), 2);
    assert_eq!(of("callback-not-passed"), 2);
    assert_eq!(of("callback-reassigned"), 2);
    assert_eq!(of("callback-data"), 2);
    assert_eq!(of("callback-wide"), 2);
    assert_eq!(of("callback-mismatch"), 2);
    assert_eq!(of("callback-mismatch-two"), 2);
    assert_eq!(of("callback-generic"), 3);
    assert_eq!(of("callback-default"), 2);
    assert_eq!(of("callback-method"), 2);
    assert_eq!(of("callback-recursive"), 2);
    assert_eq!(of("callback-mutual"), 2);
    assert_eq!(of("callback-spread"), 2);
    assert_eq!(of("callback-rest"), 2);
    assert_eq!(of("callback-not-capable"), 2);
    assert_eq!(of("callback-javascript"), 2);
    assert_eq!(of("callback-this-parameter"), 2);
    assert_eq!(of("callback-overload"), 2);
    assert_eq!(of("callback-or-data"), 2);
    assert_eq!(of("callback-two-arrows"), 2);
    assert_eq!(of("callback-method-value"), 2);
    // TC3 step 3: `capture-handler` has `main`, `start` and `start.<locals>.finish`; the others one entry, `main`
    assert_eq!(of("capture-handler"), 3);
    assert_eq!(of("capture-sibling"), 1);
    assert_eq!(of("capture-two-levels"), 1);
    assert_eq!(of("capture-through-parameter"), 1);
    assert_eq!(of("capture-written-in-nested"), 1);
    assert_eq!(of("capture-anonymous"), 1);
    assert_eq!(of("capture-self"), 1);
    // TC3 steps 4, 5 and 6: each `module-*`, `bind-*` and `relevance-*` row has one entry, `main`
    for row in [
        "relevance-ignored",
        "relevance-condition",
        "relevance-through-local",
        "relevance-captured",
        "bind-handed",
        "bind-variable",
        "bind-partial",
        "bind-own-method",
        "module-const",
        "module-const-imported",
        "module-let",
        "module-script",
        "module-call-initializer",
        "module-reads-module",
    ] {
        assert_eq!(
            entries
                .iter()
                .filter(|(f, ..)| f.ends_with(&format!("/{row}")))
                .count(),
            1,
            "{row}"
        );
    }
    assert_eq!(entries.len(), 96);
}

/// Nine functions through one conditional are more than a set holds: the one context of `run` that the graph walked has a key of one slot, wide.
#[test]
fn nine_functions_make_a_wide_slot() {
    let sem = open("tests/fixtures/context/callback-wide");
    let pick = sem.entry("src/main.ts", "pick", 0, STYLE).unwrap();
    let (solver, graph) = solve_from(&sem, pick, on());
    let lang = solver.lang();
    let runs: Vec<_> = lang
        .walked
        .iter()
        .filter(|(func, _)| graph.def(&sem.describe(*func, STYLE).id).unwrap().qname == "run")
        .collect();
    assert_eq!(runs.len(), 1, "one context of run");
    let data = lang.keys.borrow().data(runs[0].1).clone();
    assert_eq!(&*data.params, &[Value::wide()]);
}

/// A context of `graph` walked alone, by a language that remembers nothing, has exactly the edges the graph has leaving it: what the solver kept for a context
/// is not what an earlier walk left in a cache. Run with the layer as `Precise` runs it (`Caps::BUILT`), so what the walk adds beside the engine is in the
/// comparison too: the graph drops an edge that is there twice, a walk alone does not. `fixture` is opened on `file` and the graph is the one under `entry`.
fn each_context_walked_alone(fixture: &str, file: &str, entry: &str) -> usize {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture);
    let sem = Sem::open(&root, Path::new(file)).unwrap();
    let start = sem
        .entry(file, entry, 0, STYLE)
        .unwrap_or_else(|| panic!("{fixture}: no entry `{entry}`"));
    let (solver, graph) = solve_from(&sem, start, Some(Caps::BUILT));
    let walked = &solver.lang().walked;
    assert_eq!(walked.len(), graph.contexts.len(), "{fixture} {entry}");
    for (func, key) in walked {
        let digest = solver.lang().digest(key);
        let mut id = ContextId::of(sem.describe(*func, STYLE).id);
        id.digest = digest;
        let mut in_graph: Vec<String> = graph
            .edges_from(&id)
            .iter()
            .map(|edge| format!("{:?} {} {}", edge.site, edge.passed, written_edge(&edge.to)))
            .collect();
        in_graph.sort();

        // a fresh language: nothing it remembers is the solver's
        let mut fresh = TypeScript::new(&sem, STYLE).with_layer(Some(Caps::BUILT));
        let data = solver.lang().keys.borrow().data(*key).clone();
        // a closure in the key is a number into the solver's table of capture lists: the fresh language starts with a copy of it
        *fresh.captures.borrow_mut() = solver.lang().captures.borrow().clone();
        let again = fresh.intern_key(data);
        assert_eq!(fresh.digest(&again), digest, "{fixture} {entry}");
        let summary = Lang::walk(&mut fresh, *func, &again, &Cancel::new());
        let name = |func: Func<'_>| sem.describe(func, STYLE).id;
        let digest_of = |key: &KeyId| fresh.digest(key);
        let mut alone: Vec<String> = Vec::new();
        for site in &summary.sites {
            for target in &site.targets {
                let (passed, text) = match target {
                    Target::Call(func, key) => (
                        site.passed,
                        format!("call {:?} {:?}", name(*func), digest_of(key)),
                    ),
                    Target::Construct { class, init } => (
                        false,
                        format!(
                            "construct {:?} {:?}",
                            name(*class),
                            init.as_ref()
                                .map(|(init, key)| (name(*init), digest_of(key)))
                        ),
                    ),
                    Target::External(external) => (
                        false,
                        format!("external {} {}", external.package, external.name),
                    ),
                    Target::Unresolved(reason) => {
                        (false, format!("unresolved {}", reason.as_str()))
                    }
                    _ => panic!("a target this test does not know"),
                };
                alone.push(format!("{:?} {passed} {text}", site.site));
            }
        }
        alone.sort();
        assert_eq!(
            in_graph,
            alone,
            "{fixture} {entry}: {:?} under {digest:?}",
            graph.def(&id.func).map(|def| &def.qname)
        );
    }
    walked.len()
}

#[test]
fn each_context_walked_alone_has_the_edges_of_the_graph() {
    // every entry of every row of the context table, and the `keys` fixture from its `main`
    let entries = every_entry();
    let mut contexts = 0;
    for (fixture, file, entry) in &entries {
        contexts += each_context_walked_alone(fixture, file, entry);
    }
    eprintln!(
        "{} entries, {contexts} contexts walked alone",
        entries.len()
    );
}

/// Over `counts`: the median and the 95th percentile by nearest rank (`sorted[(n - 1) / 2]`, `sorted[ceil(0.95 n) - 1]`), the greatest, and how many
/// are 8 or more and 64 or more. All zeros for none.
fn spread(mut counts: Vec<usize>) -> Spread {
    counts.sort_unstable();
    let n = counts.len();
    if n == 0 {
        return Spread::default();
    }
    Spread {
        median: counts[(n - 1) / 2],
        p95: counts[(95 * n).div_ceil(100) - 1],
        max: counts[n - 1],
        at_least_8: counts.iter().filter(|count| **count >= 8).count(),
        at_least_64: counts.iter().filter(|count| **count >= 64).count(),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Spread {
    median: usize,
    p95: usize,
    max: usize,
    at_least_8: usize,
    at_least_64: usize,
}

#[test]
fn the_distribution_is_by_nearest_rank() {
    assert_eq!(
        spread((1..=20).collect()),
        Spread {
            median: 10,
            p95: 19,
            max: 20,
            at_least_8: 13,
            at_least_64: 0
        }
    );
    assert_eq!(spread(vec![]), Spread::default());
    assert_eq!(
        spread(vec![64]),
        Spread {
            median: 64,
            p95: 64,
            max: 64,
            at_least_8: 1,
            at_least_64: 1
        }
    );
}

#[test]
fn relevance_and_the_bind_record_on_keys() {
    let sem = open("tests/fixtures/keys");
    let main = sem.entry("src/main.ts", "main", 0, STYLE).unwrap();
    let def_of = |qname: &str| sem.def_id(sem.entry("src/main.ts", qname, 0, STYLE).unwrap());
    let (run, ignore, pair) = (def_of("run"), def_of("ignore"), def_of("Pair.constructor"));

    // with relevance: `ignore`'s parameter is never read, so its two calls share one context
    let (_, masked) = solve_from(&sem, main, Some(Caps::BUILT));
    let found = contexts(&masked);
    assert_eq!(found, kinds(&found, &[ZEROS]));

    // without it: what `bind` made, before interning, told apart by the arrows
    let (solver, graph) = solve_from(&sem, main, Some(Caps::BUILT - Caps::RELEVANCE));
    let distinct = |def: DefId| {
        let measure = solver.lang().measure.borrow();
        measure
            .bound
            .iter()
            .filter(|(callee, _)| *callee == def)
            .map(|(_, data)| data.clone())
            .collect::<HashSet<KeyData>>()
            .len()
    };
    assert_eq!(distinct(run), 4);
    assert_eq!(contexts(&graph)["run"].len(), 3);
    assert_eq!(distinct(ignore), 2);

    let lang = TypeScript::new(&sem, STYLE).with_layer(Some(Caps::BUILT));
    let read = |def: DefId| World::relevant(&lang, def).to_vec();
    assert_eq!(read(ignore), [false]);
    assert_eq!(read(run), [true]);
    assert_eq!(read(pair), [false, true]);
}

/// The contexts of each function of a graph, through `spread`, and the qname of the first def (in `graph.defs` order) that has the most.
fn per_function(graph: &Graph) -> (Spread, String) {
    let mut counts: HashMap<&analyzer_core::graph::StableDef, usize> = HashMap::new();
    for context in &graph.contexts {
        *counts.entry(&context.id.func).or_default() += 1;
    }
    let most = counts.values().copied().max().unwrap_or(0);
    let name = graph
        .defs
        .iter()
        .find(|def| counts.get(&def.id).copied() == Some(most))
        .map(|def| def.qname.clone())
        .unwrap_or_default();
    (spread(counts.into_values().collect()), name)
}

/// The distinct `bind` results of each callee, through `spread`, and the qname of the callee with the smallest `DefId` among those with the most.
fn per_callee(sem: &Sem, bound: &[(DefId, KeyData)]) -> (Spread, String) {
    let mut sets: HashMap<DefId, HashSet<&KeyData>> = HashMap::new();
    for (callee, data) in bound {
        sets.entry(*callee).or_default().insert(data);
    }
    let most = sets.values().map(HashSet::len).max().unwrap_or(0);
    let name = sets
        .iter()
        .filter(|(_, set)| set.len() == most)
        .map(|(def, _)| *def)
        .min()
        .and_then(|def| sem.func(def))
        .map(|func| sem.describe(func, STYLE).qname)
        .unwrap_or_default();
    (spread(sets.values().map(HashSet::len).collect()), name)
}

fn print_spread(label: &str, (spread, name): &(Spread, String)) {
    println!(
        "             {label} median {}  p95 {}  max {} ({name})  >=8 {}  >=64 {}",
        spread.median, spread.p95, spread.max, spread.at_least_8, spread.at_least_64
    );
}

/// N6 (`06-tc2` §10, step 4b) on a real project: for each entry of a sample file, three solves of its whole graph (off, on without `RELEVANCE`, on with it) and how many
/// contexts per function the keys make. `cargo test --release -p typescript-analysis --lib measure_contexts -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement on a real project, driven by TS_MEASURE_ROOT and TS_MEASURE_SAMPLE"]
fn measure_contexts() {
    let (Ok(root), Ok(sample)) = (
        std::env::var("TS_MEASURE_ROOT"),
        std::env::var("TS_MEASURE_SAMPLE"),
    ) else {
        eprintln!(
            "set TS_MEASURE_ROOT (the project) and TS_MEASURE_SAMPLE (a file of `<file>\t<qualname>[\t<occurrence>]` lines)"
        );
        return;
    };
    let text = std::fs::read_to_string(&sample).expect("the sample file can be read");
    let mut entries: Vec<(String, String, u16)> = Vec::new();
    for line in text
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[..] {
            [file] => eprintln!("skipped, a file entry: {file}"),
            [file, qualname] => entries.push((file.to_owned(), qualname.to_owned(), 0)),
            [file, qualname, occurrence] => entries.push((
                file.to_owned(),
                qualname.to_owned(),
                occurrence
                    .parse()
                    .unwrap_or_else(|_| panic!("a sample line with a bad occurrence: {line:?}")),
            )),
            _ => panic!("a sample line with too many fields: {line:?}"),
        }
    }
    let first = entries.first().expect("the sample has an entry");
    let project = Project::open(Path::new(&root), Path::new(&first.0)).unwrap();
    let sem = &project.sem;
    for (file, qualname, occurrence) in &entries {
        let func = sem
            .entry(file, qualname, *occurrence, STYLE)
            .unwrap_or_else(|| panic!("no entry: {file}\t{qualname}\t{occurrence}"));
        println!("== {file} {qualname}");
        for (label, layer) in [
            ("off      ", None),
            ("on       ", Some(Caps::BUILT - Caps::RELEVANCE)),
            ("relevance", Some(Caps::BUILT)),
        ] {
            let (solver, graph, took) = solve_timed(sem, func, layer);
            println!(
                "  {label}  contexts {}  walks {}  cuts {:?}  {:.1} ms",
                graph.contexts.len(),
                solver.lang().walked.len(),
                graph.stats.cuts,
                took.as_secs_f64() * 1000.0
            );
            if layer.is_some() {
                print_spread("per function  ", &per_function(&graph));
                print_spread(
                    "bind per callee",
                    &per_callee(sem, &solver.lang().measure.borrow().bound),
                );
            }
        }
    }
}
