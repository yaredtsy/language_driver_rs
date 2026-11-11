//! Checks on a [`Graph`] that hold for every language.

use std::collections::HashSet;

use analyzer_core::graph::{ContextId, EdgeTo, Graph};

/// Everything wrong with `graph`, one sentence each. Empty means the graph is well formed:
/// sorted, no duplicates, no edge or entry pointing at a context the graph does not hold.
pub fn violations(graph: &Graph) -> Vec<String> {
    let mut out = Vec::new();
    if !graph.contexts.windows(2).all(|w| w[0].id < w[1].id) {
        out.push("contexts are not sorted by id, or an id appears twice".to_owned());
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
            EdgeTo::Context(id) => Some(id),
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
    use analyzer_core::graph::{Context, ContextId, Edge, Site, StableDef, Stats};
    use analyzer_core::outline::DefKind;
    use analyzer_core::text::Span;

    fn id(n: &str) -> ContextId {
        ContextId::of(StableDef::Marked(n.to_owned()))
    }

    fn ctx(n: &str) -> Context {
        Context::new(id(n), n, DefKind::Function, "f", Span::new(0, 1))
    }

    fn edge(from: &str, to: &str) -> Edge {
        Edge {
            from: id(from),
            site: Site::at(0),
            to: EdgeTo::Context(id(to)),
        }
    }

    #[test]
    fn a_good_graph_has_no_violations() {
        let g = Graph::new(
            vec![id("a")],
            vec![ctx("a"), ctx("b")],
            vec![edge("a", "b")],
            Stats::default(),
        );
        assert_eq!(violations(&g), Vec::<String>::new());
    }

    #[test]
    fn dangling_and_missing_entries_are_found() {
        let g = Graph::new(
            vec![id("zz")],
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
        let mut g = Graph::new(vec![], vec![ctx("a"), ctx("b")], vec![], Stats::default());
        g.contexts.reverse();
        assert!(violations(&g)[0].contains("not sorted"));
    }

    #[test]
    #[should_panic(expected = "different graphs")]
    fn a_nondeterministic_builder_is_caught() {
        use std::cell::Cell;
        let n = Cell::new(0);
        assert_deterministic(|| {
            n.set(n.get() + 1);
            Graph::new(
                vec![],
                vec![ctx(&format!("f{}", n.get()))],
                vec![],
                Stats::default(),
            )
        });
    }
}
