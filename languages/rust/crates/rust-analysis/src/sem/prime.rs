//! Priming (RS5 §4): after a load, the project's own crates are made ready, so that the first request does not pay for the engine's first look at them.
//!
//! The engine is lazy: `Sem::entry` turns the entry's syntax into an engine def, which builds the crate's module tree and name resolution, and the standard
//! library's with it. Done in the first request that is 2.3 s on `core/` with no look at the cancel flag in it. Done here, it is part of the slow call
//! (`open`, `reload`) and the request is fast.
//!
//! Only the crates whose root file is under the project root are primed (project code, as `Loaded::files` says): a session rooted at one crate of a
//! workspace of fifty primes that crate, and not its siblings, which `Crate::all` also calls local.

use ra_ap_hir::{Crate, attach_db};

use super::load::Loaded;

impl Loaded {
    /// Primes the project's own crates. Takes seconds; call it once after a load.
    pub(crate) fn prime(&mut self) {
        let primed = attach_db(self.db(), || {
            let db = self.db();
            let mut locals: Vec<(String, Crate)> = Crate::all(db)
                .into_iter()
                .filter(|krate| krate.origin(db).is_local())
                .filter(|krate| {
                    self.root_relative(self.vfs().file_path(krate.root_file(db)))
                        .is_some()
                })
                .map(|krate| {
                    let name = krate
                        .display_name(db)
                        .map_or_else(String::new, |name| name.to_string());
                    (name, krate)
                })
                .collect();
            // rule 9: the order the engine lists crates in is not ours to rely on
            locals.sort_by(|a, b| a.0.cmp(&b.0));
            prime_crates(db, &locals);
            locals.into_iter().map(|(name, _)| name).collect::<Vec<_>>()
        });
        self.set_primed(primed);
    }
}

/// Builds each crate's module tree and name resolution (`Crate::modules` asks for the crate's def map), and with it the standard library's, which a def
/// map needs. Measured against `ide_db::prime_caches::parallel_prime_caches` over the same crates (RS5 step 5b, `ENGINE.md`): both bring `Sem::entry` of a
/// cold database under 100 ms (about 4 ms); this walk is the cheaper (a load and its priming 2.4 s against 3.1 s on `core/`, 440 MB after against
/// 490 MB), because the engine's call also builds trait impls and symbol indexes that a request for a call graph does not ask for.
fn prime_crates(db: &ra_ap_ide_db::RootDatabase, locals: &[(String, Crate)]) {
    for (_, krate) in locals {
        let _ = krate.modules(db);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::sem::common::{copy_of_fixture, one_load};
    use crate::{LoadOptions, Project};

    /// The crates primed by a project rooted at `root` (a folder of the copy), opened from `file`.
    fn primed(copy: &Path, root: &str, file: &str) -> Vec<String> {
        let root = crate::paths::absolute(&copy.join(root)).unwrap();
        let mut project =
            Project::open(&root, Path::new(file), &LoadOptions::default()).expect("it loads");
        project.prime();
        project.loaded().primed().to_vec()
    }

    /// `fixtures/owner/`: `ws/` is a workspace of `app` and `member`, and `app` depends on `inside`, a path dependency next to `ws/`. All three are `Local`
    /// to the engine. Rooted at `ws/app` only `app` is project code, so only `app` is primed; rooted over the whole fixture, all three are.
    #[test]
    fn only_the_crates_under_the_root_are_primed() {
        let _load = one_load();
        let copy = copy_of_fixture("owner", "prime_owner");
        assert_eq!(primed(&copy, "ws/app", "src/lib.rs"), ["app"]);
        assert_eq!(
            primed(&copy, "", "ws/app/src/lib.rs"),
            ["app", "inside", "member"]
        );
        assert_eq!(
            primed(&copy, "ws/member", "src/lib.rs"),
            ["member"],
            "and rooted at the other member, the other one"
        );
    }
}
