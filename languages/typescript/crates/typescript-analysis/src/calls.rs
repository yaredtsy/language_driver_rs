//! The call graph: `TypeScript` is the language the core's flat solver walks, and `call_graph` is the one public function that runs it.
//!
//! One context per function (`Key = ()`, digest zeros): TypeScript is not context sensitive yet (`docs/03-typescript/08-later-context-sensitive.md`). The seam
//! is kept: a target is `Target::Call(func, ())`, and `walk` is the only place that would take a key.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::{Basis, DefInfo, EdgeTo, Entry, Graph, StableDef};
use analyzer_core::solve::{Cancel, Lang, Limits, Solver, Summary};

use crate::Project;
use crate::env::Env;
use crate::key::Key;
use crate::sem::{Func, Handed, Sem, Uncovered};
use crate::walk;

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
    pub(crate) fn new(sem: &'p Sem, style: MarkerStyle<'p>) -> Self {
        TypeScript {
            sem,
            style,
            notes: RefCell::new(BTreeMap::new()),
            handed: RefCell::new(HashMap::new()),
        }
    }

    /// Count what a walked body hands over or calls that no edge covers (`Uncovered`), once per occurrence. A count and nothing else: no edge, no node.
    pub(crate) fn note_uncovered(&self, uncovered: &[Uncovered]) {
        for kind in uncovered {
            self.note(kind.counter());
        }
    }

    /// Remember the functions a walked body hands over (`Sem::handed_over`). A function written inside an expression and also named is counted once, as
    /// written. A name has an edge (D22), so a def named is reached and is not counted: what is left is the def written inside an expression, until it has
    /// an edge too.
    pub(crate) fn note_handed_over(&self, handed: &[Handed<'p>]) {
        let mut known = self.handed.borrow_mut();
        for held in handed {
            let id = self.sem.describe(held.func, self.style).id;
            *known.entry(id).or_insert(false) |= held.written;
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
    /// Edges that call a def of the project. A def handed over is not one (`passed`).
    pub calls: u64,
    /// Edges that hand a def over and do not call it (D22): in the graph, walked, and never counted as a call.
    pub passed: u64,
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
    /// What a body hands over or calls that **no edge covers**, per occurrence, by kind (D22 step 5): `element_access` (a function handed over by `a["k"]`),
    /// `bind` (handed over bound: `f.bind(…)`, `obj.m.bind(obj)`), `call_apply` (called through `call` or `apply`), `class` (a class named where a value is handed
    /// over). Only the kinds above 0. A count, not an edge: the def is known and is not in the tree.
    pub uncovered: BTreeMap<String, u64>,
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
            EdgeTo::Context(..) if edge.passed => summary.passed += 1,
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
    summary.uncovered = graph
        .stats
        .language
        .iter()
        .filter_map(|(key, count)| Some((key.strip_prefix("uncovered_")?.to_owned(), *count)))
        .collect();
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
        write!(
            f,
            "  edges: {} call, {} construct, {} external, {} unresolved",
            self.calls, self.constructs, self.externals, self.unresolved
        )?;
        // said only when there is one, so the breakdown of a graph with none is what it was
        if self.passed > 0 {
            write!(f, ", {} passed", self.passed)?;
        }
        writeln!(f)?;
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
        if !self.uncovered.is_empty() {
            let labels = [
                ("element_access", "element access"),
                ("bind", "bind"),
                ("call_apply", "call/apply"),
                ("class", "class"),
            ];
            let parts: Vec<String> = labels
                .iter()
                .filter_map(|(key, label)| Some(format!("{label} {}", self.uncovered.get(*key)?)))
                .collect();
            writeln!(
                f,
                "  not covered by an edge (counted, not drawn): {}",
                parts.join(", ")
            )?;
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

    use analyzer_core::graph::{Reason, Site, StableDef, Status};
    use analyzer_core::outline::DefKind;
    use analyzer_testkit::graph::check_invariants;

    use super::*;

    const STYLE: MarkerStyle<'static> = MarkerStyle {
        label: "ID",
        placement: analyzer_core::edit::Placement::Docstring,
    };

    #[test]
    fn a_passed_edge_is_not_counted_as_a_call_and_the_breakdown_says_it_is_passed() {
        use analyzer_core::graph::{Basis, Context, ContextId, DefInfo, Edge, Root};
        let id = |name: &str| ContextId::of(StableDef::Marked(name.to_owned()));
        let row = |name: &str| {
            DefInfo::new(
                StableDef::Marked(name.to_owned()),
                name,
                name,
                0,
                DefKind::Function,
                "m.ts",
                analyzer_core::text::Span::new(0, 1),
            )
        };
        let to = |name: &str| EdgeTo::Context(id(name), Basis::Type);
        let graph = Graph::new(
            vec![Root::Context(id("main"))],
            vec![row("main"), row("a"), row("b")],
            vec![
                Context::new(id("main")),
                Context::new(id("a")),
                Context::new(id("b")),
            ],
            vec![
                Edge::new(id("main"), Site::new(0, 1), to("a")),
                Edge::passed(id("main"), Site::new(2, 3), to("a")),
                Edge::passed(id("main"), Site::new(4, 5), to("b")),
            ],
            analyzer_core::graph::Stats::default(),
        );
        let summary = breakdown(&graph);
        assert_eq!((summary.calls, summary.passed), (1, 2));
        assert!(
            summary
                .to_string()
                .contains("1 call, 0 construct, 0 external, 0 unresolved, 2 passed"),
            "{summary}"
        );
        // a graph with none prints the line it always did
        let plain = Graph::new(
            vec![Root::Context(id("main"))],
            vec![row("main"), row("a")],
            vec![Context::new(id("main")), Context::new(id("a"))],
            vec![Edge::new(id("main"), Site::new(0, 1), to("a"))],
            analyzer_core::graph::Stats::default(),
        );
        assert!(
            breakdown(&plain)
                .to_string()
                .contains("1 call, 0 construct, 0 external, 0 unresolved\n"),
        );
    }

    #[test]
    fn a_def_a_walk_hands_over_that_no_edge_reaches_is_counted_once_as_written_or_named() {
        // The cross-check of D22: every hand-over has an edge now, so nothing reaches this count through the walk. Here the record is made by hand,
        // as a def the edge builder missed would leave it.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve");
        let project = Project::open(&root, Path::new("src/main.ts")).unwrap();
        let entry = Entry {
            qualname: "callers",
            occurrence: 0,
        };
        let opts = Options::new(STYLE);
        let mut graph = call_graph(&project, "src/main.ts", &entry, &opts, &Cancel::new()).unwrap();
        assert!(!graph.stats.language.contains_key("def_written_not_called"));
        let lang = TypeScript::new(&project.sem, STYLE);
        // two defs the graph has no edge to: one written inside an expression, one named
        lang.handed
            .borrow_mut()
            .insert(StableDef::Marked("NOT-IN-THE-GRAPH-1".into()), true);
        lang.handed
            .borrow_mut()
            .insert(StableDef::Marked("NOT-IN-THE-GRAPH-2".into()), false);
        count_handed_over(&mut graph, &lang);
        assert_eq!(graph.stats.language.get("def_written_not_called"), Some(&1));
        assert_eq!(graph.stats.language.get("def_named_not_called"), Some(&1));
        let text = breakdown(&graph).to_string();
        assert!(
            text.contains(
                "handed over and not called: 1 written inside an expression, 1 referenced by name"
            ),
            "{text}"
        );
    }

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
