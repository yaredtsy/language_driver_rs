//! The call graph: `TypeScript` is the language the core's flat solver walks, and `call_graph` is the one public function that runs it.
//!
//! One context per function (`Key = ()`, digest zeros): TypeScript is not context sensitive yet (`docs/03-typescript/08-later-context-sensitive.md`). The seam
//! is kept: a target is `Target::Call(func, ())`, and `walk` is the only place that would take a key.

use std::collections::BTreeMap;
use std::fmt;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::{Basis, DefInfo, EdgeTo, Graph};
use analyzer_core::solve::{Cancel, Lang, Limits, Solver, Summary};

use crate::Project;
use crate::env::Env;
use crate::key::Key;
use crate::sem::{Func, Sem};
use crate::walk;

/// A def of one file, as the outline names it: its qualname and which of the same-named defs it is (0-based, in source order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    pub qualname: &'a str,
    pub occurrence: u16,
}

/// How to build a graph. The marker style is how a def's marker is read (`ID: …` in its JSDoc).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Options {
    pub limits: Limits,
    pub marker: MarkerStyle<'static>,
}

impl Options {
    /// The default cost limits, and `marker` for reading markers.
    pub fn new(marker: MarkerStyle<'static>) -> Self {
        Options {
            limits: Limits::default(),
            marker,
        }
    }
}

/// The language the solver walks: one program, read through `Sem`.
pub(crate) struct TypeScript<'p> {
    pub(crate) sem: &'p Sem,
    pub(crate) style: MarkerStyle<'p>,
}

impl<'p> Lang for TypeScript<'p> {
    /// The engine answers with the type checker.
    const BASIS: Basis = Basis::Type;
    type Func = Func<'p>;
    type Key = Key;

    fn describe(&self, f: Self::Func) -> DefInfo {
        self.sem.describe(f, self.style)
    }

    fn digest(&self, _key: &Self::Key) -> [u8; 16] {
        [0; 16]
    }

    fn walk(&mut self, f: Self::Func, _key: &Self::Key, cancel: &Cancel) -> Summary<Self> {
        walk::body(self, f, &Env, cancel)
    }
}

/// The context graph under `entry`, a def in `file` (relative to the project's root): every function it reaches, as far as the limits allow. `None`
/// when there is no such def (no such file, no such qualname, or fewer than `occurrence + 1` defs with it). The tree and the wire are views of this
/// graph (`analyzer_core::render`).
pub fn call_graph(
    project: &Project,
    file: &str,
    entry: &Entry<'_>,
    opts: &Options,
    cancel: &Cancel,
) -> Option<Graph> {
    let sem = &project.sem;
    let func = sem.entry(file, entry.qualname, entry.occurrence, opts.marker)?;
    let lang = TypeScript {
        sem,
        style: opts.marker,
    };
    let mut solver = Solver::new(lang, opts.limits.clone());
    let mut graph = solver.graph(&[(func, ())], cancel);
    // Each unresolved edge carries its reason, and `Stats.language` counts them: a count is what the edges say
    let mut reasons: BTreeMap<String, u64> = BTreeMap::new();
    for edge in &graph.edges {
        if let EdgeTo::Unresolved(reason) = &edge.to {
            *reasons.entry(reason.as_str().to_owned()).or_default() += 1;
        }
    }
    graph.stats.language.extend(reasons);
    Some(graph)
}

/// What a graph is made of, and why its unresolved edges are unresolved. A missing edge is explained by a number here, not by reading the code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Breakdown {
    pub calls: u64,
    pub constructs: u64,
    pub externals: u64,
    pub unresolved: u64,
    /// `unresolved` by reason; the values sum to `unresolved`.
    pub reasons: BTreeMap<String, u64>,
    pub contexts: u64,
    pub walks: u64,
    pub reuse_hits: u64,
    /// Cuts by the name of the limit.
    pub cuts: BTreeMap<String, u64>,
}

/// Count the edges of `graph` by kind, and the unresolved ones by reason.
pub fn breakdown(graph: &Graph) -> Breakdown {
    let mut summary = Breakdown {
        contexts: graph.stats.contexts,
        walks: graph.stats.walks,
        reuse_hits: graph.stats.reuse_hits,
        cuts: graph
            .stats
            .cuts
            .iter()
            .map(|(cut, count)| (format!("{cut:?}"), *count))
            .collect(),
        ..Breakdown::default()
    };
    for edge in &graph.edges {
        match &edge.to {
            EdgeTo::Context(..) => summary.calls += 1,
            EdgeTo::Construct { .. } => summary.constructs += 1,
            EdgeTo::External(..) => summary.externals += 1,
            EdgeTo::Unresolved(reason) => {
                summary.unresolved += 1;
                *summary
                    .reasons
                    .entry(reason.as_str().to_owned())
                    .or_default() += 1;
            }
            // an edge kind the core adds later is not counted here until this function says what it is
            _ => {}
        }
    }
    summary
}

impl fmt::Display for Breakdown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "breakdown:")?;
        writeln!(
            f,
            "  edges: {} call, {} construct, {} external, {} unresolved",
            self.calls, self.constructs, self.externals, self.unresolved
        )?;
        if !self.reasons.is_empty() {
            writeln!(f, "  unresolved by reason:")?;
            for (reason, count) in &self.reasons {
                writeln!(f, "    {reason}: {count}")?;
            }
        }
        writeln!(
            f,
            "  contexts {}, bodies walked {}, summaries reused {}",
            self.contexts, self.walks, self.reuse_hits
        )?;
        if self.cuts.is_empty() {
            writeln!(f, "  cuts: none")
        } else {
            let cuts: Vec<String> = self
                .cuts
                .iter()
                .map(|(cut, count)| format!("{cut} {count}"))
                .collect();
            writeln!(f, "  cuts: {}", cuts.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use analyzer_core::graph::{Reason, Site, Status};
    use analyzer_core::outline::DefKind;
    use analyzer_testkit::graph::check_invariants;

    use super::*;

    const STYLE: MarkerStyle<'static> = MarkerStyle {
        label: "ID",
        placement: analyzer_core::edit::Placement::Docstring,
    };

    #[test]
    fn a_def_with_no_node_is_one_unresolved_edge_and_a_count_not_an_empty_body() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve");
        let project = Project::open(&root, Path::new("src/main.ts")).unwrap();
        let func = project
            .sem
            .entry("src/main.ts", "callers", 0, STYLE)
            .unwrap();
        // as if the outline and the program's tree had disagreed about this def
        project.sem.forget_node_of(func, STYLE);
        let entry = Entry {
            qualname: "callers",
            occurrence: 0,
        };
        let graph = call_graph(
            &project,
            "src/main.ts",
            &entry,
            &Options::new(STYLE),
            &Cancel::new(),
        )
        .unwrap();
        check_invariants(&graph);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(
            graph.edges[0].to,
            EdgeTo::Unresolved(Reason::new("def_without_node"))
        );
        assert_eq!(graph.edges[0].site, Site::at(0));
        assert_eq!(graph.stats.unresolved, 1);
        assert_eq!(graph.stats.language.get("def_without_node"), Some(&1));
        assert_eq!(breakdown(&graph).reasons.len(), 1);
        // The def is on the graph as the def it is, not as an empty one. Its context is `Finished`: `Incomplete` is for a cost limit (rule 10),
        // and this is a bug in a rule.
        assert_eq!(graph.defs[0].kind, DefKind::Function);
        assert_eq!(graph.contexts[0].status, Status::Finished);
    }
}
