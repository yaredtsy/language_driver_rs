//! The call graph: `Rust` is the language the core's flat solver walks, and `call_graph` is the one public function that runs it.
//!
//! One context per function (`Key = ()`, digest zeros): Rust is not context sensitive yet (`docs/04-rust/02-architecture.md` §6). The seam is kept: a target
//! is `Target::Call(func, key)` with `callee_key` saying which key, and `walk` is the only place that would take one.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::fmt;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::{Basis, DefInfo, EdgeTo, Entry, Graph, StableDef};
use analyzer_core::solve::{Budget, Cancel, Lang, Limits, Solver, Start, Summary};

use crate::Project;
use crate::env::Env;
use crate::key::Key;
use crate::sem::{Func, Item, Sem, attached};
use crate::walk;

/// The keys of the counters, in `Stats.language`, that are not the reason of an edge: notes of the walk and the bridge (§2). Each is absent when 0.
pub(crate) const MACRO_NOT_EXPANDED: &str = "macro_not_expanded";
pub(crate) const MACRO_GENERATED_TARGET: &str = "macro_generated_target";
pub(crate) const OUTSIDE_ROOT: &str = "outside_root";
pub(crate) const DUPLICATE_MARKER: &str = "duplicate_marker";
/// A function outside project code handed over and not called: no edge (only a call into project code can be passed, D22), counted. Whatever its cause: a
/// dependency's function, one a project macro made, a built-in derive's, one beside the root (`Cause`).
pub(crate) const EXTERNAL_PASSED: &str = "external_passed";
/// A tuple struct or variant handed over (`.map(Some)`, `let f = E::P;`): a constructor has no body, so no edge, counted.
pub(crate) const CONSTRUCTOR_PASSED: &str = "constructor_passed";
/// A trait's required method handed over (`.map(W::build)`): a declaration with no body stands for whatever implements it (D44), and a hand-over is not a
/// call, so no edge and not an `Unresolved` one either, counted. TypeScript counts a signature handed over under the same key (D43).
pub(crate) const SIGNATURE_PASSED: &str = "signature_passed";

/// How to build a graph. The marker style is how a def's marker is read (`/// ID: …` in its doc comment).
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

/// The language the solver walks: one load, read through `Sem`, inside one `attach_db`.
pub(crate) struct Rust<'a, 'p> {
    pub(crate) sem: &'a Sem<'p>,
    /// Counters that are not the reason of an edge, filled as bodies are walked, once each, and put in `Stats.language` by `finish`.
    notes: RefCell<BTreeMap<&'static str, u64>>,
    /// The ids of the defs described that have a marker another def of their file owns (D34). Described more than once, so a set; `finish` counts the ones
    /// the graph holds.
    demoted: RefCell<HashSet<StableDef>>,
}

impl<'a, 'p> Rust<'a, 'p> {
    pub(crate) fn new(sem: &'a Sem<'p>) -> Self {
        Rust {
            sem,
            notes: RefCell::new(BTreeMap::new()),
            demoted: RefCell::new(HashSet::new()),
        }
    }

    /// Count `by` more of `key`.
    pub(crate) fn note(&self, key: &'static str, by: u64) {
        if by > 0 {
            *self.notes.borrow_mut().entry(key).or_default() += by;
        }
    }
}

impl<'p> Lang for Rust<'_, 'p> {
    /// The engine answers with the type checker.
    const BASIS: Basis = Basis::Type;
    type Func = Func<'p>;
    type Key = Key;

    fn describe(&self, f: Self::Func) -> DefInfo {
        let info = self.sem.describe(f);
        if self.sem.has_a_marker_it_does_not_own(f) {
            self.demoted.borrow_mut().insert(info.id.clone());
        }
        info
    }

    fn digest(&self, _key: &Self::Key) -> [u8; 16] {
        [0; 16]
    }

    fn walk(&mut self, f: Self::Func, _key: &Self::Key, cancel: &Cancel) -> Summary<Self> {
        walk::body(self, f, &Env, cancel)
    }
}

/// The context graph under `entry`, a def in `file` (relative to the project's root, as `Project::files` lists it): every function it reaches, as far as the
/// limits allow. `None` when there is no such def (no such file, a file that is not in `files()`, no such qualname, or fewer than `occurrence + 1` defs
/// with it), when the def cannot be an entry (a trait, a module, a type alias, a macro), when the engine has no def there (code a `cfg` turns off), and for
/// `Entry::File`: nothing runs at the top of a Rust file. A struct, a union or an enum is an entry: a construction with nothing to enter. The tree and the
/// wire are views of this graph (`analyzer_core::render`).
pub fn call_graph(
    project: &Project,
    file: &str,
    entry: &Entry<'_>,
    opts: &Options,
    cancel: &Cancel,
) -> Option<Graph> {
    graph_within(project, file, entry, opts, &Budget::default(), cancel)
}

/// `call_graph` and RS5's session in one place, so they cannot drift: the graph under `entry`, walked no further than `budget`. With `Budget::default()` it
/// is `call_graph`'s.
pub(crate) fn graph_within(
    project: &Project,
    file: &str,
    entry: &Entry<'_>,
    opts: &Options,
    budget: &Budget,
    cancel: &Cancel,
) -> Option<Graph> {
    let (qualname, occurrence) = match *entry {
        Entry::Def {
            qualname,
            occurrence,
        } => (qualname, occurrence),
        // no code runs at the top of a Rust file (D32): the language says so, `None`
        _ => return None,
    };
    // one `attach_db` for the whole request; the `Sem` is built inside it and never kept
    attached(project.loaded(), opts.marker, |sem| {
        let func = sem.entry(file, qualname, occurrence)?;
        let lang = Rust::new(sem);
        let start = start_of(func);
        let mut solver = Solver::new(lang, opts.limits.clone());
        let mut graph = solver.graph_within(&[start], budget, cancel);
        finish(&mut graph, solver.lang(), project);
        Some(graph)
    })
}

/// Where a request for `func` starts. A struct, a union or an enum is a construction, as at a `S { .. }` site: its tree is empty, and Rust has no constructor
/// function to enter, so `init` is always `None`. Anything else is a function under the one key.
pub(crate) fn start_of<'a, 'p>(func: Func<'p>) -> Start<Rust<'a, 'p>> {
    match func.item() {
        Item::Adt(_) => Start::Construct {
            class: func,
            init: None,
        },
        Item::Function(_) | Item::Const(_) | Item::Static(_) => Start::Call(func, ()),
    }
}

/// What a graph has beyond its edges, in `Stats.language` (§2): each unresolved edge's reason (a count is what the edges say), the notes the walk kept, the
/// defs named by where they are because another def of their file owns their marker, what the load did not do (`Project::skipped`), and each warning of the
/// load as `warning_<code>: 1`. Counts only, never a message. Absent when 0.
fn finish(graph: &mut Graph, lang: &Rust<'_, '_>, project: &Project) {
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
    let demoted = lang.demoted.borrow();
    let duplicates = graph
        .defs
        .iter()
        .filter(|def| demoted.contains(&def.id))
        .count() as u64;
    if duplicates > 0 {
        graph
            .stats
            .language
            .insert(DUPLICATE_MARKER.to_owned(), duplicates);
    }
    for (key, count) in project.skipped() {
        graph.stats.language.insert((*key).to_owned(), *count);
    }
    for warning in project.warnings() {
        graph
            .stats
            .language
            .insert(format!("{WARNING_PREFIX}{}", warning.code()), 1);
    }
}

/// The prefix of a load's warning in `Stats.language`: `warning_no_toolchain`.
const WARNING_PREFIX: &str = "warning_";

/// What a graph is made of, and why its unresolved edges are unresolved. A missing edge is explained by a number here, not by reading the code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Breakdown {
    /// What the load could not do as asked, by code (`no_toolchain`, `no_std_source`, …): a tree read at rung 3 says so at its top. Sorted.
    pub warnings: Vec<String>,
    /// What the load did not do on purpose (`build_scripts_skipped`, `proc_macros_skipped`).
    pub skipped: BTreeMap<String, u64>,
    /// Edges that call a def of the project. A def handed over is not one (`passed`).
    pub calls: u64,
    /// Edges that hand a def over and do not call it (D22).
    pub passed: u64,
    pub constructs: u64,
    pub externals: u64,
    pub unresolved: u64,
    /// `unresolved` by reason; the values sum to `unresolved`.
    pub reasons: BTreeMap<String, u64>,
    /// The sites whose answer is a family of defs and not one (§4), by why, the `open_` of the counter taken off. A site is counted once.
    pub open: BTreeMap<String, u64>,
    /// Macro calls the engine could not expand: what was in their arguments is lost.
    pub macro_not_expanded: u64,
    /// Functions outside project code handed over and not called (`.map(String::from)`, a function a project macro made, a derive's `clone`): no edge,
    /// counted, whatever the cause.
    pub external_passed: u64,
    /// Tuple structs and variants handed over (`.map(Some)`): no edge, counted.
    pub constructor_passed: u64,
    /// A trait's required method handed over and not called: no edge, counted (D44).
    pub signature_passed: u64,
    /// Calls to a function a macro or a built-in derive made: `External`, named, not entered (a body that was there and was not walked).
    pub macro_generated_target: u64,
    /// Calls into a local crate's file outside the root (D33's rule): `External`, a body that was there to walk and was not.
    pub outside_root: u64,
    /// Defs of the graph whose marker another def of the same file owns, earlier in source order (D34), named by where they are.
    pub duplicate_marker: u64,
    /// Defs that share an id with an earlier one across files and were dropped by the graph (`Stats::merged_defs`).
    pub merged_defs: u64,
    pub contexts: u64,
    pub walks: u64,
    pub reuse_hits: u64,
    /// Cuts by the name of the limit.
    pub cuts: BTreeMap<String, u64>,
}

/// Count the edges of `graph` by kind, the unresolved ones by reason, and read the counters of `Stats.language` back.
pub fn breakdown(graph: &Graph) -> Breakdown {
    let language = &graph.stats.language;
    let count = |key: &str| language.get(key).copied().unwrap_or(0);
    let mut summary = Breakdown {
        contexts: graph.stats.contexts,
        walks: graph.stats.walks,
        reuse_hits: graph.stats.reuse_hits,
        merged_defs: graph.stats.merged_defs,
        cuts: graph
            .stats
            .cuts
            .iter()
            .map(|(cut, count)| (format!("{cut:?}"), *count))
            .collect(),
        macro_not_expanded: count(MACRO_NOT_EXPANDED),
        external_passed: count(EXTERNAL_PASSED),
        constructor_passed: count(CONSTRUCTOR_PASSED),
        signature_passed: count(SIGNATURE_PASSED),
        macro_generated_target: count(MACRO_GENERATED_TARGET),
        outside_root: count(OUTSIDE_ROOT),
        duplicate_marker: count(DUPLICATE_MARKER),
        // `Stats.language` is a sorted map: so are these
        warnings: language
            .keys()
            .filter_map(|key| key.strip_prefix(WARNING_PREFIX))
            .map(str::to_owned)
            .collect(),
        skipped: language
            .iter()
            .filter(|(key, _)| key.ends_with("_skipped"))
            .map(|(key, count)| (key.clone(), *count))
            .collect(),
        open: language
            .iter()
            .filter_map(|(key, count)| Some((key.strip_prefix("open_")?.to_owned(), *count)))
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
    summary
}

impl fmt::Display for Breakdown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "breakdown:")?;
        // first: what the load could not do as asked, because it explains what follows
        if !self.warnings.is_empty() {
            writeln!(f, "  warnings: {}", self.warnings.join(", "))?;
        }
        if !self.skipped.is_empty() {
            let skipped: Vec<String> = self
                .skipped
                .iter()
                .map(|(key, count)| format!("{key} {count}"))
                .collect();
            writeln!(f, "  skipped on purpose: {}", skipped.join(", "))?;
        }
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
        if !self.open.is_empty() {
            writeln!(f, "  open (the answer is a family of defs), by why:")?;
            for (open, count) in &self.open {
                writeln!(f, "    {open}: {count}")?;
            }
        }
        for (what, count) in [
            (
                "macro calls not expanded (their arguments are lost)",
                self.macro_not_expanded,
            ),
            (
                "functions outside project code handed over and not called (no edge)",
                self.external_passed,
            ),
            (
                "tuple structs and variants handed over and not called (no edge)",
                self.constructor_passed,
            ),
            (
                "trait methods with no body handed over and not called (no edge)",
                self.signature_passed,
            ),
            (
                "calls to a def a macro or a derive made, not entered",
                self.macro_generated_target,
            ),
            (
                "calls into source outside the root, not walked",
                self.outside_root,
            ),
            (
                "defs named by where they are, because a def of the same file owns their marker",
                self.duplicate_marker,
            ),
            (
                "defs dropped because an earlier def has their id (their calls are drawn under it)",
                self.merged_defs,
            ),
        ] {
            if count > 0 {
                writeln!(f, "  {what}: {count}")?;
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
