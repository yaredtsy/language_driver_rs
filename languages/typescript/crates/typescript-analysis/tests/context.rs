//! The context table (`languages/typescript/docs/policy.md`, "With the context layer"): every row names a project under `tests/fixtures/context/` and the
//! entries built from it, and has a snapshot of what each gives with the layer off and with it on, in `snapshots/context/<row>.txt`. A phase of the layer
//! flips the "on" half of a snapshot; the "off" half never changes. The properties of `common::context` that hold with the layer on hold on every row.

mod common;

use std::path::{Path, PathBuf};

use analyzer_testkit::expect_file;
use analyzer_testkit::graph::check_invariants;
use common::context::{a_context_is_a_function_of_its_key, never_fewer_edges};
use common::{Row, STYLE, call_graph, edges_as_text, render_graph, rows, split_policy, tests_dir};
use typescript_analysis::{
    Cancel, Cut, EdgeTo, Entry, Graph, Limits, Options, Precision, Project, Status,
};

const TABLE: &str = include_str!("../../../docs/policy.md");

fn context_table() -> String {
    split_policy(TABLE).1
}

fn snapshot_path(id: &str) -> PathBuf {
    tests_dir()
        .join("snapshots/context")
        .join(format!("{id}.txt"))
}

/// What is wrong with a table, one sentence for each: a row with no fixture, a fixture that is not there, no snapshot, a fixture no row names, a snapshot
/// no row builds. `tour.flow.txt` is not a row's snapshot (`src/sem/flow/tests.rs` writes it), nor are `locals-off.txt`, `captures-off.txt`, `modules-off.txt` and `relevance-off.txt` (`calls/keyed.rs` writes them: every entry with `LOCALS`, `CAPTURES`, `MODULES`, then `RELEVANCE`, off).
fn problems(table: &str, tests: &Path) -> Vec<String> {
    let rows = rows(table);
    let mut found = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for row in &rows {
        if row.id.is_empty() || seen.contains(&row.id.as_str()) {
            found.push(format!("row `{}`: the id is empty or used twice", row.id));
        }
        seen.push(&row.id);
        let Some(root) = row.words.first() else {
            found.push(format!("row `{}`: no fixture", row.id));
            continue;
        };
        if !tests.join(root).exists() {
            found.push(format!(
                "row `{}`: the fixture `{root}` is not there",
                row.id
            ));
            continue;
        }
        if row.words.len() < 3 {
            found.push(format!(
                "row `{}`: it needs a folder, a file and an entry",
                row.id
            ));
            continue;
        }
        if !tests.join(root).join(&row.words[1]).is_file() {
            found.push(format!(
                "row `{}`: the file `{}` is not in `{root}`",
                row.id, row.words[1]
            ));
        }
        if !tests
            .join("snapshots/context")
            .join(format!("{}.txt", row.id))
            .is_file()
        {
            found.push(format!("row `{}`: no snapshot", row.id));
        }
    }
    if let Ok(dir) = std::fs::read_dir(tests.join("fixtures/context")) {
        let mut folders: Vec<String> = dir
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        folders.sort();
        for folder in folders {
            let named = format!("fixtures/context/{folder}");
            if !rows
                .iter()
                .any(|row| row.words.first().is_some_and(|word| *word == named))
            {
                found.push(format!("no row names the fixture `{named}`"));
            }
        }
    }
    if let Ok(dir) = std::fs::read_dir(tests.join("snapshots/context")) {
        let mut names: Vec<String> = dir
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| {
                name.ends_with(".txt")
                    && !name.ends_with(".flow.txt")
                    && name != "locals-off.txt"
                    && name != "captures-off.txt"
                    && name != "modules-off.txt"
                    && name != "relevance-off.txt"
            })
            .collect();
        names.sort();
        for name in names {
            let id = name.trim_end_matches(".txt");
            if !rows.iter().any(|row| row.id == id && row.words.len() > 2) {
                found.push(format!("snapshot `{name}`: no row of the table builds it"));
            }
        }
    }
    found
}

fn precise() -> Options {
    let mut options = Options::new(STYLE);
    options.precision = Precision::Precise;
    options
}

/// The graphs of one entry of a row, the layer off and on.
fn graphs_of(project: &Project, row: &Row, entry: &str) -> (Graph, Graph) {
    let file = &row.words[1];
    let entry = Entry::Def {
        qualname: entry,
        occurrence: 0,
    };
    let make = |options: &Options| {
        let graph = call_graph(project, file, &entry, options, &Cancel::new())
            .unwrap_or_else(|| panic!("row `{}`: no def `{entry:?}` in {file}", row.id));
        check_invariants(&graph);
        graph
    };
    (make(&Options::new(STYLE)), make(&precise()))
}

fn open_row(row: &Row) -> (PathBuf, Project) {
    let root = tests_dir().join(&row.words[0]);
    let project = Project::open(&root, Path::new(&row.words[1]))
        .unwrap_or_else(|error| panic!("row `{}`: {error:?}", row.id));
    (root, project)
}

/// What a row gives: for each entry, the graph with the layer off, then with it on, each as `common::render_graph` writes it.
fn render(row: &Row) -> String {
    let (root, project) = open_row(row);
    let mut out = String::new();
    for entry in &row.words[2..] {
        let (off, on) = graphs_of(&project, row, entry);
        out.push_str(&format!("== {entry}\n-- off\n"));
        out.push_str(&render_graph(&off, &root));
        out.push_str("-- on\n");
        out.push_str(&render_graph(&on, &root));
    }
    out
}

fn context_rows() -> Vec<Row> {
    rows(&context_table())
        .into_iter()
        .filter(|row| row.words.len() > 2)
        .collect()
}

/// How many entries the table gives (the words of a row after its folder and file): what a test that goes through every entry compares its count with.
fn table_entries() -> usize {
    context_rows().iter().map(|row| row.words.len() - 2).sum()
}

#[test]
fn every_context_row_has_a_fixture_and_a_snapshot() {
    let found = problems(&context_table(), &tests_dir());
    assert!(found.is_empty(), "{}", found.join("\n"));
    assert!(!rows(&context_table()).is_empty(), "the table was not read");
}

#[test]
fn the_check_finds_a_missing_fixture_a_fixture_without_a_row_and_a_stale_snapshot() {
    let table = "| row | what | fixture |\n|---|---|---|\n| `gone` | x | y | `fixtures/context/gone` `src/a.ts` `f` |\n";
    let found = problems(table, &tests_dir());
    assert!(
        found
            .iter()
            .any(|p| p.contains("the fixture `fixtures/context/gone` is not there")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|p| p.contains("no row names the fixture `fixtures/context/tour`")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|p| p.contains("snapshot `tour.txt`: no row of the table builds it")),
        "{found:?}"
    );
    assert!(
        !found.iter().any(|p| p.contains("tour.flow.txt")),
        "{found:?}"
    );
}

#[test]
fn every_context_row_gives_its_snapshot() {
    // every row is compared, and the ones that differ are named together at the end (each diff is printed as it is found)
    let mut differ = Vec::new();
    for row in context_rows() {
        let compared = std::panic::catch_unwind(|| {
            expect_file(&snapshot_path(&row.id), &render(&row));
        });
        if compared.is_err() {
            differ.push(row.id.clone());
        }
    }
    assert!(differ.is_empty(), "rows that differ: {differ:?}");
}

#[test]
fn every_context_row_gives_the_same_text_twice() {
    for row in context_rows() {
        assert_eq!(render(&row), render(&row), "row `{}`", row.id);
    }
}

#[test]
fn the_properties_hold_on_every_context_row() {
    let mut found = Vec::new();
    let mut compared = 0;
    for row in context_rows() {
        let (_, project) = open_row(&row);
        let (mut offs, mut ons) = (Vec::new(), Vec::new());
        for entry in &row.words[2..] {
            let (off, on) = graphs_of(&project, &row, entry);
            for problem in never_fewer_edges(&off, &on) {
                found.push(format!("row `{}` `{entry}`: {problem}", row.id));
            }
            offs.push(off);
            ons.push(on);
            compared += 1;
        }
        for graphs in [&offs, &ons] {
            let graphs: Vec<&Graph> = graphs.iter().collect();
            for problem in a_context_is_a_function_of_its_key(&graphs) {
                found.push(format!("row `{}`: {problem}", row.id));
            }
        }
    }
    assert!(compared > 0, "no entry was compared");
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// The tour's `lineA`, the layer off: three edges, which the next tests take apart.
fn line_a() -> (PathBuf, Graph) {
    let root = tests_dir().join("fixtures/context/tour");
    let project = Project::open(&root, Path::new("src/tour.tsx")).unwrap();
    let graph = call_graph(
        &project,
        "src/tour.tsx",
        &Entry::Def {
            qualname: "lineA",
            occurrence: 0,
        },
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .unwrap();
    (root, graph)
}

fn index_of(graph: &Graph, root: &Path, text: &str) -> usize {
    let lines = edges_as_text(graph, root);
    lines
        .iter()
        .position(|line| line == text)
        .unwrap_or_else(|| panic!("no edge `{text}` in {lines:?}"))
}

#[test]
fn each_property_finds_what_it_is_for() {
    let (root, g) = line_a();
    assert_eq!(
        edges_as_text(&g, &root),
        [
            "each | f | unresolved function_typed_value",
            "lineA | each | call each",
            "lineA | encode | pass encode",
        ]
    );
    let call = index_of(&g, &root, "lineA | each | call each");
    let pass = index_of(&g, &root, "lineA | encode | pass encode");
    let open = index_of(&g, &root, "each | f | unresolved function_typed_value");

    let mut h = g.clone();
    h.edges.remove(call);

    // never_fewer_edges
    assert!(never_fewer_edges(&g, &g).is_empty());
    assert!(!never_fewer_edges(&g, &h).is_empty());
    // an unresolved answer replaced by a function
    let mut k = g.clone();
    k.edges[open].to = g.edges[pass].to.clone();
    assert!(never_fewer_edges(&g, &k).is_empty());
    // another function at the same site: the site still has an edge, so only check (a) can see it
    let mut m = g.clone();
    m.edges[call].to = g.edges[pass].to.clone();
    let found = never_fewer_edges(&g, &m);
    assert!(
        found
            .iter()
            .any(|p| p.contains("is not in the graph with the layer on")),
        "{found:?}"
    );
    assert!(
        !found.iter().any(|p| p.contains("has no edge")),
        "{found:?}"
    );

    // the one allowed loss: a site whose callee is only `undefined`, as many sites as the layer counted
    let mut lost = g.clone();
    lost.edges.remove(open);
    lost.stats.language.insert("callee_undefined".to_owned(), 1);
    assert!(never_fewer_edges(&g, &lost).is_empty());
    lost.stats.language.insert("callee_undefined".to_owned(), 0);
    let found = never_fewer_edges(&g, &lost);
    assert!(found.iter().any(|p| p.contains("has no edge")), "{found:?}");
    lost.stats.language.remove("callee_undefined");
    assert!(!never_fewer_edges(&g, &lost).is_empty());
    // a site that had an answer is reported even when the counter would allow one lost site
    let mut answered = g.clone();
    answered.edges.remove(call);
    answered
        .stats
        .language
        .insert("callee_undefined".to_owned(), 1);
    let found = never_fewer_edges(&g, &answered);
    assert!(found.iter().any(|p| p.contains("has no edge")), "{found:?}");

    // a_context_is_a_function_of_its_key
    assert!(a_context_is_a_function_of_its_key(&[&g, &g]).is_empty());
    let found = a_context_is_a_function_of_its_key(&[&g, &k]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("`each`"), "{found:?}");
    // a context that is not finished in a graph is not compared there
    let mut c = g.clone();
    let each = c.edges[open].from.clone();
    for context in &mut c.contexts {
        if context.id == each {
            context.status = typescript_analysis::Status::Frontier;
        }
    }
    c.edges.remove(open);
    assert!(a_context_is_a_function_of_its_key(&[&g, &c]).is_empty());
}

#[test]
fn a_digest_that_is_not_zeros_is_written_by_its_first_eight_hex_digits() {
    let (root, g) = line_a();
    let call = index_of(&g, &root, "lineA | each | call each");
    let mut copy = g.clone();
    copy.edges[call].from.digest[..4].copy_from_slice(&[0x01, 0x02, 0x03, 0x04]);
    match &mut copy.edges[call].to {
        typescript_analysis::EdgeTo::Context(id, _) => {
            id.digest[..4].copy_from_slice(&[0x1a, 0x2b, 0x3c, 0x4d]);
        }
        other => panic!("not a call: {other:?}"),
    }
    let mut lines = edges_as_text(&copy, &root);
    let mut plain = edges_as_text(&g, &root);
    let at = lines
        .iter()
        .position(|line| line.contains("@"))
        .expect("a line with a digest");
    assert_eq!(lines[at], "lineA @01020304 | each | call each @1a2b3c4d");
    lines.remove(at);
    plain.remove(call);
    assert_eq!(lines, plain);
}

/// The counters follow the edges (`06-tc2` §10, step 5a): a `function_typed_value` is counted only for an edge that is drawn, and the one site whose
/// callee is only `undefined` is counted as `callee_undefined` since it has none.
#[test]
fn the_counters_follow_the_edges() {
    // (row, entry, function_typed_value, callee_undefined) with the layer on; `None` is a counter that is not said at all
    let cases: [(&str, &str, Option<u64>, Option<u64>); 4] = [
        // replaced by the two functions: no unresolved edge is left, so none is counted
        ("callback-either", "pick", None, None),
        // `undefined` alone: no edge, one note
        ("callback-not-passed", "main", None, Some(1)),
        // TC3 step 2 reads the write (`cb = cb || fallback`): `handler` and `fallback`, a complete set, so no open entry is left to count
        ("callback-reassigned", "main", None, None),
        // an entry with the open key: the engine's answer stands, counted as before
        ("callback-either", "run", Some(1), None),
    ];
    for (id, entry, function_typed, undefined) in cases {
        let row = context_rows()
            .into_iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("no row `{id}`"));
        let (_, project) = open_row(&row);
        let (off, on) = graphs_of(&project, &row, entry);
        let said = |graph: &Graph, key: &str| graph.stats.language.get(key).copied();
        assert_eq!(
            said(&on, "function_typed_value"),
            function_typed,
            "{id} {entry}"
        );
        assert_eq!(said(&on, "callee_undefined"), undefined, "{id} {entry}");
        // the split by origin adds up to the total, with the layer on as with it off
        for graph in [&off, &on] {
            let split: u64 = graph
                .stats
                .language
                .iter()
                .filter(|(key, _)| key.starts_with("function_typed_value_"))
                .map(|(_, count)| *count)
                .sum();
            assert_eq!(
                split,
                said(graph, "function_typed_value").unwrap_or(0),
                "{id} {entry}: the split"
            );
        }
        // the layer off counts the open entry wherever the engine gave one, and never says `callee_undefined`
        assert_eq!(said(&off, "callee_undefined"), None, "{id} {entry}");
        assert_eq!(said(&off, "function_typed_value"), Some(1), "{id} {entry}");
    }
}

#[test]
fn value_widened_set_is_counted_where_a_set_passes_the_cap() {
    let mut seen = 0;
    for row in context_rows() {
        let (_, project) = open_row(&row);
        for entry in &row.words[2..] {
            let (off, on) = graphs_of(&project, &row, entry);
            let said = |graph: &Graph| graph.stats.language.get("value_widened_set").copied();
            // nine functions through one conditional: the argument is evaluated once, for the one target `run`
            let want = (row.id == "callback-wide" && entry == "pick").then_some(1);
            assert_eq!(said(&on), want, "{} {entry}", row.id);
            assert_eq!(said(&off), None, "{} {entry}: the layer is off", row.id);
            seen += 1;
        }
    }
    assert_eq!(seen, table_entries(), "every entry of every row");
    assert!(seen > 0, "no entry was compared");
}

/// The tripwire (`06-tc2` §10, step 5c): a function drawn at the call of a value that the checker says does not fit the callee's type is counted
/// (`value_type_mismatch`), and inside generic code, where the checker cannot say, `value_type_unchecked`. Every other entry of every row says neither.
#[test]
fn the_tripwire_counts_where_a_function_does_not_fit() {
    let mut seen = 0;
    for row in context_rows() {
        let (_, project) = open_row(&row);
        for entry in &row.words[2..] {
            let (off, on) = graphs_of(&project, &row, entry);
            let said = |graph: &Graph, key: &str| graph.stats.language.get(key).copied();
            let here = (row.id.as_str(), entry.as_str());
            // one count for each function that fails, so two where two fail at one site
            let mismatch = match here {
                ("callback-mismatch", "main") => Some(1),
                ("callback-mismatch-two", "main") => Some(2),
                _ => None,
            };
            let unchecked = (here == ("callback-generic", "main")).then_some(2);
            assert_eq!(
                said(&on, "value_type_mismatch"),
                mismatch,
                "{} {entry}",
                row.id
            );
            assert_eq!(
                said(&on, "value_type_unchecked"),
                unchecked,
                "{} {entry}",
                row.id
            );
            for key in ["value_type_mismatch", "value_type_unchecked"] {
                assert_eq!(
                    said(&off, key),
                    None,
                    "{} {entry}: the layer is off",
                    row.id
                );
            }
            if mismatch.is_some() {
                // D61: the mark replaces the open entry. The function is drawn, the site says `value_type_mismatch` once however many functions failed (D61), and the engine's
                // `function_typed_value` does not come back beside the call, so neither the counter nor its split has anything to count
                let split: u64 = on
                    .stats
                    .language
                    .iter()
                    .filter(|(key, _)| key.starts_with("function_typed_value_"))
                    .map(|(_, count)| *count)
                    .sum();
                assert_eq!(said(&on, "function_typed_value"), None);
                assert_eq!(split, 0, "{} {entry}: the split", row.id);
                let marks = on
                    .edges
                    .iter()
                    .filter(|edge| {
                        matches!(&edge.to, EdgeTo::Unresolved(reason) if reason.as_str() == "value_type_mismatch")
                    })
                    .count();
                assert_eq!(marks, 1, "{} {entry}: one mark for the site", row.id);
            }
            seen += 1;
        }
    }
    assert_eq!(seen, table_entries(), "every entry of every row");
    assert!(seen > 0, "no entry was compared");
}

/// The review before TC3, §1.2: the graph of `pick` under a limit on the number of contexts does not depend on what the project was asked before. A file gets
/// its number the first time one of its defs is asked for, so what `touchA` asks first gives `a.ts` its number first; the order of the functions of
/// `flag ? fb : fa` (and so which context a limit cuts) must not follow it.
#[test]
fn the_order_of_requests_does_not_change_a_graph_under_a_limit() {
    let root = tests_dir().join("fixtures/order-under-limit");
    let graph_of = |project: &Project, entry: &'static str, limit: u32| {
        let mut options = precise();
        options.limits = Limits::new(limit, None);
        call_graph(
            project,
            "src/main.ts",
            &Entry::Def {
                qualname: entry,
                occurrence: 0,
            },
            &options,
            &Cancel::new(),
        )
        .unwrap_or_else(|| panic!("no def {entry}"))
    };
    let mut differing = Vec::new();
    for limit in 1..=8 {
        let fresh = Project::open(&root, Path::new("src/main.ts")).unwrap();
        let after = Project::open(&root, Path::new("src/main.ts")).unwrap();
        // `touchA` first: it gives `a.ts` its number before `b.ts`'s
        let _ = graph_of(&after, "touchA", limit);
        let (alone, second) = (
            graph_of(&fresh, "pick", limit),
            graph_of(&after, "pick", limit),
        );
        if alone != second {
            differing.push(limit);
        }
    }
    assert!(
        differing.is_empty(),
        "the graph of `pick` depends on what was asked before it, at limits {differing:?}"
    );
}

/// The cap on the contexts of one function (`cap-per-function`): `run` has three contexts, one for each of `first`, `second` and `third`. At a cap of 2 the third one
/// the breadth-first walk reads is listed, `Incomplete`, counted, and not walked, so `c` is handed over (that edge is `third`'s own) and never called.
#[test]
fn the_cap_per_function_cuts_the_third_context_of_run() {
    let row = context_rows()
        .into_iter()
        .find(|row| row.id == "cap-per-function")
        .expect("the row `cap-per-function`");
    let (_, project) = open_row(&row);
    let graph_with = |precision: Precision, cap: Option<u32>| {
        let mut options = Options::new(STYLE);
        options.precision = precision;
        options.limits.max_contexts_per_function = cap;
        let entry = Entry::Def {
            qualname: "main",
            occurrence: 0,
        };
        let graph = call_graph(&project, &row.words[1], &entry, &options, &Cancel::new())
            .expect("a def `main`");
        check_invariants(&graph);
        graph
    };
    let off = graph_with(Precision::Default, Some(64));
    let uncapped = graph_with(Precision::Precise, None);
    let capped = graph_with(Precision::Precise, Some(2));
    let qname = |graph: &Graph, id: &typescript_analysis::ContextId| {
        graph.def(&id.func).unwrap().qname.clone()
    };

    // the default graph has one context of `run`, so the cap (64 in `Options::new`) never cut it
    assert!(off.stats.cuts.is_empty(), "{:?}", off.stats.cuts);
    assert!(uncapped.stats.cuts.is_empty(), "{:?}", uncapped.stats.cuts);

    let runs: Vec<_> = capped
        .contexts
        .iter()
        .filter(|c| qname(&capped, &c.id) == "run")
        .collect();
    assert_eq!(runs.len(), 3, "three contexts of `run`");
    let cut: Vec<_> = runs
        .iter()
        .filter(|c| c.status == Status::Incomplete(Cut::MaxContextsPerFunction))
        .collect();
    assert_eq!(cut.len(), 1, "exactly one context of `run` is cut");
    assert!(
        runs.iter()
            .filter(|c| c.status != Status::Incomplete(Cut::MaxContextsPerFunction))
            .all(|c| c.status == Status::Finished)
    );
    assert!(
        capped.edges_from(&cut[0].id).is_empty(),
        "a cut context is not walked"
    );

    // the cut one is the one `third` calls
    let third = capped
        .contexts
        .iter()
        .find(|c| qname(&capped, &c.id) == "third")
        .expect("a context of `third`");
    let third_calls_run: Vec<_> = capped
        .edges_from(&third.id)
        .iter()
        .filter_map(|e| match &e.to {
            EdgeTo::Context(c, _) if qname(&capped, c) == "run" => Some(c.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(third_calls_run, [cut[0].id.clone()]);

    // `c` is handed over by `third` (a passed edge) and no call reaches it: the walk of `run` under `third` would have made it
    let calls_to = |graph: &Graph, name: &str| {
        graph
            .edges
            .iter()
            .filter(|e| !e.passed)
            .filter(|e| matches!(&e.to, EdgeTo::Context(c, _) if qname(graph, c) == name))
            .count()
    };
    assert_eq!(calls_to(&uncapped, "c"), 1, "uncapped, `run` calls `c`");
    assert_eq!(calls_to(&capped, "c"), 0, "capped, nothing calls `c`");
    assert_eq!(calls_to(&capped, "a"), 1);
    assert_eq!(calls_to(&capped, "b"), 1);

    assert_eq!(
        capped.stats.cuts.get(&Cut::MaxContextsPerFunction),
        Some(&1)
    );
    assert_eq!(capped.stats.cuts.len(), 1, "{:?}", capped.stats.cuts);

    assert!(never_fewer_edges(&off, &capped).is_empty());
    assert!(a_context_is_a_function_of_its_key(&[&uncapped, &capped]).is_empty());

    // a cap of 3 is not reached
    let roomy = graph_with(Precision::Precise, Some(3));
    assert_eq!(roomy, uncapped);
}
