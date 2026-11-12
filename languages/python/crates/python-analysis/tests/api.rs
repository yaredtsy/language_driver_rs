//! The public surface and nothing else: `Project`, `call_graph`, `call_wire`, and core's renderer on the graph. If this file needs
//! something that is not in `lib.rs`'s list, the surface is wrong, not this file.

use python_analysis::{
    Entry, MarkerStyle, OpenError, Options, Project, WireStyle, call_graph, call_wire, render,
    to_text,
};

const DIR: &str = "tests/fixtures/06-wire/python";
const FILE: &str = "ids_and_classes.py";

/// The first def called `qualname` in its file.
fn first(qualname: &str) -> Entry<'_> {
    Entry {
        qualname,
        occurrence: 0,
    }
}

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
    let graph = call_graph(&project, FILE, &first("main"), &opts).unwrap();
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
    let plain = call_wire(&project, FILE, &first("main"), &styled(), false).unwrap();
    assert_eq!(plain.target_id, "F/06000000-0003-0000-0000-000000000000");
    assert_eq!(
        plain.children[0].target_id,
        "C/06000000-0001-0000-0000-000000000000"
    );
    assert_eq!(plain.children.len(), 4);
    let merged = call_wire(&project, FILE, &first("main"), &styled(), true).unwrap();
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
    assert!(call_graph(&project, "no_such_file.py", &first("main"), &opts).is_none());
    assert!(call_graph(&project, FILE, &first("no_such_function"), &opts).is_none());
    assert!(call_wire(&project, FILE, &first("no_such_function"), &opts, false).is_none());
}

#[test]
fn the_marker_label_is_the_callers() {
    let project = Project::open(DIR).unwrap();
    let other = Options {
        marker: MarkerStyle::docstring("Other"),
        ..styled()
    };
    let wire = call_wire(&project, FILE, &first("main"), &other, false).unwrap();
    assert_eq!(wire.target_id, "", "no `Other:` line, so no id");
}

#[test]
fn a_file_may_be_given_by_its_absolute_path() {
    let project = Project::open(DIR).unwrap();
    let absolute = std::fs::canonicalize(format!("{DIR}/{FILE}")).unwrap();
    let opts = Options::default();
    let by_absolute =
        call_graph(&project, absolute.to_str().unwrap(), &first("main"), &opts).unwrap();
    let by_relative = call_graph(&project, FILE, &first("main"), &opts).unwrap();
    assert_eq!(by_absolute, by_relative);
}

const ENTRIES: &str = "tests/fixtures/08-entries/python";

fn entries_project() -> Project {
    Project::open(ENTRIES).unwrap()
}

/// The wire of the `occurrence`th def called `qualname` in `file` of the `08-entries` project, unmerged.
fn wire_of(file: &str, qualname: &str, occurrence: u16) -> Option<python_analysis::Frame> {
    call_wire(
        &entries_project(),
        file,
        &Entry {
            qualname,
            occurrence,
        },
        &Options::default(),
        false,
    )
}

/// What a frame's children are called, by outline qualname.
fn child_names(frame: &python_analysis::Frame) -> Vec<&str> {
    frame
        .children
        .iter()
        .map(|c| c.target_qualname.as_str())
        .collect()
}

#[test]
fn twin_defs_are_asked_for_by_occurrence() {
    // the condition is not one ty can decide, so neither branch is unreachable
    for (occurrence, callee) in [(0, "a"), (1, "b")] {
        let frame = wire_of("twins.py", "f", occurrence).unwrap();
        assert_eq!(child_names(&frame), [callee], "f #{occurrence}");
        assert_eq!(frame.target_occurrence, occurrence);
    }
    assert!(
        wire_of("twins.py", "f", 2).is_none(),
        "only two defs called f"
    );
}

#[test]
fn twin_methods_are_asked_for_by_occurrence() {
    let frame = wire_of("twins.py", "Impl.run", 1).unwrap();
    assert_eq!(child_names(&frame), ["b"]);
    assert_eq!(frame.target_occurrence, 1);
}

#[test]
fn a_nested_def_is_asked_for_by_the_outlines_name() {
    let frame = wire_of("nested.py", "top.<locals>.inner", 0).unwrap();
    assert_eq!(child_names(&frame), ["leaf"]);
    // the dotted form some fixtures use is not the outline's name, and is no longer accepted
    assert!(wire_of("nested.py", "top.inner", 0).is_none());
}

#[test]
fn a_def_inside_a_block_is_an_entry() {
    for name in ["in_try", "in_with"] {
        let frame = wire_of("blocks.py", name, 0).unwrap();
        assert_eq!(child_names(&frame), ["leaf"], "{name}");
    }
}

#[test]
fn every_function_of_the_outline_round_trips() {
    use python_analysis::MarkerStyle;
    use python_syntax::DefKind;
    let mut files: Vec<_> = std::fs::read_dir(ENTRIES)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "py"))
        .collect();
    files.sort();
    let mut asked = 0;
    for path in files {
        let name = path.file_name().unwrap().to_str().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let outline = python_syntax::outline(&text, MarkerStyle::docstring("ID"));
        for (i, def) in outline.defs.iter().enumerate() {
            // classes join in E5: a class is not a function and has no tree of its own yet
            if def.kind == DefKind::Class {
                continue;
            }
            let occurrence = outline.defs[..i]
                .iter()
                .filter(|d| d.qualname == def.qualname)
                .count() as u16;
            let frame = wire_of(name, &def.qualname, occurrence)
                .unwrap_or_else(|| panic!("{name}: no frame for {} #{occurrence}", def.qualname));
            assert_eq!(
                (frame.target_qualname.as_str(), frame.target_occurrence),
                (def.qualname.as_str(), occurrence),
                "{name}"
            );
            asked += 1;
        }
    }
    assert!(asked > 15, "only {asked} defs were asked for");
}
