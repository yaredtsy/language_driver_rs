// src/interp/graph.rs
use rustc_hash::{FxHashMap, FxHashSet};

use crate::interp::Stats;
use crate::interp::canon::StableContextId;
use crate::interp::key::ContextId;
use crate::interp::solver::Solver;
use crate::interp::summary::{SiteId, Status, Target};
use crate::stable::StableDef;

#[derive(Clone, Debug, serde::Serialize)]
pub struct GraphOut {
    pub entries: Vec<StableContextId>,
    pub contexts: Vec<ContextOut>, // reachable from the entries, sorted by id
    pub edges: Vec<EdgeOut>,       // sorted by (from, site, to)
    pub stats: Stats,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ContextOut {
    pub id: StableContextId,
    pub func: StableDef,
    pub qname: String, // module-qualified, as `sem.qname` gives it
    pub status: Status,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct EdgeOut {
    pub from: StableContextId,
    pub site: SiteId, // stable: a range inside the function
    pub span: Span,   // a file range, for display
    pub to: EdgeTo,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize)]
pub enum EdgeTo {
    Context(StableContextId),
    Construct {
        class: StableDef,
        init: Option<StableContextId>,
    },
    External(StableDef),
    Unresolved,
}

/// Where a call is in its file now: one-based line and column, start and end.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Span {
    pub start: (u32, u32),
    pub end: (u32, u32),
}

pub fn graph(s: &Solver<'_>, entries: &[ContextId]) -> GraphOut {
    // 1. what the entries reach through the final edges, in no particular order
    let mut seen: FxHashSet<ContextId> = FxHashSet::default();
    let mut stack: Vec<ContextId> = entries.to_vec();
    let mut reached = Vec::new();
    while let Some(c) = stack.pop() {
        if !seen.insert(c) {
            continue;
        }
        reached.push(c);
        let Some(res) = &s.slots[c].result else {
            continue;
        };
        for e in &res.calls {
            for t in &e.targets {
                stack.extend(t.contexts());
            }
        }
    }
    // 2. each reached context's stable id, once (a lookup table: never iterated for output)
    let ids: FxHashMap<ContextId, StableContextId> =
        reached.iter().map(|&c| (c, s.stable_id(c))).collect();
    // 3. the output, then sorted
    let mut contexts = Vec::new();
    let mut edges = Vec::new();
    for &c in &reached {
        let func = s.contexts[c].func;
        let res = s.slots[c].result.as_ref();
        let def = s.sem.stable_def(func);
        contexts.push(ContextOut {
            id: ids[&c].clone(),
            func: def.clone(),
            qname: s.sem.qname(func),
            status: res.map(|r| r.status).unwrap_or_default(),
        });
        for e in res.iter().flat_map(|r| &r.calls) {
            let range = s.site_range(func, e.site);
            let span = Span {
                start: s.sem.line_col(func, range.start()),
                end: s.sem.line_col(func, range.end()),
            };
            for t in &e.targets {
                let to = match t {
                    Target::Call(ctx) => EdgeTo::Context(ids[ctx].clone()),
                    Target::Construct { class, init } => EdgeTo::Construct {
                        class: s.sem.stable_def(*class),
                        init: init.map(|i| ids[&i].clone()),
                    },
                    Target::External(d) => EdgeTo::External(s.sem.stable_def(*d)),
                    Target::Unresolved => EdgeTo::Unresolved,
                };
                edges.push(EdgeOut {
                    from: ids[&c].clone(),
                    site: e.site,
                    span,
                    to,
                });
            }
        }
    }
    contexts.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| (&a.from, a.site, &a.to).cmp(&(&b.from, b.site, &b.to)));
    GraphOut {
        entries: entries.iter().map(|e| ids[e].clone()).collect(),
        contexts,
        edges,
        stats: s.stats.clone(),
    }
}
