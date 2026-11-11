//! The answer every analyzer returns: contexts and call edges. A call graph is this with one context per function.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::outline::DefKind;
use crate::text::Span;

/// A call expression: a byte range **relative to the start of the calling function**, so an edit above the function
/// does not move it. Add `Context::range.start` to show it in an editor.
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
}

impl Cut {
    pub fn as_str(self) -> &'static str {
        match self {
            Cut::Cancelled => "cancelled",
            Cut::Deadline => "deadline",
            Cut::MaxContexts => "max_contexts",
            Cut::MaxDepth => "max_depth",
            Cut::MaxNodes => "max_nodes",
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Context {
    pub id: ContextId,
    /// The language's qualified name, for display.
    pub qname: String,
    pub kind: DefKind,
    /// Path relative to the project root, with `/`.
    pub file: String,
    /// The whole definition, in bytes.
    pub range: Span,
    pub status: Status,
}

impl Context {
    pub fn new(
        id: ContextId,
        qname: impl Into<String>,
        kind: DefKind,
        file: impl Into<String>,
        range: Span,
    ) -> Self {
        Context {
            id,
            qname: qname.into(),
            kind,
            file: file.into(),
            range,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[non_exhaustive]
pub enum EdgeTo {
    /// A call into project code.
    Context(ContextId),
    /// `Foo()`, `new Foo()`, `Foo { .. }`; `init` is the constructor function when the code has one.
    Construct {
        class: StableDef,
        init: Option<ContextId>,
    },
    External(External),
    /// There was a call; the target is not known. Reported, never guessed and never dropped.
    Unresolved,
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
    /// Sorted by id.
    pub contexts: Vec<Context>,
    /// Sorted by `(from, site, to)`.
    pub edges: Vec<Edge>,
    pub stats: Stats,
}

impl Graph {
    /// Sorts contexts and edges and drops exact duplicates. Entries keep the caller's order (it is the request's order).
    pub fn new(
        entries: Vec<ContextId>,
        mut contexts: Vec<Context>,
        mut edges: Vec<Edge>,
        stats: Stats,
    ) -> Self {
        contexts.sort_by(|a, b| a.id.cmp(&b.id));
        contexts.dedup_by(|a, b| a.id == b.id);
        edges.sort();
        edges.dedup();
        Graph {
            entries,
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
        Context::new(cid(name), name, DefKind::Function, "m.py", Span::new(0, 10))
    }

    #[test]
    fn new_sorts_and_dedups() {
        let edge = |from: &str, at: u32, to: &str| Edge {
            from: cid(from),
            site: Site::at(at),
            to: EdgeTo::Context(cid(to)),
        };
        let g = Graph::new(
            vec![cid("b")],
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
            g.contexts
                .iter()
                .map(|c| c.qname.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
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
        let g = Graph::new(vec![], vec![ctx("b"), ctx("a")], vec![], Stats::default());
        assert_eq!(g.context(&cid("a")).unwrap().qname, "a");
        assert!(g.context(&cid("c")).is_none());
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
