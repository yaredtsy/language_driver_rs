//! What the tests of the call graph share: the options, the text of an entry (its tree, every edge, its counters: the grammar of
//! `languages/rust/docs/policy.md`), and a folder of inline files for a test that wants a program of its own.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use analyzer_core::graph::StableDef;
use analyzer_testkit::graph::check_invariants;
use bend_analysis::{
    Book, Cancel, EdgeTo, Graph, Limits, MarkerStyle, Options, RenderOptions, call_graph, render,
    to_text,
};

pub(crate) fn options() -> Options {
    Options::new(MarkerStyle::above("ID"))
}

/// The graph under `entry` of `file`, checked against the graph's invariants.
pub(crate) fn graph_of(book: &Book, file: &str, entry: &str) -> Option<Graph> {
    let graph = call_graph(
        book,
        Path::new(file),
        entry,
        &options(),
        &Limits::default(),
        &Cancel::new(),
    )?;
    check_invariants(&graph);
    Some(graph)
}

/// A def as the snapshot names it: its qualname, and `#n` for the n-th of one name in its file.
pub(crate) fn label(graph: &Graph, id: &StableDef) -> String {
    let def = graph.def(id).expect("a def of the graph");
    match def.occurrence {
        0 => def.qualname.clone(),
        n => format!("{}#{n}", def.qualname),
    }
}

/// Every edge of `graph` as `from | callee text | what it is`: grouped by the def they leave (sorted by its name), in site order inside a def.
pub(crate) fn edges_as_text(graph: &Graph, root: &Path) -> Vec<String> {
    let mut lines: Vec<(String, String)> = graph
        .edges
        .iter()
        .map(|edge| {
            let from = graph.def(&edge.from.func).expect("the caller is a def");
            let text = fs::read(root.join(&from.file)).expect("the caller's file");
            let (start, end) = (
                (from.range.start + edge.site.start) as usize,
                (from.range.start + edge.site.end) as usize,
            );
            let callee = text.get(start..end).unwrap_or_else(|| {
                panic!("{}: the site {start}..{end} is outside the file", from.file)
            });
            let to = match &edge.to {
                EdgeTo::Context(id, _) if edge.passed => format!("pass {}", label(graph, &id.func)),
                EdgeTo::Context(id, _) => format!("call {}", label(graph, &id.func)),
                EdgeTo::Construct { class, init, .. } => match init {
                    Some(init) => format!(
                        "construct {} init {}",
                        label(graph, class),
                        label(graph, &init.func)
                    ),
                    None => format!("construct {} no init", label(graph, class)),
                },
                EdgeTo::External(external, _) => {
                    format!("external {} {}", external.package, external.name)
                }
                EdgeTo::Unresolved(reason) => format!("unresolved {}", reason.as_str()),
                _ => unreachable!("an edge kind this test does not know"),
            };
            let from = label(graph, &edge.from.func);
            (
                from.clone(),
                format!("{from} | {} | {to}", String::from_utf8_lossy(callee)),
            )
        })
        .collect();
    // stable: the edges of one def stay in site order
    lines.sort_by(|a, b| a.0.cmp(&b.0));
    lines.into_iter().map(|(_, line)| line).collect()
}

/// What one entry gives: the tree (the project's defs only: an external, an unresolved site and a count are not in it), every edge of the whole graph, and the
/// counters above 0. An entry that is no def says so.
pub(crate) fn render_entry(book: &Book, file: &str, entry: &str) -> String {
    let mut out = format!("== {file} {entry}\n");
    let Some(graph) = graph_of(book, file, entry) else {
        out.push_str("no def\n");
        return out;
    };
    out.push_str(&to_text(&render(
        &graph,
        &graph.entries[0],
        &RenderOptions::default(),
    )));
    let edges = edges_as_text(&graph, book.root());
    if edges.is_empty() {
        out.push_str("edges: none\n");
    } else {
        out.push_str("edges:\n");
        for line in edges {
            out.push_str(&format!("{line}\n"));
        }
    }
    let counters: Vec<String> = graph
        .stats
        .language
        .iter()
        .filter(|(_, count)| **count > 0)
        .map(|(key, count)| format!("{key}={count}"))
        .collect();
    if !counters.is_empty() {
        out.push_str(&format!("counters: {}\n", counters.join(", ")));
    }
    out
}

/// A folder of files made for one test and removed with it.
pub(crate) struct Program {
    pub(crate) root: PathBuf,
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

impl Program {
    pub(crate) fn new(files: &[(&str, &str)]) -> Program {
        let root = std::env::temp_dir().join(format!(
            "bend-analysis-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        for (name, text) in files {
            let path = root.join(name);
            fs::create_dir_all(path.parent().expect("a folder")).expect("a scratch folder");
            fs::write(path, text).expect("a scratch file");
        }
        Program { root }
    }

    pub(crate) fn book(&self, entry: &str) -> Book {
        Book::open(&self.root, Path::new(entry)).expect("the book opens")
    }
}

impl Drop for Program {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
