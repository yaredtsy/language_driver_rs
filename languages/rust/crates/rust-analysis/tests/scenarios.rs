//! The scenarios (RS4 §8): the same small program in every language, with the tree it must give. Rust's program is in the `rust/` folder of each scenario.
//! Where Rust's names differ (`Counter::bump`, not `Counter.bump`) or the program has no counterpart for a row (a struct has no constructor function) the
//! scenario has a tree of Rust's own next to the shared one (`expected.rust.tree`, `expected.default.rust.tree`; D30), which the testkit looks for first.
//!
//! **Loaded once, as one workspace** (ENGINE.md, S-RS2 "Scenario cost": a load costs about 3 s, and all the scenarios together cost the same): the folders
//! are copied under `target/` into one folder with a generated `[workspace]` manifest, and every scenario's graph is asked of that one project.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use analyzer_testkit::scenarios::{Level, Subject, run};
use rust_analysis::{Cancel, Entry, Graph, LoadOptions, MarkerStyle, Options, Project, call_graph};

mod common;
use common::{copy_dir, tmp_root};

thread_local! {
    /// The one project of this test's thread: the testkit's `Subject::graph` is a plain function with no state of its own.
    static WORKSPACE: RefCell<Option<Project>> = const { RefCell::new(None) };
}

/// How many times the workspace was loaded: one, whatever the number of scenarios.
static LOADS: AtomicUsize = AtomicUsize::new(0);

const RUST: Subject = Subject {
    folder: "rust",
    scenarios: "../../../../scenarios",
    graph: scenario_graph,
};

/// Every scenario folder under `scenarios` that has a `rust/` program, by name.
fn rust_scenarios(scenarios: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(scenarios)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            path.join("rust").is_dir().then(|| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap()
            })
        })
        .collect();
    names.sort();
    names
}

/// The scenarios' programs as one Cargo workspace under `target/`: each `rust/` copied to `<scenario>/`, and a manifest that lists them.
fn workspace_of(scenarios: &Path) -> (PathBuf, Vec<String>) {
    let names = rust_scenarios(scenarios);
    let root = tmp_root().join("scenarios_workspace");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for name in &names {
        copy_dir(&scenarios.join(name).join("rust"), &root.join(name));
    }
    let members: Vec<String> = names.iter().map(|name| format!("\"{name}\"")).collect();
    std::fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\nresolver = \"3\"\nmembers = [{}]\n",
            members.join(", ")
        ),
    )
    .unwrap();
    (root, names)
}

/// The graph of `main` in one scenario's program, asked of the shared project.
fn scenario_graph(program: &Path) -> Option<Graph> {
    let scenarios = program.parent()?.parent()?;
    let name = program.parent()?.file_name()?.to_str()?;
    WORKSPACE.with(|workspace| {
        let mut workspace = workspace.borrow_mut();
        if workspace.is_none() {
            let (root, names) = workspace_of(scenarios);
            let first = root.join(&names[0]).join("src/main.rs");
            let project = Project::open(&root, &first, &LoadOptions::default())
                .expect("the scenarios' workspace loads");
            assert_eq!(
                project.warnings(),
                [],
                "a complete load: the scenarios are about calls, not about what a load lacks"
            );
            LOADS.fetch_add(1, Ordering::SeqCst);
            *workspace = Some(project);
        }
        let project = workspace.as_ref()?;
        call_graph(
            project,
            &format!("{name}/src/main.rs"),
            &Entry::Def {
                qualname: "main",
                occurrence: 0,
            },
            &Options::new(MarkerStyle::docstring("ID")),
            &Cancel::new(),
        )
    })
}

#[test]
fn the_scenarios_give_their_trees_from_one_load() {
    let ran = run(&RUST, Level::Default);
    assert_eq!(ran, 6, "every scenario has a Rust program");
    assert_eq!(LOADS.load(Ordering::SeqCst), 1, "loaded once");
}
