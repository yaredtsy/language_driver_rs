//! One body, read once, in the solver's terms (RS4 §4). `sem` reads the engine (it is the only module that names it) and hands back plain data; this
//! file puts it where the core wants it. `resolve` is in the final form it has on the first day (`docs/02-tree-rules/03-the-seam.md` §3): ask what was
//! tracked on this path first (`resolve_from_env`, empty), the engine's resolver otherwise. Turning a `Callee` into a `Target` is step 4's, with the
//! `Lang` impl it needs.
#![expect(
    dead_code,
    reason = "RS4 step 4's calls.rs walks bodies; step 3 proves the resolver in sem's snapshot tests and this file's"
)]

use analyzer_core::graph::Site;
use analyzer_core::solve::Cancel;

use crate::env::Env;
use crate::key::Key;
use crate::sem::{Callee, Func, RawSite, Sem, SiteKind};

/// One site of a body, with what it reaches. A site has several callees when a later layer says so; layer 1 has one.
pub(crate) struct Resolved<'p> {
    pub(crate) site: Site,
    pub(crate) kind: SiteKind,
    /// The number of the closure the site is in (`RawSite::closure`): layer 1 ignores it, RC2 reads it.
    pub(crate) closure: Option<u32>,
    pub(crate) callees: Vec<Callee<'p>>,
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

/// Read `f`'s body once: its sites, each with what it reaches. The solver does the recursion.
pub(crate) fn body<'p>(sem: &Sem<'p>, f: Func<'p>, env: &Env, cancel: &Cancel) -> Body<'p> {
    let found = sem.sites(f);
    let mut sites = Vec::with_capacity(found.list.len());
    for site in &found.list {
        if cancel.is_cancelled() {
            return Body {
                sites,
                macro_not_expanded: found.macro_not_expanded,
                cut: true,
            };
        }
        sites.push(Resolved {
            site: site.span(),
            kind: site.kind(),
            closure: site.closure(),
            callees: resolve(sem, f, site, env),
        });
    }
    Body {
        sites,
        macro_not_expanded: found.macro_not_expanded,
        cut: false,
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
        let (loaded, _root) = policy("walk_body");
        attached(&loaded, STYLE, |sem| {
            let f = sem.entry("src/calls.rs", "closures", 0).unwrap();
            let body = body(sem, f, &Env, &Cancel::new());
            assert!(!body.cut);
            // the same sites, in the same order, as step A gave: layer 1 has one callee per site
            let sites = sem.sites(f);
            assert_eq!(body.sites.len(), sites.list.len());
            assert!(
                body.sites
                    .iter()
                    .all(|resolved| resolved.callees.len() == 1)
            );
            let tags: Vec<_> = body.sites.iter().map(|resolved| resolved.closure).collect();
            assert_eq!(tags, [Some(0), Some(1), Some(1), Some(2), None, None]);
            // the seams: nothing is tracked, and every callee is walked under the one key
            for (resolved, site) in body.sites.iter().zip(&sites.list) {
                assert!(resolve_from_env(sem, f, site, &Env).is_none());
                assert_eq!(callee_key(sem, f, site, &Env), ());
                assert_eq!(resolved.site, site.span());
            }
        });
    }

    #[test]
    fn a_cancelled_body_says_so_and_keeps_what_it_read() {
        let (loaded, _root) = policy("walk_cancel");
        attached(&loaded, STYLE, |sem| {
            let f = sem.entry("src/calls.rs", "plain_calls", 0).unwrap();
            let cancel = Cancel::new();
            cancel.cancel();
            let body = body(sem, f, &Env, &cancel);
            assert!(body.cut);
            assert!(body.sites.is_empty());
        });
    }
}
