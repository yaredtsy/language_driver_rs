//! Checks on a [`Graph`] that hold for every language.

use std::collections::HashSet;

use analyzer_core::graph::{ContextId, EdgeTo, Graph, StableDef};

/// Everything wrong with `graph`, one sentence each. Empty means the graph is well formed: sorted, no duplicates,
/// no edge or entry pointing at a context the graph does not hold, and `defs` has a row for every function and class
/// the graph mentions and for nothing else.
pub fn violations(graph: &Graph) -> Vec<String> {
    let mut out = Vec::new();
    if !graph.contexts.windows(2).all(|w| w[0].id < w[1].id) {
        out.push("contexts are not sorted by id, or an id appears twice".to_owned());
    }
    if !graph.defs.windows(2).all(|w| w[0].id < w[1].id) {
        out.push("defs are not sorted by id, or an id appears twice".to_owned());
    }
    if !graph.edges.windows(2).all(|w| w[0] < w[1]) {
        out.push("edges are not sorted by (from, site, to), or one appears twice".to_owned());
    }
    let known: HashSet<&ContextId> = graph.contexts.iter().map(|c| &c.id).collect();
    for entry in &graph.entries {
        if !known.contains(entry) {
            out.push(format!("entry {:?} is not in `contexts`", entry.func));
        }
    }
    for edge in &graph.edges {
        if !known.contains(&edge.from) {
            out.push(format!(
                "edge from {:?} starts at an unknown context",
                edge.from.func
            ));
        }
        let target = match &edge.to {
            EdgeTo::Context(id, _) => Some(id),
            EdgeTo::Construct { init, .. } => init.as_ref(),
            _ => None,
        };
        if let Some(id) = target.filter(|id| !known.contains(id)) {
            out.push(format!(
                "edge from {:?} points at unknown context {:?}",
                edge.from.func, id.func
            ));
        }
    }

    // `defs` and the things that need a row: the function of a context, the class of a construction. A row nothing
    // needs would come from a cache or from the order work was done in, and two runs could differ.
    // Rows are found in a set, not with `Graph::def`: that one binary-searches, and on unsorted `defs` it would turn
    // one sorting fault into a "no row" for everything.
    let rows: HashSet<&StableDef> = graph.defs.iter().map(|d| &d.id).collect();
    let mut used: HashSet<&StableDef> = HashSet::new();
    for context in &graph.contexts {
        used.insert(&context.id.func);
        if !rows.contains(&context.id.func) {
            out.push(format!("context {:?} has no row in defs", context.id.func));
        }
    }
    for edge in &graph.edges {
        if let EdgeTo::Construct { class, .. } = &edge.to {
            used.insert(class);
            if !rows.contains(class) {
                out.push(format!("class {class:?} has no row in defs"));
            }
        }
    }
    for def in &graph.defs {
        if !used.contains(&def.id) {
            out.push(format!("def {:?} is used by nothing", def.id));
        }
    }
    out
}

/// Panics with every violation.
pub fn check_invariants(graph: &Graph) {
    let bad = violations(graph);
    assert!(
        bad.is_empty(),
        "the graph breaks its invariants:\n  {}",
        bad.join("\n  ")
    );
}

/// Build the graph twice and require the same answer. Run it with the deadline off: a deadline is a cost limit,
/// and a cost limit may legitimately cut differently on two runs.
pub fn assert_deterministic(build: impl Fn() -> Graph) {
    let (first, second) = (build(), build());
    check_invariants(&first);
    assert_eq!(
        first, second,
        "two runs on the same input gave different graphs"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use analyzer_core::graph::{Basis, Context, ContextId, DefInfo, Edge, Site, StableDef, Stats};
    use analyzer_core::outline::DefKind;
    use analyzer_core::text::Span;

    fn def_id(n: &str) -> StableDef {
        StableDef::Marked(n.to_owned())
    }

    fn id(n: &str) -> ContextId {
        ContextId::of(def_id(n))
    }

    fn ctx(n: &str) -> Context {
        Context::new(id(n))
    }

    fn row(n: &str, kind: DefKind) -> DefInfo {
        DefInfo::new(def_id(n), n, kind, "f", Span::new(0, 1))
    }

    fn edge(from: &str, to: &str) -> Edge {
        Edge {
            from: id(from),
            site: Site::at(0),
            to: EdgeTo::Context(id(to), Basis::Type),
        }
    }

    fn construct(from: &str, class: &str) -> Edge {
        Edge {
            from: id(from),
            site: Site::at(0),
            to: EdgeTo::Construct {
                class: def_id(class),
                init: None,
                basis: Basis::Type,
            },
        }
    }

    /// `a` calls `b` and builds `Foo`: two contexts and three rows.
    fn good() -> Graph {
        Graph::new(
            vec![id("a")],
            vec![
                row("a", DefKind::Function),
                row("b", DefKind::Function),
                row("Foo", DefKind::Class),
            ],
            vec![ctx("a"), ctx("b")],
            vec![edge("a", "b"), construct("a", "Foo")],
            Stats::default(),
        )
    }

    /// Exactly one violation, and it says `sentence`.
    fn only(g: &Graph, sentence: &str) {
        let bad = violations(g);
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert!(bad[0].contains(sentence), "{bad:?}");
    }

    #[test]
    fn a_good_graph_has_no_violations() {
        assert_eq!(violations(&good()), Vec::<String>::new());
    }

    #[test]
    fn dangling_and_missing_entries_are_found() {
        let g = Graph::new(
            vec![id("zz")],
            vec![row("a", DefKind::Function)],
            vec![ctx("a")],
            vec![edge("a", "gone"), edge("ghost", "a")],
            Stats::default(),
        );
        let bad = violations(&g);
        assert_eq!(bad.len(), 3, "{bad:?}");
        assert!(bad.iter().any(|b| b.contains("entry")));
        assert!(
            bad.iter()
                .any(|b| b.contains("unknown context") && b.contains("gone"))
        );
        assert!(bad.iter().any(|b| b.contains("starts at an unknown")));
    }

    #[test]
    fn unsorted_output_is_found() {
        // `Graph::new` sorts, so build the broken value by editing a good one (the fields are public)
        let mut g = good();
        g.contexts.reverse();
        only(&g, "contexts are not sorted");
    }

    #[test]
    fn unsorted_defs_are_found() {
        let mut g = good();
        g.defs.reverse();
        only(&g, "defs are not sorted");
    }

    #[test]
    fn a_repeated_def_row_is_found() {
        let mut g = good();
        let first = g.defs[0].clone();
        g.defs.insert(1, first);
        only(&g, "defs are not sorted");
    }

    #[test]
    fn a_context_without_a_def_row_is_found() {
        let mut g = good();
        g.defs.retain(|d| d.id != def_id("b"));
        only(&g, "context");
        assert!(violations(&g)[0].contains("has no row in defs"));
        assert!(violations(&g)[0].contains("\"b\""));
    }

    #[test]
    fn a_constructed_class_without_a_def_row_is_found() {
        let mut g = good();
        g.defs.retain(|d| d.id != def_id("Foo"));
        only(&g, "class");
        assert!(violations(&g)[0].contains("has no row in defs"));
        assert!(violations(&g)[0].contains("\"Foo\""));
    }

    #[test]
    fn a_def_row_used_by_nothing_is_found() {
        let mut g = good();
        g.defs.push(row("zz", DefKind::Function));
        only(&g, "is used by nothing");
        assert!(violations(&g)[0].contains("\"zz\""));
    }

    #[test]
    #[should_panic(expected = "different graphs")]
    fn a_nondeterministic_builder_is_caught() {
        use std::cell::Cell;
        let n = Cell::new(0);
        assert_deterministic(|| {
            n.set(n.get() + 1);
            let name = format!("f{}", n.get());
            Graph::new(
                vec![],
                vec![row(&name, DefKind::Function)],
                vec![ctx(&name)],
                vec![],
                Stats::default(),
            )
        });
    }
}
