//! The answer every analyzer returns: contexts and call edges. A call graph is this with one context per function.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::outline::DefKind;
use crate::text::Span;

/// A call expression: a byte range **relative to the start of the calling function**, so an edit above the function
/// does not move it. Add the function's `DefInfo::range.start` to show it in an editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct Site {
    pub start: u32,
    pub end: u32,
}

impl Site {
    pub fn new(start: u32, end: u32) -> Self {
        Site {
            start,
            end: end.max(start),
        }
    }

    /// An empty site at `start`: for tests and for engines that only know where a call begins.
    pub fn at(start: u32) -> Self {
        Site { start, end: start }
    }
}

/// An identity that means the same thing in the next request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum StableDef {
    /// The def carries a marker: its text, exactly as written. Survives renames and moves.
    Marked(String),
    /// No marker: named by where it is. `module` is whatever names a file in that language; core never looks inside.
    /// `nth` tells apart two defs with one qualname in one module, in source order.
    Path {
        module: String,
        qname: String,
        nth: u16,
    },
}

impl StableDef {
    pub fn is_marked(&self) -> bool {
        matches!(self, StableDef::Marked(_))
    }
}

/// A function plus what the analysis knew about its inputs. A default resolver always writes zeros.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct ContextId {
    pub func: StableDef,
    /// A hash of the context's key, only for identity outside the process. All zeros: "the function, whatever was passed".
    pub digest: [u8; 16],
}

impl ContextId {
    /// The one context of a function under a default resolver.
    pub fn of(func: StableDef) -> Self {
        ContextId {
            func,
            digest: [0; 16],
        }
    }
}

/// The 16 bytes of a [`ContextId::digest`] for a key a language has written in its canonical form: `canonical` as compact JSON
/// (`serde_json::to_vec`), hashed with XXH3-128 (seed 0), little-endian. Bytes we wrote, never `#[derive(Hash)]`: std promises no stable
/// output, and a digest is stored outside the process.
///
/// What the canonical form holds, and the open key's zeros, are the language's rule (`docs/02-tree-rules/08-the-digest-and-scenario-levels.md`).
///
/// # Panics
///
/// When `canonical` cannot be written as JSON (a map whose keys are not strings, a `Serialize` that fails). A canonical key holds neither.
pub fn digest<T: Serialize + ?Sized>(canonical: &T) -> [u8; 16] {
    let bytes = serde_json::to_vec(canonical).expect("a canonical key always serializes");
    xxhash_rust::xxh3::xxh3_128(&bytes).to_le_bytes()
}

/// Why a walk or a graph stopped short. Every cost limit has one, and every cut is counted in [`Stats::cuts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[non_exhaustive]
pub enum Cut {
    Cancelled,
    Deadline,
    MaxContexts,
    MaxDepth,
    MaxNodes,
    /// A fixpoint over a group of recursive functions stopped at its round limit: a precision limit, not a cost one. The summary may
    /// have been refined further. Rule 10 asks for it to be marked and counted like any other.
    MaxRounds,
}

impl Cut {
    pub fn as_str(self) -> &'static str {
        match self {
            Cut::Cancelled => "cancelled",
            Cut::Deadline => "deadline",
            Cut::MaxContexts => "max_contexts",
            Cut::MaxDepth => "max_depth",
            Cut::MaxNodes => "max_nodes",
            Cut::MaxRounds => "max_rounds",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum Status {
    Finished,
    /// The body was not (fully) walked, so its edges are a prefix of the real ones.
    Incomplete(Cut),
    /// Listed and not walked, because the request's [`Budget`](crate::solve::Budget) ended here. Not a cut: nothing above it is missing, and
    /// asking again with a larger budget walks it. It has no edges, and it is counted in [`Stats::frontier`], not in [`Stats::cuts`].
    Frontier,
}

/// What a definition is. One row per definition the graph mentions, function or class: [`Graph::defs`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct DefInfo {
    pub id: StableDef,
    /// The language's qualified name, for display.
    pub qname: String,
    /// The outline's name of this def in `file`. With `file` and `occurrence` it names the def in a request (D20).
    pub qualname: String,
    /// The number of defs with the same `qualname` before this one in `file`, in source order. 0-based (D18).
    pub occurrence: u16,
    pub kind: DefKind,
    /// Path relative to the project root, with `/`.
    pub file: String,
    /// The whole definition, in bytes.
    pub range: Span,
}

impl DefInfo {
    pub fn new(
        id: StableDef,
        qname: impl Into<String>,
        qualname: impl Into<String>,
        occurrence: u16,
        kind: DefKind,
        file: impl Into<String>,
        range: Span,
    ) -> Self {
        DefInfo {
            id,
            qname: qname.into(),
            qualname: qualname.into(),
            occurrence,
            kind,
            file: file.into(),
            range,
        }
    }
}

/// One analysis of one function body. What the function is lives in [`Graph::defs`], under `id.func`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Context {
    pub id: ContextId,
    pub status: Status,
}

impl Context {
    /// A context whose body was walked to the end.
    pub fn new(id: ContextId) -> Self {
        Context {
            id,
            status: Status::Finished,
        }
    }
}

/// A library or the standard library: a named leaf, never entered.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct External {
    pub package: String,
    pub name: String,
}

/// What a resolved edge rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Basis {
    /// Followed along a path the analyzer tracked: an object built in this walk, its attributes. Only Python builds it today.
    Path,
    /// The engine's static type answered, and the answer depends on a type: a method found through the class of its receiver (a
    /// subclass may override it), a callee the engine typed but the code did not name.
    Type,
    /// The code's own names fixed it: a function or class the callee expression names (a def, an import, a closure, `Foo()`), with no
    /// receiver's class in between. The one basis a reader may take as certain, short of rebinding the name.
    Name,
    /// The engine named this def, and said it could not see everything that may be in front of it (an intersection with an unknown type):
    /// a method found after a base class that is not resolvable. The def is the engine's; that it is the one called is likely, not certain
    /// (D28). Written last, so the derived order of the three others does not move.
    Likely,
}

/// Why an edge has no target. The same string as the `Stats.language` counter key, so a count is what the edges say. An owned
/// string, not `&'static str`: a graph read back from disk could not be deserialized into a borrowed one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Reason(String);

impl Reason {
    pub fn new(key: &str) -> Self {
        Reason(key.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[non_exhaustive]
pub enum EdgeTo {
    /// A call into project code.
    Context(ContextId, Basis),
    /// `Foo()`, `new Foo()`, `Foo { .. }`; `init` is the constructor function when the code has one.
    Construct {
        class: StableDef,
        init: Option<ContextId>,
        basis: Basis,
    },
    External(External, Basis),
    /// There was a call; the target is not known. Reported, never guessed and never dropped. It says why.
    Unresolved(Reason),
}

impl EdgeTo {
    /// What this edge rests on; `None` for an edge with no target, which has a reason instead.
    pub fn basis(&self) -> Option<Basis> {
        match self {
            EdgeTo::Context(_, b) | EdgeTo::External(_, b) | EdgeTo::Construct { basis: b, .. } => {
                Some(*b)
            }
            EdgeTo::Unresolved(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[non_exhaustive]
pub struct Edge {
    pub from: ContextId,
    pub site: Site,
    pub to: EdgeTo,
    /// `true`: `from` hands the target over at `site` and does not call it there (`onClick={handleClose}`, `xs.map(parse)`; D22). The target is
    /// walked like any other, so what it calls is in the graph, but this edge is not a call and is never counted as one. Only an edge to a
    /// [`EdgeTo::Context`] can be passed (`check_invariants` says so). Last, so the order of edges that are not passed is what it was.
    pub passed: bool,
}

impl Edge {
    /// An edge for a call: what every edge was before D22.
    pub fn new(from: ContextId, site: Site, to: EdgeTo) -> Self {
        Edge {
            from,
            site,
            to,
            passed: false,
        }
    }

    /// An edge for a def handed over, not called.
    pub fn passed(from: ContextId, site: Site, to: EdgeTo) -> Self {
        Edge {
            from,
            site,
            to,
            passed: true,
        }
    }
}

/// What every language can count. A missing edge should be explainable from a number here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Stats {
    /// Bodies walked.
    pub walks: u64,
    pub contexts: u64,
    /// Summaries asked for that were already known.
    pub reuse_hits: u64,
    pub cuts: BTreeMap<Cut, u64>,
    /// Contexts listed and not walked because the request's budget ended there ([`Status::Frontier`]). Not a cut, so not in `cuts`.
    pub frontier: u64,
    /// Defs that share an id with an earlier one, dropped by [`Graph::new`], so their calls are drawn under the first (in practice two defs carrying one
    /// marker in two files, D34). A row given twice (same id, file and start) is one def and is not counted; a def given twice counts once.
    pub merged_defs: u64,
    pub unresolved: u64,
    /// A language's own counters.
    pub language: BTreeMap<String, u64>,
}

impl Stats {
    pub fn cut(&mut self, why: Cut) {
        *self.cuts.entry(why).or_default() += 1;
    }
}

/// What a request starts from: a def of one file, or the file's own code. A language answers the kinds it has and says `None` for the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Entry<'a> {
    /// A def of one file, as the outline names it: its qualname and which of the same-named defs it is (0-based, in source order). A request
    /// names its def this way (D20).
    Def { qualname: &'a str, occurrence: u16 },
    /// The file's own code: what runs when the file is loaded (D32). A language that has no such code answers `None`.
    File,
}

/// What a tree starts from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum Root {
    /// A function: the tree is its calls.
    Context(ContextId),
    /// A class: the tree is its constructor's calls. `init` is `None` when the class has no constructor function in project code. The same two
    /// fields as [`EdgeTo::Construct`] without its `basis`: it is the same thing, without a caller.
    Construct {
        class: StableDef,
        init: Option<ContextId>,
    },
}

impl Root {
    /// The def the root node is named by: the function, or the class.
    pub fn func(&self) -> &StableDef {
        match self {
            Root::Context(id) => &id.func,
            Root::Construct { class, .. } => class,
        }
    }

    /// The context the root's calls were made in: the function's own, or the constructor's. `None` for a class with no constructor function.
    pub fn context(&self) -> Option<&ContextId> {
        match self {
            Root::Context(id) => Some(id),
            Root::Construct { init, .. } => init.as_ref(),
        }
    }
}

/// Everything reachable from the entries. Built by [`Graph::new`], which sorts: the same input gives the same output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Graph {
    pub entries: Vec<Root>,
    /// One row per function or class a context or a `Construct` edge names. Sorted by id.
    pub defs: Vec<DefInfo>,
    /// Sorted by id.
    pub contexts: Vec<Context>,
    /// Sorted by `(from, site, to)`.
    pub edges: Vec<Edge>,
    pub stats: Stats,
}

impl Graph {
    /// Sorts defs, contexts and edges and drops duplicates. Entries keep the caller's order (it is the request's order).
    ///
    /// Two defs with one id (a copied function carries the same marker) keep the first by `(file, range.start)`, so the
    /// graph does not depend on the order the rows arrived in. Each def dropped that is **another def** (another file or another start) is counted
    /// in `stats.merged_defs`.
    pub fn new(
        entries: Vec<Root>,
        mut defs: Vec<DefInfo>,
        mut contexts: Vec<Context>,
        mut edges: Vec<Edge>,
        stats: Stats,
    ) -> Self {
        let mut stats = stats;
        defs.sort_by(|a, b| (&a.id, &a.file, a.range.start).cmp(&(&b.id, &b.file, b.range.start)));
        // a dropped row is another def when its file or start differs from the row kept (and from the dropped row before it: the same def given twice)
        let mut kept: Option<&DefInfo> = None;
        let mut last: Option<&DefInfo> = None;
        for row in &defs {
            let same_def =
                |other: &DefInfo| other.file == row.file && other.range.start == row.range.start;
            match kept {
                Some(first) if first.id == row.id => {
                    if !same_def(first)
                        && !last.is_some_and(|before| before.id == row.id && same_def(before))
                    {
                        stats.merged_defs += 1;
                    }
                    last = Some(row);
                }
                _ => {
                    kept = Some(row);
                    last = Some(row);
                }
            }
        }
        defs.dedup_by(|later, first| later.id == first.id);
        contexts.sort_by(|a, b| a.id.cmp(&b.id));
        contexts.dedup_by(|a, b| a.id == b.id);
        edges.sort();
        edges.dedup();
        Graph {
            entries,
            defs,
            contexts,
            edges,
            stats,
        }
    }

    pub fn context(&self, id: &ContextId) -> Option<&Context> {
        self.contexts
            .binary_search_by(|c| c.id.cmp(id))
            .ok()
            .map(|i| &self.contexts[i])
    }

    pub fn def(&self, id: &StableDef) -> Option<&DefInfo> {
        self.defs
            .binary_search_by(|d| d.id.cmp(id))
            .ok()
            .map(|i| &self.defs[i])
    }

    /// The edges leaving `id`, in site order.
    pub fn edges_from(&self, id: &ContextId) -> &[Edge] {
        let start = self.edges.partition_point(|e| e.from < *id);
        let len = self.edges[start..].partition_point(|e| e.from == *id);
        &self.edges[start..start + len]
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn marked(name: &str) -> StableDef {
        StableDef::Marked(name.to_owned())
    }

    pub fn cid(name: &str) -> ContextId {
        ContextId::of(marked(name))
    }

    pub fn ctx(name: &str) -> Context {
        Context::new(cid(name))
    }

    pub fn def(name: &str) -> DefInfo {
        DefInfo::new(
            marked(name),
            name,
            name,
            0,
            DefKind::Function,
            "m.py",
            Span::new(0, 10),
        )
    }

    #[test]
    fn new_sorts_and_dedups() {
        let edge = |from: &str, at: u32, to: &str| {
            Edge::new(
                cid(from),
                Site::at(at),
                EdgeTo::Context(cid(to), Basis::Type),
            )
        };
        let g = Graph::new(
            vec![Root::Context(cid("b"))],
            vec![def("b"), def("a"), def("b")],
            vec![ctx("b"), ctx("a"), ctx("b")],
            vec![
                edge("b", 5, "a"),
                edge("a", 9, "b"),
                edge("a", 1, "b"),
                edge("a", 1, "b"),
            ],
            Stats::default(),
        );
        assert_eq!(
            g.contexts.iter().map(|c| &c.id.func).collect::<Vec<_>>(),
            [&marked("a"), &marked("b")]
        );
        assert_eq!(
            g.defs.iter().map(|d| d.qname.as_str()).collect::<Vec<_>>(),
            ["a", "b"],
            "one row per id, sorted"
        );
        assert_eq!(g.edges.len(), 3);
        assert_eq!(
            g.edges_from(&cid("a"))
                .iter()
                .map(|e| e.site.start)
                .collect::<Vec<_>>(),
            [1, 9]
        );
        assert_eq!(g.edges_from(&cid("b")).len(), 1);
        assert!(g.edges_from(&cid("zzz")).is_empty());
    }

    fn merged(rows: Vec<DefInfo>) -> u64 {
        Graph::new(vec![], rows, vec![], vec![], Stats::default())
            .stats
            .merged_defs
    }

    fn row_at(name: &str, file: &str, start: u32) -> DefInfo {
        DefInfo::new(
            marked(name),
            name,
            name,
            0,
            DefKind::Function,
            file,
            Span::new(start, start + 10),
        )
    }

    #[test]
    fn two_rows_with_one_id_in_two_files_are_one_merged_def() {
        assert_eq!(
            merged(vec![row_at("a", "x.ts", 0), row_at("a", "y.ts", 0)]),
            1
        );
    }

    #[test]
    fn two_rows_with_one_id_in_one_file_at_two_starts_are_one_merged_def() {
        assert_eq!(
            merged(vec![row_at("a", "x.ts", 0), row_at("a", "x.ts", 40)]),
            1
        );
    }

    #[test]
    fn the_same_row_given_twice_is_not_a_merged_def() {
        assert_eq!(
            merged(vec![row_at("a", "x.ts", 0), row_at("a", "x.ts", 0)]),
            0
        );
        // a def given twice beside another def counts once
        assert_eq!(
            merged(vec![
                row_at("a", "x.ts", 0),
                row_at("a", "y.ts", 0),
                row_at("a", "y.ts", 0),
            ]),
            1
        );
    }

    #[test]
    fn no_duplicate_merges_nothing_and_a_count_already_there_is_kept() {
        assert_eq!(
            merged(vec![row_at("a", "x.ts", 0), row_at("b", "x.ts", 0)]),
            0
        );
        let before = Stats {
            merged_defs: 2,
            ..Stats::default()
        };
        let graph = Graph::new(vec![], vec![row_at("a", "x.ts", 0)], vec![], vec![], before);
        assert_eq!(graph.stats.merged_defs, 2);
    }

    #[test]
    fn context_lookup() {
        let g = Graph::new(
            vec![],
            vec![],
            vec![ctx("b"), ctx("a")],
            vec![],
            Stats::default(),
        );
        assert_eq!(g.context(&cid("a")).unwrap().id, cid("a"));
        assert!(g.context(&cid("c")).is_none());
    }

    #[test]
    fn def_lookup() {
        let g = Graph::new(
            vec![],
            vec![def("b"), def("a")],
            vec![],
            vec![],
            Stats::default(),
        );
        assert_eq!(g.def(&marked("a")).unwrap().qname, "a");
        assert_eq!(g.def(&marked("b")).unwrap().qname, "b");
        assert!(g.def(&marked("c")).is_none());
    }

    #[test]
    fn two_rows_for_one_id_keep_the_first_by_position() {
        // a copied function carries the same marker: the row that wins must not depend on the order they arrive in
        let at = |file: &str, start: u32| {
            DefInfo::new(
                marked("f"),
                "f",
                "f",
                0,
                DefKind::Function,
                file,
                Span::new(start, start + 5),
            )
        };
        let rows = [at("b.py", 0), at("a.py", 90), at("a.py", 10)];
        let forward = Graph::new(vec![], rows.to_vec(), vec![], vec![], Stats::default());
        let mut reversed_rows = rows.to_vec();
        reversed_rows.reverse();
        let backward = Graph::new(vec![], reversed_rows, vec![], vec![], Stats::default());
        assert_eq!(forward, backward);
        assert_eq!(forward.defs.len(), 1);
        assert_eq!(
            (forward.defs[0].file.as_str(), forward.defs[0].range.start),
            ("a.py", 10)
        );
    }

    #[test]
    fn every_cut_has_its_own_stable_name() {
        let all = [
            Cut::Cancelled,
            Cut::Deadline,
            Cut::MaxContexts,
            Cut::MaxDepth,
            Cut::MaxNodes,
            Cut::MaxRounds,
        ];
        let names: Vec<&str> = all.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            names,
            [
                "cancelled",
                "deadline",
                "max_contexts",
                "max_depth",
                "max_nodes",
                "max_rounds"
            ]
        );
    }

    #[test]
    fn digest_distinguishes_contexts_of_one_function() {
        let a = ContextId::of(marked("f"));
        let b = ContextId {
            func: marked("f"),
            digest: [1; 16],
        };
        assert_ne!(a, b);
        assert!(a < b);
    }

    #[test]
    fn the_files_own_code_is_not_a_def() {
        let def = Entry::Def {
            qualname: "main",
            occurrence: 0,
        };
        assert_ne!(Entry::File, def);
        assert_eq!(Entry::File, Entry::File);
        assert_ne!(
            def,
            Entry::Def {
                qualname: "main",
                occurrence: 1
            }
        );
    }

    #[derive(Serialize)]
    enum Canon {
        Unknown,
        Fn(StableDef),
        Set { known: Vec<Canon>, open: bool },
    }

    #[derive(Serialize)]
    struct Key {
        params: Vec<Canon>,
    }

    #[derive(Serialize)]
    struct Grown {
        params: Vec<Canon>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        captured: Vec<Canon>,
    }

    fn params() -> Vec<Canon> {
        vec![
            Canon::Fn(StableDef::Path {
                module: "src/a.ts".into(),
                qname: "handler".into(),
                nth: 0,
            }),
            Canon::Unknown,
            Canon::Set {
                known: vec![Canon::Fn(StableDef::Marked("abc".into()))],
                open: true,
            },
        ]
    }

    /// A change here changes every context id a consumer stored.
    #[test]
    fn the_digest_is_xxh3_128_of_the_json_little_endian() {
        assert_eq!(
            serde_json::to_string(&Key { params: params() }).unwrap(),
            concat!(
                r#"{"params":[{"Fn":{"Path":{"module":"src/a.ts","qname":"handler","nth":0}}},"#,
                r#""Unknown",{"Set":{"known":[{"Fn":{"Marked":"abc"}}],"open":true}}]}"#
            )
        );
        assert_eq!(
            digest(&Key { params: params() }),
            [
                222, 81, 217, 167, 216, 56, 119, 221, 205, 106, 67, 102, 68, 16, 248, 74
            ]
        );
        assert_eq!(
            digest(&Key { params: vec![] }),
            [
                122, 58, 146, 60, 100, 231, 89, 134, 25, 59, 41, 249, 84, 241, 149, 151
            ]
        );
        // the algorithm is the specified one: the published XXH3-128 vector for empty input
        assert_eq!(
            xxhash_rust::xxh3::xxh3_128(b""),
            0x99aa_06d3_0147_98d8_6001_c324_468d_497f_u128
        );
    }

    /// The growth rule: a field a later phase adds is skipped when empty, so a key that does not use it keeps its digest.
    #[test]
    fn a_field_skipped_when_empty_keeps_the_digest() {
        assert_eq!(
            digest(&Grown {
                params: params(),
                captured: vec![]
            }),
            digest(&Key { params: params() })
        );
        assert_eq!(
            digest(&Grown {
                params: params(),
                captured: vec![Canon::Unknown]
            }),
            [
                197, 253, 104, 34, 64, 12, 233, 198, 225, 138, 19, 150, 9, 170, 124, 236
            ]
        );
    }
}
