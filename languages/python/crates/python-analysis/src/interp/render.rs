// src/interp/render.rs
use smallvec::SmallVec;

use crate::interp::Mark;
use crate::interp::key::ContextId;
use crate::interp::solver::Solver;
use crate::interp::summary::{SiteId, Target};
use crate::interp::wire::FrameJson;
use crate::sem::Definition;

/// What one level of rendering produced. `cut`: the node budget ran out in here.
pub struct Rendered {
    pub nodes: Vec<FrameJson>,
    pub cut: bool,
}

/// How many more nodes the tree may have.
pub struct Budget(u32);

impl Budget {
    fn tick(&mut self) -> bool {
        self.0 = match self.0.checked_sub(1) {
            Some(left) => left,
            None => return false,
        };
        true
    }
}

/// The wire tree of an entry: the root, then its calls. The root counts against the budget.
pub fn render_root(s: &Solver<'_>, root: ContextId) -> FrameJson {
    let func = s.contexts[root].func;
    let qname = s.sem.qname(func);
    let mut budget = Budget(s.opts.limits.render_nodes);
    budget.tick();
    let mut node = FrameJson {
        target_qname: qname.clone(),
        target_id: s.target_id(func),
        call_count: 0,
        call_site: None,
        contexts: vec![s.stable_id(root)],
        marks: Vec::new(),
        children: Vec::new(),
    };
    let mut path = vec![qname];
    let kids = render(s, &[root], &mut path, 1, &mut budget);
    node.children = kids.nodes;
    if kids.cut {
        node.marks.push(Mark::Cut);
    }
    if s.is_incomplete(root) {
        node.marks.push(Mark::Incomplete);
    }
    node
}

/// The children of a node that stands for `ctxs`: every context of ONE function
/// reached at one site on this path. One child per (site, callee function).
/// A function that is not shown is skipped with everything under it: values
/// still flowed through it, only the view hides it.
///
/// The invariant: a path in the rendered tree is a sequence of contexts, and
/// each callee was resolved under the context above it.
pub fn render(
    s: &Solver<'_>,
    ctxs: &[ContextId],
    path: &mut Vec<String>,
    depth: u32,
    budget: &mut Budget,
) -> Rendered {
    let mut out = Rendered {
        nodes: Vec::new(),
        cut: false,
    };
    let func = s.contexts[ctxs[0]].func;
    for (site, targets) in sites_union(s, ctxs) {
        // same body: the sites line up
        for (callee, callee_ctxs) in group_by_function(s, &targets) {
            if !s.shown(callee) {
                continue;
            }
            if !budget.tick() {
                out.cut = true; // the parent gets Mark::Cut
                return out;
            }
            let q = s.sem.qname(callee);
            let mut node = s.frame_json(callee, func, site, &callee_ctxs);
            if callee_ctxs.iter().any(|&c| s.is_incomplete(c)) {
                node.marks.push(Mark::Incomplete);
            }
            if path.contains(&q) {
                node.marks.push(Mark::Recursive); // shown, not entered
            } else if s.opts.limits.render_depth.is_some_and(|d| depth >= d) {
                if !callee_ctxs.is_empty() {
                    node.marks.push(Mark::Cut); // its children are not rendered
                }
            } else if !callee_ctxs.is_empty() {
                path.push(q);
                let kids = render(s, &callee_ctxs, path, depth + 1, budget);
                path.pop();
                node.children = kids.nodes;
                if kids.cut {
                    node.marks.push(Mark::Cut);
                    out.nodes.push(node);
                    out.cut = true; // and every ancestor: its children are partial
                    return out;
                }
            }
            out.nodes.push(node);
        }
    }
    out
}

/// Every site of the contexts' function, in the first context's order, with
/// the union of their targets.
fn sites_union<'db>(
    s: &Solver<'db>,
    ctxs: &[ContextId],
) -> Vec<(SiteId, SmallVec<[Target<'db>; 2]>)> {
    let mut out: Vec<(SiteId, SmallVec<[Target<'db>; 2]>)> = Vec::new();
    for &c in ctxs {
        let Some(res) = &s.slots[c].result else {
            continue;
        };
        for edge in &res.calls {
            let i = match out.iter().position(|(site, _)| *site == edge.site) {
                Some(i) => i,
                None => {
                    out.push((edge.site, SmallVec::new()));
                    out.len() - 1
                }
            };
            for t in &edge.targets {
                if matches!(t, Target::External(_) | Target::Unresolved) {
                    continue; // the graph shows them, the tree never does
                }
                if !out[i].1.contains(t) {
                    out[i].1.push(t.clone())
                }
            }
        }
    }
    out
}

/// The definition a target's node is named by, and the context under it.
fn callee_of<'db>(s: &Solver<'db>, t: &Target<'db>) -> (Definition<'db>, Option<ContextId>) {
    match *t {
        Target::Call(ctx) => (s.contexts[ctx].func, Some(ctx)),
        Target::Construct { class, init } => (class, init),
        Target::External(_) | Target::Unresolved => unreachable!("`sites_union` drops them"),
    }
}

/// Targets grouped by the definition that names them, ordered by where each is
/// defined (step 01.09): the same order inline mode uses.
fn group_by_function<'db>(
    s: &Solver<'db>,
    targets: &[Target<'db>],
) -> Vec<(Definition<'db>, Vec<ContextId>)> {
    let mut out: Vec<(Definition<'db>, Vec<ContextId>)> = Vec::new();
    for t in targets {
        let (f, ctx) = callee_of(s, t);
        let i = match out.iter().position(|(g, _)| *g == f) {
            Some(i) => i,
            None => {
                out.push((f, Vec::new()));
                out.len() - 1
            }
        };
        out[i].1.extend(ctx);
    }
    out.sort_by_key(|(f, _)| s.sem.def_position(*f));
    out
}
