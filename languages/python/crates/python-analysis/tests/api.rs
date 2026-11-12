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

/// Every def of every file of the `08-entries` project, as (file, outline qualname, occurrence), in file order and then source order.
fn every_def() -> Vec<(String, String, u16)> {
    use python_analysis::MarkerStyle;
    let mut files: Vec<_> = std::fs::read_dir(ENTRIES)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "py"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let name = path.file_name().unwrap().to_str().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let outline = python_syntax::outline(&text, MarkerStyle::docstring("ID"));
        for (i, def) in outline.defs.iter().enumerate() {
            let occurrence = outline.defs[..i]
                .iter()
                .filter(|d| d.qualname == def.qualname)
                .count() as u16;
            out.push((name.to_owned(), def.qualname.clone(), occurrence));
        }
    }
    out
}

#[test]
fn every_def_of_the_outline_round_trips() {
    let defs = every_def();
    for (name, qualname, occurrence) in &defs {
        let frame = wire_of(name, qualname, *occurrence)
            .unwrap_or_else(|| panic!("{name}: no frame for {qualname} #{occurrence}"));
        assert_eq!(
            (frame.target_qualname.as_str(), frame.target_occurrence),
            (qualname.as_str(), *occurrence),
            "{name}"
        );
    }
    assert!(defs.len() > 15, "only {} defs were asked for", defs.len());
}

/// A frame and everything under it, as the fields a driver reads: the target's name, where the call was made, and the marks. Not the contexts,
/// which are the solver's.
#[derive(Debug, PartialEq)]
struct Shape {
    target: (Option<String>, String, u16),
    call_site: Option<(u32, u32)>,
    call_site_file: Option<String>,
    marks: Vec<String>,
    children: Vec<Shape>,
}

fn shape(frame: &python_analysis::Frame) -> Shape {
    Shape {
        target: (
            frame.target_file.clone(),
            frame.target_qualname.clone(),
            frame.target_occurrence,
        ),
        call_site: frame.call_site.map(|s| s.start),
        call_site_file: frame.call_site_file.clone(),
        marks: frame.marks.clone(),
        children: frame.children.iter().map(shape).collect(),
    }
}

#[test]
fn a_class_is_an_entry_and_its_inherited_constructor_is_in_another_file() {
    let frame = wire_of("inherit_a.py", "Child", 0).unwrap();
    assert_eq!(
        (
            frame.target_kind.clone(),
            frame.target_file.as_deref(),
            frame.call_site_file.as_deref()
        ),
        (
            Some(python_syntax::DefKind::Class),
            Some("inherit_a.py"),
            None
        )
    );
    assert_eq!(child_names(&frame), ["helper"]);
    let helper = &frame.children[0];
    assert_eq!(helper.target_file.as_deref(), Some("inherit_b.py"));
    assert_eq!(helper.call_site_file.as_deref(), Some("inherit_b.py"));
    assert_eq!(
        helper.call_site.unwrap().start.0,
        7,
        "the line of `helper()` in inherit_b.py"
    );
}

#[test]
fn an_entry_on_a_class_and_a_call_that_constructs_it_draw_the_same_subtree() {
    let entry = wire_of("inherit_a.py", "Child", 0).unwrap();
    let make = wire_of("inherit_a.py", "make", 0).unwrap();
    assert_eq!(child_names(&make), ["Child"]);
    let built = &make.children[0];
    assert_eq!(built.call_site_file.as_deref(), Some("inherit_a.py"));
    assert_eq!(
        built.call_site.unwrap().start.0,
        9,
        "the line of `Child()` in inherit_a.py"
    );
    // the class itself is the same target; only how it was reached differs
    assert_eq!(
        (
            &built.target_file,
            &built.target_qualname,
            built.target_occurrence
        ),
        (
            &entry.target_file,
            &entry.target_qualname,
            entry.target_occurrence
        )
    );
    let under = |f: &python_analysis::Frame| f.children.iter().map(shape).collect::<Vec<_>>();
    assert_eq!(under(built), under(&entry));
}

#[test]
fn a_class_with_no_project_constructor_is_an_entry_with_nothing_under_it() {
    let frame = wire_of("no_init.py", "Plain", 0).unwrap();
    assert!(
        frame.children.is_empty() && frame.marks.is_empty(),
        "{frame:?}"
    );
    assert_eq!(frame.target_qualname, "Plain");
}

#[test]
fn twin_classes_are_asked_for_by_occurrence() {
    let frame = wire_of("twins.py", "Impl", 1).unwrap();
    assert!(frame.children.is_empty());
    assert_eq!(frame.target_occurrence, 1);
}

#[test]
fn the_constructor_context_of_a_class_entry_is_the_one_a_construction_has() {
    use python_analysis::{EdgeTo, Root, call_graph};
    let project = entries_project();
    let opts = Options::default();
    let ask = |qualname| {
        let entry = Entry {
            qualname,
            occurrence: 0,
        };
        call_graph(&project, "inherit_a.py", &entry, &opts).unwrap()
    };
    let Root::Construct {
        init: Some(root), ..
    } = ask("Child").entries[0].clone()
    else {
        panic!("the class entry has no constructor context");
    };
    let make = ask("make");
    let built = make
        .edges
        .iter()
        .find_map(|e| match &e.to {
            EdgeTo::Construct {
                init: Some(init), ..
            } => Some(init.clone()),
            _ => None,
        })
        .expect("`make` constructs `Child`");
    assert_eq!(
        root, built,
        "same function and same digest: one context, shared"
    );
}

#[test]
fn a_class_whose_constructor_takes_arguments_is_an_entry_with_what_it_calls() {
    let frame = wire_of("with_args.py", "WithArgs", 0).unwrap();
    assert_eq!(child_names(&frame), ["helper"]);
    assert_eq!(
        frame.children[0].call_site_file.as_deref(),
        Some("with_args.py")
    );
}

/// A path the way a driver reads it: relative to the project root, with `/`. On Windows `canonicalize` gives `\\?\C:\proj`, and a root that is
/// not stripped leaves a drive and backslashes in every file name. This passes on every other OS before and after the fix (they have no
/// verbatim prefix to leave in); it can only fail on Windows.
fn assert_relative(what: &str, file: &str) {
    let path = std::path::Path::new(file);
    assert!(
        !path.is_absolute() && !file.contains('\\') && !file.contains(':'),
        "{what}: `{file}` is not a relative path with `/`"
    );
}

fn assert_frame_files_relative(frame: &python_analysis::Frame) {
    let mut stack = vec![frame];
    while let Some(f) = stack.pop() {
        for (what, file) in [
            ("target_file", &f.target_file),
            ("call_site_file", &f.call_site_file),
        ] {
            if let Some(file) = file {
                assert_relative(what, file);
            }
        }
        stack.extend(&f.children);
    }
}

#[test]
fn files_are_relative_and_the_root_does_not_change_the_answer() {
    let by_relative = Project::open(ENTRIES).unwrap();
    let absolute = std::fs::canonicalize(ENTRIES).unwrap();
    let by_absolute = Project::open(absolute.to_str().unwrap()).unwrap();
    let opts = Options::default();
    for (file, qualname, occurrence) in every_def() {
        let entry = Entry {
            qualname: &qualname,
            occurrence,
        };
        let graph = call_graph(&by_relative, &file, &entry, &opts).unwrap();
        for def in &graph.defs {
            assert_relative("DefInfo::file", &def.file);
        }
        let frame = call_wire(&by_relative, &file, &entry, &opts, false).unwrap();
        assert_frame_files_relative(&frame);
        assert_eq!(
            call_graph(&by_absolute, &file, &entry, &opts).as_ref(),
            Some(&graph),
            "{file} {qualname} #{occurrence}: the project opened by its absolute path"
        );
    }
}
