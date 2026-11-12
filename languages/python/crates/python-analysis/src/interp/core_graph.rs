//! The solver's answer in core's types: contexts, a table of definitions, and one edge per call site and target. The tree, the wire and
//! the text are views of this graph (`analyzer_core::render`, `crate::wire`); none of them reads the solver.

use std::collections::BTreeMap;

use analyzer_core::graph::{
    Context, ContextId as Id, Cut as CCut, DefInfo, Edge, EdgeTo, External, Graph, Reason, Site,
    Stats as CStats, Status as CStatus,
};
use analyzer_core::outline::DefKind;
use analyzer_core::text::Span;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::interp::Stats;
use crate::interp::key::ContextId;
use crate::interp::solver::Solver;
use crate::interp::summary::{Cut, Target};
use crate::sem::Definition;
use crate::stable::StableDef;

fn def_info(s: &Solver<'_>, def: Definition<'_>) -> DefInfo {
    let range = s.sem.def_range(def);
    let kind = if s.sem.is_class(def) {
        DefKind::Class
    } else {
        DefKind::Function
    };
    // no outline def at the name: an empty name, which no request can ask for, rather than a guess
    let (qualname, occurrence) = s.sem.outline_name(def).unwrap_or_default();
    DefInfo::new(
        s.sem.stable_def(def),
        s.sem.qname(def),
        qualname,
        occurrence,
        kind,
        s.sem.relative_file(def),
        Span::new(u32::from(range.start()), u32::from(range.end())),
    )
}

/// The solver's cut, by the name core knows. `ContextsPerFunc` is a context limit and shares `MaxContexts`; the solver's own counter
/// keeps them apart in `Stats::language`.
fn cut(c: Cut) -> CCut {
    match c {
        Cut::Stack => CCut::MaxDepth,
        Cut::Contexts | Cut::ContextsPerFunc => CCut::MaxContexts,
        Cut::Deadline => CCut::Deadline,
    }
}

fn status(s: &Solver<'_>, c: ContextId) -> CStatus {
    let slot = &s.slots[c];
    let Some(result) = &slot.result else {
        // never walked to the end: the limit is unknown, and is not guessed
        return slot.cut.map_or(CStatus::Incomplete(CCut::Cancelled), |k| {
            CStatus::Incomplete(cut(k))
        });
    };
    match (
        result.status.cut.or(slot.cut),
        result.status.gaps.is_empty(),
    ) {
        (Some(k), _) => CStatus::Incomplete(cut(k)),
        (None, false) => CStatus::Incomplete(CCut::MaxRounds),
        (None, true) => CStatus::Finished,
    }
}

/// Why Python's `Unresolved` edges are: the callee's value is `Unknown`, or no value at all. One reason, and the counter of the same name.
const UNKNOWN_CALLEE: &str = "unknown_callee";

fn stats(old: &Stats, unresolved: u64) -> CStats {
    let mut cuts = BTreeMap::new();
    let mut add = |k: CCut, n: u32| {
        if n > 0 {
            *cuts.entry(k).or_insert(0u64) += u64::from(n);
        }
    };
    add(CCut::MaxDepth, old.cuts.stack);
    add(CCut::MaxContexts, old.cuts.contexts);
    add(CCut::MaxContexts, old.cuts.contexts_per_func);
    add(CCut::Deadline, old.cuts.deadline);
    add(CCut::MaxRounds, old.rounds_capped);
    let mut out = CStats::default();
    out.walks = u64::from(old.walks);
    out.contexts = u64::from(old.contexts);
    out.reuse_hits = u64::from(old.reuse_hits);
    out.cuts = cuts;
    out.unresolved = unresolved;
    for (name, n) in [
        ("cut_contexts_per_func", old.cuts.contexts_per_func),
        ("chain_fanout_hits", old.chain_fanout_hits),
        ("prologue_walks", old.prologue_walks),
        ("annotation_fallbacks", old.annotation_fallbacks),
        ("types_dropped", old.types_dropped),
        ("lifted_subclass_of", old.lifted_subclass_of),
    ] {
        out.language.insert(name.to_owned(), u64::from(n));
    }
    for (name, n) in [
        ("effects_applied", old.effects_applied),
        ("effects_dropped", old.effects_dropped),
    ] {
        if n > 0 {
            out.language.insert(name.to_owned(), u64::from(n));
        }
    }
    out.language.insert(UNKNOWN_CALLEE.to_owned(), unresolved);
    out
}

/// Everything `entries` reach through the final edges, as core's graph.
pub(crate) fn core_graph(s: &Solver<'_>, entries: &[ContextId]) -> Graph {
    // 1. what the entries reach, in no particular order
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
    // 2. each reached context's identity, once (a lookup table: never iterated for output)
    let ids: FxHashMap<ContextId, Id> = reached.iter().map(|&c| (c, s.stable_id(c))).collect();
    // 3. the rows; `Graph::new` sorts them and keeps one per definition
    let (mut defs, mut contexts, mut edges) = (Vec::new(), Vec::new(), Vec::new());
    let mut unresolved = 0u64;
    for &c in &reached {
        let func = s.contexts[c].func;
        defs.push(def_info(s, func));
        let mut context = Context::new(ids[&c].clone());
        context.status = status(s, c);
        contexts.push(context);
        for e in s.slots[c].result.iter().flat_map(|r| &r.calls) {
            let site = Site::new(u32::from(e.site.start), u32::from(e.site.end));
            for t in &e.targets {
                let to = match t {
                    Target::Call(ctx, basis) => EdgeTo::Context(ids[ctx].clone(), *basis),
                    Target::Construct { class, init, basis } => {
                        defs.push(def_info(s, *class));
                        EdgeTo::Construct {
                            class: s.sem.stable_def(*class),
                            init: init.map(|i| ids[&i].clone()),
                            basis: *basis,
                        }
                    }
                    Target::External(d, basis) => EdgeTo::External(
                        match s.sem.stable_def(*d) {
                            StableDef::Path { module, qname, .. } => External {
                                package: module,
                                name: qname,
                            },
                            StableDef::Marked(id) => External {
                                package: String::new(),
                                name: id,
                            },
                        },
                        *basis,
                    ),
                    Target::Unresolved => {
                        unresolved += 1;
                        EdgeTo::Unresolved(Reason::new(UNKNOWN_CALLEE))
                    }
                };
                edges.push(Edge {
                    from: ids[&c].clone(),
                    site,
                    to,
                });
            }
        }
    }
    let entry_ids = entries.iter().map(|e| ids[e].clone()).collect();
    Graph::new(
        entry_ids,
        defs,
        contexts,
        edges,
        stats(&s.stats, unresolved),
    )
}
