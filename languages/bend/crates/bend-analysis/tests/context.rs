//! The context layer (`docs/08-bend/06-context-layer-design.md`): the precise table of `languages/bend/docs/policy.md` is a test, in the grammar of "Where a row is
//! checked" (shared with `policy.rs`, header `| Precise row |`). Each row's fixture is opened as the root of a book, and for each entry the snapshot
//! `snapshots/context/<row>.txt` holds two sections, `default` and `precise`, each in the policy snapshot's grammar (tree, edges, counters) with a `contexts:` block
//! after the edges: one line per context, its def and its full digest in hex, or `open`, sorted. A context that is not the open one is written `def@xxxxxxxx` (the first
//! eight hex digits of its digest) on both sides of an edge. The digests in a reviewed file are the cross-process determinism test.
//!
//! Beside the rows: the precise graph never has fewer edges than the default one, every precise graph is deterministic, and what a plain assertion says better than a
//! snapshot.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use analyzer_core::graph::StableDef;
use analyzer_testkit::expect_file;
use analyzer_testkit::graph::assert_deterministic;
use bend_analysis::{Book, ContextId, EdgeTo, Graph, Precision, RenderOptions, render, to_text};

mod common;
use common::table::{self, Cell, Group, Row, entry_name};
use common::{Program, edges_as_text_keyed, graph_at, hex, label};

const TABLE: &str = include_str!("../../../docs/policy.md");
const HEADER: &str = "| Precise row |";

const ZEROS: [u8; 16] = [0; 16];

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn fixtures_dir() -> PathBuf {
    tests_dir().join("fixtures/context")
}

fn snapshots_dir() -> PathBuf {
    tests_dir().join("snapshots/context")
}

fn rows() -> Vec<Row> {
    table::rows(TABLE, HEADER)
}

fn fixture(name: &str) -> Book {
    Book::open(&fixtures_dir().join(name), Path::new("main.bend")).expect("the book opens")
}

/// One entry at one precision: tree, edges (contexts keyed), the contexts, the counters.
fn section(book: &Book, file: &str, entry: &str, precision: Precision) -> String {
    let mode = match precision {
        Precision::Default => "default",
        _ => "precise",
    };
    let mut out = format!("== {file} {entry} {mode}\n");
    let graph = graph_at(book, file, entry, precision).expect("a graph");
    out.push_str(&to_text(&render(
        &graph,
        &graph.entries[0],
        &RenderOptions::default(),
    )));
    let edges = edges_as_text_keyed(&graph, book.root(), true);
    if edges.is_empty() {
        out.push_str("edges: none\n");
    } else {
        out.push_str("edges:\n");
        for line in edges {
            out.push_str(&format!("{line}\n"));
        }
    }
    let mut contexts: Vec<String> = graph
        .contexts
        .iter()
        .map(|c| {
            let digest = if c.id.digest == ZEROS {
                "open".to_owned()
            } else {
                hex(&c.id.digest)
            };
            format!("{} | {digest}", label(&graph, &c.id.func))
        })
        .collect();
    contexts.sort();
    out.push_str("contexts:\n");
    for line in contexts {
        out.push_str(&format!("{line}\n"));
    }
    let counters: Vec<String> = graph
        .stats
        .language
        .iter()
        .filter(|(_, count)| **count > 0)
        .map(|(key, count)| format!("{key}={count}"))
        .collect();
    if !counters.is_empty() {
        out.push_str(&format!("counters: {}\n", counters.join(", ")));
    }
    out
}

/// What a row gives, group by group: the fixture folder opened as the root, and for each entry its two sections.
fn render_row(groups: &[Group]) -> String {
    let mut out = String::new();
    for group in groups {
        let book = Book::open(&fixtures_dir().join(&group.folder), Path::new(&group.file))
            .expect("the book opens");
        for entry in &group.entries {
            for precision in [Precision::Default, Precision::Precise] {
                out.push_str(&section(&book, &group.file, entry_name(entry).0, precision));
            }
        }
    }
    out
}

#[test]
fn every_precise_row_has_a_fixture_and_a_snapshot() {
    let found = table::problems(TABLE, HEADER, &fixtures_dir(), &snapshots_dir());
    assert!(found.is_empty(), "{}", found.join("\n"));
    let all = rows();
    assert_eq!(all.len(), 35, "the table was not read");
    assert_eq!(all.iter().filter(|r| is_limit(r)).count(), 8);
}

fn is_limit(row: &Row) -> bool {
    row.id.starts_with("limit-")
}

#[test]
fn every_precise_row_flips_as_its_snapshot_says() {
    let mut differ = Vec::new();
    for row in &rows() {
        let Cell::Groups(groups) = &row.cell else {
            continue;
        };
        let compared = std::panic::catch_unwind(|| {
            expect_file(
                &snapshots_dir().join(format!("{}.txt", row.id)),
                &render_row(groups),
            );
        });
        if compared.is_err() {
            differ.push(row.id.clone());
        }
    }
    assert!(differ.is_empty(), "rows that differ: {differ:?}");
}

/// A section without its heading line, and with the `key_slot_*` counters left out of its counters (they exist only under `Precise`: they say why a slot is unnamed).
fn without_slots(section: &str) -> String {
    section
        .lines()
        .skip(1)
        .map(|line| match line.strip_prefix("counters: ") {
            Some(counters) => {
                let kept: Vec<&str> = counters
                    .split(", ")
                    .filter(|c| !c.starts_with("key_slot_"))
                    .collect();
                format!("counters: {}", kept.join(", "))
            }
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The `limit-*` rows are the known limits of the layer: what `Precise` draws is what `Default` draws (the tree, every edge, the contexts, the counters), and it
/// differs in nothing but the count of why a slot was left unnamed.
#[test]
fn a_limit_row_draws_the_same_in_both_sections() {
    let mut checked = 0;
    for row in rows().iter().filter(|r| is_limit(r)) {
        let Cell::Groups(groups) = &row.cell else {
            panic!("row `{}` has no fixture", row.id);
        };
        for group in groups {
            let book = Book::open(&fixtures_dir().join(&group.folder), Path::new(&group.file))
                .expect("the book opens");
            for entry in &group.entries {
                let [default, precise] = [Precision::Default, Precision::Precise]
                    .map(|p| section(&book, &group.file, entry_name(entry).0, p));
                assert_eq!(
                    without_slots(&default),
                    without_slots(&precise),
                    "row `{}`",
                    row.id
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 8);
}

/// An edge as (the def it leaves, the site, what it reaches by def or kind): digests left out, and a site that is in several contexts of one def once.
type Edge = (String, (u32, u32), String);

fn edge_set(graph: &Graph) -> BTreeSet<Edge> {
    graph
        .edges
        .iter()
        .map(|e| {
            let to = match &e.to {
                EdgeTo::Context(id, _) if e.passed => format!("pass {}", label(graph, &id.func)),
                EdgeTo::Context(id, _) => format!("call {}", label(graph, &id.func)),
                EdgeTo::Unresolved(reason) => format!("unresolved {}", reason.as_str()),
                other => format!("{other:?}"),
            };
            (label(graph, &e.from.func), (e.site.start, e.site.end), to)
        })
        .collect()
}

/// Every entry of every `.bend` file at the top of `folder`, as a book rooted at the folder.
fn books_and_entries(folder: &Path) -> Vec<(Book, String, String)> {
    let mut files: Vec<String> = fs::read_dir(folder)
        .expect("a fixture folder")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".bend"))
        .collect();
    files.sort();
    let mut found = Vec::new();
    for file in files {
        let Some(book) = Book::open(folder, Path::new(&file)) else {
            continue;
        };
        let Some(at) = book.file_at(&folder.join(&file)) else {
            continue;
        };
        let mut names: Vec<String> = book
            .decls()
            .iter()
            .filter(|d| d.id.file() == at)
            .filter_map(|d| book.def(d.id))
            .map(|def| def.qualname.clone())
            .collect();
        names.sort();
        names.dedup();
        for name in names {
            // a book is opened again for each entry: `Book` is not `Clone`, and a few hundred small books cost nothing
            if let Some(again) = Book::open(folder, Path::new(&file)) {
                found.push((again, file.clone(), name));
            }
        }
    }
    found
}

fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .expect("a folder of folders")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    found.sort();
    found
}

/// Every program of the tests: the policy fixtures, the context fixtures, and each scenario's Bend program.
fn programs() -> Vec<PathBuf> {
    let mut found = subfolders(&tests_dir().join("fixtures/policy"));
    found.extend(subfolders(&fixtures_dir()));
    found.extend(
        subfolders(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../scenarios"))
            .into_iter()
            .map(|s| s.join("bend"))
            .filter(|p| p.is_dir()),
    );
    found
}

/// Rules 4 and 5: the precise graph has every edge the default one has, and the only ones it lacks are `Unresolved(function_typed_value)` replaced at the same site.
/// And rule 9: every precise graph is the same twice.
#[test]
fn the_precise_graph_never_has_fewer_edges_and_is_deterministic() {
    let (mut entries, mut replaced) = (0, 0);
    for folder in programs() {
        for (book, file, entry) in books_and_entries(&folder) {
            let default = graph_at(&book, &file, &entry, Precision::Default).expect("a graph");
            let precise = graph_at(&book, &file, &entry, Precision::Precise).expect("a graph");
            let (before, after) = (edge_set(&default), edge_set(&precise));
            for edge in before.difference(&after) {
                assert_eq!(
                    edge.2,
                    "unresolved function_typed_value",
                    "{}: {file} {entry}: layer 1's edge {edge:?} is gone",
                    folder.display()
                );
                assert!(
                    after.iter().any(|a| (&a.0, a.1) == (&edge.0, edge.1)),
                    "{}: {file} {entry}: the site of {edge:?} has no edge at all",
                    folder.display()
                );
                replaced += 1;
            }
            assert_deterministic(|| {
                graph_at(&book, &file, &entry, Precision::Precise).expect("a graph")
            });
            entries += 1;
        }
    }
    assert!(
        entries > 100,
        "{entries} entries: the programs were not read"
    );
    println!("{entries} entries, {replaced} unresolved edges replaced");
}

fn named(qualname: &str) -> impl Fn(&&ContextId) -> bool + '_ {
    move |id| matches!(&id.func, StableDef::Path { qname, .. } if qname == qualname)
}

/// The contexts of the def named `qualname`.
fn contexts_of(graph: &Graph, qualname: &str) -> Vec<ContextId> {
    graph
        .contexts
        .iter()
        .map(|c| &c.id)
        .filter(named(qualname))
        .cloned()
        .collect()
}

fn counter(graph: &Graph, key: &str) -> u64 {
    graph.stats.language.get(key).copied().unwrap_or(0)
}

#[test]
fn two_keys_has_two_contexts_of_each_and_the_third_call_reaches_the_first() {
    let book = fixture("two-keys");
    let default = graph_at(&book, "main.bend", "main", Precision::Default).expect("a graph");
    let precise = graph_at(&book, "main.bend", "main", Precision::Precise).expect("a graph");

    let each = contexts_of(&default, "each");
    assert_eq!(each.len(), 1, "layer 1: one context of `each`");
    assert_eq!(each[0].digest, ZEROS);

    let each = contexts_of(&precise, "each");
    assert_eq!(
        each.len(),
        2,
        "one context of `each` for each of `work` and `other`"
    );
    assert_ne!(each[0].digest, each[1].digest);
    assert!(each.iter().all(|c| c.digest != ZEROS));

    let reached: Vec<&ContextId> = precise
        .edges
        .iter()
        .filter_map(|e| match &e.to {
            EdgeTo::Context(id, _) if !e.passed && named("each")(&id) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(reached.len(), 3);
    assert_eq!(reached[0], reached[2]);
    assert_ne!(reached[0], reached[1]);

    // inside one request the solver meets a repeated context in its `seen` set, before the memo: nothing is reused
    assert_eq!(precise.stats.reuse_hits, 0);
    assert_eq!(precise.stats.walks, default.stats.walks + 1);

    // a def reached under two keys counts what its code says once: layer 1's `type_position`, not twice
    assert_eq!(
        counter(&precise, "type_position"),
        counter(&default, "type_position")
    );
    // `each` calls position 0, which every caller names; the literal at position 1 is only handed to `f`: not counted
    assert_eq!(counter(&precise, "key_slot_value"), 0);
}

#[test]
fn a_parameter_the_def_never_calls_is_not_in_the_key() {
    let book = fixture("not-relevant");
    let precise = graph_at(&book, "main.bend", "main", Precision::Precise).expect("a graph");
    let log = contexts_of(&precise, "log");
    assert_eq!(
        log.len(),
        1,
        "`log(work, 1)` and `log(other, 2)` are one context"
    );
    assert_eq!(log[0].digest, ZEROS);
    assert!(precise.contexts.iter().all(|c| c.id.digest == ZEROS));
}

#[test]
fn the_digest_of_one_key_is_the_same_from_two_entries() {
    let program = Program::new(&[(
        "main.bend",
        "def work(x: U32) -> U32:\n  x\n\ndef each(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n\ndef main() -> U32:\n  each(work, 1)\n\ndef second() -> U32:\n  each(work, 2)\n",
    )]);
    let book = program.book("main.bend");
    let from = |entry: &str| {
        contexts_of(
            &graph_at(&book, "main.bend", entry, Precision::Precise).expect("a graph"),
            "each",
        )
    };
    let (from_main, from_second) = (from("main"), from("second"));
    assert_eq!((from_main.len(), from_second.len()), (1, 1));
    assert_ne!(from_main[0].digest, ZEROS);
    assert_eq!(from_main, from_second);
}

/// The key is the callee's relevant parameters the caller named, and nothing else (BC-D2): `f` is passed to a project def, so it is relevant; a constructor's
/// field is not.
#[test]
fn a_parameter_is_relevant_when_called_or_passed_to_a_project_def() {
    let program = Program::new(&[(
        "main.bend",
        concat!(
            "def work(x: U32) -> U32:\n  x\n\n",
            "def each(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n\n",
            "def relay(f: U32 -> U32, x: U32) -> U32:\n  each(f, x)\n\n",
            "def boxed(f: U32 -> U32, x: U32) -> U32:\n  Box{f}\n\n",
            "def main() -> U32:\n  a = relay(work, 1)\n  boxed(work, 2)\n",
        ),
    )]);
    let book = program.book("main.bend");
    let precise = graph_at(&book, "main.bend", "main", Precision::Precise).expect("a graph");
    assert_eq!(contexts_of(&precise, "relay").len(), 1);
    assert_ne!(contexts_of(&precise, "relay")[0].digest, ZEROS);
    assert_eq!(contexts_of(&precise, "boxed")[0].digest, ZEROS);
}

/// `S` is in no key (06 §3.2, row `template-type`): the same call with any type is the same context, and a project type handed over is `type_passed`, as in layer 1.
#[test]
fn a_type_is_in_no_key() {
    let program = Program::new(&[(
        "main.bend",
        concat!(
            "type Pix:\n  Px{v: U32}\n\n",
            "def sky(x: U32) -> U32:\n  x\n\n",
            "def blk(~S: Data, ~at: S -> U32, s: S) -> U32:\n  at(s)\n\n",
            "def main() -> U32:\n  a = blk(~U32, ~sky, 1)\n  blk(~Pix, ~sky, 2)\n",
        ),
    )]);
    let book = program.book("main.bend");
    let precise = graph_at(&book, "main.bend", "main", Precision::Precise).expect("a graph");
    let blk = contexts_of(&precise, "blk");
    assert_eq!(blk.len(), 1, "`~U32` and `~Pix` give one context: {blk:?}");
    assert_ne!(blk[0].digest, ZEROS);
    assert_eq!(counter(&precise, "type_passed"), 1);
    assert_eq!(counter(&precise, "key_slot_value"), 0);
}

/// A slot the caller left unnamed is counted by why, once per context walked, and only at a position the callee itself calls (06 §4). Positions it only passes along
/// (`x` in `each(f, x)`, everything in `relay` and `open`) are not counted whatever the caller wrote there.
#[test]
fn an_unnamed_slot_is_counted_by_why() {
    let program = Program::new(&[(
        "main.bend",
        concat!(
            "def work(x: U32) -> U32:\n  x\n\n",
            "def each(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n\n",
            "def second(x: U32, f: U32 -> U32) -> U32:\n  f(x)\n\n",
            "def relay(f: U32 -> U32) -> U32:\n  each(f)\n\n",
            "def open(f: U32 -> U32, x: U32) -> U32:\n  each(f, x)\n\n",
            "def main(m: U32 -> U32) -> U32:\n",
            "  g = m\n  a = each(g, 1)\n  b = each(U32.show, 1)\n  c = open(m, 1)\n  d = second(1)\n  e = each(m, 1)\n  relay(work)\n",
        ),
    )]);
    let book = program.book("main.bend");
    let precise = graph_at(&book, "main.bend", "main", Precision::Precise).expect("a graph");
    // `each` calls position 0, `second` position 1; `relay` and `open` call none. `main` walks:
    //   each(g, 1)          position 0 is a `let` whose value nothing says (`g = m`, and `m` is unknown): local; the `1` at position 1 is not called: not counted
    //   each(U32.show, 1)   position 0 is a Base def: value
    //   open(m, 1)          `open` calls nothing: not counted
    //   second(1)           position 1 is missing: missing
    //   each(m, 1)          position 0 is `main`'s own parameter, and `main`'s key names no `m`: open
    //   relay(work)         `relay` calls nothing: not counted; it names `f`, so `relay@{f: work}` is walked
    // `open` is walked under the open key (nothing was named): `each(f, x)` has `f` as its own parameter, unknown: open. `relay@{f: work}` walks `each(f)`: `f` is
    // known, and the missing `x` is not called: not counted.
    assert_eq!(counter(&precise, "key_slot_local"), 1);
    assert_eq!(counter(&precise, "key_slot_value"), 1);
    assert_eq!(counter(&precise, "key_slot_missing"), 1);
    assert_eq!(counter(&precise, "key_slot_open"), 2);
}

/// What a variable holds, in a program of one `main`: `defs` are written first, and the precise graph of `main` is asked for.
fn variable_graph(defs: &str, main: &str) -> Graph {
    let program = Program::new(&[("main.bend", &format!("{defs}\n{main}"))]);
    let book = program.book("main.bend");
    graph_at(&book, "main.bend", "main", Precision::Precise).expect("a graph")
}

const DEFS: &str = concat!(
    "def work(x: U32) -> U32:\n  x\n\n",
    "def other(x: U32) -> U32:\n  x\n\n",
    "def each(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n",
);

/// What the calls leaving `from` (a def's qualname) reach, as text, sorted: the callee of every call edge, or why it has none. A def handed over and a construction
/// are not calls.
fn reached_from(graph: &Graph, from: &str) -> Vec<String> {
    let mut reached: Vec<String> = graph
        .edges
        .iter()
        .filter(|e| matches!(&e.from.func, StableDef::Path { qname, .. } if qname == from))
        .filter_map(|e| match &e.to {
            EdgeTo::Context(..) if e.passed => None,
            EdgeTo::Context(id, _) => Some(label(graph, &id.func)),
            EdgeTo::Unresolved(reason) => Some(format!("unresolved {}", reason.as_str())),
            _ => None,
        })
        .collect();
    reached.sort();
    reached
}

/// A slot may hold a set (V6): a variable that holds two defs is one slot, one context of the callee, and the callee's one site has both targets.
#[test]
fn a_set_in_an_argument_is_one_slot() {
    let graph = variable_graph(
        DEFS,
        concat!(
            "def main(b: Bool) -> U32:\n",
            "  g = (match b:\n         case True{}:\n           work\n         case False{}:\n           other)\n",
            "  each(g, 1)\n",
        ),
    );
    assert_eq!(contexts_of(&graph, "each").len(), 1);
    assert_ne!(contexts_of(&graph, "each")[0].digest, ZEROS);
    assert_eq!(reached_from(&graph, "each"), ["other", "work"]);
    assert_eq!(counter(&graph, "key_slot_value"), 0);
    assert_eq!(counter(&graph, "key_slot_local"), 0);
}

/// A variable that holds a def and a lambda is both answers at the one site, the lambda's once however many there are; one that may hold something else too has the
/// book's answer beside them.
#[test]
fn a_variable_is_called_as_every_member_and_as_unknown_when_it_may_be() {
    let graph = variable_graph(
        DEFS,
        concat!(
            "def main(b: Bool) -> U32:\n",
            "  g = (match b:\n         case True{}:\n           work\n         case False{}:\n           y => other(y))\n",
            "  g(1)\n",
        ),
    );
    assert_eq!(
        reached_from(&graph, "main"),
        ["other", "unresolved anonymous_function_called", "work"]
    );
    // `h` is main's own parameter and nobody named it
    let graph = variable_graph(
        DEFS,
        concat!(
            "def main(b: Bool, h: U32 -> U32) -> U32:\n",
            "  g = (match b:\n         case True{}:\n           work\n         case False{}:\n           h)\n",
            "  g(1)\n",
        ),
    );
    assert_eq!(
        reached_from(&graph, "main"),
        ["unresolved function_typed_value", "work"]
    );
}

/// A `case` with a name pattern takes the scrutinee's value; a constructor pattern's names have none.
#[test]
fn a_case_name_takes_the_scrutinee_and_a_constructor_pattern_does_not() {
    let graph = variable_graph(
        DEFS,
        "def main() -> U32:\n  match work:\n    case g:\n      g(1)\n",
    );
    assert_eq!(reached_from(&graph, "main"), ["work"]);
    let graph = variable_graph(
        "type Holder is Data:\n  Hold{f: U32 -> U32}\n\ndef work(x: U32) -> U32:\n  x\n",
        "def main() -> U32:\n  match Hold{work}:\n    case Hold{g}:\n      g(1)\n",
    );
    assert_eq!(
        reached_from(&graph, "main"),
        ["unresolved function_typed_value"]
    );
}

/// A set of more than eight members is unknown, and counted (`value_widened_set`): the nine cases of a `match` make nine defs, and the call is the book's.
#[test]
fn a_set_past_the_cap_is_unknown_and_counted() {
    let defs: String = (1..=9)
        .map(|n| format!("def f{n}(x: U32) -> U32:\n  x\n\n"))
        .collect();
    let mut cases: Vec<String> = (1..=8)
        .map(|n| format!("         case {n}:\n           f{n}\n"))
        .collect();
    cases.push("         case m:\n           f9)\n".to_owned());
    let main = format!(
        "def main(n: U32) -> U32:\n  g = (match n:\n{}  g(1)\n",
        cases.concat()
    );
    let graph = variable_graph(&defs, &main);
    assert_eq!(counter(&graph, "value_widened_set"), 1);
    assert_eq!(
        reached_from(&graph, "main"),
        ["unresolved function_typed_value"]
    );
    // eight members is the cap itself, and fits
    let defs8: String = (1..=8)
        .map(|n| format!("def f{n}(x: U32) -> U32:\n  x\n\n"))
        .collect();
    let mut cases: Vec<String> = (1..=7)
        .map(|n| format!("         case {n}:\n           f{n}\n"))
        .collect();
    cases.push("         case m:\n           f8)\n".to_owned());
    let main = format!(
        "def main(n: U32) -> U32:\n  g = (match n:\n{}  g(1)\n",
        cases.concat()
    );
    let graph = variable_graph(&defs8, &main);
    assert_eq!(counter(&graph, "value_widened_set"), 0);
    assert_eq!(reached_from(&graph, "main").len(), 8);
}
