//! The context table (`languages/typescript/docs/policy.md`, "With the context layer"): every row names a project under `tests/fixtures/context/` and the
//! entries built from it, and has a snapshot of what each gives with the layer off and with it on, in `snapshots/context/<row>.txt`. A phase of the layer
//! flips the "on" half of a snapshot; the "off" half never changes. The properties of `common::context` that hold with the layer on hold on every row.

mod common;

use std::path::{Path, PathBuf};

use analyzer_testkit::expect_file;
use analyzer_testkit::graph::check_invariants;
use common::context::{a_context_is_a_function_of_its_key, never_fewer_edges};
use common::{Row, STYLE, call_graph, edges_as_text, render_graph, rows, split_policy, tests_dir};
use typescript_analysis::{Cancel, Entry, Graph, Options, Precision, Project};

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
/// no row builds. `tour.flow.txt` is not a row's snapshot (`src/sem/flow/tests.rs` writes it).
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
            .filter(|name| name.ends_with(".txt") && !name.ends_with(".flow.txt"))
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
        // the layer cannot read the write: the open entry is kept beside `handler`, and counted
        ("callback-reassigned", "main", Some(1), None),
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
    assert_eq!(seen, 56, "every entry of every row");
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
            let mismatch = (here == ("callback-mismatch", "main")).then_some(1);
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
                // the open entry is drawn beside the function, so the split still adds up to the one `function_typed_value`
                let split: u64 = on
                    .stats
                    .language
                    .iter()
                    .filter(|(key, _)| key.starts_with("function_typed_value_"))
                    .map(|(_, count)| *count)
                    .sum();
                assert_eq!(said(&on, "function_typed_value"), Some(1));
                assert_eq!(split, 1, "{} {entry}: the split", row.id);
            }
            seen += 1;
        }
    }
    assert_eq!(seen, 56, "every entry of every row");
}
