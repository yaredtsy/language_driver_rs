//! D48 (RS3 §3, "Which `cfg` set a load reads"): a load turns `debug_assertions` on and `miri` off, on every rung of the ladder. One program, three loads: cargo
//! reads the manifest, our reader reads a manifest cargo refuses, and a file with no manifest is a crate of its own.

use std::path::Path;

use rust_analysis::{
    Cancel, Entry, Graph, LoadOptions, MarkerStyle, Options, Project, RenderOptions, breakdown,
    call_graph, render, to_text,
};

mod common;
use common::{copy_of_fixture, one_load, write};

fn open(root: &Path) -> Project {
    Project::open(root, Path::new("src/lib.rs"), &LoadOptions::default())
        .expect("the fixture loads")
}

/// The graph of `qualname`, or `None` when it is no entry.
fn graph(project: &Project, qualname: &str) -> Option<Graph> {
    let options = Options::new(MarkerStyle::docstring("ID"));
    let entry = Entry::Def {
        qualname,
        occurrence: 0,
    };
    call_graph(project, "src/lib.rs", &entry, &options, &Cancel::new())
}

fn tree(graph: &Graph) -> String {
    to_text(&render(graph, &graph.entries[0], &RenderOptions::default()))
}

/// What the program says on every rung: the debug-only and the not-`miri` functions are defs (entries, each with its call to `leaf`), their twins are none,
/// and the entry's calls to the twins have no target.
fn check(project: &Project, rung: &str) {
    for on in ["dbg_only", "not_miri"] {
        let graph = graph(project, on).unwrap_or_else(|| panic!("{rung}: {on} is a def"));
        assert_eq!(tree(&graph), format!("{on}\n  leaf\n"), "{rung}");
    }
    for off in ["release_only", "under_miri"] {
        assert!(graph(project, off).is_none(), "{rung}: {off} is no def");
    }
    let graph = graph(project, "entry").unwrap_or_else(|| panic!("{rung}: entry"));
    assert_eq!(
        tree(&graph),
        "entry\n  dbg_only\n    leaf\n  not_miri\n    leaf\n",
        "{rung}"
    );
    let summary = breakdown(&graph).to_string();
    assert!(
        summary.contains("edges: 4 call, 0 construct, 0 external, 2 unresolved")
            && summary.contains("unknown_callee: 2"),
        "{rung}\n{summary}"
    );
}

#[test]
fn the_cargo_rung_reads_debug_assertions_and_not_miri() {
    let _load = one_load();
    let copy = copy_of_fixture("cfg_set", "cfg_cargo");
    let project = open(&copy);
    assert!(project.warnings().is_empty(), "{:?}", project.warnings());
    check(&project, "cargo");
}

#[test]
fn the_readers_rung_reads_debug_assertions_and_not_miri() {
    let _load = one_load();
    let copy = copy_of_fixture("cfg_set", "cfg_reader");
    // the manifest cargo refuses, as the `cargo_fails` fixture has it
    let refused = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cargo_fails/Cargo.toml"),
    )
    .unwrap()
    .replace("cargo_fails", "cfg_set");
    write(&copy.join("Cargo.toml"), &refused);
    let project = open(&copy);
    assert_eq!(project.warnings()[0].code(), "cargo_failed");
    check(&project, "reader");
}

#[test]
fn the_lone_file_reads_debug_assertions_and_not_miri() {
    let _load = one_load();
    let copy = copy_of_fixture("cfg_set", "cfg_lone");
    std::fs::remove_file(copy.join("Cargo.toml")).unwrap();
    let project = open(&copy);
    assert_eq!(project.warnings()[0].code(), "no_manifest");
    check(&project, "lone");
}
