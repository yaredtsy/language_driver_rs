//! The wire (docs/05-python-migration/04-the-wire.md): the output that is used, a view of the graph. Entries of the policy fixtures through `wire`, unmerged and
//! merged, with `WIRE_FORMAT_VERSION` asserted; a marked `type` has the style's class prefix and keeps its kind; and the text the wire's lines and columns are
//! counted in. `target_kind` is the outline's: `"Function"`, `"Enum"` for a `type`, `{"Other":"law"}` for a law.

use std::path::Path;

use analyzer_core::wire::WIRE_FORMAT_VERSION;
use analyzer_testkit::expect_file;
use bend_analysis::{
    Book, DefKind, Frame, Graph, RenderOptions, WireOptions, WireStyle, merge, wire,
};

mod common;
use common::graph_of;

fn style() -> WireStyle {
    WireStyle {
        function_prefix: "fn:".to_owned(),
        class_prefix: "class:".to_owned(),
    }
}

fn frame(book: &Book, graph: &Graph, merged: bool) -> Frame {
    let options = WireOptions::new(style(), merged, RenderOptions::default());
    wire(graph, &graph.entries[0], book, &options)
}

fn json(frame: &Frame) -> String {
    serde_json::to_string_pretty(frame).expect("the wire is JSON") + "\n"
}

fn count(frame: &Frame) -> usize {
    1 + frame.children.iter().map(count).sum::<usize>()
}

fn fixture(row: &str) -> Book {
    Book::open(
        &Path::new("tests/fixtures/policy").join(row),
        Path::new("main.bend"),
    )
    .unwrap_or_else(|| panic!("{row}: the book did not open"))
}

/// The policy rows where the options change the answer: a def called at two sites is two nodes unmerged and one merged with `call_count` 1 (`template-arg`:
/// `twice`); a def handed over is its own node in both, because the merged wire keys on `passed` (D22: `passed`, `template-arg`); a type built is a node of kind
/// `Enum` (`construct`); a law is the proof when it has one (`law-call`, kind `Function`) and a leaf of kind `{"Other":"law"}` when it has not (`law-open`).
#[test]
fn a_policy_entry_through_the_wire_unmerged_and_merged() {
    assert_eq!(
        WIRE_FORMAT_VERSION, 2,
        "the format the snapshots were written for"
    );
    for row in [
        "direct",
        "construct",
        "law-call",
        "law-open",
        "template-arg",
        "passed",
    ] {
        let book = fixture(row);
        let graph = graph_of(&book, "main.bend", "main").expect("a graph");
        let unmerged = frame(&book, &graph, false);
        let merged = frame(&book, &graph, true);
        let out = format!("unmerged\n{}\nmerged\n{}", json(&unmerged), json(&merged));
        expect_file(
            &Path::new("tests/snapshots/wire").join(format!("{row}.json")),
            &out,
        );
        // the merge only joins: the same graph, fewer nodes
        assert!(count(&merged) <= count(&unmerged), "{row}");
        // `merge` of the unmerged frame is the merged one
        assert_eq!(merge(unmerged), merged, "{row}");
    }
}

/// D45: a marked `type` is built, not called, so the wire gives it the style's class prefix, and what it really is stays in `target_kind`: as the entry of a
/// request (a construction with nothing to enter) and as the target of a construction in a body. A def with no marker has no prefix at all.
#[test]
fn a_marked_type_has_the_class_prefix_and_keeps_its_kind() {
    let program = Path::new("../../../../scenarios/construct-marked-class/bend");
    let book = Book::open(program, Path::new("main.bend")).expect("the scenario opens");
    let id = "class:11111111-0000-0000-0000-000000000001";

    let entry = frame(
        &book,
        &graph_of(&book, "main.bend", "Report").expect("a graph"),
        false,
    );
    assert_eq!(entry.target_id, id, "as an entry");
    assert_eq!(entry.target_kind, Some(DefKind::Enum), "as an entry");

    let built = frame(
        &book,
        &graph_of(&book, "main.bend", "main").expect("a graph"),
        false,
    );
    let ids: Vec<(&str, Option<DefKind>)> = built
        .children
        .iter()
        .map(|c| (c.target_id.as_str(), c.target_kind.clone()))
        .collect();
    assert_eq!(
        ids,
        [
            (id, Some(DefKind::Enum)),
            (
                "class:11111111-0000-0000-0000-000000000002",
                Some(DefKind::Enum)
            ),
        ]
    );
    // `main` is called and has no marker
    assert_eq!(built.target_id, "");
}

/// The wire's text is the book's, so its lines and columns use the graph's offsets: a call on line 1 and one on line 2 of a file with a byte-order mark and
/// `\r\n` line ends. Columns count characters.
#[test]
fn the_lines_and_columns_are_the_texts_with_a_bom_and_crlf() {
    let program = common::Program::new(&[(
        "main.bend",
        "\u{feff}def f() -> U32:\r\n  1\r\n\r\ndef main() -> U32:\r\n  U32.add(f(), 'é' <> f())\r\n",
    )]);
    let book = program.book("main.bend");
    let graph = graph_of(&book, "main.bend", "main").expect("a graph");
    let root = frame(&book, &graph, false);
    let spans: Vec<_> = root
        .children
        .iter()
        .map(|c| (c.target_qname.clone(), c.call_site.expect("a site").start))
        .collect();
    // line 5 of the file: `  U32.add(f(), …`: `f` is at column 11 (two spaces, `U32.add(`)
    assert_eq!(spans[0], ("f".to_owned(), (5, 11)));
}
