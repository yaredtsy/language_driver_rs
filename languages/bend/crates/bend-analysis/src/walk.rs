//! One body, in the solver's terms (docs/08-bend/03 §3.2). `sem` reads the text and says where the names, constructors and sugars are, and which names are
//! bound; this file looks each one up in the book and puts the answer where the core wants it. `resolve` is in the final form it has on the first day
//! (`docs/02-tree-rules/03-the-seam.md` §3): ask what was tracked on this path first (`resolve_from_env`, empty), the book otherwise.

use analyzer_core::graph::{External, Reason, Site};
use analyzer_core::outline::DefKind;
use analyzer_core::solve::{Cancel, SiteCalls, Summary, Target};

use crate::book::{DeclId, Resolved};
use crate::calls::{self, Bend};
use crate::env::Env;
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

/// A def of the book as a target: a `Call`, unless its file is outside the root, which is an `External` named by the path from the root (D33), counted.
fn project<'b>(lang: &Bend<'b>, id: DeclId) -> Target<Bend<'b>> {
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
    Target::Call(id, ())
}

/// What a name that is called, or that a sugar calls, resolved to.
fn call_of<'b>(lang: &Bend<'b>, resolved: Resolved, written: &str) -> Target<Bend<'b>> {
    match resolved {
        Resolved::Project(id) => project(lang, id),
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

fn site<'b>(target: Target<Bend<'b>>, passed: bool) -> Reached<'b> {
    Reached::Site {
        targets: vec![target],
        passed,
    }
}

/// The path the default resolver takes for every place of a body, and the one a later layer will answer first from what it tracked (`env`).
fn resolve<'b>(lang: &Bend<'b>, file: DeclId, item: &Item, env: &Env) -> Reached<'b> {
    resolve_from_env(lang, file, item, env).unwrap_or_else(|| resolve_from_book(lang, file, item))
}

/// Nothing is tracked yet. Context sensitivity starts by making this return `Some`.
fn resolve_from_env<'b>(
    _lang: &Bend<'b>,
    _file: DeclId,
    _item: &Item,
    _env: &Env,
) -> Option<Reached<'b>> {
    None
}

fn resolve_from_book<'b>(lang: &Bend<'b>, def: DeclId, item: &Item) -> Reached<'b> {
    let book = lang.book;
    let file = def.file();
    match &item.what {
        What::TypePosition => Reached::Counted(calls::TYPE_POSITION),
        What::BaseFixed => Reached::Counted(calls::SUGAR_BASE),
        What::Data => Reached::Quiet,
        What::Opaque => site(
            Target::Unresolved(Reason::new(calls::FUNCTION_TYPED_VALUE)),
            false,
        ),
        What::Call { name } => site(call_of(lang, book.resolve(file, name), name), false),
        What::Passed { name } => match book.resolve(file, name) {
            Resolved::Project(id) => match book.decl(id) {
                // a type handed over (`App.run(~U32, ..)`, `f(Shape)`) is a type argument: there is no code to hand over
                Some(decl) if decl.kind == DefKind::Enum => Reached::Counted(calls::TYPE_PASSED),
                // a def outside the root handed over has no edge, as a hub's or Base's has none: only a call into project code can be passed
                Some(_) if book.file(id.file()).is_none_or(|f| !f.in_root) => {
                    Reached::Counted(calls::EXTERNAL_PASSED)
                }
                Some(_) => site(project(lang, id), true),
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
                _ => site(project(lang, class), false),
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
                other => site(call_of(lang, other, written), false),
            }
        }
    }
}

/// Every place of `f`'s body with what it comes to, in source order, with its absolute span in the file.
pub(crate) fn reach<'b>(lang: &Bend<'b>, f: DeclId, env: &Env) -> Vec<(Item, Reached<'b>)> {
    lang.items(f)
        .into_iter()
        .map(|item| {
            let reached = resolve(lang, f, &item, env);
            (item, reached)
        })
        .collect()
}

/// Read `f`'s body once, in the solver's terms: its sites, each with its targets. The solver does the recursion. What the walk meets and does not draw is
/// counted here, once per place: a type skipped, Base's sugar, a name handed over below the arguments.
pub(crate) fn body<'b>(
    lang: &Bend<'b>,
    f: DeclId,
    env: &Env,
    cancel: &Cancel,
) -> Summary<Bend<'b>> {
    // a site is a span relative to the start of the def, so an edit above it moves nothing
    let start = lang.book.def(f).map_or(0, |d| d.range.start);
    let mut sites = Vec::new();
    for item in lang.items(f) {
        if cancel.is_cancelled() {
            return Summary::cut(sites);
        }
        match resolve(lang, f, &item, env) {
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
