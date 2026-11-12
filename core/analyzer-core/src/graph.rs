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
}

/// What a definition is. One row per definition the graph mentions, function or class: [`Graph::defs`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct DefInfo {
    pub id: StableDef,
    /// The language's qualified name, for display.
    pub qname: String,
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
        kind: DefKind,
        file: impl Into<String>,
        range: Span,
    ) -> Self {
        DefInfo {
            id,
            qname: qname.into(),
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
pub struct Edge {
    pub from: ContextId,
    pub site: Site,
    pub to: EdgeTo,
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
    pub unresolved: u64,
    /// A language's own counters.
    pub language: BTreeMap<String, u64>,
}

impl Stats {
    pub fn cut(&mut self, why: Cut) {
        *self.cuts.entry(why).or_default() += 1;
    }
}

/// Everything reachable from the entries. Built by [`Graph::new`], which sorts: the same input gives the same output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Graph {
    pub entries: Vec<ContextId>,
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
    /// graph does not depend on the order the rows arrived in.
    pub fn new(
        entries: Vec<ContextId>,
        mut defs: Vec<DefInfo>,
        mut contexts: Vec<Context>,
        mut edges: Vec<Edge>,
        stats: Stats,
    ) -> Self {
        defs.sort_by(|a, b| (&a.id, &a.file, a.range.start).cmp(&(&b.id, &b.file, b.range.start)));
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
            DefKind::Function,
            "m.py",
            Span::new(0, 10),
        )
    }

    #[test]
    fn new_sorts_and_dedups() {
        let edge = |from: &str, at: u32, to: &str| Edge {
            from: cid(from),
            site: Site::at(at),
            to: EdgeTo::Context(cid(to), Basis::Type),
        };
        let g = Graph::new(
            vec![cid("b")],
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
}
