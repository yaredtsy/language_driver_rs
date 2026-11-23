//! The call graph: `Bend` is the language the core's flat solver walks, and `call_graph` is the one public function that runs it.
//!
//! One context per def under `Precision::Default` (every key is the open one, digest zeros); under `Precision::Precise` a context is a def and the parameters a caller named
//! (`docs/08-bend/06-context-layer-design.md`). A target is `Target::Call(def, key)`, and `walk` is the only place that takes a key.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::{Basis, ContextId, DefInfo, EdgeTo, Entry, Graph, StableDef};
use analyzer_core::outline::DefKind;
use analyzer_core::solve::{Budget, Cancel, Lang, Limits, Solver, Start, Summary};
use analyzer_core::text::Span;

use crate::book::{Book, DeclId};
use crate::env::Env;
use crate::eval::{self, Policy};
use crate::key::{self, CanonKey, Key};
use crate::probe::{self, Count, Section};
use crate::sem::{self, Item};
use crate::value::Value;
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

/// For a parameter a callee calls or passes along, in a context being walked: the argument is the caller's own parameter and the caller's key does not know it,
/// is a `let`, a `case` variable, a lambda's binder or a do's name, is a call's result, a constructor, a literal or a Base def, or is not there (partial
/// application). Counted per context: they are about keys (`docs/08-bend/06-context-layer-design.md` §4).
pub(crate) const KEY_SLOT_OPEN: &str = "key_slot_open";
pub(crate) const KEY_SLOT_LOCAL: &str = "key_slot_local";
pub(crate) const KEY_SLOT_VALUE: &str = "key_slot_value";
pub(crate) const KEY_SLOT_MISSING: &str = "key_slot_missing";

/// A set of values with more than `value::MAX_SET` members was met in a context: it is unknown there. A precision limit, counted per context walked.
pub(crate) const VALUE_WIDENED_SET: &str = "value_widened_set";

/// A value nested deeper than `value::MAX_DEPTH` inside a value was met in a context: it is unknown there. A precision limit, counted per context walked.
pub(crate) const VALUE_WIDENED_DEPTH: &str = "value_widened_depth";

/// A def whose return value depends on itself (directly or through others) did not settle in `eval::MAX_ROUNDS` rounds: its value is unknown. Counted once for the context.
pub(crate) const RETURN_IN_CYCLE: &str = "return_in_cycle";

/// The reasons of the `Unresolved` edges; the same strings are their counters.
pub(crate) const FUNCTION_TYPED_VALUE: &str = "function_typed_value";
/// A call through a parameter that is known to be a lambda: its calls are drawn where the lambda is written (BC-D6).
pub(crate) const ANONYMOUS_FUNCTION_CALLED: &str = "anonymous_function_called";
pub(crate) const UNKNOWN_NAME: &str = "unknown_name";
pub(crate) const UNKNOWN_CONSTRUCTOR: &str = "unknown_constructor";

/// A request that continues a context (`Session::graph_from`) named a digest the session does not have, or a def of its key that is gone: it starts from the open key.
pub(crate) const CONTEXT_NOT_FOUND: &str = "context_not_found";

/// Each digest met in a request and the canonical key it stands for: what a session keeps to continue a context later (BC-D7). Only looked up, never iterated.
pub(crate) type Keys = HashMap<[u8; 16], CanonKey>;

/// How exactly a call is answered. `Precise` is the default (D53), and `Default` stays selectable for good: it is the comparison every measurement is made against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Precision {
    /// Layer 1: one context per def, a call through a parameter is `Unresolved(function_typed_value)`.
    Default,
    /// A def's parameter that a caller named (a `~` template or a plain one) is called as that def, per caller (`docs/08-bend/06-context-layer-design.md`).
    #[default]
    Precise,
}

/// How to build a graph. The marker style is how a def's marker is read (`# ID: …` in the comment block above it); the limits protect the cost of a request
/// and cut (a cut is counted in `Stats.cuts`).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Options {
    pub limits: Limits,
    pub marker: MarkerStyle<'static>,
    pub precision: Precision,
}

impl Options {
    /// The default cost limits, the context layer on (`Precision::Precise`, D53), and `marker` for reading markers.
    pub fn new(marker: MarkerStyle<'static>) -> Self {
        Options {
            limits: Limits::default(),
            marker,
            precision: Precision::Precise,
        }
    }

    /// The same options at `precision`.
    pub fn with_precision(mut self, precision: Precision) -> Self {
        self.precision = precision;
        self
    }
}

/// What a body read for its value would have counted, kept to be counted when the solver walks the context (V10b): `note` and `note_slot`, as they were called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Note(&'static str, u64),
    Slot(&'static str),
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
    pub(crate) precision: Precision,
    /// What the evaluator may know: everything the layer tracks (the test that it agrees with the item walk takes it all away).
    pub(crate) policy: Policy,
    /// Only ever looked up, never iterated (rule 9).
    /// Counters that are not the reason of an edge, filled as bodies are walked, once each, and put in `Stats.language` by `finish`.
    notes: RefCell<BTreeMap<&'static str, u64>>,
    /// The defs whose body has been walked to the end: what a body counts about itself (a type skipped, Base's sugar, a name handed over below the arguments, a call
    /// into a file outside the root, a call to a law) it counts the first time and not again under another key. Only ever looked up (rule 9).
    noted: RefCell<HashSet<DeclId>>,
    /// While a def that was walked before is walked again, under another key: `note` counts nothing, `note_slot` still does (a key is what differs).
    quiet: Cell<bool>,
    /// The ids of the defs described that have a marker another def of their file owns (D34). Described more than once, so a set.
    demoted: RefCell<HashSet<StableDef>>,
    /// The digest of each key met. Only ever looked up, never iterated (rule 9).
    digests: RefCell<HashMap<Key, [u8; 16]>>,
    /// The value each context returns (`eval::ret`): kept in Bend beside the sites. Only ever looked up, never iterated (rule 9).
    pub(crate) rets: RefCell<HashMap<(DeclId, Key), Value>>,
    /// The contexts whose value is being worked out now, if a request of `eval::ret` is under way.
    pub(crate) run: RefCell<Option<eval::Run<'b>>>,
    /// The canonical key of each digest this request made.
    canons: RefCell<Keys>,
    /// While a body is read only for its value and its sites are kept: what it would have counted, in the order it did.
    log: RefCell<Option<Vec<Event>>>,
    /// The sites and the counts of the last evaluation of each context the worklist worked out, final when its request ended (V10b). Only ever looked up, never iterated
    /// (rule 9).
    pub(crate) kept: RefCell<HashMap<(DeclId, Key), eval::Kept<'b>>>,
}

impl<'b> Bend<'b> {
    pub(crate) fn new(book: &'b Book, style: MarkerStyle<'static>, precision: Precision) -> Self {
        Bend {
            book,
            style,
            precision,
            policy: Policy::Tracked,
            notes: RefCell::new(BTreeMap::new()),
            noted: RefCell::new(HashSet::new()),
            quiet: Cell::new(false),
            demoted: RefCell::new(HashSet::new()),
            digests: RefCell::new(HashMap::new()),
            rets: RefCell::new(HashMap::new()),
            run: RefCell::new(None),
            canons: RefCell::new(HashMap::new()),
            log: RefCell::new(None),
            kept: RefCell::new(HashMap::new()),
        }
    }

    /// A counter about the code of a def: counted once per def, the first time it is walked (a second context of it counts nothing).
    pub(crate) fn note(&self, key: &'static str, by: u64) {
        if by == 0 {
            return;
        }
        if let Some(log) = self.log.borrow_mut().as_mut() {
            log.push(Event::Note(key, by));
            return;
        }
        if !self.quiet.get() {
            *self.notes.borrow_mut().entry(key).or_default() += by;
        }
    }

    /// A counter about a key: counted in every context that is walked.
    pub(crate) fn note_slot(&self, key: &'static str) {
        if let Some(log) = self.log.borrow_mut().as_mut() {
            log.push(Event::Slot(key));
            return;
        }
        *self.notes.borrow_mut().entry(key).or_default() += 1;
    }

    /// A counter about something that is decided once and kept (a value that did not settle): counted when it is decided, not when a context that read it is walked.
    pub(crate) fn note_once(&self, key: &'static str) {
        *self.notes.borrow_mut().entry(key).or_default() += 1;
    }

    /// Reads a body for its value and for its sites at once: what it counts is not counted but returned, to be counted when the solver walks the context (`replay`).
    pub(crate) fn recording<T>(&self, read: impl FnOnce() -> T) -> (T, Vec<Event>) {
        let before = self.log.replace(Some(Vec::new()));
        let out = read();
        let events = self.log.replace(before).unwrap_or_default();
        (out, events)
    }

    /// Counts what a body that was read for its value would have counted, now: the context is being walked.
    pub(crate) fn replay(&self, events: &[Event]) {
        for event in events {
            match *event {
                Event::Note(key, by) => self.note(key, by),
                Event::Slot(key) => self.note_slot(key),
            }
        }
    }

    /// Whether the body of `f` was already walked to the end; the notes of `f`'s code are quiet from then on.
    pub(crate) fn walked_before(&self, f: DeclId) -> bool {
        self.noted.borrow().contains(&f)
    }

    pub(crate) fn mute(&self, muted: bool) {
        self.quiet.set(muted);
    }

    pub(crate) fn done(&self, f: DeclId) {
        self.noted.borrow_mut().insert(f);
    }

    /// The walk of `f`'s body: its places and the same body as a tree (the book reads each file once).
    pub(crate) fn walk_of(&self, f: DeclId) -> Option<&'b sem::DefWalk> {
        let def = self.book.def(f)?;
        sem::walk_of(self.book.walks(f.file()), def.name_range)
    }

    /// The places of `f`'s body (the book reads each file once).
    pub(crate) fn items(&self, f: DeclId) -> Vec<Item> {
        self.walk_of(f).map_or_else(Vec::new, |w| w.items.clone())
    }
}

impl Bend<'_> {
    /// A def's identity outside this request, and which of the defs of its name in its file it is. A marker another def of the file owns is not this def's id
    /// (D34): it is named by where it is, as a def with no marker is.
    fn identity(&self, f: DeclId) -> (StableDef, u16) {
        let (Some(file), Some(def)) = (self.book.file(f.file()), self.book.def(f)) else {
            unreachable!("a DeclId of the book the solver was given");
        };
        let marked = self.book.marked(f.file(), self.style);
        let occurrence = marked.as_ref().map_or(0, |m| m.occurrences[f.def()]) as u16;
        let marker = marked.as_ref().and_then(|m| m.markers[f.def()].clone());
        let owns = marked.as_ref().is_some_and(|m| m.owns[f.def()]);
        let by_place = || StableDef::Path {
            module: file.relative.clone(),
            qname: def.qualname.clone(),
            nth: occurrence,
        };
        let id = match marker {
            Some(marker) if owns => StableDef::Marked(marker),
            Some(_) => {
                let id = by_place();
                self.demoted.borrow_mut().insert(id.clone());
                id
            }
            None => by_place(),
        };
        (id, occurrence)
    }

    /// The id a def has outside this request (what a key is written with, `key::canon`).
    pub(crate) fn stable_def(&self, f: DeclId) -> StableDef {
        self.identity(f).0
    }

    /// The def of the book that has the id `id`, found as an entry is: by marker (the def of its file that owns it, D34), or by file, qualname and which of that
    /// name it is. `None` when no def has it: it was removed or renamed since the id was made.
    pub(crate) fn decl_of(&self, id: &StableDef) -> Option<DeclId> {
        self.book.decls().iter().map(|d| d.id).find(|&f| match id {
            StableDef::Marked(marker) => self
                .book
                .marked(f.file(), self.style)
                .is_some_and(|m| m.owns[f.def()] && m.markers[f.def()].as_deref() == Some(marker)),
            StableDef::Path { module, qname, .. } => {
                self.book.def(f).is_some_and(|d| &d.qualname == qname)
                    && self
                        .book
                        .file(f.file())
                        .is_some_and(|file| &file.relative == module)
                    && self.stable_def(f) == *id
            }
        })
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
        let (id, occurrence) = self.identity(f);
        DefInfo::new(
            id,
            def.qualname.clone(),
            def.qualname.clone(),
            occurrence,
            def.kind.clone(),
            file.relative.clone(),
            def.range,
        )
    }

    fn digest(&self, key: &Key) -> [u8; 16] {
        if key.is_open() {
            return [0; 16];
        }
        let _time = probe::section(Section::Keys);
        probe::count(Count::DigestAsked);
        if let Some(digest) = self.digests.borrow().get(key) {
            return *digest;
        }
        probe::count(Count::DigestMade);
        let digest = key::digest(self, key);
        self.digests.borrow_mut().insert(key.clone(), digest);
        self.canons
            .borrow_mut()
            .insert(digest, key::canon(self, key));
        digest
    }

    fn walk(&mut self, f: DeclId, key: &Key, cancel: &Cancel) -> Summary<Self> {
        probe::site_walk(f, key);
        let _time = probe::section(Section::Sites);
        walk::body(self, f, &Env::new(key), cancel)
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
    let lang = Bend::new(book, options.marker, options.precision);
    Some(
        solve(
            lang,
            DeclId::new(at.index(), index),
            Key::open(),
            &options,
            &Budget::default(),
            cancel,
        )
        .0,
    )
}

/// `Session::graph`: the graph under `entry`, a def of `file` (from the root, or absolute), walked no further than `budget`, and the canonical key of every context
/// it listed. The def is named as the outline names it, by qualname and which of the same-named defs it is (D20): the law is occurrence 0 and its proof occurrence 1
/// when the law comes first. `None` when the file is not in the book, there is no such def, or `entry` is [`Entry::File`]: a Bend file has no code of its own to run
/// when it is loaded. With `Budget::default()` and the same limits it is [`call_graph`]'s graph.
pub(crate) fn graph_within(
    book: &Book,
    file: &str,
    entry: &Entry<'_>,
    options: &Options,
    budget: &Budget,
    cancel: &Cancel,
) -> Option<(Graph, Keys)> {
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
    let lang = Bend::new(book, options.marker, options.precision);
    Some(solve(
        lang,
        DeclId::new(at.index(), index),
        Key::open(),
        options,
        budget,
        cancel,
    ))
}

/// `Session::graph_from`: the graph that starts at the context `from` (BC-D7), so that a node a request cut can be continued under the key it had. The def is the
/// one `from.func` names; the key is the one `known` (the canonical key the session kept for `from.digest`) stands for. The open key when the digest is zeros; the
/// open key and `context_not_found` counted when the digest is not known or a def of the key is gone. `None` when no def of the book has the id.
pub(crate) fn continue_from(
    book: &Book,
    from: &ContextId,
    known: Option<&CanonKey>,
    options: &Options,
    budget: &Budget,
    cancel: &Cancel,
) -> Option<(Graph, Keys)> {
    let lang = Bend::new(book, options.marker, options.precision);
    let id = lang.decl_of(&from.func)?;
    let (key, found) = if from.digest == [0; 16] {
        (Key::open(), true)
    } else {
        match known.and_then(|canon| key::rebuild(&lang, canon)) {
            Some(key) => (key, true),
            None => (Key::open(), false),
        }
    };
    let (mut graph, keys) = solve(lang, id, key, options, budget, cancel);
    if !found {
        graph.stats.language.insert(CONTEXT_NOT_FOUND.to_owned(), 1);
    }
    Some((graph, keys))
}

/// One request for the def `id` under `key`: what every public way in comes to.
fn solve(
    lang: Bend<'_>,
    id: DeclId,
    key: Key,
    options: &Options,
    budget: &Budget,
    cancel: &Cancel,
) -> (Graph, Keys) {
    let book = lang.book;
    let kind = book.def(id).map_or(DefKind::Function, |d| d.kind.clone());
    let start = match kind {
        DefKind::Enum => Start::Construct {
            class: id,
            init: None,
        },
        _ => Start::Call(id, key),
    };
    let mut solver = Solver::new(lang, options.limits.clone());
    let mut graph = solver.graph_within(&[start], budget, cancel);
    finish(&mut graph, solver.lang(), book);
    let keys = std::mem::take(&mut *solver.lang().canons.borrow_mut());
    (graph, keys)
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
    let lang = Bend::new(book, options.marker, options.precision);
    let open = Key::open();
    Some(
        walk::reach(&lang, def, &Env::new(&open))
            .into_iter()
            .map(|(item, reached)| {
                let kind = match reached {
                    Reached::Quiet => WalkedKind::Quiet,
                    Reached::Counted(key) => WalkedKind::Counted(key.to_owned()),
                    Reached::Site { targets, passed } => match targets.into_iter().next() {
                        Some(analyzer_core::solve::Target::Call(callee, _)) if passed => {
                            WalkedKind::Passed(callee)
                        }
                        Some(analyzer_core::solve::Target::Call(callee, _)) => {
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
