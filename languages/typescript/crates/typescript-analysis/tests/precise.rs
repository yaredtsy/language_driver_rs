//! The context layer on never has fewer edges than the layer off (`docs/03-typescript/12-context-layer/06-tc2-functions-through-parameters.md` §10, step 5a): for
//! every entry of the shared cases and of the whole fixtures, the graph made with `Precision::Precise` loses no answer of `Precision::Default`'s and no site
//! (bar the sites a callee that is only `undefined` empties, as many as `callee_undefined` counts); it is the same graph, stats included, when made twice; and
//! a context is a function of its key across the two. `Precise` runs twice, the second time on tables already kept.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::context::{a_context_is_a_function_of_its_key, never_fewer_edges};
use common::{STYLE, call_graph, cases, open_project, tests_dir};
use typescript_analysis::{Cancel, Entry, Options, Precision, Project};
use typescript_syntax::Dialect;

/// One entry to compare: its name, its file, and the def in it (qualname and occurrence); `None` is the file's own code.
type Item = (String, String, Option<(String, u16)>);

/// One project, opened once, and the entries to compare on it.
struct Plan {
    project: Project,
    entries: Vec<Item>,
}

/// The fixtures whose every def and file are compared, and the file each is opened with.
const WHOLE: [(&str, &str); 5] = [
    ("flow", "src/flow.tsx"),
    ("handed-over", "src/main.ts"),
    ("site-kinds", "src/jsx.tsx"),
    ("object-arrows", "src/main.ts"),
    ("anonymous-in-place", "src/main.ts"),
];

#[test]
fn precise_never_has_fewer_edges() {
    let mut plans: BTreeMap<String, Plan> = BTreeMap::new();
    // the cases of the shared list: one project per root, opened on its first case
    for case in cases() {
        let key = case.root.to_string_lossy().into_owned();
        let plan = plans.entry(key).or_insert_with(|| Plan {
            project: open_project(&case),
            entries: Vec::new(),
        });
        plan.entries
            .push((case.name, case.file, Some((case.entry, 0))));
    }
    // every def and the code of every file of the fixtures that read the table
    for (fixture, open_with) in WHOLE {
        let root = tests_dir().join("fixtures").join(fixture);
        let project = Project::open(&root, Path::new(open_with))
            .unwrap_or_else(|error| panic!("{fixture}: {error:?}"));
        let mut entries = Vec::new();
        for name in project.files() {
            let Some(dialect) = Dialect::from_path(&name) else {
                continue;
            };
            let text = std::fs::read_to_string(root.join(&name)).unwrap();
            let outline = typescript_syntax::outline(&text, dialect, STYLE);
            let mut seen: BTreeMap<&str, u16> = BTreeMap::new();
            for def in &outline.defs {
                let occurrence = seen.entry(def.qualname.as_str()).or_default();
                entries.push((
                    format!("{fixture} {name} {} #{occurrence}", def.qualname),
                    name.clone(),
                    Some((def.qualname.clone(), *occurrence)),
                ));
                *occurrence += 1;
            }
            entries.push((format!("{fixture} {name} FILE"), name, None));
        }
        let key = format!("whole {fixture}");
        assert!(plans.insert(key, Plan { project, entries }).is_none());
    }

    let default = Options::new(STYLE);
    let mut precise = Options::new(STYLE);
    precise.precision = Precision::Precise;
    let cancel = Cancel::new();

    let mut compared = 0;
    let mut differ = 0;
    let mut problems = Vec::new();
    for plan in plans.values() {
        for (name, file, def) in &plan.entries {
            let entry = match def {
                Some((qualname, occurrence)) => Entry::Def {
                    qualname,
                    occurrence: *occurrence,
                },
                None => Entry::File,
            };
            let graph =
                |options: &Options| call_graph(&plan.project, file, &entry, options, &cancel);
            let (off, on, again) = (graph(&default), graph(&precise), graph(&precise));
            compared += 1;
            let (Some(off), Some(on), Some(again)) = (off, on, again) else {
                // a graph that is not there must not be there under any precision
                let (off, on, again) = (graph(&default), graph(&precise), graph(&precise));
                if off.is_some() || on.is_some() || again.is_some() {
                    problems.push(format!("{name}: only some of the three have a graph"));
                }
                continue;
            };
            if off.edges != on.edges {
                differ += 1;
            }
            for problem in never_fewer_edges(&off, &on) {
                problems.push(format!("{name}: {problem}"));
            }
            if on != again {
                problems.push(format!("{name}: Precise is not the same graph twice"));
            }
            for problem in a_context_is_a_function_of_its_key(&[&on, &again]) {
                problems.push(format!("{name}: {problem}"));
            }
        }
    }
    eprintln!(
        "compared {compared} entries, each as Default, Precise and Precise again; {differ} have other edges with the layer on"
    );
    assert!(compared > 0);
    assert!(
        problems.is_empty(),
        "{} problems: {problems:#?}",
        problems.len()
    );
}

/// Counters per def (`06-tc2` §6 and §10, step 6): what a body *has* (`uncovered_bind`, the `helper.bind(null)` in `run`) is counted once per def however many
/// contexts it is walked in, and what a context *did* (`value_type_mismatch`, the two functions that do not fit `cb`) once per walk. `run` is walked
/// under two keys with the layer on, so the two counters part ways; with it off there is one walk and one context.
#[test]
fn a_body_counter_counts_once_per_def_and_a_context_counter_once_per_walk() {
    let root = tests_dir().join("fixtures/two-keys");
    let project = Project::open(&root, Path::new("src/main.ts")).unwrap();
    let entry = Entry::Def {
        qualname: "main",
        occurrence: 0,
    };
    let cancel = Cancel::new();
    let graph = |precision: Precision| {
        let mut options = Options::new(STYLE);
        options.precision = precision;
        call_graph(&project, "src/main.ts", &entry, &options, &cancel).expect("a graph for main")
    };
    let (default, precise) = (graph(Precision::Default), graph(Precision::Precise));
    let said =
        |graph: &typescript_analysis::Graph, key: &str| graph.stats.language.get(key).copied();

    let run = |graph: &typescript_analysis::Graph| {
        graph
            .contexts
            .iter()
            .filter(|context| {
                graph
                    .def(&context.id.func)
                    .is_some_and(|def| def.qname == "run")
            })
            .count()
    };
    // the test cannot pass by the layer giving one key: `run` has a context for each function handed to it
    assert_eq!(run(&precise), 2, "two contexts of `run` with the layer on");
    assert_eq!(run(&default), 1, "one context of `run` with it off");

    assert_eq!(said(&precise, "uncovered_bind"), Some(1), "the body, once");
    assert_eq!(said(&precise, "value_type_mismatch"), Some(2), "each walk");
    assert_eq!(said(&default, "uncovered_bind"), Some(1));
    assert_eq!(said(&default, "value_type_mismatch"), None);
}
