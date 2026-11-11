//! The call graph: `TypeScript` is the language the core's flat solver walks, and `call_graph` is the one public function that runs it.
//!
//! One context per function (`Key = ()`, digest zeros): TypeScript is not context sensitive yet (`docs/03-typescript/08-later-context-sensitive.md`). The seam
//! is kept: a target is `Target::Call(func, ())`, and `walk` is the only place that would take a key.

use std::collections::BTreeMap;
use std::fmt;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::{Basis, DefInfo, EdgeTo, External, Graph, Reason, Site};
use analyzer_core::outline::DefKind;
use analyzer_core::solve::{Cancel, Lang, Limits, SiteCalls, Solver, Summary, Target};

use crate::Project;
use crate::sem::{Func, Resolved, Sem};
use crate::walk;

/// The key of `Stats.language` and of an `Unresolved` edge's reason for a def whose node is missing from the program's tree: a bug in one of the
/// two parses, which must show on the graph and not as a body with no calls.
const DEF_WITHOUT_NODE: &str = "def_without_node";

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
struct TypeScript<'p> {
    sem: &'p Sem,
    style: MarkerStyle<'p>,
}

impl<'p> TypeScript<'p> {
    /// What the engine said one declaration of a site is, in the solver's terms.
    fn target(&self, resolved: Resolved<'p>) -> Target<Self> {
        match resolved {
            // A class called without `new` has nothing to walk as a body: it is a construction, with no `init` until `new` is built (step 5).
            Resolved::Def(def) if self.sem.def(def, self.style).kind == DefKind::Class => {
                Target::Construct {
                    class: def,
                    init: None,
                }
            }
            Resolved::Def(def) => Target::Call(def, ()),
            Resolved::External { package, name } => Target::External(External { package, name }),
            Resolved::Unknown(why) => Target::Unresolved(Reason::new(why.counter())),
        }
    }
}

impl<'p> Lang for TypeScript<'p> {
    /// The engine answers with the type checker.
    const BASIS: Basis = Basis::Type;
    type Func = Func<'p>;
    type Key = ();

    fn describe(&self, f: Self::Func) -> DefInfo {
        self.sem.describe(f, self.style)
    }

    fn digest(&self, _key: &Self::Key) -> [u8; 16] {
        [0; 16]
    }

    fn walk(&mut self, f: Self::Func, _key: &Self::Key, cancel: &Cancel) -> Summary<Self> {
        let Some(sites) = walk::sites(self.sem, f, self.style) else {
            // not an empty body: one edge at the def's first byte, so the def is marked and the count is what the edges say (rule 8)
            return Summary::complete(vec![SiteCalls::new(
                Site::at(0),
                vec![Target::Unresolved(Reason::new(DEF_WITHOUT_NODE))],
            )]);
        };
        let mut found = Vec::with_capacity(sites.len());
        for (index, site) in sites.into_iter().enumerate() {
            if cancel.is_cancelled() {
                return Summary::cut(found);
            }
            let targets = self
                .sem
                .callee(f, index, self.style)
                .into_iter()
                .map(|resolved| self.target(resolved))
                .collect();
            found.push(SiteCalls::new(site, targets));
        }
        Summary::complete(found)
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
