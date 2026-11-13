//! The call graph through the public API: the shared scenarios in TypeScript, the breakdown of why an edge is unresolved, and the properties every graph has.

use std::path::{Path, PathBuf};

use analyzer_testkit::graph::check_invariants;
use analyzer_testkit::scenarios::{Level, Subject, run};
use typescript_analysis::{
    Basis, Cancel, Edge, EdgeTo, Entry, Frame, Graph, MarkerStyle, Options, Project, Reason,
    RenderOptions, Root, StableDef, WireOptions, WireStyle, breakdown, call_graph, render, to_text,
    wire,
};

mod common;

use common::edges_as_text;

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
    // every scenario has a TypeScript program: straight-line, recursion-direct, method-on-class, external-call, construct-marked-class and two-callers, which
    // at `Level::Default` is the tree a default resolver gives (`Writer.write` under both callers; the precise tree is Python's)
    assert_eq!(run(&subject, Level::Default), 6);
}

/// The scenario folders that have a TypeScript program, by name.
fn scenario_programs() -> Vec<(String, PathBuf)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../scenarios");
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|dir| dir.join("typescript").is_dir())
        .map(|dir| {
            (
                dir.file_name().unwrap().to_string_lossy().into_owned(),
                dir.join("typescript"),
            )
        })
        .collect();
    found.sort();
    found
}

/// The merged wire of a scenario's graph, as text.
fn scenario_wire(program: &Path) -> String {
    let project = Project::open(program, Path::new("main.ts")).unwrap();
    let graph = scenario_graph(program).unwrap();
    let style = WireStyle {
        function_prefix: "F/".into(),
        class_prefix: "C/".into(),
    };
    let frame = wire(
        &graph,
        &graph.entries[0],
        &project,
        &WireOptions::new(style, true, RenderOptions::default()),
    );
    format!("{frame:?}")
}

#[test]
fn each_scenario_gives_the_same_graph_the_same_tree_and_the_same_wire_every_time() {
    // rule 11: a graph built twice, each time from a project opened afresh, is equal. A map iterated to make output would differ between the two, because
    // every map seeds its own hasher
    let programs = scenario_programs();
    assert_eq!(programs.len(), 6, "{programs:?}");
    for (name, program) in programs {
        let first = scenario_graph(&program).unwrap_or_else(|| panic!("{name}: no graph"));
        let second = scenario_graph(&program).unwrap();
        check_invariants(&first);
        assert_eq!(first, second, "{name}: the graph");
        let tree =
            |graph: &Graph| to_text(&render(graph, &graph.entries[0], &RenderOptions::default()));
        assert_eq!(tree(&first), tree(&second), "{name}: the tree");
        assert_eq!(
            scenario_wire(&program),
            scenario_wire(&program),
            "{name}: the wire"
        );
    }
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
    // `no_declaration` would say the engine could not tell. Found on a real frontend project (`applyOps`: 4 of its 4 unresolved edges).
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

/// `(written inside an expression, referenced by name, passed edges)` of the graph of `entry`: the defs a walked body hands over that no edge reaches, and the
/// edges that hand a def over (D22). A def handed over by name has an edge, so the second number is 0 when the edges are right.
fn handed(root: &Path, file: &str, entry: &str) -> (u64, u64, u64) {
    let graph = graph_of(root, file, entry);
    check_invariants(&graph);
    let summary = breakdown(&graph);
    (summary.handed_written, summary.handed_named, summary.passed)
}

#[test]
fn passed_each_hand_over_is_an_edge_at_its_name_and_a_read_is_not() {
    let root = fixture("handed-over");
    let rows: &[(&str, (u64, u64, u64))] = &[
        // written inside an expression, and handed over with what holds it: a method of an argument's literal, an arrow of a literal that is passed on, a
        // named function expression
        ("argMethod", (0, 0, 1)),
        ("localLiteral", (0, 0, 1)),
        ("namedFunctionExpression", (0, 0, 1)),
        // a value that holds defs: a literal in a hand-over position (an argument, nested, a return, an element), a name that holds one (nested, by an access,
        // through a wrapper, an import, twice, from a closure)
        ("inPlaceLiteral", (0, 0, 1)),
        ("inPlaceNestedLiteral", (0, 0, 1)),
        ("literalInReturn", (0, 0, 1)),
        ("literalInArray", (0, 0, 1)),
        ("literalByName", (0, 0, 2)),
        ("nestedLiteralByName", (0, 0, 1)),
        ("nestedLiteralByAccess", (0, 0, 1)),
        ("wrappedLiteralByName", (0, 0, 1)),
        ("literalByNameTwice", (0, 0, 2)),
        ("literalCalledAndPassed", (0, 0, 1)),
        ("literalPassedInAClosure", (0, 0, 1)),
        ("importedLiteralByName", (0, 0, 1)),
        // nothing hands these over: a literal nobody names or passes, a `let` literal's arrow (not a def), a literal that is read, a stored nested literal
        ("deadLiteral", (0, 0, 0)),
        ("letLiteral", (0, 0, 0)),
        ("literalRead", (0, 0, 0)),
        ("storedLiteralNotHandedOver", (0, 0, 0)),
        ("boundNamedFunctionExpression", (0, 0, 0)),
        // written and also named: the name hands it over, so an edge reaches it and it is no longer counted
        ("writtenAndNamed", (0, 0, 1)),
        ("writtenHereNamedInAClosure", (0, 0, 1)),
        // referenced by name: a const arrow, a function declaration, an import, a property arrow, a method, a shorthand property
        ("constArrowByName", (0, 0, 1)),
        ("declByName", (0, 0, 1)),
        ("importedByName", (0, 0, 1)),
        ("propertyArrowByName", (0, 0, 1)),
        ("methodByName", (0, 0, 1)),
        ("shorthand", (0, 0, 1)),
        // the positions of a hand-over: an element, an initializer, a return, a concise arrow body, both branches, a logical right side, three wrappers, an
        // assignment, a `new` argument, a property value
        ("inArray", (0, 0, 1)),
        ("inVariable", (0, 0, 1)),
        ("inReturn", (0, 0, 1)),
        ("inConciseArrow", (0, 0, 1)),
        ("inConditional", (0, 0, 2)),
        ("inLogical", (0, 0, 1)),
        ("inWrappers", (0, 0, 3)),
        ("inAssignment", (0, 0, 1)),
        ("inNew", (0, 0, 1)),
        ("inProperty", (0, 0, 1)),
        // named twice in one body: two edges, each at its own site
        ("namedTwice", (0, 0, 2)),
        // a hook's dependency array: both are hand-overs by the rule (a known over-report, see ENGINE.md)
        ("inDependencyArray", (0, 0, 2)),
        // a name that is read: a condition, `typeof`, a comparison, `!`, a ternary's condition, a property of it, a template: no edge
        ("readNotHandedOver", (0, 0, 0)),
        // called, not a def (D21 rule 2), an anonymous function, a tagged template's tag (step 6's site), a decorator, a callee the engine cannot resolve
        ("literalCalled", (0, 0, 0)),
        ("argArrowProperty", (0, 0, 0)),
        ("anonymousArrow", (0, 0, 0)),
        ("taggedTemplate", (0, 0, 0)),
        ("unresolvedCallee", (0, 0, 0)),
        ("Decorated", (0, 0, 0)),
        // a function of its own name that nothing names is dead code: unreached and uncounted
        ("neverUsed", (0, 0, 0)),
        // called and passed in one body: both edges (the pass counts, the call does not)
        ("constArrowCalled", (0, 0, 1)),
        // handed over here, and called by another def of the same tree: both are in it
        ("calledElsewhereInTheTree", (0, 0, 1)),
        // a callee is not a hand-over
        ("callsIt", (0, 0, 0)),
        // a destructuring initializer is a read; a bound name handed over is the member's edge, at the name (a rest element and an assignment target are not
        // followed)
        ("destructureOnly", (0, 0, 0)),
        ("destructureThenPass", (0, 0, 1)),
        ("destructureTwice", (0, 0, 2)),
        ("destructureRenamed", (0, 0, 1)),
        ("destructureNested", (0, 0, 1)),
        ("destructureCalled", (0, 0, 0)),
        ("destructureRest", (0, 0, 0)),
        ("destructureAssign", (0, 0, 0)),
    ];
    for (entry, expected) in rows {
        assert_eq!(handed(&root, "src/main.ts", entry), *expected, "{entry}");
    }
}

#[test]
fn passed_jsx_attributes_are_hand_overs_and_an_elements_tag_is_not() {
    let root = fixture("handed-over");
    // `onClick={handleClose}` and `{...{ onKeyDown: onKey }}`; `<Close />` is step 6's call, not a hand-over
    assert_eq!(handed(&root, "src/panel.tsx", "Panel"), (0, 0, 2));
}

#[test]
fn passed_the_edges_name_the_def_at_the_name_and_a_call_stays_a_call() {
    let root = fixture("handed-over");
    let listing = |entry: &str| edges_as_text(&graph_of(&root, "src/main.ts", entry), &root);
    assert_eq!(
        listing("constArrowByName"),
        [
            "constArrowByName | register | call register",
            "constArrowByName | handler | pass constArrowByName.<locals>.handler",
        ]
    );
    // called and passed in one body: both edges, each at its own site, in source order
    assert_eq!(
        listing("constArrowCalled"),
        [
            "constArrowCalled | register | call register",
            "constArrowCalled | both | pass constArrowCalled.<locals>.both",
            "constArrowCalled | both | call constArrowCalled.<locals>.both",
        ]
    );
    // named twice: two edges at two sites
    assert_eq!(
        listing("namedTwice"),
        [
            "namedTwice | register | call register",
            "namedTwice | handler | pass namedTwice.<locals>.handler",
            "namedTwice | register | call register",
            "namedTwice | handler | pass namedTwice.<locals>.handler",
        ]
    );
    // a method of an argument's literal is handed over at its key; a literal that is passed on hands its members over at the name
    assert_eq!(
        listing("argMethod"),
        [
            "argMethod | register | call register",
            "argMethod | onSuccess | pass argMethod.<locals>.onSuccess",
        ]
    );
    assert_eq!(
        listing("literalByName"),
        [
            "literalByName | register | call register",
            "literalByName | o | pass literalByName.<locals>.o.a",
            "literalByName | o | pass literalByName.<locals>.o.m",
        ]
    );
    // called and passed: the call of `m` and the hand-over of it with the literal, each at its own site
    assert_eq!(
        listing("literalCalledAndPassed"),
        [
            "literalCalledAndPassed | o.m | call literalCalledAndPassed.<locals>.o.m",
            "literalCalledAndPassed | register | call register",
            "literalCalledAndPassed | o | pass literalCalledAndPassed.<locals>.o.m",
        ]
    );
    // an import, a property arrow of a literal (D21), a method of a class, a shorthand property
    assert_eq!(
        listing("importedByName")[1],
        "importedByName | util | pass util"
    );
    assert_eq!(
        listing("propertyArrowByName")[1],
        "propertyArrowByName | api.load | pass api.load"
    );
    assert_eq!(
        listing("methodByName")[1],
        "methodByName | widget.method | pass Widget.method"
    );
    assert_eq!(
        listing("shorthand")[1],
        "shorthand | f | pass shorthand.<locals>.f"
    );
    // three wrappers, three sites, all the name `handler`
    assert_eq!(
        listing("inWrappers")
            .iter()
            .filter(|l| l.contains("| handler | pass"))
            .count(),
        3
    );
}

#[test]
fn passed_the_tree_draws_the_def_under_the_one_that_hands_it_over_marked_passed() {
    let root = fixture("handed-over");
    let graph = graph_of(&root, "src/main.ts", "constArrowCalled");
    let tree = render(&graph, &graph.entries[0], &RenderOptions::default());
    assert_eq!(
        to_text(&tree),
        "constArrowCalled\n  register\n  constArrowCalled.<locals>.both [Passed]\n  constArrowCalled.<locals>.both\n"
    );
    // what a handed-over def calls is under it: `calledElsewhere...` hands `sharedHelper` over, and `callsIt` calls it
    let graph = graph_of(&root, "src/main.ts", "calledElsewhereInTheTree");
    let tree = render(&graph, &graph.entries[0], &RenderOptions::default());
    assert_eq!(
        to_text(&tree),
        "calledElsewhereInTheTree\n  register\n  sharedHelper [Passed]\n  callsIt\n    sharedHelper\n"
    );
}

#[test]
fn passed_what_a_method_of_an_arguments_literal_calls_is_in_the_tree() {
    // Q-TS2: before D22 step 4 `sharedHelper()` was in no tree, because `onSuccess` was a def nobody called
    let root = fixture("handed-over");
    let graph = graph_of(&root, "src/main.ts", "argMethodCalls");
    let tree = render(&graph, &graph.entries[0], &RenderOptions::default());
    assert_eq!(
        to_text(&tree),
        "argMethodCalls\n  register\n  argMethodCalls.<locals>.onSuccess [Passed]\n    sharedHelper\n"
    );
}

/// The counters of what no edge covers: `element_access`, `bind`, `call_apply`, `class`, per occurrence, only those that are above 0.
fn uncovered(root: &Path, entry: &str) -> Vec<(String, u64)> {
    let graph = graph_of(root, "src/main.ts", entry);
    check_invariants(&graph);
    breakdown(&graph)
        .uncovered
        .into_iter()
        .collect::<Vec<(String, u64)>>()
}

#[test]
fn uncovered_what_no_edge_covers_is_a_number_per_kind() {
    let root = fixture("handed-over");
    let row = |entry: &str, expected: &[(&str, u64)]| {
        let expected: Vec<(String, u64)> = expected
            .iter()
            .map(|(kind, n)| ((*kind).to_owned(), *n))
            .collect();
        assert_eq!(uncovered(&root, entry), expected, "{entry}");
    };
    // a function handed over by an element access with a literal key, by `bind` (a name and a method), a class named as a value
    row("elementAccessHandedOver", &[("element_access", 1)]);
    row("bindHandedOver", &[("bind", 1)]);
    row("bindOfAMethod", &[("bind", 1)]);
    row("classHandedOver", &[("class", 1)]);
    row("classInAProperty", &[("class", 1)]);
    // a call through `call` and `apply`: the tree has `Function.call` and no edge to the function
    row("callAndApply", &[("call_apply", 2)]);
    // not counted: a computed key (unknowable), a bind whose result is dropped, a class that is constructed or tested
    row("elementAccessWithAComputedKey", &[]);
    row("elementAccessCalled", &[]);
    row("elementAccessKeyIsAFunction", &[]);
    row("bindDiscarded", &[]);
    row("classNotHandedOver", &[]);
    // and the shapes the edges cover are not in it
    row("constArrowByName", &[]);
    row("literalByName", &[]);
    row("argMethod", &[]);
}

/// `uncovered`, for a file of the `uncovered` fixture (the counters of TS4 step 7: what a destructuring, a construction, a class declared in a function and a
/// getter's result leave out of the tree).
fn uncovered_in(file: &str, entry: &str) -> Vec<(String, u64)> {
    let root = fixture("uncovered");
    let graph = graph_of(&root, file, entry);
    check_invariants(&graph);
    breakdown(&graph).uncovered.into_iter().collect()
}

/// One row: the entry's counters (the whole graph under it), written before the run.
fn row_in(file: &str, entry: &str, expected: &[(&str, u64)]) {
    let expected: Vec<(String, u64)> = expected
        .iter()
        .map(|(kind, n)| ((*kind).to_owned(), *n))
        .collect();
    assert_eq!(uncovered_in(file, entry), expected, "{file} {entry}");
}

#[test]
fn uncovered_a_rest_element_or_an_assignment_target_that_takes_a_def_is_counted_where_it_is_handed_over()
 {
    let file = "src/members.ts";
    row_in(file, "restTakesFunctions", &[("rest_element", 1)]);
    row_in(file, "restHandedTwice", &[("rest_element", 2)]);
    row_in(file, "restHoldsNoDef", &[]);
    row_in(file, "restHoldsOnlyWhatIsNamed", &[]);
    row_in(file, "restIsCalled", &[]);
    row_in(file, "restIsNotUsed", &[]);
    row_in(file, "namedElement", &[]);
    row_in(
        file,
        "assignedAndHanded",
        &[("assignment_destructuring", 1)],
    );
    row_in(
        file,
        "assignedRenamedHandedTwice",
        &[("assignment_destructuring", 2)],
    );
    row_in(file, "assignedNested", &[("assignment_destructuring", 1)]);
    row_in(file, "assignedAndCalled", &[]);
    row_in(file, "assignedNotAFunction", &[]);
    row_in(file, "assignedNeverUsed", &[]);
    row_in(file, "assignedPlainly", &[]);
    row_in(file, "comparedNotAssigned", &[]);
    // a rest in an assignment pattern counts as a rest element, where the rest name is handed over
    let rest = "rest_element";
    row_in(file, "assignedRestHanded", &[(rest, 1)]);
    row_in(file, "assignedRestHandedTwice", &[(rest, 2)]);
    row_in(file, "assignedRestHoldsNoDef", &[]);
    row_in(file, "assignedRestOnlyWhatIsNamed", &[]);
    row_in(file, "assignedRestNotHanded", &[]);
    row_in(file, "assignedRestTwice", &[(rest, 1)]);
    // a `for … of` pattern
    let for_of = "for_of_destructuring";
    row_in(file, "forOfHanded", &[(for_of, 1)]);
    row_in(file, "forOfHandedTwice", &[(for_of, 2)]);
    row_in(file, "forOfRenamed", &[(for_of, 1)]);
    row_in(file, "forOfNested", &[(for_of, 1)]);
    row_in(file, "forOfAwaited", &[(for_of, 1)]);
    row_in(file, "forOfCalled", &[]);
    row_in(file, "forOfNotAFunction", &[]);
    row_in(file, "forOfUnused", &[]);
    row_in(file, "forOfArrayPattern", &[]);
    // a rest in a `for … of` pattern is the rest element's, not this one's (not counted twice)
    row_in(file, "forOfRest", &[(rest, 1)]);
    row_in(file, "initializerNotForOf", &[]);
    // the breakdown says each one by its name
    let text = breakdown(&graph_of(
        &fixture("uncovered"),
        file,
        "assignedRenamedHandedTwice",
    ))
    .to_string();
    assert!(
        text.contains("not covered by an edge (counted, not drawn): assignment destructuring 2"),
        "{text}"
    );
}

#[test]
fn uncovered_the_computed_member_names_of_a_class_at_the_top_of_a_file_are_in_no_tree() {
    let file = "src/classes.tsx";
    let counter = "computed_member_name_sites";
    row_in(file, "ComputedTop", &[(counter, 2)]);
    row_in(file, "ComputedNoSite", &[]);
    // the name's site, and `make()` in the instance initializer, which a construction does not draw either
    row_in(
        file,
        "ComputedWithInitializer",
        &[(counter, 1), ("construct_initializer_sites", 1)],
    );
    row_in(file, "ComputedAmongPlainNames", &[(counter, 1)]);
    // a construction counts the class's names too, once for each construction (the class def is not walked)
    row_in(file, "constructsComputed", &[(counter, 2)]);
    row_in(file, "declaresComputed", &[]);
    // the nested class as the entry: the function around it is where its names are drawn, not here
    row_in(file, "declaresComputed.<locals>.Local", &[]);
    // the entry is a construction: it draws the constructor (the class has none) and neither what the initializer calls nor what the name calls
    expect_edges(&fixture("uncovered"), file, "ComputedWithInitializer", &[]);
    // a class in a function: the function draws its names
    expect_edges(
        &fixture("uncovered"),
        file,
        "declaresComputed",
        &["side | call side"],
    );
}

#[test]
fn uncovered_a_construction_does_not_draw_the_sites_of_the_instance_field_initializers() {
    let file = "src/classes.tsx";
    let counter = "construct_initializer_sites";
    row_in(file, "constructsPlain", &[]);
    row_in(file, "constructsWithField", &[(counter, 1)]);
    row_in(file, "constructsTwice", &[(counter, 2)]);
    row_in(file, "constructsWithTwoFields", &[(counter, 3)]);
    row_in(file, "constructsArrowField", &[]);
    // the reason it reads 0: the arrow a field holds is a def of its own (its qualname is `WithArrowField.onClick`), and what it calls is its own
    expect_edges(
        &fixture("uncovered"),
        file,
        "WithArrowField.onClick",
        &["side | call side"],
    );
    row_in(file, "constructsWrappedArrow", &[(counter, 2)]);
    row_in(file, "constructsWithConstructor", &[(counter, 1)]);
    // static initializers and blocks run when the class is defined, not when it is constructed: not this counter, but `class_definition_sites` (2)
    row_in(
        file,
        "constructsStaticOnly",
        &[("class_definition_sites", 2)],
    );
    row_in(file, "constructsParameterProperty", &[]);
    row_in(file, "constructsDerivedNoConstructor", &[(counter, 2)]);
    row_in(file, "constructsDerivedConstructor", &[(counter, 2)]);
    row_in(file, "constructsGrandchild", &[(counter, 2)]);
    // 1, not 2, of the initializers; `static s` is a definition-time site of the constructed class
    row_in(
        file,
        "constructsMixed",
        &[("class_definition_sites", 1), (counter, 1)],
    );
    row_in(file, "constructsExternal", &[]);
}

#[test]
fn uncovered_the_decorators_of_a_class_declared_in_a_function_are_in_no_tree_of_it() {
    let file = "src/classes.tsx";
    let counter = "local_class_decorator_sites";
    row_in(file, "declaresDecorated", &[(counter, 1)]);
    row_in(file, "declaresTwo", &[(counter, 2)]);
    row_in(file, "declaresDecoratedMember", &[(counter, 1)]);
    row_in(file, "declaresFactoryDecorated", &[(counter, 2)]);
    row_in(file, "declaresDecoratedWithOtherSites", &[(counter, 1)]);
    row_in(file, "declaresPlain", &[]);
    // a decorated class at the top of a file is not inside a function, so this counter reads 0; its decorator is a definition-time site of the class
    row_in(file, "usesTopDecorated", &[("class_definition_sites", 1)]);
}

#[test]
fn uncovered_a_class_component_is_constructed_and_its_render_is_not_drawn() {
    let file = "src/classes.tsx";
    let counter = "class_component_render";
    row_in(file, "showsWidget", &[(counter, 1)]);
    row_in(file, "showsDerivedWidget", &[(counter, 1)]);
    row_in(file, "showsArrowWidget", &[(counter, 1)]);
    row_in(file, "showsTwice", &[(counter, 2)]);
    row_in(file, "showsLibraryComponent", &[]);
    row_in(file, "showsFunctionComponent", &[]);
    row_in(file, "constructsWidget", &[]);
}

#[test]
fn uncovered_what_a_getter_returns_is_called_by_a_second_call_that_is_counted() {
    let file = "src/getters.ts";
    let counter = "getter_result_calls";
    row_in(file, "callsResult", &[(counter, 1)]);
    row_in(file, "callsResultTwice", &[(counter, 2)]);
    row_in(file, "callsMethod", &[]);
    row_in(file, "readsGetter", &[]);
    row_in(file, "constructsResult", &[(counter, 1)]);
    row_in(file, "callsExternalResult", &[(counter, 1)]);
    row_in(file, "callsAnonymousExternalResult", &[(counter, 1)]);
    // still one edge, to the getter: the count is not a second edge on the same span
    expect_edges(
        &fixture("uncovered"),
        file,
        "callsResult",
        &["b.fn | call get Holder.fn"],
    );
}

#[test]
fn uncovered_a_function_handed_over_by_bind_or_an_access_has_no_edge_yet() {
    // what the counter says is missing is missing: no passed edge for any of these
    let root = fixture("handed-over");
    for entry in [
        "elementAccessHandedOver",
        "bindHandedOver",
        "bindOfAMethod",
        "classHandedOver",
        "callAndApply",
    ] {
        let graph = graph_of(&root, "src/main.ts", entry);
        assert_eq!(breakdown(&graph).passed, 0, "{entry}");
    }
}

#[test]
fn uncovered_the_breakdown_says_what_each_count_is() {
    let root = fixture("handed-over");
    let graph = graph_of(&root, "src/main.ts", "callAndApply");
    assert_eq!(graph.stats.language.get("uncovered_call_apply"), Some(&2));
    let text = breakdown(&graph).to_string();
    assert!(
        text.contains("not covered by an edge (counted, not drawn): call/apply 2"),
        "{text}"
    );
    // a graph with none prints no line for it
    let plain = graph_of(&root, "src/main.ts", "literalByName");
    assert!(!breakdown(&plain).to_string().contains("not covered"));
}

#[test]
fn handed_a_def_every_hand_over_reaches_leaves_nothing_to_count() {
    // The counters of what is handed over and reached by no edge are 0 by construction now: a name, and a value that holds defs, each has an edge. They stay as
    // a cross-check (a nonzero value would mean the edge builder and the counter disagree), and `calls.rs` tests the counting itself.
    let root = fixture("handed-over");
    for entry in [
        "argMethod",
        "literalByName",
        "constArrowByName",
        "namedFunctionExpression",
    ] {
        let graph = graph_of(&root, "src/main.ts", entry);
        assert_eq!(
            graph.stats.language.get("def_written_not_called"),
            None,
            "{entry}"
        );
        assert_eq!(
            graph.stats.language.get("def_named_not_called"),
            None,
            "{entry}"
        );
        let summary = breakdown(&graph);
        assert!(summary.passed >= 1, "{entry}");
        assert!(
            !summary.to_string().contains("handed over and not called"),
            "{entry}"
        );
    }
}

/// The edges of `entry`'s context, as `callee text | what it is`, for the entries of the `site-kinds` fixture; a def reached is named `qname#nth` (a getter and a
/// setter of one name are two defs of the same qualname, so they are written `get Box.value` and `set Box.value`).
fn edges_of(root: &Path, file: &str, entry: &str) -> Vec<String> {
    let graph = graph_of(root, file, entry);
    check_invariants(&graph);
    let named = |id: &StableDef| {
        let def = graph.def(id).unwrap();
        let text = std::fs::read_to_string(root.join(&def.file)).unwrap();
        let written =
            text[def.range.start as usize..def.range.end as usize].trim_start_matches("static ");
        if written.starts_with("get ") {
            format!("get {}", def.qname)
        } else if written.starts_with("set ") {
            format!("set {}", def.qname)
        } else {
            def.qname.clone()
        }
    };
    graph
        .edges
        .iter()
        .filter(|edge| graph.def(&edge.from.func).unwrap().qname == entry)
        .map(|edge| {
            let from = graph.def(&edge.from.func).unwrap();
            let text = std::fs::read_to_string(root.join(&from.file)).unwrap();
            let callee = &text[(from.range.start + edge.site.start) as usize
                ..(from.range.start + edge.site.end) as usize];
            let to = match &edge.to {
                EdgeTo::Context(id, _) if edge.passed => format!("pass {}", named(&id.func)),
                EdgeTo::Context(id, _) => format!("call {}", named(&id.func)),
                EdgeTo::Construct { class, init, .. } => match init {
                    Some(init) => format!("construct {} init {}", named(class), named(&init.func)),
                    None => format!("construct {} no init", named(class)),
                },
                EdgeTo::External(external, _) => {
                    format!("external {} {}", external.package, external.name)
                }
                EdgeTo::Unresolved(reason) => format!("unresolved {}", reason.as_str()),
                _ => unreachable!("an edge kind this test does not know"),
            };
            format!("{callee} | {to}")
        })
        .collect()
}

fn expect_edges(root: &Path, file: &str, entry: &str, expected: &[&str]) {
    assert_eq!(edges_of(root, file, entry), expected, "{file} {entry}");
}

#[test]
fn jsx_a_component_is_a_site_and_an_intrinsic_tag_or_a_closing_tag_is_not() {
    let root = fixture("site-kinds");
    let jsx = "src/jsx.tsx";
    // `<div>`, `<my-el />` and `<svg:rect />` are not sites
    expect_edges(&root, jsx, "intrinsic", &[]);
    // a function, an arrow (calls); a class (a construction: a class component has no constructor to enter here); `memo(...)`'s result and a property of a
    // declared object are values (function_typed_value); `<Arrow></Arrow>` is one site
    expect_edges(
        &root,
        jsx,
        "components",
        &[
            "Plain | call Plain",
            "Arrow | call Arrow",
            "ClassComponent | construct ClassComponent no init",
            "Memoized | unresolved function_typed_value",
            "Ns.Icon | unresolved function_typed_value",
        ],
    );
    expect_edges(&root, jsx, "closing", &["Plain | call Plain"]);
    expect_edges(
        &root,
        jsx,
        "fromProps",
        &["props.Icon | unresolved function_typed_value"],
    );
}

#[test]
fn tagged_template_the_tag_is_a_site_and_so_is_a_call_in_a_substitution() {
    let root = fixture("site-kinds");
    expect_edges(
        &root,
        "src/tagged.ts",
        "tagged",
        &[
            "tag | call tag",
            "helper | call helper",
            "formatter.fmt | call Formatter.fmt",
            "factory | call factory",
            // the tag is the call's result: the call is a site of its own, and its result is no declaration
            "factory() | unresolved no_declaration",
            "String.raw | external typescript:lib StringConstructor.raw",
        ],
    );
}

#[test]
fn decorator_a_class_is_a_construction_so_its_decorators_are_counted_and_not_drawn() {
    let root = fixture("site-kinds");
    let file = "src/decorators.ts";
    // the decorators of a class, of its members and of the parameters of its constructor and methods run when the class is defined: the entry draws the
    // constructor (there is none to enter here), so no edge, and each site is counted (`@factory(1)` is two; `how each resolves` is a test of `calls.rs`)
    expect_edges(&root, file, "Decorated", &[]);
    let graph = graph_of(&root, file, "Decorated");
    check_invariants(&graph);
    assert_eq!(
        breakdown(&graph).uncovered.into_iter().collect::<Vec<_>>(),
        [("class_definition_sites".to_owned(), 11)]
    );
}

#[test]
fn accessor_a_read_is_the_getter_a_write_the_setter_and_a_compound_write_both() {
    let root = fixture("site-kinds");
    let file = "src/accessors.ts";
    let (get, set) = ("call get Box.value", "call set Box.value");
    let on = |text: &str, to: &str| format!("{text} | {to}");
    let cases: &[(&str, Vec<String>)] = &[
        ("reads", vec![on("box.value", get)]),
        ("writes", vec![on("box.value", set)]),
        ("compound", vec![on("box.value", get), on("box.value", set)]),
        (
            "increment",
            vec![
                on("box.value", get),
                on("box.value", set),
                on("box.value", get),
                on("box.value", set),
            ],
        ),
        (
            "logicalAssign",
            vec![on("box.value", get), on("box.value", set)],
        ),
        ("inForOf", vec![on("box.value", set)]),
        ("inDestructuringTarget", vec![on("box.value", set)]),
        // through `as`, `!` and parentheses
        ("wrapped", vec![on("box.value", set), on("box.value", set)]),
        // `box.method()` is a call of the method; `box.handler()` is a call whose callee is the getter's property: one edge each, and no accessor site besides
        (
            "callee",
            vec![
                on("box.method", "call Box.method"),
                on("box.handler", "call get Box.handler"),
            ],
        ),
        (
            "chain",
            vec![
                on("box.inner", "call get Box.inner"),
                on("box.inner.value", get),
            ],
        ),
        // a field, `delete`, a computed key; `const m = box.method` hands the method over (D22) and is no accessor site
        ("none", vec![on("box.method", "pass Box.method")]),
        ("elementWithLiteral", vec![on("box[\"value\"]", get)]),
        ("staticGetter", vec![on("Box.count", "call get Box.count")]),
        (
            "accessorInObjectLiteral",
            vec![
                on("literal.lit", "call set literal.lit"),
                on("literal.lit", "call get literal.lit"),
            ],
        ),
        // `box[remote.size]`: the key is a read of an accessor, the element access is not a site
        (
            "keyThatIsAnAccessor",
            vec![on("remote.size", "external src/types.d.ts Remote.size")],
        ),
        // the getter of every member that has one
        (
            "unionOfAccessors",
            vec![
                on("either.value", get),
                on("either.value", "call get Other.value"),
            ],
        ),
        // a member that is a plain field runs nothing
        ("unionWithAField", vec![on("mixed.value", get)]),
        // the getter and the setter of an external class are one name: one edge
        (
            "declaredAccessor",
            vec![on("remote.size", "external src/types.d.ts Remote.size")],
        ),
    ];
    for (entry, expected) in cases {
        assert_eq!(&edges_of(&root, file, entry), expected, "{entry}");
    }
    // what the accessors call is under them, in the tree
    expect_edges(&root, file, "Box.value", &["read | call read"]);
    expect_edges(
        &root,
        file,
        "Box.peek",
        &["this.#secret | call get Box.#secret"],
    );
    expect_edges(
        &root,
        file,
        "Sub.viaSuper",
        &["super.value | call get Box.value"],
    );
    expect_edges(
        &root,
        file,
        "Box.viaThis",
        &["this.value | call get Box.value"],
    );
}

#[test]
fn accessor_a_getter_read_by_destructuring_is_counted_and_has_no_edge() {
    let root = fixture("site-kinds");
    let file = "src/accessors.ts";
    // `(entry, getters run by the pattern)`: `value`; `inner` and `value`; the external `size`; a field and a rest element run nothing
    let rows = [
        ("destructuredGetter", 1),
        ("destructuredNested", 2),
        ("destructuredExternal", 1),
        ("destructuredField", 0),
        ("destructuredRest", 0),
    ];
    for (entry, counted) in rows {
        expect_edges(&root, file, entry, &[]);
        let graph = graph_of(&root, file, entry);
        assert_eq!(
            graph
                .stats
                .language
                .get("uncovered_destructured_accessor")
                .copied()
                .unwrap_or(0),
            counted,
            "{entry}"
        );
    }
    let graph = graph_of(&root, file, "destructuredGetter");
    assert!(
        breakdown(&graph)
            .to_string()
            .contains("destructured accessor 1")
    );
}

// ---- a class is an entry like a `new C()` site (D25 (1)): the same tree and the same counters

/// `(class, what its tree is, its init, the counters of what no edge covers, the notes of a construction that could not enter a constructor)`, written before
/// the run. Each class has a `newX` caller in the fixture that constructs it once.
type Counts = Vec<(&'static str, u64)>;

/// One row of the table: the class, its tree, its init, its uncovered counters, its notes.
type ClassRow = (
    &'static str,
    &'static str,
    Option<&'static str>,
    Counts,
    Counts,
);

fn class_entries() -> Vec<ClassRow> {
    vec![
        // no constructor anywhere: a class and nothing under it
        ("NoConstructor", "NoConstructor\n", None, vec![], vec![]),
        (
            "WithConstructor",
            "WithConstructor\n  helper\n",
            Some("WithConstructor.constructor"),
            vec![],
            vec![],
        ),
        // only an inherited constructor: the base's is the init, one level up and two levels up
        (
            "Inherits",
            "Inherits\n  helper\n",
            Some("WithConstructor.constructor"),
            vec![],
            vec![],
        ),
        (
            "Grandchild",
            "Grandchild\n  helper\n",
            Some("WithConstructor.constructor"),
            vec![],
            vec![],
        ),
        // an inherited constructor that cannot be entered is counted, never read as "no constructor" (Q-TS1)
        (
            "ExternalBase",
            "ExternalBase\n",
            None,
            vec![],
            vec![("inherited_constructor_external", 1)],
        ),
        (
            "UnknownBase",
            "UnknownBase\n",
            None,
            vec![("class_definition_sites", 1)],
            vec![("inherited_constructor_unknown", 1)],
        ),
        (
            "Fields",
            "Fields\n",
            None,
            vec![("construct_initializer_sites", 2)],
            vec![],
        ),
        (
            "Statics",
            "Statics\n",
            None,
            vec![("class_definition_sites", 2)],
            vec![],
        ),
        // the class's decorator, a member's and a parameter's
        (
            "Decorated",
            "Decorated\n  helper\n",
            Some("Decorated.constructor"),
            vec![("class_definition_sites", 3)],
            vec![],
        ),
        (
            "Computed",
            "Computed\n",
            None,
            vec![("computed_member_name_sites", 2)],
            vec![],
        ),
        (
            "Base",
            "Base\n  helper\n",
            Some("Base.constructor"),
            vec![("class_definition_sites", 1)],
            vec![],
        ),
        // `Rich`: its own two definition sites and one name, and the initializers of the chain (its `x`); `Base`'s static field ran when `Base` was defined
        (
            "Rich",
            "Rich\n  helper\n",
            Some("Base.constructor"),
            vec![
                ("class_definition_sites", 2),
                ("computed_member_name_sites", 1),
                ("construct_initializer_sites", 1),
            ],
            vec![],
        ),
    ]
}

type Owned = Vec<(String, u64)>;

fn counters_of(graph: &Graph) -> (Owned, Owned) {
    let summary = breakdown(graph);
    (
        summary.uncovered.into_iter().collect(),
        summary.notes.into_iter().collect(),
    )
}

fn owned(pairs: &[(&str, u64)]) -> Vec<(String, u64)> {
    pairs.iter().map(|(k, n)| ((*k).to_owned(), *n)).collect()
}

#[test]
fn class_entry_is_a_construction_with_the_tree_and_counters_the_table_says() {
    let root = fixture("class-entry");
    for (class, text, init, uncovered, notes) in class_entries() {
        let graph = graph_of(&root, "src/main.ts", class);
        check_invariants(&graph);
        let Root::Construct {
            class: row,
            init: found,
        } = &graph.entries[0]
        else {
            panic!("{class}: {:?}", graph.entries[0]);
        };
        assert_eq!(graph.def(row).unwrap().qname, class);
        let init_name = found
            .as_ref()
            .map(|id| graph.def(&id.func).unwrap().qname.as_str());
        assert_eq!(init_name, init, "{class}");
        assert_eq!(
            to_text(&render(
                &graph,
                &graph.entries[0],
                &RenderOptions::default()
            )),
            text,
            "{class}"
        );
        assert_eq!(
            counters_of(&graph),
            (owned(&uncovered), owned(&notes)),
            "{class}"
        );
    }
}

#[test]
fn class_entry_equals_what_a_new_site_of_the_class_draws() {
    let root = fixture("class-entry");
    for (class, ..) in class_entries() {
        let entry = graph_of(&root, "src/main.ts", class);
        let caller = graph_of(&root, "src/main.ts", &format!("new{class}"));
        // the tree: the caller's one child is the class, and under it what the entry's root has (the site is the only thing a root has not)
        let options = RenderOptions::default();
        let from_entry = render(&entry, &entry.entries[0], &options);
        let from_caller = render(&caller, &caller.entries[0], &options);
        assert_eq!(from_caller.children.len(), 1, "{class}");
        let mut at_site = from_caller.children[0].clone();
        assert!(at_site.site.is_some(), "{class}");
        at_site.site = None;
        assert_eq!(at_site, from_entry, "{class}");
        // the graph: everything the caller's graph has, less what leaves the caller
        let caller_name = format!("new{class}");
        let leaves_caller = |id: &StableDef| caller.def(id).unwrap().qname == caller_name;
        let edges: Vec<&Edge> = caller
            .edges
            .iter()
            .filter(|e| !leaves_caller(&e.from.func))
            .collect();
        assert_eq!(edges, entry.edges.iter().collect::<Vec<_>>(), "{class}");
        let defs: Vec<_> = caller
            .defs
            .iter()
            .filter(|d| !leaves_caller(&d.id))
            .collect();
        assert_eq!(defs, entry.defs.iter().collect::<Vec<_>>(), "{class}");
        // the counters, all of them: `Stats.language` holds the reasons, the notes and the uncovered counters
        assert_eq!(caller.stats.language, entry.stats.language, "{class}");
        assert_eq!(caller.stats.unresolved, entry.stats.unresolved, "{class}");
    }
}

#[test]
fn class_entry_a_function_entry_is_a_call_as_before() {
    // the entry that is not a class: `Root::Context`, the def's own body
    let graph = graph_of(&fixture("class-entry"), "src/main.ts", "newWithConstructor");
    assert!(matches!(graph.entries[0], Root::Context(_)));
}

#[test]
fn class_entry_a_class_declared_in_a_function_has_its_definition_sites_in_no_tree_and_no_counter_yet()
 {
    // KNOWN GAP, reported at the step-3 review and not fixed there: the function's body stops at a nested class, and `definition_sites` is for a class at the
    // top of a file. `make()` in `static s = make()` is neither an edge nor counted.
    let root = fixture("class-entry");
    let graph = graph_of(&root, "src/main.ts", "newLocal");
    check_invariants(&graph);
    assert_eq!(
        edges_as_text(&graph, &root),
        ["newLocal | Local | construct newLocal.<locals>.Local no init"]
    );
    assert_eq!(counters_of(&graph), (vec![], vec![]));
}
