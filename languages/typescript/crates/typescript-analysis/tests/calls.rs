//! The call graph through the public API: the shared scenarios in TypeScript, the breakdown of why an edge is unresolved, and the properties every graph has.

use std::path::{Path, PathBuf};

use analyzer_testkit::graph::check_invariants;
use analyzer_testkit::scenarios::{Level, Subject, run};
use typescript_analysis::{
    Basis, Cancel, Edge, EdgeTo, Entry, Frame, Graph, MarkerStyle, Options, Project, Reason,
    RenderOptions, StableDef, WireOptions, WireStyle, breakdown, call_graph, wire,
};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: typescript_analysis::Placement::Docstring,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn graph_of(root: &Path, file: &str, entry: &str) -> Graph {
    let project = Project::open(root, Path::new(file)).unwrap();
    let entry = Entry {
        qualname: entry,
        occurrence: 0,
    };
    call_graph(&project, file, &entry, &Options::new(STYLE), &Cancel::new())
        .unwrap_or_else(|| panic!("no def `{}` in {file}", entry.qualname))
}

fn scenario_graph(program: &Path) -> Option<Graph> {
    let project = Project::open(program, Path::new("main.ts")).ok()?;
    let entry = Entry {
        qualname: "main",
        occurrence: 0,
    };
    call_graph(
        &project,
        "main.ts",
        &entry,
        &Options::new(STYLE),
        &Cancel::new(),
    )
}

#[test]
fn the_shared_scenarios_in_typescript_give_their_trees() {
    let subject = Subject {
        folder: "typescript",
        scenarios: "../../../../scenarios",
        graph: scenario_graph,
    };
    // every scenario that has a TypeScript program: straight-line, recursion-direct, method-on-class, external-call, construct-marked-class; `two-callers` is not written in TypeScript yet
    assert_eq!(run(&subject, Level::Default), 5);
}

#[test]
fn every_resolved_edge_rests_on_the_type_and_every_unresolved_edge_says_why() {
    let graph = graph_of(&fixture("resolve"), "src/main.ts", "callers");
    check_invariants(&graph);
    let mut resolved = 0;
    for Edge { to, .. } in &graph.edges {
        match to {
            EdgeTo::Context(_, basis) | EdgeTo::External(_, basis) => {
                assert_eq!(*basis, Basis::Type);
                resolved += 1;
            }
            EdgeTo::Construct { basis, .. } => assert_eq!(*basis, Basis::Type),
            EdgeTo::Unresolved(reason) => assert!(!reason.as_str().is_empty()),
            _ => unreachable!("an edge kind this test does not know"),
        }
    }
    assert!(resolved > 20);
}

#[test]
fn the_breakdown_says_why_each_unresolved_edge_is_unresolved_and_adds_up() {
    let graph = graph_of(&fixture("resolve"), "src/main.ts", "callers");
    let summary = breakdown(&graph);
    // `cb()` is a function-typed value, two calls are on `any`, one has a computed name (`tests/fixtures/resolve`)
    let reasons: Vec<(&str, u64)> = summary
        .reasons
        .iter()
        .map(|(reason, count)| (reason.as_str(), *count))
        .collect();
    assert_eq!(
        reasons,
        [
            ("any_or_untyped", 2),
            ("computed_name", 1),
            ("function_typed_value", 1)
        ]
    );
    // every unresolved edge has exactly one reason, and the counters in `Stats.language` are the same numbers
    assert_eq!(summary.unresolved, graph.stats.unresolved);
    assert_eq!(
        summary.reasons.values().sum::<u64>(),
        graph.stats.unresolved
    );
    for (reason, count) in &summary.reasons {
        assert_eq!(graph.stats.language.get(reason), Some(count), "{reason}");
    }
    // the four externals: a package, a repo .d.ts, and two bundled-library methods
    assert_eq!(summary.externals, 4);
}

#[test]
fn the_breakdown_of_the_fixture_is_reviewed() {
    let graph = graph_of(&fixture("resolve"), "src/main.ts", "callers");
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/breakdown-resolve.txt"),
        &breakdown(&graph).to_string(),
    );
}

#[test]
fn a_def_with_a_marker_is_named_by_it_in_the_graph() {
    let graph = graph_of(&fixture("resolve"), "src/main.ts", "callers");
    let marked: Vec<&StableDef> = graph
        .defs
        .iter()
        .map(|def| &def.id)
        .filter(|id| matches!(id, StableDef::Marked(_)))
        .collect();
    assert_eq!(marked, [&StableDef::Marked("PLAIN-0001".into())]);
}

#[test]
fn the_same_input_gives_the_same_graph() {
    let root = fixture("resolve");
    assert_eq!(
        graph_of(&root, "src/main.ts", "callers"),
        graph_of(&root, "src/main.ts", "callers")
    );
}

#[test]
fn a_cancelled_request_marks_the_entry_instead_of_inventing_edges() {
    let project = Project::open(&fixture("resolve"), Path::new("src/main.ts")).unwrap();
    let cancel = Cancel::new();
    cancel.cancel();
    let entry = Entry {
        qualname: "callers",
        occurrence: 0,
    };
    let graph = call_graph(
        &project,
        "src/main.ts",
        &entry,
        &Options::new(STYLE),
        &cancel,
    )
    .unwrap();
    check_invariants(&graph);
    assert!(graph.edges.is_empty());
    assert!(!graph.stats.cuts.is_empty());
}

#[test]
fn a_def_that_is_not_there_has_no_graph() {
    let project = Project::open(&fixture("resolve"), Path::new("src/main.ts")).unwrap();
    let opts = Options::new(STYLE);
    let none = |file: &str, qualname: &str, occurrence: u16| {
        call_graph(
            &project,
            file,
            &Entry {
                qualname,
                occurrence,
            },
            &opts,
            &Cancel::new(),
        )
        .is_none()
    };
    assert!(none("src/main.ts", "nope", 0));
    assert!(none("src/main.ts", "callers", 1));
    assert!(none("src/gone.ts", "callers", 0));
}

#[test]
fn an_unresolved_reason_is_a_plain_string_that_survives_the_graph() {
    // `Reason` is the key of the counter, owned, so a graph read back from disk could hold it
    assert_eq!(Reason::new("computed_name").as_str(), "computed_name");
}

#[test]
fn a_constructor_reached_by_super_is_the_init_of_the_construction() {
    let graph = graph_of(&fixture("resolve"), "src/lib.ts", "Derived2.constructor");
    let constructs: Vec<_> = graph
        .edges
        .iter()
        .filter_map(|edge| match &edge.to {
            EdgeTo::Construct { class, init, .. } => Some((class.clone(), init.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(constructs.len(), 1);
    let (class, init) = &constructs[0];
    assert!(
        matches!(class, StableDef::Path { qname, .. } if qname == "Base2"),
        "{class:?}"
    );
    // the base class's own constructor is entered: `init: None` would read as "no constructor"
    let init = init.as_ref().expect("Base2 has a constructor");
    assert!(
        matches!(&init.func, StableDef::Path { qname, .. } if qname == "Base2.constructor"),
        "{init:?}"
    );
}

#[test]
fn a_class_with_no_constructor_of_its_own_is_a_construction_with_no_init() {
    // `super()` of a class that declares none: its init is not "none because we did not look", it is none because there is none to enter
    // (an inherited one is step 5's)
    let graph = graph_of(
        &fixture("resolve"),
        "src/lib.ts",
        "FromNoConstructor.constructor",
    );
    let inits: Vec<_> = graph
        .edges
        .iter()
        .filter_map(|edge| match &edge.to {
            EdgeTo::Construct { init, .. } => Some(init.is_none()),
            _ => None,
        })
        .collect();
    assert_eq!(inits, [true]);
}

fn wire_of(entry: &str, merged: bool) -> Frame {
    let root = fixture("resolve");
    let project = Project::open(&root, Path::new("src/main.ts")).unwrap();
    let graph = graph_of(&root, "src/main.ts", entry);
    let style = WireStyle {
        function_prefix: "F/".into(),
        class_prefix: "C/".into(),
    };
    wire(
        &graph,
        &graph.entries[0],
        &project,
        &WireOptions::new(style, merged, RenderOptions::default()),
    )
}

#[test]
fn the_wire_writes_the_prefix_for_a_marked_def_and_nothing_for_an_unmarked_one() {
    let frame = wire_of("callers", false);
    assert_eq!(frame.target_qname, "callers");
    // `plain` carries `ID: PLAIN-0001`; `Counter.bump` carries none: it is still on the wire, named by its qualname
    let child = |name: &str| {
        frame
            .children
            .iter()
            .find(|c| c.target_qname == name)
            .unwrap()
    };
    assert_eq!(child("plain").target_id, "F/PLAIN-0001");
    assert_eq!(child("Counter.bump").target_id, "");
    // where the call is: the callee expression, as 1-based [line, column] in the file
    assert!(child("plain").call_site.is_some());
    assert!(frame.call_site.is_none());
}

#[test]
fn merged_turns_two_sibling_calls_of_one_function_into_one_node_with_call_count_one() {
    let unmerged = wire_of("twice", false);
    assert_eq!(unmerged.children.len(), 2);
    assert!(unmerged.children.iter().all(|c| c.call_count == 0));
    let merged = wire_of("twice", true);
    assert_eq!(merged.children.len(), 1);
    assert_eq!(merged.children[0].target_qname, "plain");
    // "the calls joined into this node, less one"
    assert_eq!(merged.children[0].call_count, 1);
}

/// Every edge of a graph as `from-qname | callee text | what it is`, in the graph's order (edges are sorted: rule 9). The callee text is read from the
/// file at the site, so a test names what an editor would highlight.
fn edges_as_text(graph: &Graph, root: &Path) -> Vec<String> {
    let name = |id: &StableDef| graph.def(id).unwrap().qname.clone();
    graph
        .edges
        .iter()
        .map(|edge| {
            let from = graph.def(&edge.from.func).unwrap();
            let text = std::fs::read_to_string(root.join(&from.file)).unwrap();
            let callee = &text[(from.range.start + edge.site.start) as usize
                ..(from.range.start + edge.site.end) as usize];
            let to = match &edge.to {
                EdgeTo::Context(id, _) if edge.passed => format!("pass {}", name(&id.func)),
                EdgeTo::Context(id, _) => format!("call {}", name(&id.func)),
                EdgeTo::Construct { class, init, .. } => match init {
                    Some(init) => format!("construct {} init {}", name(class), name(&init.func)),
                    None => format!("construct {} no init", name(class)),
                },
                EdgeTo::External(external, _) => {
                    format!("external {} {}", external.package, external.name)
                }
                EdgeTo::Unresolved(reason) => format!("unresolved {}", reason.as_str()),
                _ => unreachable!("an edge kind this test does not know"),
            };
            format!("{} | {callee} | {to}", from.qname)
        })
        .collect()
}

#[test]
fn construct_each_new_is_a_construction_an_external_or_an_unresolved_edge_that_says_why() {
    let root = fixture("construct");
    let graph = graph_of(&root, "src/main.ts", "build");
    check_invariants(&graph);
    let found = edges_as_text(&graph, &root);
    let build = |site: &str, to: &str| format!("build | {site} | {to}");
    let expected = [
        build("Renamed", "construct Report init Report.constructor"),
        build("Plain", "construct Plain no init"),
        build("Plain", "construct Plain no init"),
        // no constructor of its own: the base class's, one level up and two levels up
        build("Child", "construct Child init Parent.constructor"),
        build("Grandchild", "construct Grandchild init Parent.constructor"),
        build("Own", "construct Own init Own.constructor"),
        // the base is outside the project (a package's class, a library's), or is not a class the checker can name: there is no constructor to enter, and
        // the graph says it has none because it could not look (counted below), not because there is none
        build("FromPackage", "construct FromPackage no init"),
        build("MyError", "construct MyError no init"),
        // an own constructor with an external base: the construction has its init, and the base's constructor is an external edge from the `super()` site
        build(
            "WithSuper",
            "construct WithSuper init WithSuper.constructor",
        ),
        build(
            "PackageWithSuper",
            "construct PackageWithSuper init PackageWithSuper.constructor",
        ),
        build("Mixed", "construct Mixed no init"),
        // overloads: the implementation
        build(
            "Overloaded",
            "construct Overloaded init Overloaded.constructor",
        ),
        build("Generic", "construct Generic init Generic.constructor"),
        build(
            "Anonymous",
            "construct Anonymous init Anonymous.constructor",
        ),
        build("classes.Report", "construct Report init Report.constructor"),
        // a class outside the project is an external edge, never a construction (the graph's `defs` table is for classes of the project)
        build("Map", "external typescript:lib Map"),
        build("Boxed", "external pkg Boxed"),
        build("Declared", "external src/types.d.ts Declared"),
        // a class held by a value, as a callback parameter is held (D16)
        build("Ctor", "unresolved function_typed_value"),
        build("Kind", "unresolved function_typed_value"),
        build("(pick())", "unresolved no_declaration"),
        // a `declare function` of the project is a def with no body: a leaf, as an interface method is
        build("pick", "call pick"),
        build("(class {})", "unresolved no_declaration"),
        build("(Legacy as any)", "unresolved any_or_untyped"),
        // a constructor function is a function: it is entered as a call
        build("Legacy", "call Legacy"),
        "Own.constructor | super | construct Parent init Parent.constructor".to_owned(),
        "WithSuper.constructor | super | external typescript:lib Error".to_owned(),
        "PackageWithSuper.constructor | super | external pkg Base".to_owned(),
        "Legacy | prepare | call prepare".to_owned(),
    ];
    // Sorted by the graph, so compare as sets of the same lines and then check the lines of `build` in source order
    let mut found_sorted = found.clone();
    found_sorted.sort();
    let mut expected_sorted: Vec<String> = expected.iter().map(|line| line.to_owned()).collect();
    expected_sorted.extend(
        [
            "Anonymous.constructor | prepare | call prepare",
            "Generic.constructor | prepare | call prepare",
            "Overloaded.constructor | prepare | call prepare",
            "Parent.constructor | prepare | call prepare",
            "Report.constructor | prepare | call prepare",
        ]
        .map(str::to_owned),
    );
    expected_sorted.sort();
    assert_eq!(found_sorted, expected_sorted);
    let in_build: Vec<&String> = found.iter().filter(|l| l.starts_with("build |")).collect();
    let expected_build: Vec<&String> = expected
        .iter()
        .filter(|l| l.starts_with("build |"))
        .collect();
    assert_eq!(in_build, expected_build);
}

#[test]
fn construct_a_construction_that_cannot_enter_an_inherited_constructor_is_counted() {
    let graph = graph_of(&fixture("construct"), "src/main.ts", "build");
    // `FromPackage` and `MyError` extend a class outside the project; `Mixed` extends something the checker cannot name. The count is the evidence
    // that `no init` is a limit and not an answer.
    assert_eq!(
        graph.stats.language.get("inherited_constructor_external"),
        Some(&2)
    );
    assert_eq!(
        graph.stats.language.get("inherited_constructor_unknown"),
        Some(&1)
    );
    assert_eq!(breakdown(&graph).notes.len(), 2);
    // and `Plain`, `Child`, `Own` did not add to either: a class with no base, or a base we entered, is complete
    assert!(
        breakdown(&graph)
            .notes
            .keys()
            .all(|key| key.starts_with("inherited_constructor_"))
    );
}

#[test]
fn construct_a_library_global_called_as_a_function_is_an_external_edge_not_an_unknown_one() {
    // `String(x)` has no declaration with a body: the library declares `String` as an interface and a `var`. It is a call into the library, and
    // `no_declaration` would say the engine could not tell. Found on the V-NOC frontend (`applyOps`: 4 of its 4 unresolved edges).
    let root = fixture("construct");
    let graph = graph_of(&root, "src/main.ts", "convert");
    check_invariants(&graph);
    assert_eq!(
        edges_as_text(&graph, &root),
        [
            "convert | String | external typescript:lib String",
            "convert | Number | external typescript:lib Number",
            "convert | Boolean | external typescript:lib Boolean",
            "convert | Date | external typescript:lib Date",
            "convert | Intl.DateTimeFormat | external typescript:lib Intl.DateTimeFormat",
        ]
    );
    assert_eq!(graph.stats.unresolved, 0);
}

#[test]
fn construct_a_counter_that_is_not_about_constructions_does_not_print_under_their_heading() {
    let mut graph = graph_of(&fixture("construct"), "src/main.ts", "build");
    graph
        .stats
        .language
        .insert("some_later_counter".to_owned(), 7);
    let summary = breakdown(&graph);
    assert!(!summary.notes.contains_key("some_later_counter"));
    assert!(!summary.to_string().contains("some_later_counter"));
}

#[test]
fn object_arrows_a_property_arrow_of_a_const_literal_is_a_call_and_any_other_property_holder_is_still_d16()
 {
    let root = fixture("object-arrows");
    let graph = graph_of(&root, "src/main.ts", "run");
    check_invariants(&graph);
    let found = edges_as_text(&graph, &root);
    let run = |site: &str, to: &str| format!("run | {site} | {to}");
    let in_run: Vec<String> = vec![
        run("api.load", "call api.load"),
        run("api[\"load\"]", "call api.load"),
        run("api.tasks.update", "call api.tasks.update"),
        run("wrapped.a", "call wrapped.a"),
        run("documented.marked", "call documented.marked"),
        // a `let`, a default export and a class property's literal hold arrows that are not defs: the old answer, with its reason
        run("mutable.a", "unresolved function_typed_value"),
        run("defaulted.fromDefault", "unresolved function_typed_value"),
        run("Holder.config.read", "unresolved function_typed_value"),
        // a union receiver: one target per member, as for methods
        run("either.go", "call left.go"),
        run("either.go", "call right.go"),
        run("local.l", "call run.<locals>.local.l"),
        // the literal that is an argument holds an anonymous function: its calls belong to `run`
        run("other", "call other"),
        run("register", "call register"),
    ];
    let mut expected = in_run.clone();
    expected.extend(
        [
            "api.load | helper | call helper",
            "api.tasks.update | helper | call helper",
            "wrapped.a | helper | call helper",
            "run.<locals>.local.l | other | call other",
        ]
        .map(str::to_owned),
    );
    let mut found_sorted = found.clone();
    found_sorted.sort();
    expected.sort();
    assert_eq!(found_sorted, expected);
    // in source order (the `other` of the local literal is the local def's, not `run`'s)
    let in_run_found: Vec<&String> = found.iter().filter(|l| l.starts_with("run |")).collect();
    assert_eq!(in_run_found.len(), in_run.len());
    assert_eq!(graph.stats.unresolved, 3);
    // the marker is the JSDoc line above the property, and it is the def's identity
    let marked: Vec<(&StableDef, &str)> = graph
        .defs
        .iter()
        .filter(|def| def.id.is_marked())
        .map(|def| (&def.id, def.qname.as_str()))
        .collect();
    assert_eq!(
        marked,
        [(&StableDef::Marked("ARROW-0001".into()), "documented.marked")]
    );
}

/// `(written inside an expression, referenced by name)`: the defs of a walked body that are handed over and that no edge of the graph reaches.
fn handed(root: &Path, file: &str, entry: &str) -> (u64, u64) {
    let graph = graph_of(root, file, entry);
    check_invariants(&graph);
    let summary = breakdown(&graph);
    (summary.handed_written, summary.handed_named)
}

#[test]
fn handed_a_def_that_is_written_or_named_and_not_called_in_the_tree_is_counted_once_per_shape() {
    let root = fixture("handed-over");
    let rows: [(&str, (u64, u64)); 21] = [
        // written inside an expression: a method or an arrow property of a literal, a named function expression
        ("argMethod", (1, 0)),
        ("localLiteral", (1, 0)),
        ("namedFunctionExpression", (1, 0)),
        // written and named: once, as written
        ("writtenAndNamed", (1, 0)),
        ("writtenHereNamedInAClosure", (1, 0)),
        // referenced by name: a const arrow, a function declaration, an import, a property arrow, a method, a shorthand property
        ("constArrowByName", (0, 1)),
        ("declByName", (0, 1)),
        ("importedByName", (0, 1)),
        ("propertyArrowByName", (0, 1)),
        ("methodByName", (0, 1)),
        ("shorthand", (0, 1)),
        // not counted: called, not a def (D21 rule 2), an anonymous function, a tagged template's tag (step 6's site)
        ("literalCalled", (0, 0)),
        ("constArrowCalled", (0, 0)),
        ("argArrowProperty", (0, 0)),
        ("anonymousArrow", (0, 0)),
        ("taggedTemplate", (0, 0)),
        // a function of its own name that nothing names is dead code
        ("neverUsed", (0, 0)),
        // a callee the engine cannot resolve is still the thing called; a decorator is called
        ("unresolvedCallee", (0, 0)),
        ("Decorated", (0, 0)),
        // handed over here, called by another def of the same tree
        ("calledElsewhereInTheTree", (0, 0)),
        // a callee is not a reference
        ("callsIt", (0, 0)),
    ];
    for (entry, expected) in rows {
        assert_eq!(handed(&root, "src/main.ts", entry), expected, "{entry}");
    }
}

#[test]
fn handed_jsx_attributes_are_references_by_name_and_an_elements_tag_is_not() {
    let root = fixture("handed-over");
    // `onClick={handleClose}` and `{...{ onKeyDown: onKey }}`; `<Close />` is step 6's call, not a hand-over
    assert_eq!(handed(&root, "src/panel.tsx", "Panel"), (0, 2));
}

#[test]
fn handed_the_counts_are_in_the_graph_stats_and_the_breakdown_says_what_they_are() {
    let root = fixture("handed-over");
    let graph = graph_of(&root, "src/main.ts", "argMethod");
    assert_eq!(graph.stats.language.get("def_written_not_called"), Some(&1));
    assert_eq!(graph.stats.language.get("def_named_not_called"), None);
    let text = breakdown(&graph).to_string();
    assert!(
        text.contains(
            "handed over and not called: 1 written inside an expression, 0 referenced by name"
        ),
        "{text}"
    );
    // a graph with nothing handed over prints no line for it
    let plain = graph_of(&root, "src/main.ts", "literalCalled");
    assert!(!breakdown(&plain).to_string().contains("handed over"));
}
