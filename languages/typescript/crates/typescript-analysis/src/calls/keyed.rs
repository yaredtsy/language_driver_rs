//! The key a call passes (`docs/03-typescript/12-context-layer/06-tc2-functions-through-parameters.md` §5), proved in the crate: the layer is turned on with
//! `with_layer(Some(Caps::PARAMS))` because `Caps::BUILT` is still empty, so `Precision::Precise` makes no key yet. The layer draws nothing new, so every edge is
//! the engine's and only the contexts split.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use analyzer_core::graph::{ContextId, EdgeTo, Graph};
use analyzer_core::solve::{Budget, Cancel, Lang, Limits, Solver, Target};

use super::*;

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: analyzer_core::edit::Placement::Docstring,
};

const ZEROS: [u8; 16] = [0; 16];
const A: [u8; 16] = [
    93, 46, 87, 96, 216, 61, 155, 17, 211, 86, 34, 118, 74, 224, 161, 128,
];
const B: [u8; 16] = [
    172, 21, 36, 62, 234, 108, 51, 149, 129, 38, 137, 144, 112, 95, 31, 68,
];
const HANDLER: [u8; 16] = [
    63, 169, 7, 244, 195, 117, 63, 232, 189, 253, 184, 232, 224, 101, 115, 200,
];
const OTHER: [u8; 16] = [
    182, 165, 153, 246, 90, 12, 30, 220, 138, 89, 219, 215, 130, 124, 131, 66,
];
const UNKNOWN_A: [u8; 16] = [
    64, 110, 214, 97, 245, 134, 207, 177, 242, 253, 50, 72, 57, 215, 243, 92,
];
const ANON: [u8; 16] = [
    227, 59, 111, 114, 81, 178, 31, 219, 156, 77, 31, 52, 186, 66, 183, 107,
];
const UNDEFINED: [u8; 16] = [
    96, 15, 54, 81, 114, 34, 137, 20, 173, 89, 117, 222, 99, 2, 190, 64,
];
const A_OR_B: [u8; 16] = [
    55, 225, 1, 109, 241, 243, 47, 117, 120, 129, 24, 217, 191, 92, 235, 3,
];

const FIXTURES: [&str; 3] = [
    "tests/fixtures/context/callback-two-callers",
    "tests/fixtures/context/callback-passed-through",
    "tests/fixtures/keys",
];

fn open(fixture: &str) -> Sem {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture);
    Sem::open(&root, Path::new("src/main.ts")).unwrap()
}

/// The graph under `main`, with the layer as `layer` says, and the solver that made it (its language remembers what it walked).
fn solve<'p>(sem: &'p Sem, layer: Option<Caps>) -> (Solver<TypeScript<'p>>, Graph) {
    let lang = TypeScript::new(sem, STYLE).with_layer(layer);
    let main = sem.entry("src/main.ts", "main", 0, STYLE).unwrap();
    let start = start_of(&lang, main);
    let mut solver = Solver::new(lang, Limits::default());
    let graph = solver.graph_within(&[start], &Budget::default(), &Cancel::new());
    (solver, graph)
}

fn on() -> Option<Caps> {
    Some(Caps::PARAMS)
}

/// Each def's qualname and the sorted digests of its contexts.
fn contexts(graph: &Graph) -> BTreeMap<String, Vec<[u8; 16]>> {
    let mut found: BTreeMap<String, Vec<[u8; 16]>> = BTreeMap::new();
    for context in &graph.contexts {
        let name = graph.def(&context.id.func).unwrap().qname.clone();
        found.entry(name).or_default().push(context.id.digest);
    }
    for digests in found.values_mut() {
        digests.sort();
    }
    found
}

/// What `contexts` should say, from `(qname, digests)` pairs written as in the design (each list sorted here, as arrays).
fn expected(pairs: &[(&str, &[[u8; 16]])], zeros_for: &[&str]) -> BTreeMap<String, Vec<[u8; 16]>> {
    let mut found: BTreeMap<String, Vec<[u8; 16]>> = BTreeMap::new();
    for (name, digests) in pairs {
        let mut digests = digests.to_vec();
        digests.sort();
        found.insert((*name).to_owned(), digests);
    }
    for name in zeros_for {
        found.insert((*name).to_owned(), vec![ZEROS]);
    }
    found
}

/// An edge's target with its digest set aside.
fn target_without_digest(to: &EdgeTo) -> String {
    match to {
        EdgeTo::Context(id, basis) => format!("call {:?} {basis:?}", id.func),
        EdgeTo::Construct { class, init, basis } => format!(
            "construct {class:?} {:?} {basis:?}",
            init.as_ref().map(|id| &id.func)
        ),
        EdgeTo::External(external, basis) => format!("{external:?} {basis:?}"),
        EdgeTo::Unresolved(reason) => format!("unresolved {}", reason.as_str()),
        other => format!("{other:?}"),
    }
}

/// The edges of a graph, each with its digests set aside, once each.
fn edges_without_digests(graph: &Graph) -> BTreeSet<String> {
    graph
        .edges
        .iter()
        .map(|edge| {
            format!(
                "{:?} {:?} {} {}",
                edge.from.func,
                edge.site,
                edge.passed,
                target_without_digest(&edge.to)
            )
        })
        .collect()
}

#[test]
fn two_callers_make_two_contexts_and_no_new_edge() {
    let sem = open("tests/fixtures/context/callback-two-callers");
    let (_, with) = solve(&sem, on());
    let (_, without) = solve(&sem, None);
    assert_eq!(
        contexts(&with),
        expected(
            &[("run", &[A, B])],
            &["main", "first", "second", "again", "a", "b"]
        )
    );
    assert_eq!(with.contexts.len(), 8);
    assert_eq!(without.contexts.len(), 7);
    assert_eq!(
        edges_without_digests(&with),
        edges_without_digests(&without)
    );
}

#[test]
fn a_key_passes_through_a_parameter() {
    let sem = open("tests/fixtures/context/callback-passed-through");
    let (_, with) = solve(&sem, on());
    let (_, without) = solve(&sem, None);
    assert_eq!(
        contexts(&with),
        expected(
            &[("relay", &[HANDLER, OTHER]), ("each", &[HANDLER, OTHER])],
            &["main", "direct", "handler", "other"]
        )
    );
    assert_eq!(
        edges_without_digests(&with),
        edges_without_digests(&without)
    );
}

#[test]
fn each_kind_of_argument_makes_its_key() {
    let sem = open("tests/fixtures/keys");
    let (_, with) = solve(&sem, on());
    let (_, without) = solve(&sem, None);
    let found = contexts(&with);
    let mut want = expected(
        &[
            // the spread, the conditional, the two arrows sharing one
            ("run", &[ZEROS, A_OR_B, ANON]),
            ("opt", &[UNDEFINED]),
            ("withDefault", &[UNDEFINED]),
            // a rest parameter
            ("many", &[ZEROS]),
            // a `string` first
            ("second", &[UNKNOWN_A]),
            // a tagged template passes nothing
            ("tagged", &[ZEROS]),
            // `new Box(a)` and `Pair`'s `super(cb)` share `A`; `new Sub(b)` runs the inherited constructor with `B`
            ("Box.constructor", &[A, B]),
            ("Pair.constructor", &[UNKNOWN_A]),
        ],
        &[],
    );
    for name in found.keys() {
        want.entry(name.clone()).or_insert_with(|| vec![ZEROS]);
    }
    assert_eq!(found, want);
    assert_eq!(
        edges_without_digests(&with),
        edges_without_digests(&without)
    );
}

#[test]
fn the_bit_off_is_the_layer_that_knows_nothing() {
    for fixture in FIXTURES {
        let sem = open(fixture);
        let (_, none) = solve(&sem, Some(Caps::empty()));
        let (_, off) = solve(&sem, None);
        assert_eq!(none, off, "{fixture}");
    }
}

/// A target of the graph, written so that a graph's and a summary's can be compared.
fn written_edge(to: &EdgeTo) -> String {
    match to {
        EdgeTo::Context(id, _) => format!("call {:?} {:?}", id.func, id.digest),
        EdgeTo::Construct { class, init, .. } => format!(
            "construct {class:?} {:?}",
            init.as_ref().map(|id| (&id.func, id.digest))
        ),
        EdgeTo::External(external, _) => {
            format!("external {} {}", external.package, external.name)
        }
        EdgeTo::Unresolved(reason) => format!("unresolved {}", reason.as_str()),
        other => panic!("an edge this test does not know: {other:?}"),
    }
}

#[test]
fn each_context_walked_alone_has_the_edges_of_the_graph() {
    for fixture in FIXTURES {
        let sem = open(fixture);
        let (solver, graph) = solve(&sem, on());
        let walked = &solver.lang().walked;
        assert_eq!(walked.len(), graph.contexts.len(), "{fixture}");
        for (func, key) in walked {
            let digest = solver.lang().digest(key);
            let mut id = ContextId::of(sem.describe(*func, STYLE).id);
            id.digest = digest;
            let mut in_graph: Vec<String> = graph
                .edges_from(&id)
                .iter()
                .map(|edge| format!("{:?} {} {}", edge.site, edge.passed, written_edge(&edge.to)))
                .collect();
            in_graph.sort();

            // a fresh language: nothing it remembers is the solver's
            let mut fresh = TypeScript::new(&sem, STYLE).with_layer(on());
            let data = solver.lang().keys.borrow().data(*key).clone();
            let again = fresh.intern_key(data);
            assert_eq!(fresh.digest(&again), digest, "{fixture}");
            let summary = Lang::walk(&mut fresh, *func, &again, &Cancel::new());
            let name = |func: Func<'_>| sem.describe(func, STYLE).id;
            let digest_of = |key: &KeyId| fresh.digest(key);
            let mut alone: Vec<String> = Vec::new();
            for site in &summary.sites {
                for target in &site.targets {
                    let (passed, text) = match target {
                        Target::Call(func, key) => (
                            site.passed,
                            format!("call {:?} {:?}", name(*func), digest_of(key)),
                        ),
                        Target::Construct { class, init } => (
                            false,
                            format!(
                                "construct {:?} {:?}",
                                name(*class),
                                init.as_ref()
                                    .map(|(init, key)| (name(*init), digest_of(key)))
                            ),
                        ),
                        Target::External(external) => (
                            false,
                            format!("external {} {}", external.package, external.name),
                        ),
                        Target::Unresolved(reason) => {
                            (false, format!("unresolved {}", reason.as_str()))
                        }
                        _ => panic!("a target this test does not know"),
                    };
                    alone.push(format!("{:?} {passed} {text}", site.site));
                }
            }
            alone.sort();
            assert_eq!(
                in_graph,
                alone,
                "{fixture}: {:?} under {digest:?}",
                graph.def(&id.func).map(|def| &def.qname)
            );
        }
    }
}
