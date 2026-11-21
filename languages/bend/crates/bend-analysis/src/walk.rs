//! One body, in the solver's terms (docs/08-bend/03 §3.2). `sem` reads the text and says where the names, constructors and sugars are, and which names are
//! bound; this file looks each one up in the book and puts the answer where the core wants it. `resolve` is in the final form it has on the first day
//! (`docs/02-tree-rules/03-the-seam.md` §3): ask what was tracked on this path first (`resolve_from_env`, empty), the book otherwise.

use analyzer_core::graph::{External, Reason, Site};
use analyzer_core::outline::DefKind;
use analyzer_core::solve::{Cancel, SiteCalls, Summary, Target};

use crate::book::{DeclId, Resolved};
use crate::calls::{self, Bend, Precision};
use crate::env::Env;
use crate::eval;
use crate::key::{self, Key, Known};
use crate::sem::{Item, Sugar, What};

/// What `External`s of Base's are named by: its package.
const BASE: &str = "Base";

/// What one place of a body comes to.
pub(crate) enum Reached<'b> {
    /// Neither a site nor a count: what Bend makes of a literal.
    Quiet,
    /// No site, one more of this counter.
    Counted(&'static str),
    /// A site: what it reaches, and whether that is handed over and not called (D22).
    Site {
        targets: Vec<Target<Bend<'b>>>,
        passed: bool,
    },
}

fn is_law(kind: &DefKind) -> bool {
    matches!(kind, DefKind::Other(k) if k == "law")
}

/// A def of the book as a target: a `Call` under `key`, unless its file is outside the root, which is an `External` named by the path from the root (D33), counted.
pub(crate) fn project<'b>(lang: &Bend<'b>, id: DeclId, key: Key) -> Target<Bend<'b>> {
    let (Some(file), Some(def), Some(decl)) = (
        lang.book.file(id.file()),
        lang.book.def(id),
        lang.book.decl(id),
    ) else {
        return Target::Unresolved(Reason::new(calls::UNKNOWN_NAME));
    };
    if !file.in_root {
        lang.note(calls::OUTSIDE_ROOT, 1);
        return Target::External(External {
            package: file.relative.clone(),
            name: def.qualname.clone(),
        });
    }
    // the table means a law only when it has no proof: a call to it is a call to a def with no code, which the tree draws as a leaf
    if is_law(&decl.kind) {
        lang.note(calls::OPEN_LAW, 1);
    }
    Target::Call(id, key)
}

/// What a name that is called, or that a sugar calls, resolved to; a project def is called under `key`.
pub(crate) fn call_of<'b>(
    lang: &Bend<'b>,
    resolved: Resolved,
    written: &str,
    key: Key,
) -> Target<Bend<'b>> {
    match resolved {
        Resolved::Project(id) => project(lang, id, key),
        Resolved::Base => Target::External(External {
            package: BASE.to_owned(),
            name: written.to_owned(),
        }),
        Resolved::Hub(alias) => Target::External(External {
            package: alias,
            name: written.to_owned(),
        }),
        Resolved::Unknown => Target::Unresolved(Reason::new(calls::UNKNOWN_NAME)),
    }
}

pub(crate) fn site<'b>(target: Target<Bend<'b>>, passed: bool) -> Reached<'b> {
    Reached::Site {
        targets: vec![target],
        passed,
    }
}

/// The path the default resolver takes for every place of a body, and the one a later layer will answer first from what it tracked (`env`).
fn resolve<'b>(lang: &Bend<'b>, file: DeclId, item: &Item, env: &Env<'_>) -> Reached<'b> {
    resolve_from_env(lang, file, item, env)
        .unwrap_or_else(|| resolve_from_book(lang, file, item, env))
}

/// What this context's key says about a call through one of the def's own parameters (06 §5): the def a caller named is called, under the key its own
/// arguments make; a lambda is called where it is written, so the site says why it has no target. Anything else is the book's answer: a name the book fixes is never
/// overridden, and a parameter nobody named is `Unresolved(function_typed_value)` as before. Off under `Precision::Default`.
fn resolve_from_env<'b>(
    lang: &Bend<'b>,
    def: DeclId,
    item: &Item,
    env: &Env<'_>,
) -> Option<Reached<'b>> {
    if lang.precision == Precision::Default {
        return None;
    }
    let What::Opaque {
        param: Some(at),
        args,
    } = &item.what
    else {
        return None;
    };
    Some(match env.known(*at)? {
        Known::Def(callee) => site(
            project(lang, *callee, key::key_of(lang, def, *callee, args, env)),
            false,
        ),
        Known::Lambda { .. } => site(
            Target::Unresolved(Reason::new(calls::ANONYMOUS_FUNCTION_CALLED)),
            false,
        ),
    })
}

pub(crate) fn resolve_from_book<'b>(
    lang: &Bend<'b>,
    def: DeclId,
    item: &Item,
    env: &Env<'_>,
) -> Reached<'b> {
    let book = lang.book;
    let file = def.file();
    match &item.what {
        What::TypePosition => Reached::Counted(calls::TYPE_POSITION),
        What::BaseFixed => Reached::Counted(calls::SUGAR_BASE),
        What::Data => Reached::Quiet,
        What::Opaque { .. } => site(
            Target::Unresolved(Reason::new(calls::FUNCTION_TYPED_VALUE)),
            false,
        ),
        What::Call { name, args } => {
            let resolved = book.resolve(file, name);
            // the callee's own key, from the arguments (the open key under `Precision::Default`)
            let key = match &resolved {
                Resolved::Project(id) => key::key_of(lang, def, *id, args, env),
                _ => Key::open(),
            };
            site(call_of(lang, resolved, name, key), false)
        }
        What::Passed { name } => match book.resolve(file, name) {
            Resolved::Project(id) => match book.decl(id) {
                // a type handed over (`App.run(~U32, ..)`, `f(Shape)`) is a type argument: there is no code to hand over
                Some(decl) if decl.kind == DefKind::Enum => Reached::Counted(calls::TYPE_PASSED),
                // a def outside the root handed over has no edge, as a hub's or Base's has none: only a call into project code can be passed
                Some(_) if book.file(id.file()).is_none_or(|f| !f.in_root) => {
                    Reached::Counted(calls::EXTERNAL_PASSED)
                }
                // the open key: nobody knows what the receiver will pass it (BC-D4)
                Some(_) => site(project(lang, id, Key::open()), true),
                None => Reached::Quiet,
            },
            _ => Reached::Quiet,
        },
        What::Deep { name } => match book.resolve(file, name) {
            Resolved::Project(_) => Reached::Counted(calls::PASSED_DEEP),
            _ => Reached::Quiet,
        },
        What::Ctr { name } => match book.ctor(file, name) {
            Resolved::Project(class) => match book.file(class.file()) {
                Some(f) if f.in_root => site(Target::Construct { class, init: None }, false),
                _ => site(project(lang, class, Key::open()), false),
            },
            Resolved::Base => Reached::Quiet,
            Resolved::Hub(alias) => site(
                Target::External(External {
                    package: alias,
                    name: name.clone(),
                }),
                false,
            ),
            Resolved::Unknown => site(
                Target::Unresolved(Reason::new(calls::UNKNOWN_CONSTRUCTOR)),
                false,
            ),
        },
        What::Sugar(sugar) => {
            let (resolved, written) = match sugar {
                Sugar::Own(name) => (book.resolve_own(file, name), name.as_str()),
                Sugar::Named(name) => (book.resolve(file, name), name.as_str()),
                Sugar::Bound => {
                    return site(
                        Target::Unresolved(Reason::new(calls::FUNCTION_TYPED_VALUE)),
                        false,
                    );
                }
                Sugar::Unknown => {
                    return site(Target::Unresolved(Reason::new(calls::UNKNOWN_NAME)), false);
                }
            };
            match resolved {
                // Base's own arithmetic, indexing and do blocks are the language's, not call sites (BD-D8)
                Resolved::Base => Reached::Counted(calls::SUGAR_BASE),
                // a sugar's target gets the open key (BC-D5)
                other => site(call_of(lang, other, written, Key::open()), false),
            }
        }
    }
}

/// Every place of `f`'s body with what it comes to, in source order, with its absolute span in the file.
pub(crate) fn reach<'b>(lang: &Bend<'b>, f: DeclId, env: &Env<'_>) -> Vec<(Item, Reached<'b>)> {
    lang.items(f)
        .into_iter()
        .map(|item| {
            let reached = resolve(lang, f, &item, env);
            (item, reached)
        })
        .collect()
}

/// Read `f`'s body once, in the solver's terms: its sites, each with its targets. The solver does the recursion. What the walk meets and does not draw is
/// counted here, once per place: a type skipped, Base's sugar, a name handed over below the arguments. Once per def, not once per context: the first time a def
/// is walked to the end it counts, and under another key it does not.
pub(crate) fn body<'b>(
    lang: &Bend<'b>,
    f: DeclId,
    env: &Env<'_>,
    cancel: &Cancel,
) -> Summary<Bend<'b>> {
    lang.mute(lang.walked_before(f));
    let summary = sites_of(lang, f, env, cancel);
    lang.mute(false);
    if summary.complete {
        lang.done(f);
    }
    summary
}

fn sites_of<'b>(lang: &Bend<'b>, f: DeclId, env: &Env<'_>, cancel: &Cancel) -> Summary<Bend<'b>> {
    // a site is a span relative to the start of the def, so an edit above it moves nothing
    let start = lang.book.def(f).map_or(0, |d| d.range.start);
    // under `Precise` the body is evaluated, and it gives what each item comes to; an item that is not a node (a type skipped, Base's `&&`) is the item walk's. Under
    // `Default` every item is the item walk's, as layer 1 has always had it
    let mut evaluated = match lang.precision {
        Precision::Default => Vec::new(),
        Precision::Precise => eval::reached(lang, f, env),
    };
    let mut sites = Vec::new();
    for (at, item) in lang.items(f).into_iter().enumerate() {
        if cancel.is_cancelled() {
            return Summary::cut(sites);
        }
        let from_eval = evaluated.get_mut(at).and_then(Option::take);
        let reached = match from_eval {
            Some(reached) => reached,
            None if lang.precision == Precision::Default => resolve(lang, f, &item, env),
            None => {
                debug_assert!(
                    matches!(item.what, What::TypePosition | What::BaseFixed | What::Data),
                    "a site no node of the flow carries: {:?}",
                    item.what
                );
                resolve_from_book(lang, f, &item, env)
            }
        };
        match reached {
            Reached::Quiet => {}
            Reached::Counted(key) => lang.note(key, 1),
            Reached::Site { targets, passed } => {
                let at = Site::new(
                    item.span.start.saturating_sub(start),
                    item.span.end.saturating_sub(start),
                );
                sites.push(if passed {
                    SiteCalls::passed(at, targets)
                } else {
                    SiteCalls::new(at, targets)
                });
            }
        }
    }
    Summary::complete(sites)
}
