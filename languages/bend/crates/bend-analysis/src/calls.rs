//! The call graph: `Bend` is the language the core's flat solver walks, and `call_graph` is the one public function that runs it.
//!
//! One context per def (`Key = ()`, digest zeros): Bend is not context sensitive yet (docs/08-bend/04-phase-3-context-layer.md). The seam is kept: a target is
//! `Target::Call(def, key)`, and `walk` is the only place that would take a key.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::{Basis, DefInfo, EdgeTo, Entry, Graph, StableDef};
use analyzer_core::outline::DefKind;
use analyzer_core::solve::{Budget, Cancel, Lang, Limits, Solver, Start, Summary};
use analyzer_core::text::Span;

use crate::book::{Book, DeclId};
use crate::env::Env;
use crate::key::Key;
use crate::sem::{self, Item};
use crate::walk::{self, Reached};

/// The keys of the counters in `Stats.language` that the walk keeps, besides the reason of each unresolved edge. Each is absent when 0.
pub(crate) const TYPE_POSITION: &str = "type_position";
pub(crate) const SUGAR_BASE: &str = "sugar_base";
pub(crate) const PASSED_DEEP: &str = "passed_deep";
pub(crate) const OPEN_LAW: &str = "open_law";
pub(crate) const OUTSIDE_ROOT: &str = "outside_root";
pub(crate) const DUPLICATE_MARKER: &str = "duplicate_marker";
/// A def outside the root handed over and not called: no edge (only a call into project code can be passed, D22), counted.
pub(crate) const EXTERNAL_PASSED: &str = "external_passed";
/// A `type` handed over (`f(Shape)`): a type argument, no code to hand over, so no edge, counted.
pub(crate) const TYPE_PASSED: &str = "type_passed";

/// The reasons of the `Unresolved` edges; the same strings are their counters.
pub(crate) const FUNCTION_TYPED_VALUE: &str = "function_typed_value";
pub(crate) const UNKNOWN_NAME: &str = "unknown_name";
pub(crate) const UNKNOWN_CONSTRUCTOR: &str = "unknown_constructor";

/// How to build a graph. The marker style is how a def's marker is read (`# ID: …` in the comment block above it); the limits protect the cost of a request
/// and cut (a cut is counted in `Stats.cuts`).
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

/// The markers of one file, read with the caller's style.
#[derive(Debug)]
pub(crate) struct Marked {
    pub(crate) markers: Vec<Option<String>>,
    /// Which defs own their marker: the first in source order of each text (D34). Another def carrying it is named by where it is.
    pub(crate) owns: Vec<bool>,
    pub(crate) occurrences: Vec<u32>,
}

impl Marked {
    pub(crate) fn new(text: &str, style: MarkerStyle<'_>) -> Self {
        let outline = bend_syntax::outline(text, style);
        let mut order: Vec<usize> = (0..outline.defs.len()).collect();
        order.sort_by_key(|&at| (outline.defs[at].range.start, at));
        let mut seen: HashSet<&str> = HashSet::new();
        let mut owns = vec![false; outline.defs.len()];
        for at in order {
            if let Some(marker) = outline.defs[at].marker.as_deref() {
                owns[at] = seen.insert(marker);
            }
        }
        Marked {
            markers: outline.defs.iter().map(|d| d.marker.clone()).collect(),
            owns,
            occurrences: bend_syntax::occurrences(&outline),
        }
    }
}

/// The language the solver walks: one book, read through `sem`.
pub(crate) struct Bend<'b> {
    pub(crate) book: &'b Book,
    style: MarkerStyle<'static>,
    /// Only ever looked up, never iterated (rule 9).
    /// Counters that are not the reason of an edge, filled as bodies are walked, once each, and put in `Stats.language` by `finish`.
    notes: RefCell<BTreeMap<&'static str, u64>>,
    /// The ids of the defs described that have a marker another def of their file owns (D34). Described more than once, so a set.
    demoted: RefCell<HashSet<StableDef>>,
}

impl<'b> Bend<'b> {
    pub(crate) fn new(book: &'b Book, style: MarkerStyle<'static>) -> Self {
        Bend {
            book,
            style,
            notes: RefCell::new(BTreeMap::new()),
            demoted: RefCell::new(HashSet::new()),
        }
    }

    pub(crate) fn note(&self, key: &'static str, by: u64) {
        if by > 0 {
            *self.notes.borrow_mut().entry(key).or_default() += by;
        }
    }

    /// The places of `f`'s body (the book reads each file once).
    pub(crate) fn items(&self, f: DeclId) -> Vec<Item> {
        let Some(def) = self.book.def(f) else {
            return Vec::new();
        };
        sem::walk_of(self.book.walks(f.file()), def.name_range)
            .map_or_else(Vec::new, |w| w.items.clone())
    }
}

impl Lang for Bend<'_> {
    /// The code's own names fixed every edge (BD-D11).
    const BASIS: Basis = Basis::Name;
    type Func = DeclId;
    type Key = Key;

    fn describe(&self, f: DeclId) -> DefInfo {
        let (Some(file), Some(def)) = (self.book.file(f.file()), self.book.def(f)) else {
            unreachable!("a DeclId of the book the solver was given");
        };
        let marked = self.book.marked(f.file(), self.style);
        let occurrence = marked.as_ref().map_or(0, |m| m.occurrences[f.def()]);
        let marker = marked.as_ref().and_then(|m| m.markers[f.def()].clone());
        let owns = marked.as_ref().is_some_and(|m| m.owns[f.def()]);
        // a marker another def of the file owns is not this def's id (D34): it is named by where it is, as a def with no marker is
        let id = match marker {
            Some(marker) if owns => StableDef::Marked(marker),
            Some(_) => {
                let id = StableDef::Path {
                    module: file.relative.clone(),
                    qname: def.qualname.clone(),
                    nth: occurrence as u16,
                };
                self.demoted.borrow_mut().insert(id.clone());
                id
            }
            None => StableDef::Path {
                module: file.relative.clone(),
                qname: def.qualname.clone(),
                nth: occurrence as u16,
            },
        };
        DefInfo::new(
            id,
            def.qualname.clone(),
            def.qualname.clone(),
            occurrence as u16,
            def.kind.clone(),
            file.relative.clone(),
            def.range,
        )
    }

    fn digest(&self, _key: &Key) -> [u8; 16] {
        [0; 16]
    }

    fn walk(&mut self, f: DeclId, _key: &Key, cancel: &Cancel) -> Summary<Self> {
        walk::body(self, f, &Env, cancel)
    }
}

/// The context graph under `entry`, a def of `file` (from the root, or absolute): every def it reaches, as far as `limits` allow. `entry` is the def's
/// qualname; when a file has two defs of it, a law and its proof, the one with code is the entry (the law alone when there is no proof). `None` when the
/// file is not in the book or has no such def. A `type` is an entry too: a construction, with nothing to enter. The tree and the wire are views of this graph
/// (`analyzer_core::render`). `options.limits` is not read: `limits` is.
pub fn call_graph(
    book: &Book,
    file: &Path,
    entry: &str,
    options: &Options,
    limits: &Limits,
    cancel: &Cancel,
) -> Option<Graph> {
    let at = book.file_at(file)?;
    let defs = &book.file(at)?.outline.defs;
    let named = |def: &&analyzer_core::outline::Def| def.qualname == entry;
    let not_a_law =
        |def: &&analyzer_core::outline::Def| !matches!(&def.kind, DefKind::Other(k) if k == "law");
    let index = defs
        .iter()
        .position(|d| named(&d) && not_a_law(&d))
        .or_else(|| defs.iter().position(|d| named(&d)))?;
    let mut options = options.clone();
    options.limits = limits.clone();
    Some(run(
        book,
        DeclId::new(at.index(), index),
        &options,
        &Budget::default(),
        cancel,
    ))
}

/// `Session::graph`: the graph under `entry`, a def of `file` (from the root, or absolute), walked no further than `budget`. The def is named as the outline
/// names it, by qualname and which of the same-named defs it is (D20): the law is occurrence 0 and its proof occurrence 1 when the law comes first. `None`
/// when the file is not in the book, there is no such def, or `entry` is [`Entry::File`]: a Bend file has no code of its own to run when it is loaded. With
/// `Budget::default()` and the same limits it is [`call_graph`]'s graph.
pub(crate) fn graph_within(
    book: &Book,
    file: &str,
    entry: &Entry<'_>,
    options: &Options,
    budget: &Budget,
    cancel: &Cancel,
) -> Option<Graph> {
    let Entry::Def {
        qualname,
        occurrence,
    } = *entry
    else {
        return None;
    };
    let at = book.file_at(Path::new(file))?;
    let outline = &book.file(at)?.outline;
    let occurrences = bend_syntax::occurrences(outline);
    let index = outline
        .defs
        .iter()
        .enumerate()
        .position(|(i, d)| d.qualname == qualname && occurrences[i] == u32::from(occurrence))?;
    Some(run(
        book,
        DeclId::new(at.index(), index),
        options,
        budget,
        cancel,
    ))
}

/// One request for the def `id`: what both public ways in come to.
fn run(book: &Book, id: DeclId, options: &Options, budget: &Budget, cancel: &Cancel) -> Graph {
    let kind = book.def(id).map_or(DefKind::Function, |d| d.kind.clone());
    let lang = Bend::new(book, options.marker);
    let start = match kind {
        DefKind::Enum => Start::Construct {
            class: id,
            init: None,
        },
        _ => Start::Call(id, ()),
    };
    let mut solver = Solver::new(lang, options.limits.clone());
    let mut graph = solver.graph_within(&[start], budget, cancel);
    finish(&mut graph, solver.lang(), book);
    graph
}

/// What a graph has beyond its edges, in `Stats.language`: each unresolved edge's reason (a count is what the edges say), the notes the walk kept, the defs
/// named by where they are because another def of their file owns their marker, and the book's own counters (`import_missing`, `hub_import`, …).
/// Counts only. Absent when 0.
fn finish(graph: &mut Graph, lang: &Bend<'_>, book: &Book) {
    graph
        .stats
        .language
        .extend(book.stats().language.iter().map(|(k, n)| (k.clone(), *n)));
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
}

/// What one place of a body came to, for a person or a test that wants to see all of them and not only the sites (`walked`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WalkedKind {
    /// A call to a def of the book.
    Call(DeclId),
    /// A def of the book handed over (D22).
    Passed(DeclId),
    /// A `K{..}` of a type of the book.
    Construct(DeclId),
    /// A name of Base's, of a hub, or of a file outside the root, called: the name.
    External(String),
    /// A call with no target, and why (the key of the counter).
    Unresolved(String),
    /// Not a site: one more of this counter (`type_position`, `sugar_base`, `passed_deep`, `type_passed`, `external_passed`).
    Counted(String),
    /// Not a site and not counted: what Bend makes of a literal.
    Quiet,
}

/// One place of a body.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Walked {
    /// Where, in bytes of the file the def is in (a site of the graph is relative to the def's start).
    pub span: Span,
    pub kind: WalkedKind,
}

/// Every place of the body of `def`, in source order, with what it came to: the sites of the graph, and the terms the walk skipped or counted. `None`
/// when `def` is not a def of the book.
pub fn walked(book: &Book, def: DeclId, options: &Options) -> Option<Vec<Walked>> {
    book.decl(def)?;
    let lang = Bend::new(book, options.marker);
    Some(
        walk::reach(&lang, def, &Env)
            .into_iter()
            .map(|(item, reached)| {
                let kind = match reached {
                    Reached::Quiet => WalkedKind::Quiet,
                    Reached::Counted(key) => WalkedKind::Counted(key.to_owned()),
                    Reached::Site { targets, passed } => match targets.into_iter().next() {
                        Some(analyzer_core::solve::Target::Call(callee, ())) if passed => {
                            WalkedKind::Passed(callee)
                        }
                        Some(analyzer_core::solve::Target::Call(callee, ())) => {
                            WalkedKind::Call(callee)
                        }
                        Some(analyzer_core::solve::Target::Construct { class, .. }) => {
                            WalkedKind::Construct(class)
                        }
                        Some(analyzer_core::solve::Target::External(external)) => {
                            WalkedKind::External(external.name)
                        }
                        Some(analyzer_core::solve::Target::Unresolved(reason)) => {
                            WalkedKind::Unresolved(reason.as_str().to_owned())
                        }
                        _ => WalkedKind::Quiet,
                    },
                };
                Walked {
                    span: item.span,
                    kind,
                }
            })
            .collect(),
    )
}
