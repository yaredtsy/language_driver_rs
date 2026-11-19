//! D33: the root bounds project code. The tests open `outside-root/app`; `outside-root/shared` is beside it, so from the root it is `../shared`.

use std::path::{Path, PathBuf};

use typescript_analysis::{
    Cancel, Entry, Graph, MarkerStyle, OpenError, Options, Placement, Project, RenderOptions,
    Session, StableDef, WireOptions, WireStyle, breakdown, wire,
};

mod common;

use common::{call_graph, edges_as_text};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn app() -> PathBuf {
    fixture("outside-root/app")
}

fn graph_in(root: &Path, entry: &str) -> Graph {
    let project = Project::open(root, Path::new("src/main.ts")).unwrap();
    let entry = Entry::Def {
        qualname: entry,
        occurrence: 0,
    };
    call_graph(
        &project,
        "src/main.ts",
        &entry,
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .unwrap()
}

fn graph_of(entry: &str) -> Graph {
    graph_in(&app(), entry)
}

#[test]
fn no_def_has_a_file_or_an_id_outside_the_root() {
    for entry in ["main", "build", "hands"] {
        let graph = graph_of(entry);
        for def in &graph.defs {
            assert!(
                !Path::new(&def.file).is_absolute() && !def.file.starts_with(".."),
                "{entry}: {}",
                def.file
            );
            if let StableDef::Path { module, .. } = &def.id {
                assert!(
                    !Path::new(module).is_absolute() && !module.starts_with(".."),
                    "{entry}: {module}"
                );
            }
        }
    }
}

#[test]
fn a_call_into_source_outside_the_root_is_an_external_and_its_body_is_not_walked() {
    let graph = graph_of("main");
    let edges = edges_as_text(&graph, &app());
    assert!(
        edges.contains(&"main | helper | external ../shared/lib.ts helper".to_owned()),
        "{edges:#?}"
    );
    assert!(
        edges.iter().all(|edge| !edge.contains("inner")),
        "{edges:#?}"
    );
    assert!(graph.defs.iter().all(|def| def.qname != "inner"));
}

#[test]
fn a_class_outside_the_root_is_an_external_class_and_an_external_method() {
    let edges = edges_as_text(&graph_of("main"), &app());
    for expected in [
        "main | Tool | external ../shared/lib.ts Tool",
        "main | new Tool().run | external ../shared/lib.ts Tool.run",
    ] {
        assert!(edges.contains(&expected.to_owned()), "{edges:#?}");
    }
}

#[test]
fn a_base_class_outside_the_root_stops_the_chain_like_a_packages() {
    let graph = graph_of("build");
    let edges = edges_as_text(&graph, &app());
    assert_eq!(edges, ["build | Mine | construct Mine no init"]);
    assert_eq!(
        graph.stats.language.get("inherited_constructor_external"),
        Some(&1)
    );
}

#[test]
fn a_def_outside_the_root_handed_over_has_no_passed_edge() {
    let graph = graph_of("hands");
    assert!(
        graph.edges.iter().all(|edge| !edge.passed),
        "{:#?}",
        edges_as_text(&graph, &app())
    );
}

#[test]
fn outside_root_counts_source_files_and_not_declaration_files() {
    let graph = graph_of("main");
    // `helper`, `Tool` and `Tool.run`; `typed` is in a `.d.ts`
    assert_eq!(graph.stats.language.get("outside_root"), Some(&3));
    let summary = breakdown(&graph);
    assert_eq!(summary.outside_root, 3);
    assert!(
        summary
            .to_string()
            .contains("calls into source outside the root, not walked: 3"),
        "{summary}"
    );
    // a graph with no such call says nothing
    let none = graph_in(&fixture("three-files"), "run");
    assert_eq!(breakdown(&none).outside_root, 0);
    assert!(!breakdown(&none).to_string().contains("outside the root"));
}

fn wire_text(root: &Path) -> String {
    let project = Project::open(root, Path::new("src/main.ts")).unwrap();
    ["main", "build", "hands"]
        .iter()
        .map(|entry| {
            let graph = graph_in(root, entry);
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
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The point of D33: the same project in another folder gives the same answer, byte for byte.
#[test]
fn the_same_project_in_another_folder_gives_the_same_wire() {
    let elsewhere = std::env::temp_dir().join("typescript-analysis-outside-root-copy");
    let _ = std::fs::remove_dir_all(&elsewhere);
    copy_dir(&fixture("outside-root"), &elsewhere);
    let copy = wire_text(&elsewhere.join("app"));
    let _ = std::fs::remove_dir_all(&elsewhere);
    assert_eq!(copy, wire_text(&app()));
}

#[test]
fn files_lists_only_what_is_under_the_root() {
    let project = Project::open(&app(), Path::new("src/main.ts")).unwrap();
    assert_eq!(project.files(), ["src/local.ts", "src/main.ts"]);
}

fn entry_in(root: &Path, file: &str, qualname: &str) -> Option<Graph> {
    let project = Project::open(root, Path::new("src/main.ts")).unwrap();
    let entry = Entry::Def {
        qualname,
        occurrence: 0,
    };
    call_graph(&project, file, &entry, &Options::new(STYLE), &Cancel::new())
}

#[test]
fn a_def_in_a_file_outside_the_root_is_not_an_entry() {
    assert!(entry_in(&app(), "../shared/lib.ts", "helper").is_none());
    assert!(entry_in(&app(), "../shared/lib.ts", "Tool.run").is_none());
}

#[test]
fn a_def_in_node_modules_is_not_an_entry() {
    let root = fixture("node-modules-source");
    assert!(entry_in(&root, "src/main.ts", "main").is_some());
    assert!(entry_in(&root, "node_modules/lib/index.ts", "inside").is_none());
}

#[test]
fn a_file_outside_the_root_does_not_open() {
    let beside = Path::new("../shared/lib.ts");
    assert_eq!(
        Project::open(&app(), beside).err(),
        Some(OpenError::OutsideRoot)
    );
    let absolute = fixture("outside-root/shared/lib.ts");
    assert_eq!(
        Project::open(&app(), &absolute).err(),
        Some(OpenError::OutsideRoot)
    );
    // a folder whose name starts with the root's is not under it
    assert_eq!(
        Project::open(
            &fixture("outside-root/app"),
            &fixture("outside-root/app-x/f.ts")
        )
        .err(),
        Some(OpenError::OutsideRoot)
    );
}

#[test]
fn a_session_with_a_file_outside_the_root_gives_the_same_error() {
    let error = Session::open(&app(), Path::new("../shared/lib.ts"), Options::new(STYLE)).err();
    assert_eq!(error, Some(OpenError::OutsideRoot));
    assert_eq!(
        OpenError::OutsideRoot.to_string(),
        "the file is outside the root"
    );
}

#[test]
fn a_file_under_the_root_by_its_absolute_path_still_opens() {
    let project = Project::open(&app(), &app().join("src/main.ts")).unwrap();
    assert_eq!(project.files(), ["src/local.ts", "src/main.ts"]);
}

#[test]
fn a_file_that_is_not_project_code_has_no_defs_to_choose_from() {
    let root = fixture("node-modules-source");
    let project = Project::open(&root, Path::new("src/main.ts")).unwrap();
    assert!(
        project
            .defs_named("node_modules/lib/index.ts", "inside", STYLE)
            .is_none()
    );
    assert!(project.defs_named("src/main.ts", "main", STYLE).is_some());
    let beside = Project::open(&app(), Path::new("src/main.ts")).unwrap();
    assert!(
        beside
            .defs_named("../shared/lib.ts", "helper", STYLE)
            .is_none()
    );
}
