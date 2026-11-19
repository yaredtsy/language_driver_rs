//! What the integration tests of this crate share.
// each test crate includes all of this and uses part of it (`policy.rs` and `session.rs` read the policy table, `calls.rs` does not)
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use typescript_analysis::{
    Cancel, EdgeTo, Entry, Graph, MarkerStyle, Options, Placement, Project, Session, StableDef,
};

/// Every edge of `graph` as `from | callee text | what it is`, in the order the graph holds them. A def is named by its qualname.
pub(crate) fn edges_as_text(graph: &Graph, root: &Path) -> Vec<String> {
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

/// One row of the table: its id, and the backticked words of its last cell. A row with one word names a folder that must exist (the outline's fixtures, a
/// rule of the outline and not of the calls). A row with more names the project's folder, a file of it, and the entries (qualnames) built from that file.
pub(crate) struct Row {
    pub(crate) id: String,
    pub(crate) words: Vec<String>,
}

/// The text between pairs of backticks, in order.
pub(crate) fn backticked(text: &str) -> Vec<String> {
    text.split('`')
        .enumerate()
        .filter(|(n, _)| n % 2 == 1)
        .map(|(_, word)| word.to_owned())
        .collect()
}

/// The rows of a table: the lines that begin with `` | `id` | ``. The fixture is the last cell.
pub(crate) fn rows(table: &str) -> Vec<Row> {
    table
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            let id = backticked(line).into_iter().next().unwrap_or_default();
            let last = line
                .trim_end()
                .trim_end_matches('|')
                .rsplit_once(" | ")
                .map_or("", |(_, cell)| cell);
            Row {
                id,
                words: backticked(last),
            }
        })
        .collect()
}

pub(crate) const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

const TABLE: &str = include_str!("../../../../docs/policy.md");

pub(crate) fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

/// A project and one entry of it.
pub(crate) struct Case {
    pub(crate) name: String,
    pub(crate) root: PathBuf,
    pub(crate) file: String,
    pub(crate) entry: String,
}

/// The six shared scenarios, every entry of every row of the policy table, and the `recursion` fixture with each of its defs as an entry.
pub(crate) fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let scenarios = tests_dir().join("../../../../../scenarios");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&scenarios)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|dir| dir.join("typescript").is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        cases.push(Case {
            name: format!("scenario {}", dir.file_name().unwrap().to_string_lossy()),
            root: dir.join("typescript"),
            file: "main.ts".into(),
            entry: "main".into(),
        });
    }
    for row in rows(TABLE).iter().filter(|row| row.words.len() > 2) {
        for entry in &row.words[2..] {
            cases.push(Case {
                name: format!("row {} {entry}", row.id),
                root: tests_dir().join(&row.words[0]),
                file: row.words[1].clone(),
                entry: entry.clone(),
            });
        }
    }
    for entry in [
        "main", "ping", "pong", "ring1", "ring2", "ring3", "Tree", "makeTree", "grow", "visit",
        "schedule",
    ] {
        cases.push(Case {
            name: format!("recursion {entry}"),
            root: tests_dir().join("fixtures/recursion"),
            file: "src/main.ts".into(),
            entry: entry.into(),
        });
    }
    cases
}

pub(crate) fn open(case: &Case) -> Session {
    Session::open(&case.root, Path::new(&case.file), Options::new(STYLE))
        .unwrap_or_else(|error| panic!("{}: {error:?}", case.name))
}

/// `typescript_analysis::call_graph`, and a failure when the graph counted `decl_not_in_outline`: the engine found a declaration in project code that the
/// outline does not list, one of the two rules of what a def is is wrong (D43: the count replaced a `debug_assert!`, so that a user's text can never panic a
/// debug build). Every graph test goes through this or through `checked`, so a test finds the gap and a release build only counts it.
pub(crate) fn call_graph(
    project: &Project,
    file: &str,
    entry: &Entry<'_>,
    options: &Options,
    cancel: &Cancel,
) -> Option<Graph> {
    checked(typescript_analysis::call_graph(
        project, file, entry, options, cancel,
    ))
}

/// A graph, as it came (`Session::graph` too), after the check of `call_graph` above.
pub(crate) fn checked(graph: Option<Graph>) -> Option<Graph> {
    checked_expecting(graph, 0)
}

/// `checked`, for a test that says it expects `gaps` declarations the outline does not list.
pub(crate) fn checked_expecting(graph: Option<Graph>, gaps: u64) -> Option<Graph> {
    if let Some(graph) = &graph {
        assert_outline_gaps(graph, gaps);
    }
    graph
}

/// For a graph a test got from a `Session`: no declaration the outline does not list.
pub(crate) fn assert_no_outline_gap(graph: &Graph) {
    assert_outline_gaps(graph, 0);
}

fn assert_outline_gaps(graph: &Graph, gaps: u64) {
    let count = |key: &str| graph.stats.language.get(key).copied().unwrap_or(0);
    assert_eq!(
        count("decl_not_in_outline"),
        gaps,
        "decl_not_in_outline: the engine's declaration is not a def of the outline"
    );
    // a node of a kind the engine's accessors do not handle, met where one was expected (H14): a missing answer nobody expects
    assert_eq!(
        count("engine_kind_unhandled"),
        0,
        "engine_kind_unhandled: a view met a kind of node it does not handle"
    );
}

/// The project of a case, opened (a `Project`, not a `Session`).
pub(crate) fn open_project(case: &Case) -> Project {
    Project::open(&case.root, Path::new(&case.file))
        .unwrap_or_else(|error| panic!("{}: {error:?}", case.name))
}
