//! Two properties of graphs made with the context layer (`docs/03-typescript/12-context-layer/03-design.md` §8): `never_fewer_edges` (rules 4 and 5, a pair
//! of graphs, the layer off and on) and `a_context_is_a_function_of_its_key` (a set of graphs). Each returns the problems it found, one sentence each; empty
//! means it holds. They take graphs, so any test can run them on any pair. Rule 7, "off is identical" (the graph with the bit off is the graph without the
//! layer), is not here: it is held by the `-- off` halves of the snapshots and by `the_bit_off_is_the_layer_that_knows_nothing` (`calls/keyed.rs`).

use std::collections::BTreeMap;

use typescript_analysis::{EdgeTo, Graph, Site, StableDef, Status};

/// A target with the key set aside: what the layer may not change of an answer that is not unresolved.
fn shape(to: &EdgeTo) -> Option<String> {
    match to {
        EdgeTo::Context(id, _) => Some(format!("context {:?}", id.func)),
        EdgeTo::Construct { class, init, .. } => Some(format!(
            "construct {class:?} init {:?}",
            init.as_ref().map(|init| &init.func)
        )),
        EdgeTo::External(external, _) => {
            Some(format!("external {} {}", external.package, external.name))
        }
        EdgeTo::Unresolved(_) => None,
        _ => unreachable!("an edge kind this test does not know"),
    }
}

/// Rules 4 and 5 of `docs/03-typescript/12-context-layer/03-design.md` §8: the layer may replace an unresolved answer, never drop one.
/// (a) every edge of `default` that is not `Unresolved` has an edge in `precise` with the same `from.func`, the same `site`, the same `passed`
///     and the same target with the key set aside (a context: its `func`; a construction: its `class` and its `init`'s `func`; an external: its
///     package and name);
/// (b) every `(from.func, site)` that has an edge in `default` has at least one edge in `precise`, with one allowance (`06-tc2` §10, step 5a; step 0b of
///     `08-tc4` §0): a site whose callee the layer knows to be only `undefined` has none. The walk lists such sites (`emptied`, of the graph `precise`:
///     `typescript_analysis::testing`). A site with no edge in `precise` is allowed only when every edge it had in `default` was `Unresolved` **and**
///     it is in `emptied`; otherwise it is reported;
/// (c) a listed site where `default` has an edge that is not `Unresolved` is reported: the layer emptied a site the engine had an answer for.
pub(crate) fn never_fewer_edges(
    default: &Graph,
    precise: &Graph,
    emptied: &[(StableDef, Site)],
) -> Vec<String> {
    let name = |graph: &Graph, id: &typescript_analysis::StableDef| {
        graph
            .def(id)
            .map_or_else(|| format!("{id:?}"), |def| def.qname.clone())
    };
    let mut kept: BTreeMap<String, ()> = BTreeMap::new();
    let mut sites: BTreeMap<String, ()> = BTreeMap::new();
    for edge in &precise.edges {
        sites.insert(format!("{:?} {:?}", edge.from.func, edge.site), ());
        if let Some(to) = shape(&edge.to) {
            kept.insert(
                format!("{:?} {:?} {} {to}", edge.from.func, edge.site, edge.passed),
                (),
            );
        }
    }
    let listed: BTreeMap<String, ()> = emptied
        .iter()
        .map(|(func, site)| (format!("{func:?} {site:?}"), ()))
        .collect();
    let mut found = Vec::new();
    // the sites of `default` with no edge in `precise`: each with the text to report, and whether every edge it had was unresolved
    let mut lost: BTreeMap<String, (String, bool)> = BTreeMap::new();
    for edge in &default.edges {
        let at = format!("{:?} {:?}", edge.from.func, edge.site);
        if let Some(to) = shape(&edge.to)
            && !kept.contains_key(&format!("{at} {} {to}", edge.passed))
        {
            found.push(format!(
                "an edge of `{}` at {:?} ({to}) is not in the graph with the layer on",
                name(default, &edge.from.func),
                edge.site
            ));
        }
        if !sites.contains_key(&at) {
            let entry = lost.entry(at).or_insert_with(|| {
                (
                    format!(
                        "the site {:?} of `{}` has no edge in the graph with the layer on",
                        edge.site,
                        name(default, &edge.from.func)
                    ),
                    true,
                )
            });
            entry.1 &= matches!(edge.to, EdgeTo::Unresolved(_));
        }
    }
    for (at, (report, only_unresolved)) in lost {
        if !(only_unresolved && listed.contains_key(&at)) {
            found.push(report);
        }
    }
    // (c) each listed site where `default` has an answer
    for (func, site) in emptied {
        let answered = default.edges.iter().any(|edge| {
            edge.from.func == *func
                && edge.site == *site
                && !matches!(edge.to, EdgeTo::Unresolved(_))
        });
        if answered {
            found.push(format!(
                "the layer emptied the site {site:?} of `{}`, where the graph with the layer off has an answer",
                name(default, func)
            ));
        }
    }
    found
}

/// A context is a function of its key: a context id that is `Status::Finished` in two or more of `graphs` has the same edges leaving it in
/// each (compared as the sorted `format!("{:?} {} {:?}", edge.site, edge.passed, edge.to)` of the edges whose `from` is that id).
pub(crate) fn a_context_is_a_function_of_its_key(graphs: &[&Graph]) -> Vec<String> {
    // the context's id, then for each graph that finished it: the graph's place and the sorted edges that leave it
    type Seen = Vec<(usize, Vec<String>)>;
    let mut by_context: BTreeMap<String, (String, Seen)> = BTreeMap::new();
    for (place, graph) in graphs.iter().enumerate() {
        for context in graph
            .contexts
            .iter()
            .filter(|context| matches!(context.status, Status::Finished))
        {
            let mut edges: Vec<String> = graph
                .edges
                .iter()
                .filter(|edge| edge.from == context.id)
                .map(|edge| format!("{:?} {} {:?}", edge.site, edge.passed, edge.to))
                .collect();
            edges.sort();
            let shown = graph
                .def(&context.id.func)
                .map_or_else(|| format!("{:?}", context.id.func), |def| def.qname.clone());
            by_context
                .entry(format!("{:?}", context.id))
                .or_insert_with(|| (shown, Vec::new()))
                .1
                .push((place, edges));
        }
    }
    by_context
        .into_values()
        .filter_map(|(shown, seen)| {
            let (first, edges) = seen.first()?;
            let other = seen.iter().find(|(_, other)| other != edges)?;
            Some(format!(
                "the context of `{shown}` has different edges in graph {first} and in graph {}",
                other.0
            ))
        })
        .collect()
}
