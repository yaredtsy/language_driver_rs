//! Three properties of a pair of graphs, one with the context layer off and one with it on (`docs/03-typescript/12-context-layer/03-design.md` §8). Each
//! returns the problems it found, one sentence each; empty means it holds. They take graphs, so any test can run them on any pair.

use std::collections::BTreeMap;
use std::path::Path;

use typescript_analysis::{EdgeTo, Graph, Status};

use super::edges_as_text;

/// Rule 7: the graph with the layer on is the graph with it off. When not: the edge lines (`edges_as_text`) only one of them has, each as
/// `only off: …` / `only on: …`, and `the stats differ` when they do; `the graphs differ elsewhere` when neither explains it.
pub(crate) fn off_is_identical(off: &Graph, on: &Graph, root: &Path) -> Vec<String> {
    if off == on {
        return Vec::new();
    }
    let mut found = Vec::new();
    let (off_lines, on_lines) = (edges_as_text(off, root), edges_as_text(on, root));
    for line in only_in(&off_lines, &on_lines) {
        found.push(format!("only off: {line}"));
    }
    for line in only_in(&on_lines, &off_lines) {
        found.push(format!("only on: {line}"));
    }
    if off.stats != on.stats {
        found.push("the stats differ".to_owned());
    }
    if found.is_empty() {
        found.push("the graphs differ elsewhere".to_owned());
    }
    found
}

/// The lines of `a` that `b` has fewer of, a line that is there twice counting twice.
fn only_in(a: &[String], b: &[String]) -> Vec<String> {
    let mut left: BTreeMap<&str, i64> = BTreeMap::new();
    for line in a {
        *left.entry(line).or_default() += 1;
    }
    for line in b {
        *left.entry(line).or_default() -= 1;
    }
    left.into_iter()
        .flat_map(|(line, count)| std::iter::repeat_n(line.to_owned(), count.max(0) as usize))
        .collect()
}

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
/// (b) every `(from.func, site)` that has an edge in `default` has at least one edge in `precise`, with one allowance (`06-tc2` §10, step 5a): a site
///     whose callee the layer knows to be only `undefined` has none, and the layer says so (`Stats.language["callee_undefined"]`, per walk, so an upper
///     bound). At most that many sites may have lost every edge, and only when each edge they had in `default` was `Unresolved`. A site that lost an
///     edge that was an answer is always reported.
pub(crate) fn never_fewer_edges(default: &Graph, precise: &Graph) -> Vec<String> {
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
    let mut allowed = precise
        .stats
        .language
        .get("callee_undefined")
        .copied()
        .unwrap_or(0);
    for (report, only_unresolved) in lost.into_values() {
        if only_unresolved && allowed > 0 {
            allowed -= 1;
        } else {
            found.push(report);
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
