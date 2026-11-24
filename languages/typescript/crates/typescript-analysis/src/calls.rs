//! The call graph: `TypeScript` is the language the core's flat solver walks, and `call_graph` is the one public function that runs it.
//!
//! One context per function, every context under the open key `KeyId::OPEN` (digest zeros): TypeScript is not context sensitive yet
//! (`docs/03-typescript/08-later-context-sensitive.md`). The seam is kept: a target is `Target::Call(func, KeyId::OPEN)`, and `walk` is the only place
//! that will make another key.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::{Basis, DefInfo, EdgeTo, Entry, Graph, StableDef};
use analyzer_core::outline::DefKind;
use analyzer_core::solve::{Budget, Cancel, Lang, Limits, Solver, Start, Summary};

use crate::Project;
use crate::env::Env;
use crate::key::{Key, KeyId, Keys};
use crate::sem::{Func, FunctionOrigin, Handed, Sem, UNHANDLED_KEY, Uncovered, counted};
use crate::walk;

/// How to build a graph. The marker style is how a def's marker is read (`ID: …` in its JSDoc).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Options {
    pub limits: Limits,
    pub marker: MarkerStyle<'static>,
    /// Whether the context layer is on. `Precision::Default` unless set.
    pub precision: Precision,
}

/// How much the analyzer may know about the values a function is given (`docs/03-typescript/12-context-layer/`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Precision {
    /// The engine's resolver alone: what every release so far gives.
    #[default]
    Default,
    /// The context layer on. It is not built yet: until TC1 step 4 and the phases after it, the graphs are `Default`'s.
    Precise,
}

impl Options {
    /// The default cost limits and `Precision::Default`, and `marker` for reading markers.
    pub fn new(marker: MarkerStyle<'static>) -> Self {
        Options {
            limits: Limits::default(),
            marker,
            precision: Precision::Default,
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
    /// The ids of the defs described that have a marker another def of their file owns (D34). Described more than once, so a set; `finish` counts the ones the
    /// graph holds.
    demoted: RefCell<HashSet<StableDef>>,
    /// Every key of this request (`key.rs`); `digest` reads it, and the walk will intern into it from TC2 on.
    keys: RefCell<Keys>,
}

impl<'p> TypeScript<'p> {
    pub(crate) fn new(sem: &'p Sem, style: MarkerStyle<'p>) -> Self {
        TypeScript {
            sem,
            style,
            notes: RefCell::new(BTreeMap::new()),
            handed: RefCell::new(HashMap::new()),
            demoted: RefCell::new(HashSet::new()),
            keys: RefCell::new(Keys::new()),
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
        self.note_n(key, 1);
    }

    /// `n` more of `key`; nothing when `n` is 0, so the key is in `Stats.language` only when there is something to say.
    pub(crate) fn note_n(&self, key: &'static str, n: u64) {
        if n > 0 {
            *self.notes.borrow_mut().entry(key).or_default() += n;
        }
    }
}

impl<'p> Lang for TypeScript<'p> {
    /// The engine answers with the type checker.
    const BASIS: Basis = Basis::Type;
    type Func = Func<'p>;
    type Key = Key;

    fn describe(&self, f: Self::Func) -> DefInfo {
        let info = self.sem.describe(f, self.style);
        if self.sem.has_a_marker_it_does_not_own(f, self.style) {
            self.demoted.borrow_mut().insert(info.id.clone());
        }
        info
    }

    fn digest(&self, key: &Self::Key) -> [u8; 16] {
        self.keys.borrow().digest(*key)
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
    graph_within(project, file, entry, opts, &Budget::default(), cancel)
}

/// `call_graph` and `Session::graph` in one place, so they cannot drift: the graph under `entry`, walked no further than `budget`. With
/// `Budget::default()` it is `call_graph`'s.
pub(crate) fn graph_within(
    project: &Project,
    file: &str,
    entry: &Entry<'_>,
    opts: &Options,
    budget: &Budget,
    cancel: &Cancel,
) -> Option<Graph> {
    let sem = &project.sem;
    let func = match *entry {
        Entry::Def {
            qualname,
            occurrence,
        } => sem.entry(file, qualname, occurrence, opts.marker)?,
        // the file's own code (D32): the module, a def of its own
        Entry::File => sem.file_entry(file)?,
        _ => return None,
    };
    let lang = TypeScript::new(sem, opts.marker);
    // what the walk of a body says it counted is in `lang`'s notes; what the request itself met (a class entry's construction) is said here
    let ((solver, mut graph), unhandled) = counted(|| {
        let start = start_of(&lang, func);
        let mut solver = Solver::new(lang, opts.limits.clone());
        let graph = solver.graph_within(&[start], budget, cancel);
        (solver, graph)
    });
    solver.lang().note_n(UNHANDLED_KEY, unhandled);
    finish(&mut graph, solver.lang());
    Some(graph)
}

/// Where a request for `func` starts. A class is a construction, as at a `new C()` site (D25): its tree is its constructor's calls, not the class def's body,
/// and what the construction counts is counted here, so the entry and the site agree.
pub(crate) fn start_of<'p>(lang: &TypeScript<'p>, func: Func<'p>) -> Start<TypeScript<'p>> {
    if lang.sem.kind_of(func, lang.style) == DefKind::Class {
        let (class, init) = walk::construction(lang, func, false);
        Start::Construct { class, init }
    } else {
        Start::Call(func, KeyId::OPEN)
    }
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
    count_duplicate_markers(graph, lang);
}

/// The defs of the graph named by where they are because another def of their file owns their marker (D34), once each. Only when there is one, so a graph
/// with none is what it was.
fn count_duplicate_markers(graph: &mut Graph, lang: &TypeScript<'_>) {
    let demoted = lang.demoted.borrow();
    let count = graph
        .defs
        .iter()
        .filter(|def| demoted.contains(&def.id))
        .count() as u64;
    if count > 0 {
        graph
            .stats
            .language
            .insert(walk::DUPLICATE_MARKER.to_owned(), count);
    }
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
    found.sort();
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
    /// `function_typed_value` by where the function comes from, the leftmost name of the callee (`from_parameter`, `from_call_result`, `from_this`,
    /// `from_other`, from `Stats.language`): the values sum to `reasons["function_typed_value"]`. Only the origins above 0.
    pub function_typed: BTreeMap<String, u64>,
    /// Calls of a function a library made (`setOpen` from `useState`), drawn as an `External` named after the call that made it (D56, policy row `library-made`):
    /// `Stats.language["library_made_function"]`, said only when above 0. Not unresolved, and no longer in `function_typed` either.
    pub library_made: u64,
    /// Defs a walked body hands over that no edge of the graph reaches, written inside an expression (`run({ onSuccess() {} })`, a property arrow of a
    /// literal passed on) and referenced by name (`onClick={handleClose}`). Counted once per def. Neither is an unresolved edge: the def is known and
    /// is not in the tree, because nothing calls it.
    pub handed_written: u64,
    pub handed_named: u64,
    /// Calls into a **source** file outside the root (D33): an `External` edge named by its path from the root (`../shared/lib.ts`), once per target of a
    /// site. A body was there to walk and was not. A declaration file out there is `External` too and is not counted: it has no body.
    pub outside_root: u64,
    /// Defs of the graph whose marker another def **of the same file** owns, earlier in source order (D34): each is named by where it is, as a def with
    /// no marker is, and counted once. The outline still reports the marker on both.
    pub duplicate_marker: u64,
    /// Places where a body hands over a signature that has no body (`use(w.write)` with `w` an interface, a type literal's method, an abstract method): no edge,
    /// nothing to enter (D43). From `Stats.language["signature_passed"]`, said only when above 0.
    pub signature_passed: u64,
    /// Nodes of a kind that an engine accessor does not handle, met where the code expected one (`engine_kind_unhandled`): a missing answer, never expected, said
    /// only when above 0.
    pub engine_kind_unhandled: u64,
    /// Defs that share an id with an earlier one **across files** and were dropped by the graph (`Stats::merged_defs`, D34 rule 2): their calls are drawn under
    /// the first. Nothing is renamed across files, because an id must not depend on what the request reaches.
    pub merged_defs: u64,
    /// What a body hands over or calls that **no edge covers**, per occurrence, by kind (D22 step 5): `element_access` (a function handed over by `a["k"]`),
    /// `bind` (handed over bound: `f.bind(…)`, `obj.m.bind(obj)`), `call_apply` (called through `call` or `apply`), `class` (a class named where a value is handed
    /// over). Only the kinds above 0. A count, not an edge: the def is known and is not in the tree.
    pub uncovered: BTreeMap<String, u64>,
    pub contexts: u64,
    pub walks: u64,
    pub reuse_hits: u64,
    /// Cuts by the name of the limit.
    pub cuts: BTreeMap<String, u64>,
    /// Contexts the request's budget stopped at (`Stats::frontier`): listed and not walked. Not in `cuts`, which are the solver's limits; the `cuts in the
    /// graph` line says both.
    pub frontier: u64,
}

/// Count the edges of `graph` by kind, and the unresolved ones by reason.
pub fn breakdown(graph: &Graph) -> Breakdown {
    let mut summary = Breakdown {
        contexts: graph.stats.contexts,
        walks: graph.stats.walks,
        reuse_hits: graph.stats.reuse_hits,
        frontier: graph.stats.frontier,
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
    summary.library_made = language(walk::LIBRARY_MADE);
    summary.outside_root = language(walk::OUTSIDE_ROOT);
    summary.duplicate_marker = language(walk::DUPLICATE_MARKER);
    summary.signature_passed = language(walk::SIGNATURE_PASSED);
    summary.engine_kind_unhandled = language(UNHANDLED_KEY);
    summary.merged_defs = graph.stats.merged_defs;
    summary.function_typed = FunctionOrigin::ALL
        .iter()
        .filter_map(|origin| {
            let key = origin.counter();
            let count = graph.stats.language.get(key).copied()?;
            Some((key.strip_prefix("function_typed_value_")?.to_owned(), count))
        })
        .collect();
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
        if !self.function_typed.is_empty() {
            writeln!(
                f,
                "  function_typed_value, by where the function comes from:"
            )?;
            // `ALL` is every origin, so none is left out
            for origin in FunctionOrigin::ALL {
                let key = origin
                    .counter()
                    .strip_prefix("function_typed_value_")
                    .unwrap_or_default();
                if let Some(count) = self.function_typed.get(key) {
                    writeln!(f, "    {}: {count}", origin.label())?;
                }
            }
        }
        if self.library_made > 0 {
            writeln!(
                f,
                "  library-made functions (named after the call that made them): {}",
                self.library_made
            )?;
        }
        if !self.notes.is_empty() {
            writeln!(f, "  constructions with no constructor entered, by why:")?;
            for (note, count) in &self.notes {
                writeln!(f, "    {note}: {count}")?;
            }
        }
        if !self.uncovered.is_empty() {
            // the keys are the counters' names less the prefix `breakdown` took off; `ALL` is every kind, so none is left out
            let parts: Vec<String> = Uncovered::ALL
                .iter()
                .filter_map(|kind| {
                    let key = kind.counter().strip_prefix("uncovered_")?;
                    Some(format!("{} {}", kind.label(), self.uncovered.get(key)?))
                })
                .collect();
            writeln!(
                f,
                "  not covered by an edge (counted, not drawn): {}",
                parts.join(", ")
            )?;
        }
        // said only when there is one, so every other breakdown is what it was
        if self.duplicate_marker > 0 {
            writeln!(
                f,
                "  defs named by where they are, because a def of the same file owns their marker: {}",
                self.duplicate_marker
            )?;
        }
        if self.merged_defs > 0 {
            writeln!(
                f,
                "  defs dropped because an earlier def has their id (their calls are drawn under it): {}",
                self.merged_defs
            )?;
        }
        if self.signature_passed > 0 {
            writeln!(
                f,
                "  signatures handed over, with no body to enter (counted, not drawn): {}",
                self.signature_passed
            )?;
        }
        if self.engine_kind_unhandled > 0 {
            writeln!(
                f,
                "  nodes of a kind the engine's accessors do not handle, met where one was expected: {}",
                self.engine_kind_unhandled
            )?;
        }
        if self.outside_root > 0 {
            writeln!(
                f,
                "  calls into source outside the root, not walked: {}",
                self.outside_root
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
        // the budget's stop first, then the solver's limits: the line says "none" only when neither cut anything
        let mut cuts: Vec<String> = Vec::new();
        if self.frontier > 0 {
            cuts.push(format!(
                "the budget stopped at {} (listed, not walked)",
                self.frontier
            ));
        }
        if !self.cuts.is_empty() {
            let limits: Vec<String> = self
                .cuts
                .iter()
                .map(|(cut, count)| format!("{cut} {count}"))
                .collect();
            cuts.push(limits.join(", "));
        }
        if cuts.is_empty() {
            writeln!(f, "  cuts in the graph: none")
        } else {
            writeln!(f, "  cuts in the graph: {}", cuts.join("; "))
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
        let entry = Entry::Def {
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
        let entry = Entry::Def {
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

    /// Every kind of `Uncovered` reaches the line of a breakdown, in the order of `Uncovered::ALL`: a key that the line did not know used to be left out
    /// without a word.
    /// The two counters of H14 (D43): a signature handed over has no edge, and a kind of node an engine accessor does not handle is a missing answer. Each is said
    /// when above 0, in the words the label table gives, and not otherwise.
    #[test]
    fn the_breakdown_says_signatures_handed_over_and_kinds_the_engine_did_not_handle() {
        let mut graph_language = BTreeMap::new();
        graph_language.insert(walk::SIGNATURE_PASSED.to_owned(), 2);
        graph_language.insert(UNHANDLED_KEY.to_owned(), 1);
        let mut graph = Graph::default();
        graph.stats.language = graph_language;
        let said = breakdown(&graph).to_string();
        assert!(
            said.contains("signatures handed over, with no body to enter (counted, not drawn): 2"),
            "{said}"
        );
        assert!(
            said.contains(
                "a kind the engine's accessors do not handle, met where one was expected: 1"
            ),
            "{said}"
        );
        let quiet = breakdown(&Graph::default()).to_string();
        assert!(!quiet.contains("signatures handed over") && !quiet.contains("accessors do not"));
    }

    /// The `cuts in the graph` line says what stopped the request: the budget (the frontier), the solver's limits, both, or neither. "none" only when neither did.
    #[test]
    fn the_cuts_line_says_the_frontier_and_the_solvers_limits() {
        let line = |frontier: u64, solver: bool| {
            let mut graph = Graph::default();
            graph.stats.frontier = frontier;
            if solver {
                graph
                    .stats
                    .cuts
                    .insert(analyzer_core::graph::Cut::MaxContexts, 2);
            }
            let said = breakdown(&graph).to_string();
            said.lines().last().unwrap().to_owned()
        };
        assert_eq!(line(0, false), "  cuts in the graph: none");
        assert_eq!(
            line(3, false),
            "  cuts in the graph: the budget stopped at 3 (listed, not walked)"
        );
        assert_eq!(line(0, true), "  cuts in the graph: MaxContexts 2");
        assert_eq!(
            line(3, true),
            "  cuts in the graph: the budget stopped at 3 (listed, not walked); MaxContexts 2"
        );
    }

    #[test]
    fn the_breakdown_line_says_every_uncovered_kind() {
        // the line of `Display` that counts what no edge covers, and nothing else of the breakdown
        let line = |uncovered: BTreeMap<String, u64>| {
            Breakdown {
                uncovered,
                ..Breakdown::default()
            }
            .to_string()
            .lines()
            .find(|line| line.contains("not covered by an edge"))
            .expect("the line is there")
            .to_owned()
        };
        let key = |kind: Uncovered| {
            kind.counter()
                .strip_prefix("uncovered_")
                .unwrap()
                .to_owned()
        };
        assert_eq!(
            line(Uncovered::ALL.iter().map(|&kind| (key(kind), 1)).collect()),
            "  not covered by an edge (counted, not drawn): element access 1, bind 1, call/apply 1, class 1, destructured accessor 1, rest element 1, assignment destructuring 1, construction initializer sites 1, class component render 1, getter result calls 1, for-of destructuring 1"
        );
        for kind in Uncovered::ALL {
            assert_eq!(
                line([(key(kind), 2)].into_iter().collect()),
                format!(
                    "  not covered by an edge (counted, not drawn): {} 2",
                    kind.label()
                )
            );
        }
    }

    /// How each decorator site of a class resolves. No request draws a class body any more (a class is a construction, and its decorators are drawn where it is declared), so this walks
    /// it as a call, the way `Start::Call` of a class would, to keep what the resolver says about a decorator tested.
    #[test]
    fn the_sites_of_a_decorated_class_body_resolve_as_calls_with_a_second_call_for_a_factorys_result()
     {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/site-kinds");
        let project = Project::open(&root, Path::new("src/decorators.ts")).unwrap();
        let sem = &project.sem;
        let class = sem
            .entry("src/decorators.ts", "Decorated", 0, STYLE)
            .unwrap();
        let mut solver = Solver::new(TypeScript::new(sem, STYLE), Limits::default());
        let graph = solver.graph(&[(class, KeyId::OPEN)], &Cancel::new());
        let text = std::fs::read_to_string(root.join("src/decorators.ts")).unwrap();
        let name = |id: &StableDef| graph.def(id).unwrap().qname.clone();
        let from_the_class: Vec<String> = graph
            .edges
            .iter()
            .filter(|edge| name(&edge.from.func) == "Decorated")
            .map(|edge| {
                let def = graph.def(&edge.from.func).unwrap();
                let at = def.range.start;
                let callee = &text[(at + edge.site.start) as usize..(at + edge.site.end) as usize];
                let to = match &edge.to {
                    EdgeTo::Context(id, _) => format!("call {}", name(&id.func)),
                    EdgeTo::Unresolved(reason) => format!("unresolved {}", reason.as_str()),
                    other => panic!("{other:?}"),
                };
                format!("{callee} | {to}")
            })
            .collect();
        assert_eq!(
            from_the_class,
            [
                "plain | call plain",
                "declared | call declared",
                // `@declared()`: a function type, nothing to enter
                "declared() | unresolved decorator_result",
                "inlineFactory | call inlineFactory",
                // the arrow it returns is part of its body and no def
                "inlineFactory(1) | unresolved decorator_result",
                "onMethod | call onMethod",
                "namedFactory | call namedFactory",
                // it returns a function with a name, which the checker's signature finds
                "namedFactory(2) | call applyNamed",
                "parameterFactory | call parameterFactory",
                "parameterFactory(\"x\") | call onParameter",
                "onParameter | call onParameter",
            ]
        );
        // the class has no instance field, so every site of its body is one of the decorators
        assert_eq!(
            sem.call_sites(class, STYLE).unwrap().len(),
            from_the_class.len()
        );
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
        let lang = TypeScript::new(sem, STYLE);
        let funcs: Vec<_> = match std::env::var("TS_MEASURE_ENTRY") {
            Ok(qualname) => vec![sem.entry(&file, &qualname, 0, STYLE).unwrap()],
            Err(_) => sem.project_defs(STYLE),
        };
        // the starts a request makes: a class is a construction
        let entries: Vec<_> = funcs.into_iter().map(|f| start_of(&lang, f)).collect();
        let mut solver = Solver::new(lang, Limits::new(u32::MAX, None));
        let started = std::time::Instant::now();
        let mut graph = solver.graph_within(&entries, &Budget::default(), &Cancel::new());
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

    /// The gate of TC1 step 2, on a real project, not a check (`docs/03-typescript/12-context-layer/05-tc1-the-body-as-data.md` §8). Same variables as
    /// `measure_handed_over` (no entry), and `TS_MEASURE_PART`, one per process so that its peak memory can be read from `/usr/bin/time -l`:
    /// `sweep` (the depth-1 graph of every project def in one request), `flows` (that request, then the table of every def, checked against `call_sites`),
    /// `cold` (a fresh project, only the tables). `cargo test --release -p typescript-analysis --lib measure_flow -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement on a real project, driven by TS_MEASURE_ROOT, TS_MEASURE_FILE and TS_MEASURE_PART"]
    fn measure_flow() {
        use std::time::Instant;

        let (Ok(root), Ok(file), Ok(part)) = (
            std::env::var("TS_MEASURE_ROOT"),
            std::env::var("TS_MEASURE_FILE"),
            std::env::var("TS_MEASURE_PART"),
        ) else {
            eprintln!(
                "set TS_MEASURE_ROOT, TS_MEASURE_FILE and TS_MEASURE_PART (sweep, flows or cold)"
            );
            return;
        };
        let project = Project::open(Path::new(&root), Path::new(&file)).unwrap();
        let sem = &project.sem;
        let defs = sem.project_defs(STYLE);

        // the tables of every def, each checked against the sites `call_sites` lists
        let tables = || -> (usize, usize, usize, usize, usize) {
            let (mut sites, mut arguments, mut anons, mut handed, mut handed_defs) =
                (0, 0, 0, 0, 0);
            for &func in &defs {
                let Some(flow) = sem.flow(sem.def_id(func), STYLE) else {
                    assert!(sem.call_sites(func, STYLE).is_none());
                    continue;
                };
                let spans: Vec<_> = flow.sites.iter().map(|site| site.span).collect();
                assert_eq!(Some(spans), sem.call_sites(func, STYLE));
                sites += flow.sites.len();
                arguments += flow
                    .sites
                    .iter()
                    .map(|site| site.args.as_ref().map_or(0, |args| args.len()))
                    .sum::<usize>();
                anons += flow.anons.len();
                // the table sees what D22 sees: the defs of `handed_over` are the defs of the table, and no other entry holds one
                fn holds_a_def(expr: &crate::flow::Expr) -> bool {
                    match expr {
                        crate::flow::Expr::Def(_) => true,
                        crate::flow::Expr::OneOf(members) => members.iter().any(holds_a_def),
                        _ => false,
                    }
                }
                let key =
                    |at: Option<crate::Span>, id: crate::flow::DefId| (at, id.file(), id.index());
                let mut wanted: Vec<_> = sem
                    .handed_over(func, STYLE)
                    .handed
                    .iter()
                    .map(|found| key(found.at, sem.def_id(found.func)))
                    .collect();
                let mut found: Vec<_> = flow
                    .handed
                    .iter()
                    .filter_map(|entry| match entry.value {
                        crate::flow::Expr::Def(id) => Some(key(entry.at, id)),
                        _ => None,
                    })
                    .collect();
                wanted.sort();
                found.sort();
                assert_eq!(found, wanted);
                for entry in &flow.handed {
                    assert!(
                        wanted.iter().any(|&(at, ..)| at == entry.at) || !holds_a_def(&entry.value)
                    );
                }
                handed += flow.handed.len();
                handed_defs += found.len();
            }
            (sites, arguments, anons, handed, handed_defs)
        };
        let sweep = || {
            let lang = TypeScript::new(sem, STYLE);
            let entries: Vec<_> = defs.iter().map(|&f| start_of(&lang, f)).collect();
            let mut solver = Solver::new(lang, Limits::new(u32::MAX, None));
            let started = Instant::now();
            let _graph = solver.graph_within(&entries, &Budget::new(Some(1), None), &Cancel::new());
            started.elapsed()
        };
        match part.as_str() {
            "sweep" => eprintln!("defs {}, graph {:?}", defs.len(), sweep()),
            "flows" => {
                let graph = sweep();
                let started = Instant::now();
                let (sites, arguments, anons, handed, handed_defs) = tables();
                eprintln!(
                    "defs {}, graph {graph:?}, flows {:?}, sites {sites}, arguments {arguments}, anonymous functions {anons}, hand-overs {handed}, of them defs {handed_defs}",
                    defs.len(),
                    started.elapsed()
                );
            }
            "cold" => {
                let started = Instant::now();
                let (sites, arguments, anons, handed, handed_defs) = tables();
                eprintln!(
                    "defs {}, flows {:?}, sites {sites}, arguments {arguments}, anonymous functions {anons}, hand-overs {handed}, of them defs {handed_defs}",
                    defs.len(),
                    started.elapsed()
                );
            }
            other => eprintln!("TS_MEASURE_PART is sweep, flows or cold, not {other}"),
        }
    }

    /// A measurement on a real project, not a check: the two steps of a request that a cancel cannot stop (`docs/03-typescript/07-ts5-driver-needs.md` §4). Listing a
    /// body's sites happens before the walk looks at the cancel for the first time, and one site's resolution happens between two looks. Same variables as
    /// `measure_handed_over` (the entry is not used), `cargo test --release -p typescript-analysis --lib measure_uninterruptible_steps -- --ignored --nocapture`.
    /// Cold: a project opened afresh, every def asked once, in the order of `project_defs`, so the first def of each file pays for that file's outline.
    #[test]
    #[ignore = "a measurement on a real project, driven by TS_MEASURE_ROOT and TS_MEASURE_FILE"]
    fn measure_uninterruptible_steps() {
        use std::time::{Duration, Instant};

        let (Ok(root), Ok(file)) = (
            std::env::var("TS_MEASURE_ROOT"),
            std::env::var("TS_MEASURE_FILE"),
        ) else {
            eprintln!("set TS_MEASURE_ROOT and TS_MEASURE_FILE");
            return;
        };
        fn median(times: &mut [Duration]) -> Duration {
            times.sort();
            times.get(times.len() / 2).copied().unwrap_or_default()
        }
        let project = Project::open(Path::new(&root), Path::new(&file)).unwrap();
        let sem = &project.sem;
        let defs = sem.project_defs(STYLE);

        // listing the sites of each body
        let mut listing: Vec<(Duration, usize, usize)> = Vec::new();
        for (index, func) in defs.iter().enumerate() {
            let started = Instant::now();
            let sites = sem.call_sites(*func, STYLE).map_or(0, |sites| sites.len());
            listing.push((started.elapsed(), sites, index));
        }
        let mut times: Vec<Duration> = listing.iter().map(|(time, ..)| *time).collect();
        let (slowest, sites, slowest_at) = *listing.iter().max().unwrap();
        let name = |index: usize| {
            let info = sem.describe(defs[index], STYLE);
            format!("{} `{}`", info.file, info.qualname)
        };
        eprintln!(
            "listing sites, {} defs: max {slowest:?} ({sites} sites, {}), median {:?}",
            defs.len(),
            name(slowest_at),
            median(&mut times)
        );
        // the first def asked of a file pays for the file's outline, which is not the listing
        let mut seen_files = HashSet::new();
        let first_of_file: Vec<bool> = defs
            .iter()
            .map(|func| seen_files.insert(sem.describe(*func, STYLE).file))
            .collect();
        let max_of = |first: bool| {
            listing
                .iter()
                .filter(|(_, _, index)| first_of_file[*index] == first)
                .max()
                .copied()
        };
        for (first, label) in [
            (true, "first def of its file"),
            (false, "not the first of its file"),
        ] {
            if let Some((time, sites, index)) = max_of(first) {
                eprintln!(
                    "    max, {label}: {time:?} ({sites} sites, {})",
                    name(index)
                );
            }
        }
        let mut by_time = listing.clone();
        by_time.sort_by(|a, b| b.cmp(a));
        for (time, sites, index) in by_time.iter().take(5) {
            eprintln!("    {time:?}  {sites} sites  {}", name(*index));
        }

        // resolving each site of each body, after the listing, in the same process
        let mut resolution: Vec<(Duration, usize, usize)> = Vec::new();
        for (index, func) in defs.iter().enumerate() {
            let count = sem.call_sites(*func, STYLE).map_or(0, |sites| sites.len());
            for site in 0..count {
                let started = Instant::now();
                let _ = sem.callee_with_facts(*func, site, STYLE);
                resolution.push((started.elapsed(), index, site));
            }
        }
        let mut times: Vec<Duration> = resolution.iter().map(|(time, ..)| *time).collect();
        let (time, index, site) = *resolution.iter().max().unwrap();
        eprintln!(
            "resolving one site, {} sites after the listing: max {time:?} (site {site} of {}), median {:?}",
            resolution.len(),
            name(index),
            median(&mut times)
        );
        let mut the_def: Vec<Duration> = resolution
            .iter()
            .filter(|(_, at, _)| *at == slowest_at)
            .map(|(time, ..)| *time)
            .collect();
        eprintln!(
            "    the def with the slowest listing: its slowest site {:?}, median {:?}",
            the_def.iter().max().copied().unwrap_or_default(),
            median(&mut the_def)
        );

        // that def again on a project nothing has asked anything of: its sites listed, then each resolved once
        let fresh = Project::open(Path::new(&root), Path::new(&file)).unwrap();
        let func = fresh.sem.project_defs(STYLE)[slowest_at];
        let count = fresh.sem.call_sites(func, STYLE).map_or(0, |s| s.len());
        let mut cold: Vec<Duration> = (0..count)
            .map(|site| {
                let started = Instant::now();
                let _ = fresh.sem.callee_with_facts(func, site, STYLE);
                started.elapsed()
            })
            .collect();
        eprintln!(
            "    and on a fresh project, only that def asked: its slowest site {:?}, median {:?}, {count} sites",
            cold.iter().max().copied().unwrap_or_default(),
            median(&mut cold)
        );
    }
}
