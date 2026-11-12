//! The public surface and nothing else: `Project`, `call_graph`, `call_wire`, and core's renderer on the graph. If this file needs
//! something that is not in `lib.rs`'s list, the surface is wrong, not this file.

use python_analysis::{
    MarkerStyle, OpenError, Options, Project, WireStyle, call_graph, call_wire, render, to_text,
};

const DIR: &str = "tests/fixtures/06-wire/python";
const FILE: &str = "ids_and_classes.py";

fn styled() -> Options {
    Options {
        wire: WireStyle {
            function_prefix: "F/".into(),
            class_prefix: "C/".into(),
        },
        ..Options::default()
    }
}

#[test]
fn a_directory_that_is_not_one_is_an_error_and_says_which() {
    let Err(error) = Project::open("tests/fixtures/no-such-directory") else {
        panic!("a missing directory opened");
    };
    assert_eq!(
        error,
        OpenError::NotADirectory("tests/fixtures/no-such-directory".into())
    );
    assert_eq!(
        error.to_string(),
        "tests/fixtures/no-such-directory is not a directory"
    );
    // a file is not a directory either
    assert!(Project::open(&format!("{DIR}/{FILE}")).is_err());
}

#[test]
fn the_graph_draws_as_a_tree_with_core_s_renderer() {
    let project = Project::open(DIR).unwrap();
    let opts = Options::default();
    let graph = call_graph(&project, FILE, "main", &opts).unwrap();
    let tree = render(&graph, &graph.entries[0], &opts.render_options());
    assert_eq!(
        to_text(&tree),
        "ids_and_classes.main\n  ids_and_classes.Report\n    ids_and_classes.Report.prepare\n  ids_and_classes.marked\n    ids_and_classes.unmarked\n  ids_and_classes.unmarked\n  ids_and_classes.unmarked\n"
    );
    // every context's function has a row, every class too: the graph is well formed
    assert!(graph.def(graph.entries[0].func()).is_some());
}

#[test]
fn the_wire_has_ids_by_kind_and_merges_on_request() {
    let project = Project::open(DIR).unwrap();
    let plain = call_wire(&project, FILE, "main", &styled(), false).unwrap();
    assert_eq!(plain.target_id, "F/06000000-0003-0000-0000-000000000000");
    assert_eq!(
        plain.children[0].target_id,
        "C/06000000-0001-0000-0000-000000000000"
    );
    assert_eq!(plain.children.len(), 4);
    let merged = call_wire(&project, FILE, "main", &styled(), true).unwrap();
    assert_eq!(
        merged.children.len(),
        3,
        "the two plain calls to `unmarked` are one node"
    );
    assert!(plain.call_site.is_none(), "the root is not a call");
    assert!(merged.children.iter().all(|c| c.call_site.is_some()));
}

#[test]
fn a_file_or_an_entry_that_is_not_there_is_none() {
    let project = Project::open(DIR).unwrap();
    let opts = Options::default();
    assert!(call_graph(&project, "no_such_file.py", "main", &opts).is_none());
    assert!(call_graph(&project, FILE, "no_such_function", &opts).is_none());
    assert!(call_wire(&project, FILE, "no_such_function", &opts, false).is_none());
}

#[test]
fn the_marker_label_is_the_callers() {
    let project = Project::open(DIR).unwrap();
    let other = Options {
        marker: MarkerStyle::docstring("Other"),
        ..styled()
    };
    let wire = call_wire(&project, FILE, "main", &other, false).unwrap();
    assert_eq!(wire.target_id, "", "no `Other:` line, so no id");
}

#[test]
fn a_file_may_be_given_by_its_absolute_path() {
    let project = Project::open(DIR).unwrap();
    let absolute = std::fs::canonicalize(format!("{DIR}/{FILE}")).unwrap();
    let opts = Options::default();
    let by_absolute = call_graph(&project, absolute.to_str().unwrap(), "main", &opts).unwrap();
    let by_relative = call_graph(&project, FILE, "main", &opts).unwrap();
    assert_eq!(by_absolute, by_relative);
}
