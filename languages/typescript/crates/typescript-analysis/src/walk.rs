//! One body, read once, in the solver's terms. `sem` reads the tree (it is the only file that touches tsrs nodes) and hands back plain data; this file
//! puts it where the core wants it: the sites of a def relative to its start, and for each site the targets, through `resolve(site, &env)` in the final
//! form it has on the first day (`docs/02-tree-rules/03-the-seam.md` §3).

use analyzer_core::graph::{External, Reason, Site};
use analyzer_core::outline::DefKind;
use analyzer_core::solve::{Cancel, SiteCalls, Summary, Target};
use typescript_syntax::MarkerStyle;

use crate::calls::TypeScript;
use crate::env::Env;
use crate::sem::{Func, Init, Resolved, Sem};

/// The reason of an `Unresolved` edge, and the key of its counter in `Stats.language`, for a def whose node is missing from the program's tree: a bug
/// in one of the two parses, which must show on the graph and not as a body with no calls.
const DEF_WITHOUT_NODE: &str = "def_without_node";

/// The keys of the counters, in `Stats.language`, for a construction whose `init` is none **because the chain of base classes could not be followed**
/// and not because no class on it has a constructor. They are not reasons of an `Unresolved` edge (the construction is known, and so is its class), so
/// they are counted apart and `Breakdown` lists them as notes.
pub(crate) const INHERITED_EXTERNAL: &str = "inherited_constructor_external";
pub(crate) const INHERITED_UNKNOWN: &str = "inherited_constructor_unknown";

/// The keys of the counters, in `Stats.language`, for the defs of a walked body that are handed over (written inside an expression, or named) and that no
/// edge of the graph reaches. Counted by `call_graph` once the edges are known; a count and nothing else: no edge and no node is added for them.
pub(crate) const WRITTEN_NOT_CALLED: &str = "def_written_not_called";
pub(crate) const NAMED_NOT_CALLED: &str = "def_named_not_called";

/// Read `f`'s body once: its sites, each with its targets. The solver does the recursion.
pub(crate) fn body<'p>(
    lang: &TypeScript<'p>,
    f: Func<'p>,
    env: &Env,
    cancel: &Cancel,
) -> Summary<TypeScript<'p>> {
    let Some(sites) = sites(lang.sem, f, lang.style) else {
        // Not an empty body: one edge at the def's first byte, so the def shows on the graph and the count is what the edges say (rule 8). The
        // context stays `Finished`: `Status::Incomplete` is for a cost limit (rule 10), and this is a bug in a rule, not a cost.
        return Summary::complete(vec![SiteCalls::new(
            Site::at(0),
            vec![Target::Unresolved(Reason::new(DEF_WITHOUT_NODE))],
        )]);
    };
    let mut found = Vec::with_capacity(sites.len());
    for (index, site) in sites.into_iter().enumerate() {
        if cancel.is_cancelled() {
            return Summary::cut(found);
        }
        found.push(SiteCalls::new(site, resolve(lang, f, index, env)));
    }
    // What the body hands over is a site too (D22): the def's context, at the name, marked passed. It is walked like a call, so what it calls is in the
    // graph, and the edge says it is not one. Placed among the calls by where the name is.
    let handed = lang.sem.handed_over(f, lang.style);
    let start = lang.sem.def(f, lang.style).range.start;
    for held in &handed {
        if let Some(at) = held.at {
            found.push(SiteCalls::passed(
                Site::new(at.start.saturating_sub(start), at.end.saturating_sub(start)),
                vec![Target::Call(held.func, ())],
            ));
        }
    }
    found.sort_by_key(|calls| (calls.site.start, calls.site.end));
    lang.note_handed_over(&handed);
    Summary::complete(found)
}

/// What the site at `site_index` of `f` can reach. In its final form already: ask what was tracked on this path first, and the engine's resolver
/// otherwise. Nothing is tracked yet, so this is the engine's answer.
fn resolve<'p>(
    lang: &TypeScript<'p>,
    f: Func<'p>,
    site_index: usize,
    env: &Env,
) -> Vec<Target<TypeScript<'p>>> {
    resolve_from_env(lang, f, site_index, env).unwrap_or_else(|| {
        lang.sem
            .callee(f, site_index, lang.style)
            .into_iter()
            .map(|resolved| target(lang, resolved))
            .collect()
    })
}

/// What a tracked value says the site calls, if anything is tracked. Nothing is yet: context sensitivity starts by making this return `Some`.
fn resolve_from_env<'p>(
    _lang: &TypeScript<'p>,
    _f: Func<'p>,
    _site_index: usize,
    _env: &Env,
) -> Option<Vec<Target<TypeScript<'p>>>> {
    None
}

/// What the engine said one declaration of a site is, in the solver's terms.
fn target<'p>(lang: &TypeScript<'p>, resolved: Resolved<'p>) -> Target<TypeScript<'p>> {
    match resolved {
        // A class reached by `new` (or by `super(x)`, which names the base class) has nothing to walk as a body: it is a construction, and its `init` is
        // the constructor that runs, its own or the one it inherits. A function reached by `new` is a function: its body is the constructor, so it is a
        // call.
        Resolved::Def(def) if lang.sem.def(def, lang.style).kind == DefKind::Class => {
            let init = match lang.sem.init_of(def, lang.style) {
                Init::Enter(init) => Some((init, ())),
                Init::Nothing => None,
                // no constructor to enter, and it is a limit rather than an answer: say so on the graph (rule 8)
                Init::External => {
                    lang.note(INHERITED_EXTERNAL);
                    None
                }
                Init::Unknown => {
                    lang.note(INHERITED_UNKNOWN);
                    None
                }
            };
            Target::Construct { class: def, init }
        }
        Resolved::Def(def) => Target::Call(def, ()),
        Resolved::External { package, name } => Target::External(External { package, name }),
        Resolved::Unknown(why) => Target::Unresolved(Reason::new(why.counter())),
    }
}

/// The sites of `func`'s body in source order, each a byte range **relative to the start of the def** (`Def::range.start`), so an edit above the
/// def does not move them. `None` for a def with no node in the program's tree (`FileDefs::nodes`: a bug in one of the two parses): the caller says so
/// on the graph and counts it (`def_without_node`) instead of reporting a body with no calls.
fn sites(sem: &Sem, func: Func<'_>, style: MarkerStyle<'_>) -> Option<Vec<Site>> {
    let spans = sem.call_sites(func, style)?;
    let start = sem.def(func, style).range.start;
    Some(
        spans
            .into_iter()
            .map(|span| {
                Site::new(
                    span.start.saturating_sub(start),
                    span.end.saturating_sub(start),
                )
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use typescript_syntax::Placement;

    use super::*;

    const STYLE: MarkerStyle<'static> = MarkerStyle {
        label: "ID",
        placement: Placement::Docstring,
    };

    #[test]
    fn sites_are_relative_to_the_start_of_the_def_and_in_source_order() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sites");
        let sem = Sem::open(&root, Path::new("src/sites.ts")).unwrap();
        let text = std::fs::read_to_string(root.join("src/sites.ts")).unwrap();
        let plain = sem.entry("src/sites.ts", "plain", 0, STYLE).unwrap();
        let start = sem.def(plain, STYLE).range.start;
        let found = sites(&sem, plain, STYLE).unwrap();
        // adding the def's start gives the callee back in the file
        let callees: Vec<&str> = found
            .iter()
            .map(|site| &text[(start + site.start) as usize..(start + site.end) as usize])
            .collect();
        assert_eq!(
            callees,
            ["f", "a.b", "a?.b", "obj.x.y", "a[\"key\"]", "(g)"]
        );
        assert!(found.windows(2).all(|pair| pair[0] < pair[1]), "{found:?}");
        // the first site is inside the body, not at the def's first byte
        assert!(found[0].start > 0);
    }

    #[test]
    fn moving_a_def_does_not_change_its_sites() {
        // the same function under a different amount of text above it: the relative sites are the same
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sites");
        let sem = Sem::open(&root, Path::new("src/sites.ts")).unwrap();
        let at = |qualname: &str| {
            let func = sem.entry("src/sites.ts", qualname, 0, STYLE).unwrap();
            sites(&sem, func, STYLE).unwrap()
        };
        // `Widget.method` and `table.load` both hold exactly `f();` as their body, at different places in the file and under different headers
        let method = at("Widget.method");
        let load = at("table.load");
        assert_eq!(method.len(), 1);
        assert_eq!(load.len(), 1);
        assert_eq!(method[0].end - method[0].start, load[0].end - load[0].start);
    }

    #[test]
    fn the_hand_overs_of_a_body_are_sites_among_its_calls_in_source_order() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/handed-over");
        let sem = Sem::open(&root, Path::new("src/main.ts")).unwrap();
        let both = sem
            .entry("src/main.ts", "constArrowCalled", 0, STYLE)
            .unwrap();
        let lang = TypeScript::new(&sem, STYLE);
        let summary = body(&lang, both, &Env, &Cancel::new());
        // `register(both); both();`: the call of `register`, the hand-over of `both` (the argument), then the call of `both`
        let shape: Vec<bool> = summary.sites.iter().map(|s| s.passed).collect();
        assert_eq!(shape, [false, true, false]);
        let starts: Vec<u32> = summary.sites.iter().map(|s| s.site.start).collect();
        assert!(starts.windows(2).all(|w| w[0] < w[1]), "{starts:?}");
        // the hand-over names one def, as a call target, so the solver walks it
        assert_eq!(summary.sites[1].targets.len(), 1);
        assert!(matches!(summary.sites[1].targets[0], Target::Call(..)));
    }
}
