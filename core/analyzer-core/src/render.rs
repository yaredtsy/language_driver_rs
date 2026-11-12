//! A graph drawn as a tree. The tree is exponential in depth, so it is a view with a budget, never the stored answer.
//!
//! The rules (see `docs/02-tree-rules/02-the-rules.md`): one node per call site and callee function (7), recursion is
//! shown and not entered (8), hidden means hidden with everything under it (9), every cut is marked (10).
//!
//! Nothing here recurses once per level (the walk, [`to_text`] and `Node`'s drop use explicit stacks), so how deep a
//! tree goes is decided by [`RenderOptions`] and by nothing about the machine. `Node`'s derived `Clone`, `Debug` and
//! `PartialEq` do recurse: do not use them on a tree tens of thousands of levels deep.

use std::collections::{BTreeMap, HashSet};

use crate::graph::{ContextId, Edge, EdgeTo, Graph, Root, Site, StableDef, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Mark {
    /// The function is already on the path above: shown as a leaf, not entered.
    Recursive,
    /// The node budget or depth limit ran out here: the children are partial.
    Cut,
    /// A context this node stands for has a gap: a cost limit hit while solving it.
    Incomplete,
    /// A library function: a leaf by definition.
    External,
    /// A call with no known target.
    Unresolved,
}

impl Mark {
    pub fn as_str(self) -> &'static str {
        match self {
            Mark::Recursive => "Recursive",
            Mark::Cut => "Cut",
            Mark::Incomplete => "Incomplete",
            Mark::External => "External",
            Mark::Unresolved => "Unresolved",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Node {
    pub func: StableDef,
    pub qname: String,
    /// `None` for the root.
    pub site: Option<Site>,
    /// The parent hands this def over at `site` and does not call it (D22): `onClick={handleClose}`. Its children are what the def calls when it is
    /// walked. A field and not a `Mark`: a passed node can also be `Recursive`, and a merge must see it as a value.
    pub passed: bool,
    /// Which graph contexts this node stands for. A construction stands for its `init` contexts (none if the class has
    /// no constructor function).
    pub contexts: Vec<ContextId>,
    pub marks: Vec<Mark>,
    pub children: Vec<Node>,
}

impl Drop for Node {
    /// The derived drop would recurse once per level and overflow the stack on a deep tree.
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.children);
        while let Some(mut node) = pending.pop() {
            pending.append(&mut node.children);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RenderOptions {
    /// Counts every node, the root included.
    pub max_nodes: u32,
    /// `None`: the node budget is the guard.
    pub max_depth: Option<u32>,
    /// Hide a function with no marker, and everything under it.
    pub only_marked: bool,
    /// Off: externals are in the graph, not in the tree.
    pub show_external: bool,
    pub show_unresolved: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            max_nodes: 100_000,
            max_depth: None,
            only_marked: false,
            show_external: false,
            show_unresolved: false,
        }
    }
}

/// The name of a definition the graph has no row for. It is drawn, not skipped: the invariant checker reports the
/// missing row, and the renderer never fails on a broken graph.
const UNKNOWN: &str = "<unknown>";

#[derive(PartialEq, Eq)]
enum Kind {
    Code,
    External,
    Unresolved,
}

/// What one group of edges (one site, one callee function) becomes.
struct Child<'g> {
    site: Site,
    /// Sorts siblings of one site: project code by where it is defined, then externals, then unresolved.
    order: (u8, &'g str, u32),
    func: StableDef,
    qname: String,
    contexts: Vec<&'g ContextId>,
    kind: Kind,
    /// Handed over at `site`, not called (`Edge::passed`). Only project code can be.
    passed: bool,
}

/// A node being built, and the children it has not visited yet.
struct Frame<'g> {
    node: Node,
    kids: std::vec::IntoIter<Child<'g>>,
}

struct Renderer<'g> {
    graph: &'g Graph,
    opts: &'g RenderOptions,
}

/// The tree under `root`. A root that is not in the graph gives a bare root (the caller asked for something we
/// do not have; it is not a panic).
pub fn render(graph: &Graph, root: &Root, opts: &RenderOptions) -> Node {
    let r = Renderer { graph, opts };
    let mut budget = opts.max_nodes.saturating_sub(1);
    let func = root.func().clone();
    let qname = graph
        .def(&func)
        .map_or_else(|| UNKNOWN.to_owned(), |d| d.qname.clone());
    // A construction stands for its constructor's context, if the class has one: what `Renderer::class` does for a callee, with no site.
    let contexts: Vec<&ContextId> = root.context().into_iter().collect();
    // Functions on the path from the root to the node being built (rule 8 asks "is it on the path?").
    let mut path: HashSet<StableDef> = HashSet::from([func.clone()]);
    let mut stack = vec![r.open(func, qname, None, false, contexts, 0)];

    loop {
        let top = stack.last_mut().expect("the root frame is popped last");
        let Some(child) = top.kids.next() else {
            let mut done = stack.pop().expect("just looked at it").node;
            done.marks.sort();
            done.marks.dedup();
            let Some(parent) = stack.last_mut() else {
                return done;
            };
            path.remove(&done.func);
            parent.node.children.push(done);
            continue;
        };
        if budget == 0 {
            top.node.marks.push(Mark::Cut);
            top.kids = Vec::new().into_iter();
            continue;
        }
        budget -= 1;
        let site = Some(child.site);
        match child.kind {
            Kind::External => top.node.children.push(leaf(child, Mark::External)),
            Kind::Unresolved => top.node.children.push(leaf(child, Mark::Unresolved)),
            // Rule 8: by function, not by context.
            Kind::Code if path.contains(&child.func) => {
                top.node.children.push(leaf(child, Mark::Recursive))
            }
            Kind::Code => {
                path.insert(child.func.clone());
                let depth = stack.len() as u32;
                stack.push(r.open(
                    child.func,
                    child.qname,
                    site,
                    child.passed,
                    child.contexts,
                    depth,
                ));
            }
        }
    }
}

fn leaf(c: Child<'_>, mark: Mark) -> Node {
    Node {
        func: c.func,
        qname: c.qname,
        site: Some(c.site),
        passed: c.passed,
        contexts: c.contexts.into_iter().cloned().collect(),
        marks: vec![mark],
        children: Vec::new(),
    }
}

impl<'g> Renderer<'g> {
    fn open(
        &self,
        func: StableDef,
        qname: String,
        site: Option<Site>,
        passed: bool,
        contexts: Vec<&'g ContextId>,
        depth: u32,
    ) -> Frame<'g> {
        let mut marks = Vec::new();
        if contexts.iter().any(|id| {
            self.graph
                .context(id)
                .is_some_and(|c| matches!(c.status, Status::Incomplete(_)))
        }) {
            marks.push(Mark::Incomplete);
        }
        let mut groups = self.children_of(&contexts);
        if !groups.is_empty() && self.opts.max_depth.is_some_and(|limit| depth >= limit) {
            marks.push(Mark::Cut);
            groups.clear();
        }
        let node = Node {
            func,
            qname,
            site,
            passed,
            contexts: contexts.into_iter().cloned().collect(),
            marks,
            children: Vec::new(),
        };
        Frame {
            node,
            kids: groups.into_iter(),
        }
    }

    /// Rule 7: group the edges of all `contexts` by (site, callee function, called or passed); rule 9: leave out what is hidden.
    fn children_of(&self, contexts: &[&'g ContextId]) -> Vec<Child<'g>> {
        let mut groups: BTreeMap<(Site, StableDef, u8, bool), Child<'g>> = BTreeMap::new();
        for id in contexts {
            for edge in self.graph.edges_from(id) {
                let Some(child) = self.child(edge) else {
                    continue;
                };
                match groups.entry((edge.site, child.func.clone(), child.order.0, child.passed)) {
                    std::collections::btree_map::Entry::Occupied(mut o) => {
                        for id in child.contexts {
                            if !o.get().contexts.contains(&id) {
                                o.get_mut().contexts.push(id);
                            }
                        }
                    }
                    std::collections::btree_map::Entry::Vacant(v) => {
                        v.insert(child);
                    }
                }
            }
        }
        let mut out: Vec<Child<'g>> = groups.into_values().collect();
        for c in &mut out {
            c.contexts.sort();
        }
        out.sort_by(|a, b| {
            (a.site, a.order, &a.func, a.passed).cmp(&(b.site, b.order, &b.func, b.passed))
        });
        out
    }

    fn child(&self, edge: &'g Edge) -> Option<Child<'g>> {
        let site = edge.site;
        match &edge.to {
            EdgeTo::Context(id, _) => self.code(site, id, edge.passed),
            EdgeTo::Construct { class, init, .. } => self.class(site, class, init.as_ref()),
            EdgeTo::External(ext, _) => self.opts.show_external.then(|| Child {
                site,
                order: (1, "", 0),
                func: StableDef::Path {
                    module: ext.package.clone(),
                    qname: ext.name.clone(),
                    nth: 0,
                },
                qname: if ext.package.is_empty() {
                    ext.name.clone()
                } else {
                    format!("{}.{}", ext.package, ext.name)
                },
                contexts: Vec::new(),
                kind: Kind::External,
                passed: false,
            }),
            EdgeTo::Unresolved(_) => self.opts.show_unresolved.then(|| Child {
                site,
                order: (2, "", 0),
                func: StableDef::Path {
                    module: String::new(),
                    qname: "<unresolved>".to_owned(),
                    nth: 0,
                },
                qname: "<unresolved>".to_owned(),
                contexts: Vec::new(),
                kind: Kind::Unresolved,
                passed: false,
            }),
        }
    }

    fn code(&self, site: Site, id: &'g ContextId, passed: bool) -> Option<Child<'g>> {
        if self.opts.only_marked && !id.func.is_marked() {
            return None;
        }
        let (qname, order) = self.describe(&id.func);
        Some(Child {
            site,
            order,
            func: id.func.clone(),
            qname,
            contexts: vec![id],
            kind: Kind::Code,
            passed,
        })
    }

    /// Rule 5: a node for the class, named and placed by its row in `defs`; its children come from `init`, if the class
    /// has a constructor function.
    fn class(
        &self,
        site: Site,
        class: &'g StableDef,
        init: Option<&'g ContextId>,
    ) -> Option<Child<'g>> {
        if self.opts.only_marked && !class.is_marked() {
            return None;
        }
        let (qname, order) = self.describe(class);
        Some(Child {
            site,
            order,
            func: class.clone(),
            qname,
            contexts: init.into_iter().collect(),
            kind: Kind::Code,
            passed: false,
        })
    }

    /// The name and the sibling order of a definition. Without a row: `<unknown>`, ordered before everything else.
    fn describe(&self, func: &StableDef) -> (String, (u8, &'g str, u32)) {
        match self.graph.def(func) {
            Some(d) => (d.qname.clone(), (0, d.file.as_str(), d.range.start)),
            None => (UNKNOWN.to_owned(), (0, "", 0)),
        }
    }
}

/// One line per node, two spaces per level: `qname`, then ` [Passed]` when the parent hands the def over and does not call it (D22), then ` [Mark]`
/// for each mark. The format of the scenario files; a tree with no passed node prints what it always printed.
pub fn to_text(root: &Node) -> String {
    let mut out = String::new();
    let mut stack = vec![(root, 0usize)];
    while let Some((node, level)) = stack.pop() {
        out.push_str(&"  ".repeat(level));
        out.push_str(&node.qname);
        if node.passed {
            out.push_str(" [Passed]");
        }
        for m in &node.marks {
            out.push_str(" [");
            out.push_str(m.as_str());
            out.push(']');
        }
        out.push('\n');
        stack.extend(node.children.iter().rev().map(|c| (c, level + 1)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::tests::{cid, ctx, def, marked};
    use crate::graph::{Basis, Context, DefInfo, External, Reason, Stats};
    use crate::outline::DefKind;
    use crate::text::Span;

    /// The `n`th definition of `m.py`, 100 bytes apart.
    fn def_at(name: &str, kind: DefKind, n: u32) -> DefInfo {
        DefInfo::new(
            marked(name),
            name,
            name,
            0,
            kind,
            "m.py",
            Span::new(n * 100, n * 100 + 50),
        )
    }

    /// `spec`: `"a: b c"` means `a` calls `b` then `c`, one site each, 10 bytes apart. Each line is a function with a
    /// row and one context.
    fn graph(spec: &[&str]) -> Graph {
        let mut defs = Vec::new();
        let mut contexts = Vec::new();
        let mut edges = Vec::new();
        for (n, line) in spec.iter().enumerate() {
            let (name, calls) = line.split_once(':').unwrap();
            defs.push(def_at(name.trim(), DefKind::Function, n as u32));
            contexts.push(ctx(name.trim()));
            for (i, callee) in calls.split_whitespace().enumerate() {
                let to = match callee {
                    "?" => EdgeTo::Unresolved(Reason::new("toy")),
                    "ext" => EdgeTo::External(
                        External {
                            package: "lib".into(),
                            name: "call".into(),
                        },
                        Basis::Type,
                    ),
                    callee => EdgeTo::Context(cid(callee), Basis::Type),
                };
                edges.push(Edge::new(cid(name.trim()), Site::at(i as u32 * 10), to));
            }
        }
        Graph::new(vec![], defs, contexts, edges, Stats::default())
    }

    /// `g` with other edges, keeping its rows and contexts.
    fn with_edges(g: &Graph, edges: Vec<Edge>) -> Graph {
        Graph::new(
            vec![],
            g.defs.clone(),
            g.contexts.clone(),
            edges,
            Stats::default(),
        )
    }

    fn text(g: &Graph, root: &str, opts: &RenderOptions) -> String {
        to_text(&render(g, &Root::Context(cid(root)), opts))
    }

    #[test]
    fn rule_3_children_are_in_source_order() {
        let g = graph(&["main: z a m", "a:", "m:", "z:"]);
        assert_eq!(
            text(&g, "main", &RenderOptions::default()),
            "main\n  z\n  a\n  m\n"
        );
    }

    #[test]
    fn rule_7_a_callee_called_twice_is_two_nodes() {
        let g = graph(&["main: f f", "f:"]);
        assert_eq!(
            text(&g, "main", &RenderOptions::default()),
            "main\n  f\n  f\n"
        );
    }

    #[test]
    fn rule_7_one_site_one_function_many_contexts_is_one_node() {
        let mut g = graph(&["main:", "f:"]);
        let f2 = ContextId {
            func: marked("f"),
            digest: [7; 16],
        };
        let site = Site::at(0);
        let mut edges = vec![
            Edge::new(cid("main"), site, EdgeTo::Context(cid("f"), Basis::Type)),
            Edge::new(cid("main"), site, EdgeTo::Context(f2.clone(), Basis::Type)),
        ];
        edges.extend(g.edges.clone());
        let mut contexts = g.contexts.clone();
        contexts.push(Context::new(f2.clone()));
        g = Graph::new(vec![], g.defs.clone(), contexts, edges, Stats::default());
        let tree = render(&g, &Root::Context(cid("main")), &RenderOptions::default());
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].contexts, vec![cid("f"), f2]);
    }

    #[test]
    fn rule_7_one_site_different_functions_are_siblings_ordered_by_definition() {
        // `b` is defined (index 1) before `a` (index 2): the order is by file and offset, not by name or discovery
        let mut g = graph(&["main:", "b:", "a:"]);
        let site = Site::at(0);
        let mut edges = vec![
            Edge::new(cid("main"), site, EdgeTo::Context(cid("a"), Basis::Type)),
            Edge::new(cid("main"), site, EdgeTo::Context(cid("b"), Basis::Type)),
        ];
        edges.extend(g.edges.clone());
        g = with_edges(&g, edges);
        assert_eq!(
            text(&g, "main", &RenderOptions::default()),
            "main\n  b\n  a\n"
        );
    }

    #[test]
    fn rule_8_recursion_is_a_leaf() {
        let g = graph(&["main: f", "f: f g", "g: f"]);
        assert_eq!(
            text(&g, "main", &RenderOptions::default()),
            "main\n  f\n    f [Recursive]\n    g\n      f [Recursive]\n"
        );
    }

    #[test]
    fn rule_8_the_root_counts_as_on_the_path() {
        let g = graph(&["main: main"]);
        assert_eq!(
            text(&g, "main", &RenderOptions::default()),
            "main\n  main [Recursive]\n"
        );
    }

    #[test]
    fn rule_8_recursion_is_checked_by_function_not_context() {
        let f2 = ContextId {
            func: marked("f"),
            digest: [1; 16],
        };
        let mut g = graph(&["f:", "main:"]);
        let mut contexts = g.contexts.clone();
        contexts.push(Context::new(f2.clone()));
        let edges = vec![Edge::new(
            cid("f"),
            Site::at(0),
            EdgeTo::Context(f2, Basis::Type),
        )];
        g = Graph::new(vec![], g.defs.clone(), contexts, edges, Stats::default());
        assert_eq!(
            text(&g, "f", &RenderOptions::default()),
            "f\n  f [Recursive]\n"
        );
    }

    #[test]
    fn rule_9_hidden_hides_the_subtree() {
        let mut g = graph(&["main: a", "a: b", "b:"]);
        // `a` is unmarked: it and `b` (marked, but only reachable through it) disappear
        let unmarked = StableDef::Path {
            module: "m".into(),
            qname: "a".into(),
            nth: 0,
        };
        for e in &mut g.edges {
            if e.to == EdgeTo::Context(cid("a"), Basis::Type) {
                e.to = EdgeTo::Context(ContextId::of(unmarked.clone()), Basis::Type);
            }
        }
        let mut row = def("a");
        row.id = unmarked.clone();
        g.defs.push(row);
        g.defs.sort_by(|a, b| a.id.cmp(&b.id));
        g.contexts.push(Context::new(ContextId::of(unmarked)));
        g.contexts.sort_by(|a, b| a.id.cmp(&b.id));
        let opts = RenderOptions {
            only_marked: true,
            ..RenderOptions::default()
        };
        assert_eq!(text(&g, "main", &opts), "main\n");
        assert_eq!(text(&g, "main", &RenderOptions::default()), "main\n  a\n");
    }

    #[test]
    fn externals_and_unresolved_are_hidden_unless_asked() {
        let g = graph(&["main: ext ? f", "f:"]);
        assert_eq!(text(&g, "main", &RenderOptions::default()), "main\n  f\n");
        let opts = RenderOptions {
            show_external: true,
            show_unresolved: true,
            ..RenderOptions::default()
        };
        assert_eq!(
            text(&g, "main", &opts),
            "main\n  lib.call [External]\n  <unresolved> [Unresolved]\n  f\n"
        );
    }

    /// `main` builds `Foo` at one site and `Bar` (no constructor function) at the next. The graph holds a row for each
    /// class and no context, the way a solver writes it.
    fn construct_graph() -> Graph {
        let g = graph(&["main:", "Foo.init: helper", "helper:"]);
        let mut defs = g.defs.clone();
        defs.push(def_at("Foo", DefKind::Class, 3));
        defs.push(def_at("Bar", DefKind::Class, 4));
        let mut edges = g.edges.clone();
        edges.push(Edge::new(
            cid("main"),
            Site::at(0),
            EdgeTo::Construct {
                class: marked("Foo"),
                init: Some(cid("Foo.init")),
                basis: Basis::Type,
            },
        ));
        edges.push(Edge::new(
            cid("main"),
            Site::at(5),
            EdgeTo::Construct {
                class: marked("Bar"),
                init: None,
                basis: Basis::Type,
            },
        ));
        Graph::new(vec![], defs, g.contexts.clone(), edges, Stats::default())
    }

    #[test]
    fn rule_5_a_construct_is_a_node_for_the_class_with_the_children_of_init() {
        let g = construct_graph();
        assert_eq!(
            text(&g, "main", &RenderOptions::default()),
            "main\n  Foo\n    helper\n  Bar\n"
        );
        let tree = render(&g, &Root::Context(cid("main")), &RenderOptions::default());
        assert_eq!(tree.children[0].func, marked("Foo"));
        assert_eq!(tree.children[0].contexts, vec![cid("Foo.init")]);
        assert!(
            tree.children[1].contexts.is_empty(),
            "Bar has no constructor function"
        );
    }

    #[test]
    fn rule_5_a_construct_without_init_is_still_a_leaf_node() {
        let mut g = graph(&["main:"]);
        let edge = Edge::new(
            cid("main"),
            Site::at(0),
            EdgeTo::Construct {
                class: marked("Bar"),
                init: None,
                basis: Basis::Type,
            },
        );
        let mut defs = g.defs.clone();
        defs.push(def_at("Bar", DefKind::Class, 1));
        g = Graph::new(
            vec![],
            defs,
            g.contexts.clone(),
            vec![edge],
            Stats::default(),
        );
        assert_eq!(text(&g, "main", &RenderOptions::default()), "main\n  Bar\n");
    }

    #[test]
    fn a_def_the_graph_does_not_describe_is_drawn_as_unknown() {
        // a class with no row, and a called function with a context and no row, each beside a described function that
        // is defined later: the unknown one sorts first at its site
        let site = Site::at(0);
        let edge = |to| Edge::new(cid("main"), site, to);
        let g = graph(&["main:", "a:"]);
        let construct = EdgeTo::Construct {
            class: marked("Ghost"),
            init: None,
            basis: Basis::Type,
        };
        let with = with_edges(
            &g,
            vec![
                edge(EdgeTo::Context(cid("a"), Basis::Type)),
                edge(construct),
            ],
        );
        assert_eq!(
            text(&with, "main", &RenderOptions::default()),
            "main\n  <unknown>\n  a\n"
        );

        let mut contexts = g.contexts.clone();
        contexts.push(ctx("ghost"));
        let with = Graph::new(
            vec![],
            g.defs.clone(),
            contexts,
            vec![
                edge(EdgeTo::Context(cid("a"), Basis::Type)),
                edge(EdgeTo::Context(cid("ghost"), Basis::Type)),
            ],
            Stats::default(),
        );
        assert_eq!(
            text(&with, "main", &RenderOptions::default()),
            "main\n  <unknown>\n  a\n"
        );
    }

    #[test]
    fn an_edge_to_a_context_the_graph_does_not_hold_is_still_drawn() {
        let g = graph(&["main:"]);
        let edge = Edge::new(
            cid("main"),
            Site::at(0),
            EdgeTo::Context(cid("gone"), Basis::Type),
        );
        let g = with_edges(&g, vec![edge]);
        assert_eq!(
            text(&g, "main", &RenderOptions::default()),
            "main\n  <unknown>\n",
            "the call is in the tree even though nothing describes its target"
        );
    }

    #[test]
    fn a_construct_of_a_class_already_on_the_path_is_recursive() {
        // Foo's constructor builds a Foo
        let g = graph(&["main:", "Foo:", "Foo.init:"]);
        let edges = vec![
            Edge::new(
                cid("main"),
                Site::at(0),
                EdgeTo::Construct {
                    class: marked("Foo"),
                    init: Some(cid("Foo.init")),
                    basis: Basis::Type,
                },
            ),
            Edge::new(
                cid("Foo.init"),
                Site::at(0),
                EdgeTo::Construct {
                    class: marked("Foo"),
                    init: Some(cid("Foo.init")),
                    basis: Basis::Type,
                },
            ),
        ];
        let g = with_edges(&g, edges);
        assert_eq!(
            text(&g, "main", &RenderOptions::default()),
            "main\n  Foo\n    Foo [Recursive]\n"
        );
    }

    #[test]
    fn only_marked_hides_an_unmarked_class() {
        let g = graph(&["main:"]);
        let class = StableDef::Path {
            module: "m".into(),
            qname: "Foo".into(),
            nth: 0,
        };
        let edge = Edge::new(
            cid("main"),
            Site::at(0),
            EdgeTo::Construct {
                class: class.clone(),
                init: None,
                basis: Basis::Type,
            },
        );
        let mut row = def_at("Foo", DefKind::Class, 1);
        row.id = class;
        let mut defs = g.defs.clone();
        defs.push(row);
        let g = Graph::new(
            vec![],
            defs,
            g.contexts.clone(),
            vec![edge],
            Stats::default(),
        );
        let opts = RenderOptions {
            only_marked: true,
            ..RenderOptions::default()
        };
        assert_eq!(text(&g, "main", &opts), "main\n");
        assert_eq!(text(&g, "main", &RenderOptions::default()), "main\n  Foo\n");
    }

    #[test]
    fn rule_10_node_budget_cuts_and_marks() {
        let g = graph(&["main: a b c", "a:", "b:", "c:"]);
        let opts = RenderOptions {
            max_nodes: 3,
            ..RenderOptions::default()
        };
        assert_eq!(text(&g, "main", &opts), "main [Cut]\n  a\n  b\n");
    }

    #[test]
    fn rule_10_depth_limit_cuts_only_where_there_was_more() {
        let g = graph(&["main: a", "a: b", "b:"]);
        let opts = RenderOptions {
            max_depth: Some(1),
            ..RenderOptions::default()
        };
        assert_eq!(text(&g, "main", &opts), "main\n  a [Cut]\n");
        let opts = RenderOptions {
            max_depth: Some(2),
            ..RenderOptions::default()
        };
        assert_eq!(text(&g, "main", &opts), "main\n  a\n    b\n");
    }

    #[test]
    fn an_incomplete_context_is_marked() {
        use crate::graph::Cut;
        let mut g = graph(&["main: a", "a:"]);
        let a = g.contexts.iter_mut().find(|c| c.id == cid("a")).unwrap();
        a.status = Status::Incomplete(Cut::Deadline);
        assert_eq!(
            text(&g, "main", &RenderOptions::default()),
            "main\n  a [Incomplete]\n"
        );
    }

    #[test]
    fn an_entry_the_graph_does_not_hold_is_a_bare_root() {
        let g = graph(&["main:"]);
        assert_eq!(text(&g, "nope", &RenderOptions::default()), "<unknown>\n");
    }

    /// Levels of a chain, by walking down (a recursive helper would be the thing under test).
    fn chain_length(root: &Node) -> (usize, Vec<Mark>) {
        let (mut n, mut node) = (1, root);
        while let Some(child) = node.children.first() {
            n += 1;
            node = child;
        }
        (n, node.marks.clone())
    }

    #[test]
    fn depth_is_limited_by_the_options_and_nothing_else() {
        // deep enough to overflow any per-level recursion, in the walk and in drop
        let n = 60_000;
        let spec: Vec<String> = (0..n)
            .map(|i| {
                if i + 1 < n {
                    format!("f{i}: f{}", i + 1)
                } else {
                    format!("f{i}:")
                }
            })
            .collect();
        let spec_refs: Vec<&str> = spec.iter().map(String::as_str).collect();
        let g = graph(&spec_refs);
        let tree = render(&g, &Root::Context(cid("f0")), &RenderOptions::default());
        assert_eq!(
            chain_length(&tree),
            (n, vec![]),
            "every level is there, nothing is cut"
        );
        drop(tree);

        let opts = RenderOptions {
            max_depth: Some(40_000),
            ..RenderOptions::default()
        };
        let tree = render(&g, &Root::Context(cid("f0")), &opts);
        assert_eq!(chain_length(&tree), (40_001, vec![Mark::Cut]));
    }

    #[test]
    fn to_text_does_not_recurse_either() {
        // the text of a chain is quadratic in its depth (two spaces per level), so keep this one moderate
        let n = 4_000;
        let spec: Vec<String> = (0..n)
            .map(|i| {
                if i + 1 < n {
                    format!("f{i}: f{}", i + 1)
                } else {
                    format!("f{i}:")
                }
            })
            .collect();
        let spec_refs: Vec<&str> = spec.iter().map(String::as_str).collect();
        let out = text(&graph(&spec_refs), "f0", &RenderOptions::default());
        assert_eq!(out.lines().count(), n);
        assert!(
            out.lines()
                .last()
                .unwrap()
                .ends_with(&format!("f{}", n - 1))
        );
    }

    #[test]
    fn same_graph_same_text() {
        let g = graph(&["main: a b", "a: b", "b:"]);
        let opts = RenderOptions::default();
        assert_eq!(text(&g, "main", &opts), text(&g, "main", &opts));
    }

    /// `graph(spec)` and a class row `Foo`, with a root that constructs `Foo`, whose constructor is the context `init`.
    fn constructed(spec: &[&str], init: Option<&str>) -> (Graph, Root) {
        let g = graph(spec);
        let mut defs = g.defs.clone();
        defs.push(def_at("Foo", DefKind::Class, 9));
        let root = Root::Construct {
            class: marked("Foo"),
            init: init.map(cid),
        };
        let g = Graph::new(
            vec![root.clone()],
            defs,
            g.contexts.clone(),
            g.edges.clone(),
            Stats::default(),
        );
        (g, root)
    }

    #[test]
    fn a_construct_root_is_drawn_like_a_class_callee() {
        let (g, root) = constructed(&["init: a b", "a:", "b:"], Some("init"));
        let tree = render(&g, &root, &RenderOptions::default());
        assert_eq!((tree.qname.as_str(), tree.site), ("Foo", None));
        assert_eq!(tree.func, marked("Foo"));
        assert_eq!(tree.contexts, vec![cid("init")]);
        assert_eq!(to_text(&tree), "Foo\n  a\n  b\n");
    }

    #[test]
    fn a_construct_root_without_a_constructor_has_no_children_and_no_marks() {
        let (g, root) = constructed(&["a:"], None);
        let tree = render(&g, &root, &RenderOptions::default());
        assert_eq!(to_text(&tree), "Foo\n");
        assert!(tree.contexts.is_empty() && tree.marks.is_empty());
    }

    #[test]
    fn rule_8_a_construct_root_is_on_the_path() {
        // the constructor builds the class it belongs to again: the inner node is a leaf, marked, not entered
        let (g, root) = constructed(&["init:"], Some("init"));
        let g = with_edges(
            &g,
            vec![Edge::new(
                cid("init"),
                Site::at(0),
                EdgeTo::Construct {
                    class: marked("Foo"),
                    init: Some(cid("init")),
                    basis: Basis::Type,
                },
            )],
        );
        let tree = render(&g, &root, &RenderOptions::default());
        assert_eq!(to_text(&tree), "Foo\n  Foo [Recursive]\n");
    }

    // D22

    fn toy_text(
        program: &[(&'static str, &[&'static str])],
        entry: &'static str,
        opts: &RenderOptions,
    ) -> String {
        let g = crate::solve::tests::graph_of(program, entry);
        to_text(&render(&g, &g.entries[0], opts))
    }

    #[test]
    fn d22_a_def_handed_over_is_a_node_marked_passed_with_what_it_calls_under_it() {
        let program: &[(&'static str, &[&'static str])] = &[
            ("main", &["a", ">a", ">b"]),
            ("a", &[]),
            ("b", &["c"]),
            ("c", &[]),
        ];
        assert_eq!(
            toy_text(program, "main", &RenderOptions::default()),
            "main\n  a\n  a [Passed]\n  b [Passed]\n    c\n"
        );
    }

    #[test]
    fn d22_a_passed_def_already_on_the_path_is_a_recursive_leaf_and_still_passed() {
        assert_eq!(
            toy_text(&[("f", &[">f"])], "f", &RenderOptions::default()),
            "f\n  f [Passed] [Recursive]\n"
        );
    }

    #[test]
    fn d22_a_tree_with_no_passed_node_prints_what_it_printed() {
        let g = graph(&["main: a b", "a: b", "b:"]);
        let tree = render(
            &g,
            &g.entries
                .first()
                .cloned()
                .unwrap_or(Root::Context(cid("main"))),
            &RenderOptions::default(),
        );
        assert_eq!(to_text(&tree), "main\n  a\n    b\n  b\n");
        fn any_passed(n: &Node) -> bool {
            n.passed || n.children.iter().any(any_passed)
        }
        assert!(!any_passed(&tree));
    }

    #[test]
    fn d22_hidden_hides_a_passed_node_and_what_is_under_it() {
        let mut g = graph(&["main: a", "a: b", "b:"]);
        let unmarked = StableDef::Path {
            module: "m".into(),
            qname: "a".into(),
            nth: 0,
        };
        for e in &mut g.edges {
            if e.to == EdgeTo::Context(cid("a"), Basis::Type) {
                e.to = EdgeTo::Context(ContextId::of(unmarked.clone()), Basis::Type);
                e.passed = true;
            }
        }
        let mut row = def("a");
        row.id = unmarked.clone();
        g.defs.push(row);
        g.defs.sort_by(|a, b| a.id.cmp(&b.id));
        g.contexts.push(Context::new(ContextId::of(unmarked)));
        g.contexts.sort_by(|a, b| a.id.cmp(&b.id));
        let only = RenderOptions {
            only_marked: true,
            ..RenderOptions::default()
        };
        assert_eq!(text(&g, "main", &only), "main\n");
        assert_eq!(
            text(&g, "main", &RenderOptions::default()),
            "main\n  a [Passed]\n"
        );
    }
}
