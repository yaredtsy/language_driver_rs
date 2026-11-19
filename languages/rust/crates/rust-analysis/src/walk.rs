//! One body, read once, in the solver's terms (RS4 §4). `sem` reads the engine (it is the only module that names it) and hands back plain data; this
//! file puts it where the core wants it. `resolve` is in the final form it has on the first day (`docs/02-tree-rules/03-the-seam.md` §3): ask what was
//! tracked on this path first (`resolve_from_env`, empty), the engine's resolver otherwise.

use analyzer_core::graph::{External, Reason, Site};
use analyzer_core::solve::{Cancel, SiteCalls, Summary, Target};

use crate::calls::{self, Rust};
use crate::env::Env;
use crate::key::Key;
use crate::sem::{Callee, Cause, Found, Func, RawSite, Reason as SemReason, Sem};

/// One site of a body, with what it reaches. A site has several callees when a later layer says so; layer 1 has one.
pub(crate) struct Resolved<'p> {
    pub(crate) raw: RawSite,
    pub(crate) callees: Vec<Callee<'p>>,
}

impl Resolved<'_> {
    /// The span, relative to the start of the def.
    pub(crate) fn site(&self) -> Site {
        self.raw.span()
    }
}

/// A body read once.
pub(crate) struct Body<'p> {
    /// In source order, each a span relative to the start of the def.
    pub(crate) sites: Vec<Resolved<'p>>,
    /// Macro calls the engine could not expand (`macro_not_expanded`).
    pub(crate) macro_not_expanded: u32,
    /// The request was cancelled before every site was read: `sites` is what was.
    pub(crate) cut: bool,
}

/// Read `f`'s body once: its sites, each with what it reaches, in `sem`'s terms.
pub(crate) fn read<'p>(sem: &Sem<'p>, f: Func<'p>, env: &Env, cancel: &Cancel) -> Body<'p> {
    #[cfg(test)]
    let started = std::time::Instant::now();
    let found = sem.sites(f);
    #[cfg(test)]
    probe::sites(started.elapsed());
    let mut sites = Vec::with_capacity(found.list.len());
    for site in &found.list {
        let cancelled = cancel.is_cancelled();
        #[cfg(test)]
        probe::check();
        if cancelled {
            return Body {
                sites,
                macro_not_expanded: found.macro_not_expanded,
                cut: true,
            };
        }
        #[cfg(test)]
        let started = std::time::Instant::now();
        let callees = resolve(sem, f, site, env);
        #[cfg(test)]
        probe::resolve(started.elapsed(), site.span());
        sites.push(Resolved {
            raw: site.clone(),
            callees,
        });
    }
    Body {
        sites,
        macro_not_expanded: found.macro_not_expanded,
        cut: false,
    }
}

/// Read `f`'s body once, in the solver's terms: its sites, each with its targets. The solver does the recursion. What the walk meets and does not draw is
/// counted here, once per site: how open an answer is, a call to a def a macro made, a macro that could not be expanded.
pub(crate) fn body<'a, 'p>(
    lang: &Rust<'a, 'p>,
    f: Func<'p>,
    env: &Env,
    cancel: &Cancel,
) -> Summary<Rust<'a, 'p>> {
    let read = read(lang.sem, f, env, cancel);
    lang.note(
        calls::MACRO_NOT_EXPANDED,
        u64::from(read.macro_not_expanded),
    );
    let mut found = Vec::with_capacity(read.sites.len());
    for resolved in &read.sites {
        let passed = resolved.raw.is_passed();
        let mut targets = Vec::with_capacity(resolved.callees.len());
        for callee in &resolved.callees {
            if let Some(open) = callee.open {
                lang.note(open.counter(), 1);
            }
            match (&callee.found, passed) {
                // a def of the project handed over: an edge, walked like a call, that says it is not one (D22)
                (Found::Def(_), _) => {}
                // only a call into project code can be passed: a function outside project code handed over (whatever its cause), and a constructor, have no edge and are counted
                (
                    Found::External {
                        construct: false, ..
                    },
                    true,
                ) => {
                    lang.note(calls::EXTERNAL_PASSED, 1);
                    continue;
                }
                (
                    Found::Construct(_)
                    | Found::External {
                        construct: true, ..
                    },
                    true,
                ) => {
                    lang.note(calls::CONSTRUCTOR_PASSED, 1);
                    continue;
                }
                // a trait's required method handed over (D44): a declaration with no body is no def to hand over, and a hand-over is not a call, so no
                // `Unresolved` edge either. Counted; its `open` was counted above
                (Found::Unknown(SemReason::FunctionTypedValue), true) => {
                    lang.note(calls::SIGNATURE_PASSED, 1);
                    continue;
                }
                // a call to a def a macro made, or one beside the root, is a body that was there and was not walked (rule 8). A construction has no body: a
                // struct has none and Rust has no constructor function, so there was nothing to walk. A hand-over is counted above, not here
                (
                    Found::External {
                        cause,
                        construct: false,
                        ..
                    },
                    false,
                ) => match cause {
                    Cause::MacroFile | Cause::OtherCrate => {
                        lang.note(calls::MACRO_GENERATED_TARGET, 1)
                    }
                    Cause::OutsideRoot => lang.note(calls::OUTSIDE_ROOT, 1),
                    Cause::Dependency => {}
                },
                _ => {}
            }
            targets.push(target(lang, callee, resolved, env));
        }
        // what is handed over has no edge unless it is a def of the project; a site with nothing to say is not drawn. (A def the outline could not name
        // is the one `Unresolved` a hand-over can have, and it is an ordinary edge: only a call into project code can be marked passed.)
        let handed = passed && targets.iter().any(|t| matches!(t, Target::Call(..)));
        if handed {
            found.push(SiteCalls::passed(resolved.site(), targets));
        } else if !targets.is_empty() {
            found.push(SiteCalls::new(resolved.site(), targets));
        }
    }
    if read.cut {
        Summary::cut(found)
    } else {
        Summary::complete(found)
    }
}

/// What the engine said a site reaches, in the solver's terms. The only place a `Target::Call` is built: its key comes from `callee_key`.
#[expect(
    clippy::unit_arg,
    reason = "the seam: `Key` is `()` until RC1 gives a context something to hold, and the call keeps its place"
)]
fn target<'a, 'p>(
    lang: &Rust<'a, 'p>,
    callee: &Callee<'p>,
    resolved: &Resolved<'p>,
    env: &Env,
) -> Target<Rust<'a, 'p>> {
    match &callee.found {
        Found::Def(def) => Target::Call(*def, callee_key(lang.sem, *def, &resolved.raw, env)),
        // Rust has no constructor function: nothing to enter
        Found::Construct(class) => Target::Construct {
            class: *class,
            init: None,
        },
        Found::External { package, name, .. } => Target::External(External {
            package: package.clone(),
            name: name.clone(),
        }),
        Found::Unknown(reason) => Target::Unresolved(Reason::new(reason.counter())),
    }
}

/// What `site` of `f` can reach. In its final form already: what was tracked on this path first, and the engine's resolver otherwise. Nothing is
/// tracked yet, so this is the engine's answer.
pub(crate) fn resolve<'p>(
    sem: &Sem<'p>,
    f: Func<'p>,
    site: &RawSite,
    env: &Env,
) -> Vec<Callee<'p>> {
    resolve_from_env(sem, f, site, env).unwrap_or_else(|| vec![sem.callee(f, site)])
}

/// What a tracked value says the site calls, if anything is tracked. Nothing is yet: context sensitivity starts by making this return `Some`.
fn resolve_from_env<'p>(
    _sem: &Sem<'p>,
    _f: Func<'p>,
    _site: &RawSite,
    _env: &Env,
) -> Option<Vec<Callee<'p>>> {
    None
}

/// Under which key the callee is walked. Every callee is walked under the one key there is.
pub(crate) fn callee_key<'p>(
    _sem: &Sem<'p>,
    _callee: Func<'p>,
    _site: &RawSite,
    _env: &Env,
) -> Key {
    Key::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sem::attached;
    use crate::sem::testing::{STYLE, policy};

    #[test]
    fn a_body_is_its_sites_each_with_its_one_callee_and_the_one_key() {
        let (loaded, _root) = policy();
        attached(&loaded, STYLE, |sem| {
            let f = sem.entry("src/calls.rs", "closures", 0).unwrap();
            let body = read(sem, f, &Env, &Cancel::new());
            assert!(!body.cut);
            // the same sites, in the same order, as step A gave: layer 1 has one callee per site
            let sites = sem.sites(f);
            assert_eq!(body.sites.len(), sites.list.len());
            assert!(
                body.sites
                    .iter()
                    .all(|resolved| resolved.callees.len() == 1)
            );
            let tags: Vec<_> = body
                .sites
                .iter()
                .map(|resolved| resolved.raw.closure())
                .collect();
            assert_eq!(tags, [Some(0), Some(1), Some(1), Some(2), None, None]);
            // the seams: nothing is tracked, and every callee is walked under the one key
            for (resolved, site) in body.sites.iter().zip(&sites.list) {
                assert!(resolve_from_env(sem, f, site, &Env).is_none());
                assert_eq!(callee_key(sem, f, site, &Env), ());
                assert_eq!(resolved.site(), site.span());
            }
        });
    }

    #[test]
    fn a_cancelled_body_says_so_and_keeps_what_it_read() {
        let (loaded, _root) = policy();
        attached(&loaded, STYLE, |sem| {
            let f = sem.entry("src/calls.rs", "plain_calls", 0).unwrap();
            let cancel = Cancel::new();
            cancel.cancel();
            let body = read(sem, f, &Env, &cancel);
            assert!(body.cut);
            assert!(body.sites.is_empty());
        });
    }
}

/// What RS5 §4's measurement needs to see inside a request: how long each engine call between two looks at the cancel takes, and when the looks are. It is
/// off (a test turns it on with `start`) and costs nothing when off; it is compiled only into the tests.
#[cfg(test)]
pub(crate) mod probe {
    use std::cell::RefCell;
    use std::time::{Duration, Instant};

    use analyzer_core::graph::Site;

    use crate::sem::{Func, Sem};

    /// One timed engine call: `sem.sites(f)` or one `resolve`.
    pub(crate) struct Call {
        pub(crate) took: Duration,
        /// The def whose body was being read, and its file.
        pub(crate) def: String,
        pub(crate) file: String,
        /// For a `resolve`: the site, relative to the def.
        pub(crate) site: Option<Site>,
    }

    /// A place where the request looks at its cancel: when, and what was being read after it.
    pub(crate) struct Check {
        pub(crate) at: Instant,
        pub(crate) def: String,
        pub(crate) file: String,
    }

    #[derive(Default)]
    pub(crate) struct Log {
        pub(crate) sites: Vec<Call>,
        pub(crate) resolves: Vec<Call>,
        pub(crate) checks: Vec<Check>,
        /// Places in the request that are not looks at the cancel, to say where a stretch between two looks goes.
        pub(crate) marks: Vec<(Instant, &'static str)>,
        current: (String, String),
    }

    thread_local! {
        static LOG: RefCell<Option<Log>> = const { RefCell::new(None) };
    }

    fn with(f: impl FnOnce(&mut Log)) {
        LOG.with(|log| {
            if let Some(log) = log.borrow_mut().as_mut() {
                f(log);
            }
        });
    }

    /// Turns the probe on, with the start of the request as the first place the cancel is looked at.
    pub(crate) fn start() {
        let log = Log {
            checks: vec![Check {
                at: Instant::now(),
                def: "the start of the request".to_owned(),
                file: String::new(),
            }],
            current: ("the first body".to_owned(), String::new()),
            ..Log::default()
        };
        LOG.with(|slot| *slot.borrow_mut() = Some(log));
    }

    /// Turns it off, with the end of the request as the last place.
    pub(crate) fn finish() -> Log {
        let mut log = LOG
            .with(|slot| slot.borrow_mut().take())
            .unwrap_or_default();
        log.checks.push(Check {
            at: Instant::now(),
            def: "the end of the request".to_owned(),
            file: String::new(),
        });
        log
    }

    /// The solver is about to walk `f`: it has just looked at the cancel, and what follows is `f`'s body.
    pub(crate) fn walk<'p>(sem: &Sem<'p>, f: Func<'p>) {
        let at = Instant::now();
        if LOG.with(|log| log.borrow().is_none()) {
            return;
        }
        let info = sem.describe(f);
        with(|log| {
            log.current = (info.qname.clone(), info.file.clone());
            log.checks.push(Check {
                at,
                def: info.qname,
                file: info.file,
            });
        });
    }

    /// A place in the request, for the report: not a look at the cancel.
    pub(crate) fn mark(what: &'static str) {
        let at = Instant::now();
        with(|log| log.marks.push((at, what)));
    }

    /// The walk looked at the cancel.
    pub(crate) fn check() {
        let at = Instant::now();
        with(|log| {
            let (def, file) = log.current.clone();
            log.checks.push(Check { at, def, file });
        });
    }

    pub(crate) fn sites(took: Duration) {
        with(|log| {
            let (def, file) = log.current.clone();
            log.sites.push(Call {
                took,
                def,
                file,
                site: None,
            });
        });
    }

    pub(crate) fn resolve(took: Duration, site: Site) {
        with(|log| {
            let (def, file) = log.current.clone();
            log.resolves.push(Call {
                took,
                def,
                file,
                site: Some(site),
            });
        });
    }
}
