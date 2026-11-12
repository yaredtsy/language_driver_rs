//! The public surface and nothing else: `Project`, `call_graph`, `call_wire`, and core's renderer on the graph. If this file needs
//! something that is not in `lib.rs`'s list, the surface is wrong, not this file.

use analyzer_core::graph::{Cut, Status};
use python_analysis::{
    Cancel, EdgeTo, Entry, Graph, MarkerStyle, OpenError, Options, Project, StableDef, WireStyle,
    call_graph, call_wire, render, to_text,
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

/// `Options::default()` with `cancel` in place of the one that never fires.
fn cancelled_with(cancel: Cancel) -> Options {
    Options {
        cancel,
        ..Options::default()
    }
}

/// The status of the root context of `main` in the wire fixture.
fn root_status(graph: &Graph) -> Status {
    let root = graph.entries[0].context().expect("the entry has a context");
    graph
        .context(root)
        .expect("the root is in the graph")
        .status
}

#[test]
fn a_cancel_fired_before_the_request_cuts_the_root_and_is_counted() {
    let project = Project::open(DIR).unwrap();
    let cancel = Cancel::new();
    cancel.cancel();
    let opts = cancelled_with(cancel);
    let graph = call_graph(&project, FILE, &first("main"), &opts).unwrap();
    assert_eq!(root_status(&graph), Status::Incomplete(Cut::Cancelled));
    assert_eq!(graph.stats.cuts.get(&Cut::Cancelled), Some(&1));
    let wire = call_wire(&project, FILE, &first("main"), &opts, false).unwrap();
    assert!(
        wire.marks.iter().any(|m| m == "incomplete"),
        "{:?}",
        wire.marks
    );
}

#[test]
fn a_deadline_of_the_cancel_is_a_deadline_cut_and_not_a_cancelled_one() {
    let project = Project::open(DIR).unwrap();
    let opts = cancelled_with(Cancel::new().with_deadline(std::time::Instant::now()));
    let graph = call_graph(&project, FILE, &first("main"), &opts).unwrap();
    assert_eq!(root_status(&graph), Status::Incomplete(Cut::Deadline));
    assert_eq!(graph.stats.cuts.get(&Cut::Deadline), Some(&1));
    assert_eq!(graph.stats.cuts.get(&Cut::Cancelled), None);
}

#[test]
fn a_cancelled_request_leaves_the_project_as_it_was() {
    let wire_json = |project: &Project, opts: &Options| {
        let frame = call_wire(project, FILE, &first("main"), opts, false).unwrap();
        serde_json::to_string(&frame).unwrap()
    };
    let kept = Project::open(DIR).unwrap();
    let cancel = Cancel::new();
    cancel.cancel();
    let cut_short = wire_json(&kept, &cancelled_with(cancel));
    let after = wire_json(&kept, &Options::default());
    let fresh = wire_json(&Project::open(DIR).unwrap(), &Options::default());
    assert_ne!(cut_short, fresh, "the cancelled request was cut");
    assert_eq!(after, fresh);
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

const ROOT: &str = "tests/fixtures/09-root";
const ROOT_CASES: [&str; 3] = ["above-pyproject", "above-ty", "own-config"];

/// `main` of `inner/app.py`, asked for on the project opened at `inner`: the folder with the program, whatever config files sit above it.
fn wire_of_inner(case: &str) -> Option<python_analysis::Frame> {
    let project = Project::open(&format!("{ROOT}/{case}/inner")).unwrap();
    call_wire(
        &project,
        "app.py",
        &first("main"),
        &Options::default(),
        false,
    )
}

#[test]
fn a_config_above_the_folder_does_not_move_the_root() {
    let none: Vec<_> = ROOT_CASES
        .into_iter()
        .filter(|case| wire_of_inner(case).is_none())
        .collect();
    assert!(
        none.is_empty(),
        "`app.py` is looked for under the folder that was opened, and was not found in {none:?}"
    );
}

#[test]
fn files_are_relative_to_the_folder_that_was_opened() {
    for case in ROOT_CASES {
        let frame = wire_of_inner(case).unwrap_or_else(|| panic!("{case}: no answer"));
        assert_eq!(frame.target_file.as_deref(), Some("app.py"), "{case}");
        assert_eq!(frame.children.len(), 1, "{case}");
        let work = &frame.children[0];
        assert_eq!(work.target_file.as_deref(), Some("helper.py"), "{case}");
        assert_eq!(work.call_site_file.as_deref(), Some("app.py"), "{case}");
    }
}

#[test]
fn module_names_do_not_carry_the_folders_above_the_root() {
    for case in ROOT_CASES {
        let frame = wire_of_inner(case).unwrap_or_else(|| panic!("{case}: no answer"));
        assert_eq!(frame.target_qname, "app.main", "{case}");
        assert_eq!(frame.children[0].target_qname, "helper.work", "{case}");
    }
}

#[test]
fn the_sibling_files_of_a_parent_are_not_project_code_and_a_folder_with_its_own_config_is_its_own_root()
 {
    let inner = Project::open(&format!("{ROOT}/above-pyproject/inner")).unwrap();
    let graph = call_graph(&inner, "app.py", &first("main"), &Options::default()).unwrap();
    for def in &graph.defs {
        assert!(
            def.file != "sibling.py" && !def.file.starts_with(".."),
            "`{}` is not in the folder that was opened",
            def.file
        );
    }
    // the parent, opened itself, is a root with the config it has: its files are named from it
    let outer = Project::open(&format!("{ROOT}/above-pyproject")).unwrap();
    let frame = call_wire(
        &outer,
        "inner/app.py",
        &first("main"),
        &Options::default(),
        false,
    )
    .unwrap();
    assert_eq!(frame.target_file.as_deref(), Some("inner/app.py"));
}

const ENGINE_CALLEE: &str = "tests/fixtures/10-engine-callee/python";

/// The graph of `run` (or the entry `qualname`) of `file` in the `10-engine-callee` project.
fn engine_graph(file: &str, qualname: &str) -> Graph {
    let project = Project::open(ENGINE_CALLEE).unwrap();
    call_graph(&project, file, &first(qualname), &Options::default()).unwrap()
}

/// What each edge out of the entry's context says, in site order: the target's qualified name and basis, or the reason it has none.
fn edges_of_entry(graph: &Graph) -> Vec<String> {
    let entry = graph.entries[0].context().unwrap();
    let name = |id: &StableDef| graph.def(id).map_or("?".to_owned(), |d| d.qname.clone());
    graph
        .edges_from(entry)
        .iter()
        .map(|edge| match &edge.to {
            EdgeTo::Context(id, basis) => format!("{} {basis:?}", name(&id.func)),
            EdgeTo::Construct { class, init, basis } => format!(
                "new {} {basis:?} init={}",
                name(class),
                init.as_ref().map_or("-".to_owned(), |i| name(&i.func))
            ),
            EdgeTo::External(external, basis) => format!("external {} {basis:?}", external.name),
            EdgeTo::Unresolved(reason) => format!("unresolved {}", reason.as_str()),
            _ => "other".to_owned(),
        })
        .collect()
}

fn tree_of(graph: &Graph) -> String {
    to_text(&render(
        graph,
        &graph.entries[0],
        &Options::default().render_options(),
    ))
}

#[test]
fn a_bound_method_the_engine_solved_for_is_called() {
    let graph = engine_graph("method.py", "Service.run");
    assert_eq!(
        edges_of_entry(&graph),
        ["wrap.passthrough Name", "method.Service.work Type"].map(String::from)
    );
    assert_eq!(graph.stats.unresolved, 0);
    assert_eq!(
        tree_of(&graph),
        "method.Service.run\n  wrap.passthrough\n  method.Service.work\n    wrap.leaf\n"
    );
}

#[test]
fn a_function_the_engine_solved_for_is_called() {
    let graph = engine_graph("function.py", "run");
    assert_eq!(
        edges_of_entry(&graph),
        ["wrap.passthrough Name", "function.helper Name"].map(String::from)
    );
    assert_eq!(graph.stats.unresolved, 0);
}

#[test]
fn a_function_held_in_a_variable_is_called() {
    let graph = engine_graph("variable.py", "run");
    assert_eq!(
        edges_of_entry(&graph),
        ["wrap.passthrough Name", "variable.helper Name"].map(String::from)
    );
    assert_eq!(graph.stats.unresolved, 0);
}

#[test]
fn a_class_the_engine_solved_for_is_constructed() {
    let graph = engine_graph("klass.py", "run");
    assert_eq!(
        edges_of_entry(&graph),
        [
            "wrap.passthrough Name",
            "new klass.Thing Name init=klass.Thing.__init__"
        ]
        .map(String::from)
    );
    assert_eq!(graph.stats.unresolved, 0);
}

#[test]
fn a_union_of_defs_the_engine_solved_for_is_two_edges_at_one_site() {
    let graph = engine_graph("either.py", "run");
    assert_eq!(
        edges_of_entry(&graph),
        ["wrap.passthrough Name", "either.a Name", "either.b Name"].map(String::from)
    );
    assert_eq!(graph.stats.unresolved, 0);
}

#[test]
fn a_callable_parameter_stays_unresolved() {
    let graph = engine_graph("callback.py", "run");
    assert_eq!(edges_of_entry(&graph), ["unresolved unknown_callee"]);
}

#[test]
fn a_def_with_unknown_beside_it_stays_unresolved() {
    let graph = engine_graph("unknown_beside.py", "run");
    let edges = edges_of_entry(&graph);
    assert!(
        edges.contains(&"unresolved unknown_callee".to_owned()),
        "{edges:?}"
    );
    assert!(
        edges
            .iter()
            .all(|e| !e.starts_with("unknown_beside.helper")),
        "{edges:?}"
    );
}

const LIKELY_CALLEE: &str = "tests/fixtures/11-likely-callee/python";

/// The graph of `run` of `file` in the `11-likely-callee` project.
fn likely_graph(file: &str) -> Graph {
    let project = Project::open(LIKELY_CALLEE).unwrap();
    call_graph(&project, file, &first("run"), &Options::default()).unwrap()
}

/// The unmerged wire of `run` of `file` in the `11-likely-callee` project.
fn likely_wire(file: &str) -> python_analysis::Frame {
    let project = Project::open(LIKELY_CALLEE).unwrap();
    call_wire(&project, file, &first("run"), &Options::default(), false).unwrap()
}

/// The `likely_callee` counter, `None` when the graph has no such key.
fn likely_count(graph: &Graph) -> Option<u64> {
    graph.stats.language.get("likely_callee").copied()
}

/// The one child of `run` carries the mark, and `leaf` under it does not: the mark is on that node only.
fn assert_marked_once(file: &str, method: &str) {
    let frame = likely_wire(file);
    assert_eq!(frame.children.len(), 1, "{file}");
    let call = &frame.children[0];
    assert_eq!(call.target_qname, method);
    assert_eq!(call.marks, ["likely"]);
    assert_eq!(call.children.len(), 1, "{file}");
    assert_eq!(call.children[0].target_qname, "base.leaf");
    assert!(call.children[0].marks.is_empty(), "{file}");
}

#[test]
fn a_classmethod_found_past_an_unknown_base_is_a_likely_call() {
    let graph = likely_graph("classmethod.py");
    assert_eq!(edges_of_entry(&graph), ["base.Mixin.find Likely"]);
    assert_eq!(likely_count(&graph), Some(1));
    assert_eq!(graph.stats.unresolved, 0);
    assert_marked_once("classmethod.py", "base.Mixin.find");
}

#[test]
fn a_method_found_past_an_unknown_base_is_a_likely_call() {
    let graph = likely_graph("instance.py");
    assert_eq!(edges_of_entry(&graph), ["base.Mixin.save Likely"]);
    assert_eq!(likely_count(&graph), Some(1));
    assert_eq!(graph.stats.unresolved, 0);
    assert_marked_once("instance.py", "base.Mixin.save");
}

#[test]
fn a_method_found_past_an_unknown_base_on_an_object_built_here_is_a_likely_call() {
    let graph = likely_graph("built_here.py");
    // the class is named, so its construction is certain; the method on the new object is the engine's, past the unknown base
    assert_eq!(
        edges_of_entry(&graph),
        ["new base.Model Name init=-", "base.Mixin.save Likely"].map(String::from)
    );
    assert_eq!(likely_count(&graph), Some(1));
    assert_eq!(graph.stats.unresolved, 0);
}

#[test]
fn a_method_found_before_the_unknown_base_is_certain() {
    let graph = likely_graph("own.py");
    assert_eq!(edges_of_entry(&graph), ["base.Model.own Type"]);
    assert_eq!(likely_count(&graph), None);
}

#[test]
fn a_union_with_unknown_stays_unresolved() {
    let graph = likely_graph("either_unknown.py");
    assert_eq!(edges_of_entry(&graph), ["unresolved unknown_callee"]);
    assert_eq!(likely_count(&graph), None);
}

#[test]
fn an_untyped_receiver_stays_unresolved() {
    let graph = likely_graph("untyped.py");
    assert_eq!(edges_of_entry(&graph), ["unresolved unknown_callee"]);
    assert_eq!(likely_count(&graph), None);
}

// ---- PY9.2: `Project::refresh` ----

use std::io::Write as _;

/// A fresh folder for one test under the temp dir, named after the test and the process, emptied first in case a run before panicked.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(test: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("py92-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn path(&self, relative: &str) -> std::path::PathBuf {
        self.0.join(relative)
    }

    fn str(&self, relative: &str) -> String {
        self.path(relative).to_str().unwrap().to_owned()
    }

    /// Writes `text` and moves the modified time a few seconds forward: the engine sees a change only through that time, and on a
    /// coarse clock a file rewritten right after it was read could keep the same one.
    fn write(&self, relative: &str, text: &str) {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // A `File` and not `fs::write`: `set_modified` below needs the open file
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        file.set_modified(later).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const MAIN_PY: &str = "from helper import work\ndef main():\n    work()\n";
const HELPER_PY: &str = "def first(): pass\ndef second(): pass\ndef work():\n    first()\n";

/// `main` and `work` in `helper.py`, written into `dir` (a project root, or a folder in one).
fn write_small_project(dir: &Scratch, prefix: &str) {
    dir.write(&format!("{prefix}main.py"), MAIN_PY);
    dir.write(&format!("{prefix}helper.py"), HELPER_PY);
}

/// The edges out of `entry` of `file`, as `edges_of_entry` writes them.
fn edges_of(project: &Project, file: &str, entry: &str) -> Vec<String> {
    let graph = call_graph(project, file, &first(entry), &Options::default()).unwrap();
    edges_of_entry(&graph)
}

/// The edges out of `work`, which calls what `helper.py` says it calls.
fn work_edges(project: &Project) -> Vec<String> {
    edges_of(project, "helper.py", "work")
}

#[test]
fn a_refresh_shows_a_rewritten_file_and_without_it_the_old_answer_stays() {
    let dir = Scratch::new("rewrite");
    write_small_project(&dir, "");
    let mut project = Project::open(&dir.str("")).unwrap();
    assert_eq!(
        work_edges(&project),
        ["helper.first Name"].map(String::from)
    );

    dir.write(
        "helper.py",
        &HELPER_PY.replace("    first()", "    second()"),
    );
    // not refreshed: the answer comes from memory
    assert_eq!(
        work_edges(&project),
        ["helper.first Name"].map(String::from)
    );
    assert!(!project.refresh().reopened);
    assert_eq!(
        work_edges(&project),
        ["helper.second Name"].map(String::from)
    );
}

#[test]
fn a_refresh_finds_a_module_that_did_not_exist_at_the_first_request() {
    let dir = Scratch::new("new-module");
    dir.write("main.py", "import extra\ndef main():\n    extra.run()\n");
    let mut project = Project::open(&dir.str("")).unwrap();
    let before = edges_of(&project, "main.py", "main");
    assert_eq!(before.len(), 1);
    assert!(before[0].starts_with("unresolved"), "{before:?}");

    dir.write("extra.py", "def run(): pass\n");
    assert!(!project.refresh().reopened);
    let after = edges_of(&project, "main.py", "main");
    assert_eq!(after.len(), 1);
    assert!(after[0].starts_with("extra.run "), "{after:?}");
}

#[test]
fn a_refresh_after_a_file_is_deleted_leaves_the_call_unresolved() {
    let dir = Scratch::new("deleted");
    write_small_project(&dir, "");
    let mut project = Project::open(&dir.str("")).unwrap();
    assert_eq!(
        edges_of(&project, "main.py", "main"),
        ["helper.work Name"].map(String::from)
    );

    std::fs::remove_file(dir.path("helper.py")).unwrap();
    assert!(!project.refresh().reopened);
    let after = edges_of(&project, "main.py", "main");
    assert_eq!(after.len(), 1);
    assert!(after[0].starts_with("unresolved"), "{after:?}");
}

#[test]
fn a_new_config_opens_the_project_again_and_the_answer_is_the_same() {
    let dir = Scratch::new("config");
    write_small_project(&dir, "");
    let mut project = Project::open(&dir.str("")).unwrap();
    let before = work_edges(&project);
    let tree = |p: &Project| {
        let graph = call_graph(p, "main.py", &first("main"), &Options::default()).unwrap();
        tree_of(&graph)
    };
    let tree_before = tree(&project);

    dir.write("ty.toml", ""); // an empty file is a valid `ty.toml`
    assert!(project.refresh().reopened);
    assert!(!project.refresh().reopened, "nothing changed since");
    assert_eq!(work_edges(&project), before);
    assert_eq!(tree(&project), tree_before);
}

#[test]
fn a_config_added_above_the_root_after_open_does_not_move_it() {
    let dir = Scratch::new("parent");
    write_small_project(&dir, "proj/");
    let mut project = Project::open(&dir.str("proj")).unwrap();
    let opts = Options::default();
    let wire = |p: &Project| call_wire(p, "main.py", &first("main"), &opts, false).unwrap();
    let before = wire(&project);

    // as in `09-root/above-pyproject`: a project table above, which discovery would take as the root
    dir.write("pyproject.toml", "[project]\nname = \"outer\"\n");
    assert!(!project.refresh().reopened);
    let after = wire(&project);
    assert_eq!(after, before);
    assert_eq!(after.target_file.as_deref(), Some("main.py"));
    assert_eq!(after.children[0].target_file.as_deref(), Some("helper.py"));
}

#[test]
fn a_project_can_be_moved_to_another_thread() {
    fn send<T: Send>() {}
    send::<Project>();
    // `Project` is `Send`; it is not `Sync`, because salsa's `ZalsaLocal` (in the database's storage) holds a `RefCell<QueryStack>` and
    // an `UnsafeCell` map, state of the current thread (`sync::<Project>()` fails with E0277, tried 2026-11-13). So the driver may move
    // a project to the thread that serves it, but must not share `&Project` across threads: one owner, or a mutex around it.
}

#[test]
fn a_refresh_after_the_root_is_removed_does_not_panic_and_requests_answer_none() {
    let dir = Scratch::new("gone");
    write_small_project(&dir, "proj/");
    let mut project = Project::open(&dir.str("proj")).unwrap();
    assert!(call_graph(&project, "main.py", &first("main"), &Options::default()).is_some());

    std::fs::remove_dir_all(dir.path("proj")).unwrap();
    assert!(!project.refresh().reopened);
    assert!(call_graph(&project, "main.py", &first("main"), &Options::default()).is_none());
}

// ---- PY9.3: `Project::environment_inputs` ----

/// The entries every project lists, whatever the machine has: the config, the lock files and `.venv/pyvenv.cfg`. Sorted.
const FIXED_INPUTS: [&str; 7] = [
    ".venv/pyvenv.cfg",
    "Pipfile.lock",
    "poetry.lock",
    "pyproject.toml",
    "requirements*.txt",
    "ty.toml",
    "uv.lock",
];

/// The smallest `pyvenv.cfg` the engine accepts: it needs the `home` key. A `home` that is not a folder is tolerated (a warning in the
/// engine) unless `include-system-site-packages` is true.
const PYVENV_CFG: &str = "home = /usr/bin\nversion = 3.12.0\n";

fn assert_sorted(list: &[String]) {
    let mut sorted = list.to_vec();
    sorted.sort();
    assert_eq!(list, sorted, "the list is in a fixed order (rule 9)");
}

#[test]
fn without_a_config_the_list_has_the_fixed_entries_and_what_else_is_absolute() {
    let dir = Scratch::new("inputs-none");
    write_small_project(&dir, "");
    let project = Project::open(&dir.str("")).unwrap();
    let list = project.environment_inputs();
    // On a machine with `VIRTUAL_ENV` set or a Python with a `pyvenv.cfg` found by the engine, one more entry is here, and it is absolute.
    println!("environment_inputs, no config: {list:?}");
    for fixed in FIXED_INPUTS {
        assert!(
            list.iter().any(|e| e == fixed),
            "{fixed} is missing: {list:?}"
        );
    }
    for other in list.iter().filter(|e| !FIXED_INPUTS.contains(&e.as_str())) {
        assert!(std::path::Path::new(other).is_absolute(), "{other}");
    }
    assert_sorted(&list);
}

fn write_configured_env(dir: &Scratch, python: &str, cfg_at: &str) {
    write_small_project(dir, "");
    dir.write(
        "ty.toml",
        &format!("[environment]\npython = \"{python}\"\n"),
    );
    dir.write(cfg_at, PYVENV_CFG);
}

#[test]
fn an_environment_named_by_the_config_is_in_the_list_under_the_root() {
    let dir = Scratch::new("inputs-config");
    write_configured_env(&dir, "env", "env/pyvenv.cfg");
    let project = Project::open(&dir.str("")).unwrap();
    let list = project.environment_inputs();
    assert!(list.iter().any(|e| e == "env/pyvenv.cfg"), "{list:?}");
    assert_sorted(&list);
}

#[test]
fn a_rewritten_pyvenv_cfg_of_the_environment_reopens_the_project() {
    let dir = Scratch::new("inputs-reopen");
    write_configured_env(&dir, "env", "env/pyvenv.cfg");
    let mut project = Project::open(&dir.str("")).unwrap();
    assert!(!project.refresh().reopened, "nothing changed");

    dir.write(
        "env/pyvenv.cfg",
        &format!("{PYVENV_CFG}implementation = CPython\n"),
    );
    assert!(project.refresh().reopened);
    assert!(!project.refresh().reopened, "nothing changed since");
}

#[test]
fn an_environment_outside_the_root_is_in_the_list_as_an_absolute_path() {
    let dir = Scratch::new("inputs-outside");
    write_small_project(&dir, "proj/");
    dir.write("outside/pyvenv.cfg", PYVENV_CFG);
    let outside = dir.str("outside");
    dir.write(
        "proj/ty.toml",
        &format!("[environment]\npython = '{outside}'\n"),
    );
    let mut project = Project::open(&dir.str("proj")).unwrap();
    let list = project.environment_inputs();
    // the engine's `sys_prefix` is canonical (on macOS the temp dir is behind a symlink), so the entry is the canonical path
    let expected = std::fs::canonicalize(dir.path("outside"))
        .unwrap()
        .join("pyvenv.cfg");
    let expected = expected.to_str().unwrap();
    // on Windows `canonicalize` returns the verbatim form (`\\?\C:\…`); the engine's `canonicalize_path` simplifies it, so the entry has no prefix
    let expected = expected.strip_prefix(r"\\?\").unwrap_or(expected);
    assert!(
        list.iter().any(|e| e == expected),
        "{expected} not in {list:?}"
    );
    assert!(std::path::Path::new(expected).is_absolute());
    assert_sorted(&list);

    // an absolute entry is stamped too
    assert!(!project.refresh().reopened);
    dir.write(
        "outside/pyvenv.cfg",
        &format!("{PYVENV_CFG}implementation = CPython\n"),
    );
    assert!(project.refresh().reopened);
}

#[test]
fn a_configured_environment_that_is_broken_is_still_in_the_list() {
    let dir = Scratch::new("inputs-broken");
    write_small_project(&dir, "");
    // `env` is a folder with no `pyvenv.cfg`: not a virtual environment the engine can use, but the config names it and the user may fix it
    dir.write("ty.toml", "[environment]\npython = \"env\"\n");
    std::fs::create_dir_all(dir.path("env")).unwrap();
    let project = Project::open(&dir.str("")).unwrap();
    let list = project.environment_inputs();
    assert!(list.iter().any(|e| e == "env/pyvenv.cfg"), "{list:?}");
}
