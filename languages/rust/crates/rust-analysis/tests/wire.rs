//! The wire (RS4 §8, §9 step 8; `docs/05-python-migration/04-the-wire.md`): the output that is used, a view of the graph. One policy entry through `wire`
//! unmerged and merged, with `WIRE_FORMAT_VERSION` asserted, and the text the wire's lines and columns are counted in.

use std::path::{Path, PathBuf};

use analyzer_core::wire::WIRE_FORMAT_VERSION;
use rust_analysis::{
    Cancel, Entry, Frame, Graph, LoadOptions, MarkerStyle, Options, Project, RenderOptions,
    Sources, WireOptions, WireStyle, call_graph, wire,
};

mod common;
use common::{copy_of_fixture, one_load, tmp_root};

fn open(root: &Path, file: &str) -> Project {
    Project::open(root, Path::new(file), &LoadOptions::default()).expect("the fixture loads")
}

fn graph(project: &Project, file: &str, qualname: &str) -> Graph {
    call_graph(
        project,
        file,
        &Entry::Def {
            qualname,
            occurrence: 0,
        },
        &Options::new(MarkerStyle::docstring("ID")),
        &Cancel::new(),
    )
    .unwrap_or_else(|| panic!("{file}:{qualname}"))
}

fn frame(project: &Project, graph: &Graph, merged: bool, externals: bool) -> Frame {
    let style = WireStyle {
        function_prefix: "fn:".to_owned(),
        class_prefix: "class:".to_owned(),
    };
    let render = {
        let mut render = RenderOptions::default();
        render.show_external = externals;
        render
    };
    let options = WireOptions::new(style, merged, render);
    wire(graph, &graph.entries[0], project, &options)
}

fn json(frame: &Frame) -> String {
    serde_json::to_string_pretty(frame).unwrap() + "\n"
}

/// The wire is a version of the format, and the policy entries are where the options change the answer: a def called at two sites is two nodes unmerged and one
/// merged with `call_count` 1; a def both called and passed is two nodes in both, because the merged wire keys on `passed` (D22); `Some`, `Ok` and `Err` are
/// three nodes in both, because an external variant keeps its name (RS4 §5), where naming them by their enum would have merged `Ok` and `Err`; a marked def
/// is named with the caller's prefix and a def named by where it is is not.
#[test]
fn a_policy_entry_through_the_wire_unmerged_and_merged() {
    let _load = one_load();
    assert_eq!(
        WIRE_FORMAT_VERSION, 2,
        "the format the snapshots were written for"
    );
    let copy = copy_of_fixture("policy", "wire_policy");
    let project = open(&copy, "src/lib.rs");
    for (file, qualname, externals) in [
        ("src/calls.rs", "calls_helper_twice", false),
        ("src/passed.rs", "called_and_passed", false),
        ("src/constructions.rs", "build_external", true),
        ("src/lib.rs", "uses_the_twins", false),
    ] {
        let graph = graph(&project, file, qualname);
        let unmerged = frame(&project, &graph, false, externals);
        let merged = frame(&project, &graph, true, externals);
        let out = format!("unmerged\n{}\nmerged\n{}", json(&unmerged), json(&merged));
        let name = format!("{}__{qualname}.json", file.replace('/', "__"));
        let snapshot = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/wire")
            .join(name);
        analyzer_testkit::expect_file(&snapshot, &out);
        // the merge only joins: the same graph, fewer nodes
        assert!(count(&merged) <= count(&unmerged), "{qualname}");
    }
}

/// D45. A marked struct, enum and union are built, not called, so the wire gives them the style's class prefix, and what each really is stays in
/// `target_kind`: as the entry of a request (a construction with nothing to enter) and as the target of a construction in a body.
#[test]
fn a_struct_an_enum_and_a_union_have_the_class_prefix_and_keep_their_kind() {
    use rust_analysis::DefKind;

    let _load = one_load();
    let copy = copy_of_fixture("kinds", "wire_kinds");
    let project = open(&copy, "src/lib.rs");
    let types = [
        ("S", "build_struct", "class:kinds-struct", DefKind::Struct),
        ("E", "build_enum", "class:kinds-enum", DefKind::Enum),
        ("U", "build_union", "class:kinds-union", DefKind::Union),
    ];
    for (name, builder, id, kind) in types {
        // as an entry: the root frame is the type
        let entry = frame(&project, &graph(&project, "src/lib.rs", name), false, false);
        assert_eq!(entry.target_id, id, "{name} as an entry");
        assert_eq!(entry.target_kind, Some(kind.clone()), "{name} as an entry");

        // as a construction: the child frame of the function that builds it
        let built = frame(
            &project,
            &graph(&project, "src/lib.rs", builder),
            false,
            false,
        );
        let [child] = built.children.as_slice() else {
            panic!("{builder}: {:?}", built.children.len());
        };
        assert_eq!(child.target_id, id, "{name} as a construction");
        assert_eq!(child.target_kind, Some(kind), "{name} as a construction");
        // the function that builds it is called, and has no marker: no prefix at all
        assert_eq!(built.target_id, "");
    }
}

fn count(frame: &Frame) -> usize {
    1 + frame.children.iter().map(count).sum::<usize>()
}

/// A crate of one file, written byte by byte: a byte-order mark and `\r\n` line ends.
fn bom_crlf_crate(name: &str, source: &str) -> PathBuf {
    let root = tmp_root().join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"bomcrlf\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), source).unwrap();
    root
}

/// The wire's text is the engine's, so its lines and columns use the graph's offsets. A file with a byte-order mark and `\r\n` line ends: a call on line 1 and
/// one on line 2.
#[test]
fn the_wire_counts_lines_and_columns_in_the_text_the_graph_counts_bytes_in() {
    let source = "\u{feff}pub fn first() { target(); }\r\npub fn second() { target(); }\r\nfn target() {}\r\n";
    let root = bom_crlf_crate("wire_bom_crlf", source);
    let project = open(&root, "src/lib.rs");

    // the text is what the engine loaded, byte for byte: the mark and the `\r\n` are there
    let text = project.text("src/lib.rs").expect("a file of the project");
    assert_eq!(text, source);
    assert_eq!(project.text("src/missing.rs"), None);

    let site = |qualname: &str| {
        let graph = graph(&project, "src/lib.rs", qualname);
        let frame = frame(&project, &graph, false, false);
        let [child] = frame.children.as_slice() else {
            panic!("{qualname}: {:?}", frame.children.len());
        };
        (
            child.call_site.expect("a call site"),
            child.call_site_file.clone(),
        )
    };
    let (first, file) = site("first");
    assert_eq!(file.as_deref(), Some("src/lib.rs"));
    let (second, _) = site("second");
    // lines: `\r\n` ends a line, once, and the mark is on line 1
    assert_eq!((first.start.0, second.start.0), (1, 2));
    assert_eq!((first.end.0, second.end.0), (1, 2));
    // columns count characters, 1-based, and a leading byte-order mark is no column (core's `Positions::at` skips it, and the offsets stay bytes). `target` is
    // after 17 characters on line 1, `pub fn first() { `, so column 18, where an editor puts it; and after 18 on line 2, `pub fn second() { `, so column 19.
    assert_eq!((first.start.1, first.end.1), (18, 24));
    assert_eq!((second.start.1, second.end.1), (19, 25));
}
