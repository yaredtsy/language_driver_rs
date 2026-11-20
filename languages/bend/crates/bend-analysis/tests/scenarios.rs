//! The scenarios (docs/08-bend/03 §5 step 4): the same small program in every language, with the tree it must give. Bend's program is the `bend/` folder of
//! each scenario, and its entry is `main` in `main.bend`. Where Bend has no form for what a scenario pins (a constructor function, a receiver) the scenario has
//! a tree of Bend's own beside the shared one (`expected.bend.tree`, `expected.default.bend.tree`), which the testkit looks for first.

use std::path::Path;

use analyzer_core::graph::StableDef;
use analyzer_testkit::scenarios::{Level, Subject, run};
use bend_analysis::{Book, Cancel, Graph, Limits, call_graph};

mod common;
use common::options;

const BEND: Subject = Subject {
    folder: "bend",
    scenarios: "../../../../scenarios",
    graph: scenario_graph,
};

fn scenario_graph(program: &Path) -> Option<Graph> {
    let book = Book::open(program, Path::new("main.bend"))?;
    call_graph(
        &book,
        Path::new("main.bend"),
        "main",
        &options(),
        &Limits::default(),
        &Cancel::new(),
    )
}

#[test]
fn every_scenario_gives_its_tree() {
    // the six that every language has, and `law-across-files`, which only Bend has so far
    assert_eq!(run(&BEND, Level::Default), 7);
}

/// What `construct-marked-class` pins: a marked type is identified by its marker alone, which holds no name, so the graph must hold a row for the type, or the
/// tree would say `<class>`.
#[test]
fn a_marked_type_is_a_node_named_and_identified_by_its_marker() {
    let graph = scenario_graph(&Path::new(BEND.scenarios).join("construct-marked-class/bend"))
        .expect("a graph");
    let mut marked: Vec<(String, StableDef)> = graph
        .defs
        .iter()
        .filter(|d| d.id.is_marked())
        .map(|d| (d.qualname.clone(), d.id.clone()))
        .collect();
    marked.sort();
    assert_eq!(
        marked,
        [
            (
                "Plain".to_owned(),
                StableDef::Marked("11111111-0000-0000-0000-000000000002".to_owned())
            ),
            (
                "Report".to_owned(),
                StableDef::Marked("11111111-0000-0000-0000-000000000001".to_owned())
            ),
        ]
    );
}
