//! What the integration tests of this crate share.
// each test crate includes all of this and uses part of it (`policy.rs` and `session.rs` read the policy table, `calls.rs` does not)
#![allow(dead_code)]

use std::path::Path;

use typescript_analysis::{EdgeTo, Graph, StableDef};

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
