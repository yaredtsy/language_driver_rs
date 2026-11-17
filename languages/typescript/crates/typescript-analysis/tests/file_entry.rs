//! `Entry::File` (D32, M step 2): the file's own code is a def of its own, the module. Each fixture is one pattern of the 461 call sites that no tree drew
//! (`docs/03-typescript/09-module-def.md` §1). **The expected trees were written by hand before the first run.**

mod common;

use std::path::{Path, PathBuf};

use analyzer_testkit::graph::check_invariants;
use common::{STYLE, edges_as_text};
use typescript_analysis::{
    Budget, Cancel, DefKind, Entry, Graph, Options, Project, RenderOptions, Session, StableDef,
    WireOptions, WireStyle, call_graph, render, to_text, wire,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/file-entry")
}

fn project() -> Project {
    Project::open(&root(), Path::new("src/store.ts")).unwrap()
}

fn file_graph(file: &str) -> Option<Graph> {
    call_graph(
        &project(),
        file,
        &Entry::File,
        &Options::new(STYLE),
        &Cancel::new(),
    )
}

fn tree_of(file: &str) -> String {
    let graph = file_graph(file).unwrap_or_else(|| panic!("no file entry for {file}"));
    check_invariants(&graph);
    to_text(&render(
        &graph,
        &graph.entries[0],
        &RenderOptions::default(),
    ))
}

#[test]
fn a_store_draws_the_calls_of_its_anonymous_actions_under_the_module() {
    // `create` is a call at the top, and `log` is in the arrow held by a property of an object literal passed to it: an anonymous function, so the module's
    // own. `set` is a parameter: unresolved, and the tree draws no unresolved edge.
    assert_eq!(tree_of("src/store.ts"), "src/store.ts\n  create\n  log\n");
}

#[test]
fn a_forward_ref_component_is_drawn_under_the_module() {
    assert_eq!(
        tree_of("src/button.ts"),
        "src/button.ts\n  forwardRef\n  label\n"
    );
}

#[test]
fn a_test_files_callbacks_are_drawn_under_the_module() {
    assert_eq!(
        tree_of("src/spec.test.ts"),
        "src/spec.test.ts\n  describe\n  it\n  f\n"
    );
}

#[test]
fn a_schema_callback_is_drawn_under_the_module() {
    // `schema.object` is a def (an arrow-or-method of a `const` literal), it constructs a `Schema`; `refine` is a method; `check` is in the callback
    assert_eq!(
        tree_of("src/schema.ts"),
        "src/schema.ts\n  schema.object\n    Schema\n  Schema.refine\n  check\n"
    );
}

#[test]
fn a_property_assigned_function_is_drawn_under_the_module() {
    assert_eq!(tree_of("src/assigned.ts"), "src/assigned.ts\n  g\n");
}

#[test]
fn an_iife_is_drawn_under_the_module() {
    assert_eq!(tree_of("src/iife.ts"), "src/iife.ts\n  h\n");
}

#[test]
fn a_file_with_no_top_level_code_is_the_root_alone() {
    assert_eq!(tree_of("src/empty.ts"), "src/empty.ts\n");
    let graph = file_graph("src/empty.ts").unwrap();
    assert!(graph.edges.is_empty());
}

#[test]
fn the_root_row_is_the_files_own() {
    let graph = file_graph("src/store.ts").unwrap();
    let row = graph
        .defs
        .iter()
        .find(|def| def.kind == DefKind::Module)
        .expect("the module's row");
    assert_eq!(row.qname, "src/store.ts");
    assert_eq!(row.qualname, "");
    assert_eq!(row.occurrence, 0);
    assert_eq!(row.file, "src/store.ts");
    let text = std::fs::read_to_string(root().join("src/store.ts")).unwrap();
    assert_eq!(
        (row.range.start, row.range.end),
        (0, u32::try_from(text.len()).unwrap())
    );
    assert_eq!(
        row.id,
        StableDef::Path {
            module: "src/store.ts".into(),
            qname: String::new(),
            nth: 0
        }
    );
}

#[test]
fn the_wire_root_is_a_module_with_no_name_and_no_id() {
    let graph = file_graph("src/store.ts").unwrap();
    let style = WireStyle {
        function_prefix: "F/".into(),
        class_prefix: "C/".into(),
    };
    let frame = wire(
        &graph,
        &graph.entries[0],
        &project(),
        &WireOptions::new(style, true, RenderOptions::default()),
    );
    assert_eq!(frame.target_kind, Some(DefKind::Module));
    assert_eq!(frame.target_qualname, "");
    assert_eq!(frame.target_id, "");
    assert_eq!(frame.target_file.as_deref(), Some("src/store.ts"));
    let names: Vec<&str> = frame
        .children
        .iter()
        .map(|child| child.target_qualname.as_str())
        .collect();
    assert_eq!(names, ["create", "log"]);
}

#[test]
fn the_edges_of_the_module_are_read_at_the_files_own_offsets() {
    let graph = file_graph("src/store.ts").unwrap();
    assert_eq!(
        edges_as_text(&graph, &root()),
        [
            "src/store.ts | create | call create",
            "src/store.ts | set | unresolved function_typed_value",
            "src/store.ts | log | call log",
            "create | init | unresolved function_typed_value",
        ]
    );
}

#[test]
fn a_function_entry_is_what_it_was() {
    let entry = Entry::Def {
        qualname: "mid",
        occurrence: 0,
    };
    let graph = call_graph(
        &project(),
        "src/cut.ts",
        &entry,
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .unwrap();
    assert_eq!(
        to_text(&render(
            &graph,
            &graph.entries[0],
            &RenderOptions::default()
        )),
        "mid\n  leaf\n"
    );
}

#[test]
fn a_session_gives_the_module_a_budget_and_a_cut_def_continues_from_there() {
    let session = Session::open(&root(), Path::new("src/cut.ts"), Options::new(STYLE)).unwrap();
    let first = session
        .graph(
            "src/cut.ts",
            &Entry::File,
            &Budget::new(Some(1), None),
            &Cancel::new(),
        )
        .unwrap();
    check_invariants(&first);
    assert_eq!(first.stats.frontier, 1);
    let drawn = to_text(&render(
        &first,
        &first.entries[0],
        &RenderOptions::default(),
    ));
    assert!(drawn.starts_with("src/cut.ts\n  mid"), "{drawn}");
    // `mid` asked for as a new entry continues from there
    let continued = session
        .graph(
            "src/cut.ts",
            &Entry::Def {
                qualname: "mid",
                occurrence: 0,
            },
            &Budget::default(),
            &Cancel::new(),
        )
        .unwrap();
    assert_eq!(
        to_text(&render(
            &continued,
            &continued.entries[0],
            &RenderOptions::default()
        )),
        "mid\n  leaf\n"
    );
}

#[test]
fn a_file_outside_the_root_or_in_node_modules_has_no_file_entry() {
    let outside = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/outside-root/app");
    let app = Project::open(&outside, Path::new("src/main.ts")).unwrap();
    let none = |project: &Project, file: &str| {
        call_graph(
            project,
            file,
            &Entry::File,
            &Options::new(STYLE),
            &Cancel::new(),
        )
        .is_none()
    };
    assert!(none(&app, "../shared/lib.ts"));
    assert!(
        call_graph(
            &app,
            "src/main.ts",
            &Entry::File,
            &Options::new(STYLE),
            &Cancel::new()
        )
        .is_some()
    );
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/node-modules-source");
    let inner = Project::open(&modules, Path::new("src/main.ts")).unwrap();
    assert!(none(&inner, "node_modules/lib/index.ts"));
    assert!(
        call_graph(
            &inner,
            "src/main.ts",
            &Entry::File,
            &Options::new(STYLE),
            &Cancel::new()
        )
        .is_some()
    );
    // and a file the project does not hold, or that is not code
    assert!(none(&inner, "src/nothing.ts"));
}
