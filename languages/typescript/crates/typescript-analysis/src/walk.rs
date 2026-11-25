//! One body, read once, in the solver's terms. `sem` reads the tree (it is the only file that touches tsrs nodes) and hands back plain data; this file
//! puts it where the core wants it: the sites of a def relative to its start, and for each site the targets, through `resolve(site, env)` in the final
//! form it has on the first day (`docs/02-tree-rules/03-the-seam.md` §3).

use analyzer_core::graph::{External, Reason, Site};
use analyzer_core::outline::DefKind;
use analyzer_core::solve::{Cancel, SiteCalls, Summary, Target};
use typescript_syntax::MarkerStyle;

use crate::calls::TypeScript;
use crate::caps::Caps;
use crate::env::{Env, VARIABLE_ADDED_TO_ENGINE, VARIABLE_ASSIGNED_MORE_THAN_ONCE};
use crate::eval::{World, eval};
use crate::flow::Expr;
use crate::key::{Key, KeyId, bind};
use crate::sem::{Fits, Func, Init, Resolved, Sem, UNHANDLED_KEY, Uncovered, Why, counted};
use crate::value::{One, Value};

/// The reason of an `Unresolved` edge, and the key of its counter in `Stats.language`, for a def whose node is missing from the program's tree: a bug
/// in one of the two parses, which must show on the graph and not as a body with no calls.
const DEF_WITHOUT_NODE: &str = "def_without_node";

/// The keys of the counters, in `Stats.language`, for a construction whose `init` is none **because the chain of base classes could not be followed**
/// and not because no class on it has a constructor. They are not reasons of an `Unresolved` edge (the construction is known, and so is its class), so
/// they are counted apart and `Breakdown` lists them as notes.
pub(crate) const INHERITED_EXTERNAL: &str = "inherited_constructor_external";
pub(crate) const INHERITED_UNKNOWN: &str = "inherited_constructor_unknown";

/// The key of the counter, in `Stats.language`, for a place where a body hands over a signature that has no body (`use(w.write)` with `w` an interface): no
/// edge, nothing to enter (D43). The key Rust uses for the same thing (D44).
pub(crate) const SIGNATURE_PASSED: &str = "signature_passed";

/// The key of the counter, in `Stats.language`, for a call into a source file outside the root (D33): an `External` edge for a body that was there to walk.
pub(crate) const OUTSIDE_ROOT: &str = "outside_root";

/// The key of the counter, in `Stats.language`, for a call whose callee, in a context, is only `undefined` (a parameter nobody passed): no edge is drawn,
/// and the counter says that is why the site has none (rule 8).
pub(crate) const CALLEE_UNDEFINED: &str = "callee_undefined";

/// The key of the counter, in `Stats.language`, for a function the layer drew at the call of a value (`06-tc2` §10, step 5c) that the checker says does **not** fit
/// the callee's type, where no type parameter is in scope. On a project that compiles it must be 0: one there is a bug of the layer (a wrong binding). The
/// engine's answer for the site stays beside the edge, so a wrong edge never stands alone.
pub(crate) const VALUE_TYPE_MISMATCH: &str = "value_type_mismatch";

/// The key of the counter, in `Stats.language`, for a function the layer drew at the call of a value that the checker could not check: inside generic code a
/// correct function is not assignable to a type that names a type parameter, so there the layer is believed. The share the tripwire cannot see.
pub(crate) const VALUE_TYPE_UNCHECKED: &str = "value_type_unchecked";

/// The key of the counter, in `Stats.language`, for a def of the graph whose marker another def of its file owns (D34): named by where it is, counted once.
pub(crate) const DUPLICATE_MARKER: &str = "duplicate_marker";

/// The keys of the counters, in `Stats.language`, for the defs of a walked body that are handed over (written inside an expression, or named) and that no
/// edge of the graph reaches. Counted by `call_graph` once the edges are known; a count and nothing else: no edge and no node is added for them.
pub(crate) const WRITTEN_NOT_CALLED: &str = "def_written_not_called";
pub(crate) const NAMED_NOT_CALLED: &str = "def_named_not_called";

/// The key of the counter, in `Stats.language`, for a call of a function a library made (`setOpen` from `useState`, D56): drawn as an `External` named after the
/// call that made it, and counted beside the edge so how it is drawn is decided in one place (`target`).
pub(crate) const LIBRARY_MADE: &str = "library_made_function";

/// Read `f`'s body once: its sites, each with its targets. The solver does the recursion. Says (`engine_kind_unhandled`) how many nodes of a kind the engine's
/// accessors do not handle were met where one was expected: while reading the body, and while resolving what it calls and hands over.
pub(crate) fn body<'p>(
    lang: &TypeScript<'p>,
    f: Func<'p>,
    env: Option<&Env>,
    cancel: &Cancel,
) -> Summary<TypeScript<'p>> {
    let (summary, unhandled) = counted(|| body_of(lang, f, env, cancel));
    lang.note_body_n(
        UNHANDLED_KEY,
        unhandled + lang.sem.unhandled_in_body(f, lang.style),
    );
    summary
}

fn body_of<'p>(
    lang: &TypeScript<'p>,
    f: Func<'p>,
    env: Option<&Env>,
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
    let hand_overs = lang.sem.handed_over(f, lang.style);
    let start = lang.sem.range_of(f, lang.style).start;
    for held in &hand_overs.handed {
        if let Some(at) = held.at {
            found.push(SiteCalls::passed(
                Site::new(at.start.saturating_sub(start), at.end.saturating_sub(start)),
                // a hand-over passes nothing it knows yet: TC3 gives it what the def captures
                vec![Target::Call(held.func, KeyId::OPEN)],
            ));
        }
    }
    found.sort_by_key(|calls| (calls.site.start, calls.site.end));
    lang.note_handed_over(&hand_overs.handed);
    lang.note_uncovered(&hand_overs.uncovered);
    lang.note_body_n(SIGNATURE_PASSED, hand_overs.signatures_passed);
    Summary::complete(found)
}

/// What the site at `site_index` of `f` can reach. In its final form already: the engine answers first, always; then, when the layer knows `PARAMS`, it
/// may speak (`resolve_from_env`): where the engine's answer is a value (a function-typed parameter, an untyped one) and the layer knows which functions
/// the value holds, those functions are the targets. Each call into project code then carries the key its arguments make (`key_of`), made here and
/// nowhere else.
fn resolve<'p>(
    lang: &TypeScript<'p>,
    f: Func<'p>,
    site_index: usize,
    env: Option<&Env>,
) -> Vec<Target<TypeScript<'p>>> {
    let (found, facts) = lang.sem.callee_with_facts(f, site_index, lang.style);
    if facts.getter_result {
        lang.note_uncovered(&[Uncovered::GetterResultCall]);
    }
    for _ in 0..facts.outside_root {
        lang.note_body(OUTSIDE_ROOT);
    }
    // the ceiling of step 2b: a site that reads a variable with more than one source
    if let Some(env) = env
        && lang.knows(Caps::LOCALS)
        && site_reads_many_sources(env, site_index)
    {
        lang.note_walk(VARIABLE_ASSIGNED_MORE_THAN_ONCE);
    }
    let spoken = match env {
        Some(env) if lang.knows(Caps::PARAMS) => {
            resolve_from_env(lang, env, f, site_index, &found, facts.jsx)
        }
        _ => None,
    };
    // The split of `function_typed_value` by where the function comes from: one note for each such edge **that is drawn**, so the four add up to the
    // total. When the layer replaced an open entry by the functions it holds, the entry is not an edge and is not counted.
    let (targets, open_drawn) = match spoken {
        Some(answer) => (answer.targets, answer.open_drawn),
        None => (
            found
                .iter()
                .cloned()
                .map(|resolved| target(lang, resolved, facts.jsx))
                .collect(),
            true,
        ),
    };
    if open_drawn && let Some(origin) = facts.function_typed {
        for _ in found
            .iter()
            .filter(|resolved| **resolved == Resolved::Unknown(Why::FunctionTypedValue))
        {
            lang.note_walk(origin.counter());
        }
    }
    match env {
        Some(env) if lang.knows(Caps::PARAMS) => targets
            .into_iter()
            .map(|target| keyed(lang, env, site_index, target))
            .collect(),
        _ => targets,
    }
}

/// `target` with the key its call passes (`06-tc2` §5): a call, and the constructor a construction runs, get `key_of`; anything else is as it was.
fn keyed<'p>(
    lang: &TypeScript<'p>,
    env: &Env,
    site_index: usize,
    target: Target<TypeScript<'p>>,
) -> Target<TypeScript<'p>> {
    match target {
        Target::Call(func, _) => Target::Call(func, key_of(lang, env, site_index, func)),
        Target::Construct {
            class,
            init: Some((init, _)),
        } => Target::Construct {
            class,
            init: Some((init, key_of(lang, env, site_index, init))),
        },
        other => other,
    }
}

/// The key the site at `site_index` passes to `callee`: its arguments, evaluated in this body's env, bound to the callee's parameters (`key::bind`). The
/// open key when the site's arguments were not read (`args: None`: a tagged template, a decorator, an accessor) or the callee has no table. A JSX
/// element's one argument is `Unknown` until TC4, so it passes the open key with no rule of its own.
fn key_of<'p>(lang: &TypeScript<'p>, env: &Env, site_index: usize, callee: Func<'p>) -> KeyId {
    let Some(args) = env
        .flow
        .sites
        .get(site_index)
        .and_then(|site| site.args.as_deref())
    else {
        return KeyId::OPEN;
    };
    let Some(flow) = World::flow(lang, lang.sem.def_id(callee)) else {
        return KeyId::OPEN;
    };
    let data = bind(&flow.params, args, |expr| eval(lang, env, expr));
    #[cfg(test)]
    let data = lang.measured(lang.sem.def_id(callee), &flow, data);
    lang.intern_key(data)
}

/// What the layer says a site calls: the targets, and whether the engine's open entries are among them (so `resolve` counts them).
struct Spoken<'p> {
    targets: Vec<Target<TypeScript<'p>>>,
    open_drawn: bool,
}

/// Whether the engine's answer for one declaration depends on a value: a function-typed value or an untyped one. The only answers a value can replace
/// (`03-design.md` §7); a `NoDeclaration` on a call's result has no value before TC6.
fn is_open_entry(resolved: &Resolved<'_>) -> bool {
    matches!(
        resolved,
        Resolved::Unknown(Why::FunctionTypedValue | Why::AnyOrUntyped)
    )
}

/// What the targets for the members of a value are (`members`), and what they leave to the caller.
struct Members<'p> {
    targets: Vec<Target<TypeScript<'p>>>,
    /// A member could not be turned into a function: the value stays open at the site.
    open: bool,
    /// `Undefined` was a member (a parameter nobody passed): the caller says whether that is a note (`callee_undefined`), since only a site that has no callee
    /// because of it can say so.
    saw_undefined: bool,
    /// The checker says a function of the value does not fit the callee's type: one mark is due for the site (D61).
    mismatch: bool,
}

/// The targets for each member of `value` at the site `site_index` of `f`, **the one place** that makes them, so the two ways the layer speaks (replacing an
/// open entry, adding beside a named function) cannot drift apart: a function is a call (the tripwire is asked first: the checker agrees it fits the callee's
/// type, or could not be asked inside generic code, or it does not fit and the mark is due), an anonymous function is `anonymous_function_called`, `undefined`
/// is reported, not noted (`Members::saw_undefined`: only the open-entry path notes `callee_undefined`, the new path never). A function in `already` (what the engine calls at this site) is skipped before the tripwire is asked.
fn members<'p>(
    lang: &TypeScript<'p>,
    f: Func<'p>,
    site_index: usize,
    value: &Value,
    already: &[Func<'p>],
) -> Members<'p> {
    let mut found = Members {
        targets: Vec::new(),
        open: false,
        saw_undefined: false,
        mismatch: false,
    };
    for one in value.known() {
        // no `_` arm: a kind of member a later phase adds is decided here before this compiles
        match one {
            One::Fn(def) => match lang.sem.func(*def) {
                Some(func) if already.contains(&func) => {}
                Some(func) => {
                    // the tripwire: the checker agrees that the function fits the callee's type, or the layer says it could not ask (inside generic code).
                    // A failure is drawn, counted, and marked at the site with its own reason (D61); it does not open the value.
                    match lang.sem.fits(f, site_index, func, lang.style) {
                        Fits::Yes => {}
                        Fits::Unchecked => lang.note_walk(VALUE_TYPE_UNCHECKED),
                        Fits::No => {
                            lang.note_walk(VALUE_TYPE_MISMATCH);
                            found.mismatch = true;
                        }
                    }
                    found.targets.push(Target::Call(func, KeyId::OPEN));
                }
                None => found.open = true,
            },
            // an arrow is `AnonAny` in the body of a callee, where its key holds it
            One::Anon(..) | One::AnonAny => found.targets.push(Target::Unresolved(Reason::new(
                Why::AnonymousFunctionCalled.counter(),
            ))),
            One::Undefined => found.saw_undefined = true,
        }
    }
    found
}

/// Whether `expr` is a name the layer tracks (a variable, or a parameter: a parameter is a variable too, D59), or has one among the members of its `OneOf`. A def
/// or an anonymous function written as the callee is not a name: the engine's answer for it is the whole answer.
fn holds_name(expr: &Expr) -> bool {
    match expr {
        Expr::Var(_) | Expr::Param(_) => true,
        Expr::OneOf(members) => members.iter().any(holds_name),
        Expr::Unknown | Expr::Data | Expr::Def(_) | Expr::Anon(_) => false,
    }
}

/// Whether `expr` reads a row of this body's variables that has two or more sources: its assignments, and what was passed when it is a parameter. A read is a
/// `Var` of this def, or a parameter the body writes. Only a capable row counts.
fn reads_many_sources(env: &Env, expr: &Expr) -> bool {
    let row = match expr {
        Expr::Var(id) if id.def == env.def => env.flow.var_index(id.at),
        Expr::Param(index) => env
            .flow
            .params
            .get(*index as usize)
            .filter(|param| param.assigned)
            .and_then(|_| env.flow.param_var(*index)),
        Expr::OneOf(members) => {
            return members.iter().any(|member| reads_many_sources(env, member));
        }
        Expr::Var(_) | Expr::Unknown | Expr::Data | Expr::Def(_) | Expr::Anon(_) => None,
    };
    row.map(|row| &env.flow.vars[row]).is_some_and(|var| {
        var.capable && var.assigned.len() + usize::from(var.param.is_some()) >= 2
    })
}

/// Whether the callee or an argument of the site reads a variable with more than one source (`reads_many_sources`).
fn site_reads_many_sources(env: &Env, site_index: usize) -> bool {
    let Some(site) = env.flow.sites.get(site_index) else {
        return false;
    };
    reads_many_sources(env, &site.callee)
        || site
            .args
            .iter()
            .flat_map(|args| args.iter())
            .any(|arg| reads_many_sources(env, &arg.value))
}

/// What the context layer says the site at `site_index` calls, given the engine's answer `found`; `None` when it does not speak, and then the engine's
/// answer stands as it was. Two ways to speak, both only where the callee's value has a known member (`06-tc2` §10, step 5a):
///
/// - **`found` has an open entry**: the targets are the engine's entries that are not open, then one for each member of the value (`members`), then one
///   `value_type_mismatch` mark if due (D61), then the open entries again if the value is open (a write we cannot read, a way in we do not track).
/// - **`found` has none, and the layer knows `LOCALS`, and the callee is a variable or a parameter** (`07-tc3` §3, D59): the engine named a function through its
///   type and the value may hold more. The targets are **all** the engine's, then one for each member it does not already call, then the mark if due. The
///   layer adds and never removes; if nothing is added, the engine's answer stands.
fn resolve_from_env<'p>(
    lang: &TypeScript<'p>,
    env: &Env,
    f: Func<'p>,
    site_index: usize,
    found: &[Resolved<'p>],
    jsx: bool,
) -> Option<Spoken<'p>> {
    let site = env.flow.sites.get(site_index)?;
    if !found.iter().any(is_open_entry) {
        if !lang.knows(Caps::LOCALS) || !holds_name(&site.callee) {
            return None;
        }
        let value = eval(lang, env, &site.callee);
        if value.known().is_empty() {
            return None;
        }
        // the functions the engine already calls here (a class is a construction, not a call)
        let already: Vec<Func<'p>> = found
            .iter()
            .filter_map(|resolved| match resolved {
                Resolved::Def(def) if lang.sem.kind_of(*def, lang.style) != DefKind::Class => {
                    Some(*def)
                }
                _ => None,
            })
            .collect();
        let added = members(lang, f, site_index, &value, &already);
        if added.targets.is_empty() && !added.mismatch {
            return None;
        }
        lang.note_walk(VARIABLE_ADDED_TO_ENGINE);
        // the engine's answer first, all of it; `target` is asked only now, since it counts what it draws
        let mut targets: Vec<Target<TypeScript<'p>>> = found
            .iter()
            .cloned()
            .map(|resolved| target(lang, resolved, jsx))
            .collect();
        targets.extend(added.targets);
        if added.mismatch {
            targets.push(Target::Unresolved(Reason::new(VALUE_TYPE_MISMATCH)));
        }
        return Some(Spoken {
            targets,
            open_drawn: true,
        });
    }
    let value = eval(lang, env, &site.callee);
    if value.known().is_empty() {
        return None;
    }
    let mut targets: Vec<Target<TypeScript<'p>>> = found
        .iter()
        .filter(|resolved| !is_open_entry(resolved))
        .cloned()
        .map(|resolved| target(lang, resolved, jsx))
        .collect();
    // a member that cannot be turned into a function leaves the value open at this site
    let made = members(lang, f, site_index, &value, &[]);
    let open = value.is_open() || made.open;
    if made.saw_undefined {
        lang.note_walk(CALLEE_UNDEFINED);
    }
    targets.extend(made.targets);
    if made.mismatch {
        // one mark for the site, however many of the value's functions failed
        targets.push(Target::Unresolved(Reason::new(VALUE_TYPE_MISMATCH)));
    }
    if open {
        targets.extend(
            found
                .iter()
                .filter(|resolved| is_open_entry(resolved))
                .cloned()
                .map(|resolved| target(lang, resolved, jsx)),
        );
    }
    Some(Spoken {
        targets,
        open_drawn: open,
    })
}

/// What the engine said one declaration of a site is, in the solver's terms.
fn target<'p>(lang: &TypeScript<'p>, resolved: Resolved<'p>, jsx: bool) -> Target<TypeScript<'p>> {
    match resolved {
        // A class reached by `new` (or by `super(x)`, which names the base class) has nothing to walk as a body: it is a construction, and its `init` is
        // the constructor that runs, its own or the one it inherits. A function reached by `new` is a function: its body is the constructor, so it is a
        // call.
        Resolved::Def(def) if lang.sem.kind_of(def, lang.style) == DefKind::Class => {
            let (class, init) = construction(lang, def, jsx);
            Target::Construct { class, init }
        }
        Resolved::Def(def) => Target::Call(def, KeyId::OPEN),
        Resolved::External { package, name, .. } => Target::External(External { package, name }),
        // D56: **the one place** that decides how a function a library made is drawn. Today an `External` named after the call that made it and the path to
        // the name (`useState()[1]`); a different display later is a change of these lines alone.
        Resolved::LibraryMade {
            package,
            maker,
            path,
        } => {
            lang.note_body(LIBRARY_MADE);
            Target::External(External {
                package,
                name: format!("{maker}{path}"),
            })
        }
        Resolved::Unknown(why) => Target::Unresolved(Reason::new(why.counter())),
    }
}

/// What constructing `class` is: the class, and the constructor that runs, its own or the one it inherits (`None` when there is none to enter). Counts what
/// the construction runs and does not draw, and the limits that left `init` empty. A `new C()` site and a class taken as an entry are the same construction,
/// so both come here and have the same tree and the same counters.
pub(crate) fn construction<'p>(
    lang: &TypeScript<'p>,
    class: Func<'p>,
    jsx: bool,
) -> (Func<'p>, Option<(Func<'p>, Key)>) {
    let (init, chain) = lang.sem.init_chain(class, lang.style);
    // what the construction runs and does not draw: the instance field initializers of each class it goes through, and a class component's `render`
    let undrawn: usize = chain
        .iter()
        .map(|class| lang.sem.initializer_sites(*class, lang.style).len())
        .sum();
    lang.note_uncovered(&vec![Uncovered::ConstructInitializerSite; undrawn]);
    if jsx && lang.sem.has_render(class, lang.style) {
        lang.note_uncovered(&[Uncovered::ClassComponentRender]);
    }
    let init = match init {
        Init::Enter(init) => Some((init, KeyId::OPEN)),
        Init::Nothing => None,
        // no constructor to enter, and it is a limit rather than an answer: say so on the graph (rule 8)
        Init::External => {
            lang.note_body(INHERITED_EXTERNAL);
            None
        }
        Init::Unknown => {
            lang.note_body(INHERITED_UNKNOWN);
            None
        }
    };
    (class, init)
}

/// The sites of `func`'s body in source order, each a byte range **relative to the start of the def** (`Def::range.start`), so an edit above the
/// def does not move them. `None` for a def with no node in the program's tree (`FileDefs::nodes`: a bug in one of the two parses): the caller says so
/// on the graph and counts it (`def_without_node`) instead of reporting a body with no calls.
fn sites(sem: &Sem, func: Func<'_>, style: MarkerStyle<'_>) -> Option<Vec<Site>> {
    let spans = sem.call_sites(func, style)?;
    let start = sem.range_of(func, style).start;
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
        let start = sem.range_of(plain, STYLE).start;
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
        let summary = body(&lang, both, None, &Cancel::new());
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
