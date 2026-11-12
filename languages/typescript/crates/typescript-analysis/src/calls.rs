//! The call graph: `TypeScript` is the language the core's flat solver walks, and `call_graph` is the one public function that runs it.
//!
//! One context per function (`Key = ()`, digest zeros): TypeScript is not context sensitive yet (`docs/03-typescript/08-later-context-sensitive.md`). The seam
//! is kept: a target is `Target::Call(func, ())`, and `walk` is the only place that would take a key.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::{Basis, DefInfo, EdgeTo, Graph, StableDef};
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
    /// Counters that are not the reason of an edge (`walk::INHERITED_*`), filled as bodies are walked, once each, and put in `Stats.language` by
    /// `call_graph`.
    notes: RefCell<BTreeMap<&'static str, u64>>,
    /// The defs the walked bodies hand over, by identity, and whether each was written inside an expression (`Sem::handed_over`). Compared with the
    /// graph's edges by `call_graph`, once they are known.
    handed: RefCell<HashMap<StableDef, bool>>,
}

impl<'p> TypeScript<'p> {
    fn new(sem: &'p Sem, style: MarkerStyle<'p>) -> Self {
        TypeScript {
            sem,
            style,
            notes: RefCell::new(BTreeMap::new()),
            handed: RefCell::new(HashMap::new()),
        }
    }

    /// Remember the functions `f`'s body hands over instead of calling. A function written inside an expression and also named is counted once, as written.
    pub(crate) fn note_handed_over(&self, f: Func<'p>) {
        let mut handed = self.handed.borrow_mut();
        for held in self.sem.handed_over(f, self.style) {
            let id = self.sem.describe(held.func, self.style).id;
            *handed.entry(id).or_insert(false) |= held.written;
        }
    }

    /// Count one more of `key`.
    pub(crate) fn note(&self, key: &'static str) {
        *self.notes.borrow_mut().entry(key).or_default() += 1;
    }
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
    let lang = TypeScript::new(sem, opts.marker);
    let mut solver = Solver::new(lang, opts.limits.clone());
    let mut graph = solver.graph(&[(func, ())], cancel);
    finish(&mut graph, solver.lang());
    Some(graph)
}

/// What a graph has beyond its edges: each unresolved edge's reason in `Stats.language` (a count is what the edges say), the notes the walk kept, and the
/// defs handed over and not called.
fn finish(graph: &mut Graph, lang: &TypeScript<'_>) {
    let mut reasons: BTreeMap<String, u64> = BTreeMap::new();
    for edge in &graph.edges {
        if let EdgeTo::Unresolved(reason) = &edge.to {
            *reasons.entry(reason.as_str().to_owned()).or_default() += 1;
        }
    }
    graph.stats.language.extend(reasons);
    for (key, count) in lang.notes.borrow().iter() {
        graph.stats.language.insert((*key).to_owned(), *count);
    }
    count_handed_over(graph, lang);
}

/// The defs the walked bodies hand over that no edge of the graph reaches, with whether each was written inside an expression. Sorted, so what is
/// listed from it is the same every time (rule 9).
fn handed_not_called(graph: &Graph, lang: &TypeScript<'_>) -> Vec<(StableDef, bool)> {
    let reached: HashSet<&StableDef> = graph
        .edges
        .iter()
        .filter_map(|edge| match &edge.to {
            EdgeTo::Context(id, _) => Some(&id.func),
            EdgeTo::Construct { init: Some(id), .. } => Some(&id.func),
            _ => None,
        })
        .collect();
    let mut found: Vec<(StableDef, bool)> = lang
        .handed
        .borrow()
        .iter()
        .filter(|(id, _)| !reached.contains(id))
        .map(|(id, written)| (id.clone(), *written))
        .collect();
    found.sort_by(|a, b| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)));
    found
}

/// The defs the walked bodies hand over that no edge of the graph reaches, in `Stats.language`: written inside an expression, and referenced by name. A
/// count only (for the owner's decision on how a handed-over def shows in the tree): no edge, node or context is added.
fn count_handed_over(graph: &mut Graph, lang: &TypeScript<'_>) {
    let found = handed_not_called(graph, lang);
    let written = found.iter().filter(|(_, written)| *written).count() as u64;
    let named = found.len() as u64 - written;
    for (key, count) in [
        (walk::WRITTEN_NOT_CALLED, written),
        (walk::NAMED_NOT_CALLED, named),
    ] {
        if count > 0 {
            graph.stats.language.insert(key.to_owned(), count);
        }
    }
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
    /// A construction with no `init` because its base classes could not be followed (`inherited_constructor_external`, `inherited_constructor_unknown`,
    /// from `Stats.language`). The edge is a known class, so it is not `unresolved`, but it is not complete. Only these two keys: the heading of `Display`
    /// says what they are, so another counter is added to a field of its own.
    pub notes: BTreeMap<String, u64>,
    /// Defs a walked body hands over that no edge of the graph reaches, written inside an expression (`run({ onSuccess() {} })`, a property arrow of a
    /// literal passed on) and referenced by name (`onClick={handleClose}`). Counted once per def. Neither is an unresolved edge: the def is known and
    /// is not in the tree, because nothing calls it.
    pub handed_written: u64,
    pub handed_named: u64,
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
    let language = |key: &str| graph.stats.language.get(key).copied().unwrap_or(0);
    summary.handed_written = language(walk::WRITTEN_NOT_CALLED);
    summary.handed_named = language(walk::NAMED_NOT_CALLED);
    summary.notes = graph
        .stats
        .language
        .iter()
        .filter(|(key, _)| {
            key.as_str() == walk::INHERITED_EXTERNAL || key.as_str() == walk::INHERITED_UNKNOWN
        })
        .map(|(key, count)| (key.clone(), *count))
        .collect();
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
        if !self.notes.is_empty() {
            writeln!(f, "  constructions with no constructor entered, by why:")?;
            for (note, count) in &self.notes {
                writeln!(f, "    {note}: {count}")?;
            }
        }
        if self.handed_written + self.handed_named > 0 {
            writeln!(
                f,
                "  defs handed over and not called: {} written inside an expression, {} referenced by name",
                self.handed_written, self.handed_named
            )?;
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

    /// A measurement on a real project, not a check. `TS_MEASURE_ROOT=<dir> TS_MEASURE_FILE=<a file of it> [TS_MEASURE_ENTRY=<qualname in that file>]
    /// [TS_MEASURE_LIST=<n>] cargo test -p typescript-analysis --lib measure_handed_over -- --ignored --nocapture`. With an entry it is that def's graph; without,
    /// every function, method and class of the project is an entry, so "not called" means "called by nothing in the project".
    #[test]
    #[ignore = "a measurement on a real project, driven by TS_MEASURE_ROOT and TS_MEASURE_FILE"]
    fn measure_handed_over() {
        let (Ok(root), Ok(file)) = (
            std::env::var("TS_MEASURE_ROOT"),
            std::env::var("TS_MEASURE_FILE"),
        ) else {
            eprintln!("set TS_MEASURE_ROOT and TS_MEASURE_FILE");
            return;
        };
        let project = Project::open(Path::new(&root), Path::new(&file)).unwrap();
        let sem = &project.sem;
        let entries: Vec<_> = match std::env::var("TS_MEASURE_ENTRY") {
            Ok(qualname) => vec![(sem.entry(&file, &qualname, 0, STYLE).unwrap(), ())],
            Err(_) => sem
                .project_defs(STYLE)
                .into_iter()
                .map(|f| (f, ()))
                .collect(),
        };
        let mut solver = Solver::new(TypeScript::new(sem, STYLE), Limits::new(u32::MAX, None));
        let started = std::time::Instant::now();
        let mut graph = solver.graph(&entries, &Cancel::new());
        finish(&mut graph, solver.lang());
        eprintln!("entries {}, graph {:?}", entries.len(), started.elapsed());
        eprint!("{}", breakdown(&graph));
        let list: usize = std::env::var("TS_MEASURE_LIST")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        for (id, written) in handed_not_called(&graph, solver.lang())
            .into_iter()
            .take(list)
        {
            let what = if written { "written" } else { "named  " };
            match id {
                StableDef::Path { module, qname, nth } => {
                    eprintln!("{what} {module} {qname}#{nth}")
                }
                other => eprintln!("{what} {other:?}"),
            }
        }
    }
}
