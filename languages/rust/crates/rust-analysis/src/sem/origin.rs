//! Who owns a def: project code, or external (RS3 §4). Owned data comes out; no engine type does.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "RS4's call sites ask this; RS3 step 3 proves it in the tests below"
    )
)]

use ra_ap_hir::{AsAssocItem, AssocItemContainer, Function, HasSource, Module, attach_db};

use super::load::Loaded;

/// Who owns a def. Everything the engine knows about a def that is not the user's own code is `External`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Owner {
    /// Written in a file under the root, in a crate of the project: the file, relative to the root with `/`.
    Project { file: String },
    /// `package` is the crate's name (`alloc`, `serde`, `member`), `name` the path of the def in it (`alloc::string::String::len`).
    External { package: String, name: String },
}

/// §4 as a table. `local` is `CrateOrigin::Local`: a workspace member, or a path dependency. `relative` is where the def is written: `Some(file)`
/// only for a def that is in the text (not inside a macro expansion: that has no place in any file) and in a file under the root. The other origins
/// (`Library`: registry and git; `Lang`: `std`, `core`, `alloc`; `Rustc`) are never project code, even when their files sit under the root (a
/// vendored crate's do).
fn project_file(local: bool, relative: Option<String>) -> Option<String> {
    if local { relative } else { None }
}

impl Loaded {
    /// The owner of a function (a call's target, RS4 §3), found inside `attach_db` as every use of the engine's `hir` is.
    pub(super) fn owner_of_function(&self, function: Function) -> Owner {
        attach_db(self.db(), || {
            let db = self.db();
            let module = function.module(db);
            // Where the def is written. `file_id()` is `None` for a macro file: the def is not in the text. A def with no source (a derived impl's
            // methods have none) is nowhere either.
            let relative = function
                .source(db)
                .and_then(|source| source.file_id.file_id())
                .and_then(|file| self.root_relative(self.vfs().file_path(file.file_id(db))));
            let mut path = module_path(db, module);
            match function.as_assoc_item(db).map(|item| item.container(db)) {
                Some(AssocItemContainer::Trait(t)) => path.push(t.name(db).as_str().to_owned()),
                Some(AssocItemContainer::Impl(i)) => {
                    if let Some(adt) = i.self_ty(db).as_adt() {
                        path.push(adt.name(db).as_str().to_owned());
                    }
                }
                None => {}
            }
            path.push(function.name(db).as_str().to_owned());
            self.owner(module, relative, path)
        })
    }

    /// `module`'s crate decides `local`; `relative` is where the def is written, under the root (`project_file`).
    fn owner(&self, module: Module, relative: Option<String>, path: Vec<String>) -> Owner {
        let db = self.db();
        let krate = module.krate(db);
        match project_file(krate.origin(db).is_local(), relative) {
            Some(file) => Owner::Project { file },
            None => {
                // a crate with no name (a `rust-project.json` crate may have none): the engine has nothing to call it by
                let package = krate
                    .display_name(db)
                    .map_or_else(|| "unnamed".to_owned(), |name| name.to_string());
                Owner::External {
                    name: format!("{package}::{}", path.join("::")),
                    package,
                }
            }
        }
    }
}

/// The names of the modules from the crate's root down to `module`, the crate's own name left out.
fn module_path(db: &ra_ap_ide_db::RootDatabase, module: Module) -> Vec<String> {
    let mut names: Vec<String> = module
        .path_to_root(db)
        .into_iter()
        .filter_map(|module| module.name(db))
        .map(|name| name.as_str().to_owned())
        .collect();
    names.reverse();
    names
}

// the helpers of the integration tests, which are one place for "copy a fixture under target/" (that file says `#![allow(dead_code)]`: each program
// uses some of it). A unit test cannot `use` code from `tests/`, so the file is included by path.
#[cfg(test)]
#[path = "../../tests/common/mod.rs"]
mod common;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ra_ap_hir::{CallableKind, Semantics};
    use ra_ap_syntax::{AstNode, ast};

    use super::common;
    use super::*;
    use crate::LoadOptions;
    use crate::sem::load;

    #[test]
    fn what_is_project_code_row_by_row() {
        let file = || Some("a/b.rs".to_owned());
        // a Local crate, with the def written in a file under the root: project code
        assert_eq!(project_file(true, file()).as_deref(), Some("a/b.rs"));
        // a Local crate, but no such file: outside the root (a path dependency), or inside a macro expansion
        assert_eq!(project_file(true, None), None);
        // Library, Lang and Rustc are `local == false`: external whatever else is true, a vendored crate under the root included
        assert_eq!(project_file(false, file()), None);
        assert_eq!(project_file(false, None), None);
    }

    /// Every call written in `file` (relative to the root), as (the text of the callee's name, who owns the target), in source order. Only calls that
    /// resolve to a function: this is the part of RS4's site walk that the fixture needs.
    fn call_owners(loaded: &Loaded, file: &str) -> Vec<(String, Owner)> {
        attach_db(loaded.db(), || {
            let sema = Semantics::new(loaded.db());
            let (file_id, _) = loaded
                .vfs()
                .iter()
                .find(|(_, path)| loaded.root_relative(path).as_deref() == Some(file))
                .unwrap_or_else(|| panic!("{file} is not in the load"));
            let editioned = sema.attach_first_edition_opt(file_id).unwrap();
            let tree = sema.parse(editioned);
            let mut out = Vec::new();
            for node in tree.syntax().descendants() {
                let function = if let Some(call) = ast::CallExpr::cast(node.clone()) {
                    let callee = call.expr().unwrap();
                    match sema.resolve_expr_as_callable(&callee).map(|c| c.kind()) {
                        Some(CallableKind::Function(f)) => {
                            Some((callee.syntax().text().to_string(), f))
                        }
                        _ => None,
                    }
                } else if let Some(call) = ast::MethodCallExpr::cast(node) {
                    // the answer is a function or a field of function type; `left()` is the function
                    sema.resolve_method_call_fallback(&call)
                        .and_then(|(function, _)| function.left())
                        .map(|f| (call.name_ref().unwrap().text().to_string(), f))
                } else {
                    None
                };
                if let Some((text, f)) = function {
                    out.push((text, loaded.owner_of_function(f)));
                }
            }
            out
        })
    }

    fn project(file: &str) -> Owner {
        Owner::Project {
            file: file.to_owned(),
        }
    }

    fn external(package: &str, name: &str) -> Owner {
        Owner::External {
            package: package.to_owned(),
            name: name.to_owned(),
        }
    }

    fn open(root: &Path, manifest: &Path) -> Loaded {
        open_with(root, manifest, &LoadOptions::default())
    }

    fn open_with(root: &Path, manifest: &Path, opts: &LoadOptions) -> Loaded {
        let root = crate::paths::absolute(root).unwrap();
        let manifest = crate::paths::absolute(manifest).unwrap();
        load(&root, &manifest, opts).expect("the fixture loads")
    }

    /// `fixtures/owner/`: `ws/` is a workspace of `app` and `member`; `app` also depends on `inside`, a path dependency next to `ws/`; `member`
    /// has a function a `macro_rules!` makes. `app::run` calls each of them, a `String` function and a `String` method.
    #[test]
    fn owners_row_by_row_with_the_root_over_the_whole_fixture() {
        let copy = common::copy_of_fixture("owner", "owner_whole");
        let loaded = open(&copy, &copy.join("ws/app/Cargo.toml"));
        let owners = call_owners(&loaded, "ws/app/src/lib.rs");
        assert_eq!(
            owners,
            [
                ("local".to_owned(), project("ws/app/src/lib.rs")),
                // a workspace member
                (
                    "member::in_member".to_owned(),
                    project("ws/member/src/lib.rs")
                ),
                // a def a macro made: in no file, so external, named by its own crate
                (
                    "member::generated".to_owned(),
                    external("member", "member::generated")
                ),
                // a path dependency inside the root
                ("inside::in_inside".to_owned(), project("inside/src/lib.rs")),
                // std: `Lang` origin, whatever the root
                (
                    "String::new".to_owned(),
                    external("alloc", "alloc::string::String::new")
                ),
                (
                    "len".to_owned(),
                    external("alloc", "alloc::string::String::len")
                ),
            ]
        );
    }

    #[test]
    fn a_path_dependency_outside_the_root_is_external() {
        let copy = common::copy_of_fixture("owner", "owner_app");
        let app = copy.join("ws/app");
        let loaded = open(&app, &app.join("Cargo.toml"));
        let owners = call_owners(&loaded, "src/lib.rs");
        assert_eq!(
            owners,
            [
                ("local".to_owned(), project("src/lib.rs")),
                // `member` is a member of the workspace and `inside` a path dependency, and both are `Local`; neither file is under the root
                (
                    "member::in_member".to_owned(),
                    external("member", "member::in_member")
                ),
                (
                    "member::generated".to_owned(),
                    external("member", "member::generated")
                ),
                (
                    "inside::in_inside".to_owned(),
                    external("inside", "inside::in_inside")
                ),
                (
                    "String::new".to_owned(),
                    external("alloc", "alloc::string::String::new")
                ),
                (
                    "len".to_owned(),
                    external("alloc", "alloc::string::String::len")
                ),
            ]
        );
    }

    /// §3, build scripts: what a `build.rs` writes into `OUT_DIR` and the crate `include!`s is not there while they are off, and is there when on.
    #[test]
    fn a_build_script_adds_the_code_it_writes_only_when_asked() {
        let copy = common::copy_of_fixture("scripts", "scripts_effect");
        let manifest = copy.join("Cargo.toml");

        let off = open(&copy, &manifest);
        assert_eq!(call_owners(&off, "src/lib.rs"), []);

        let on = open_with(
            &copy,
            &manifest,
            &LoadOptions {
                build_scripts: true,
                proc_macros: false,
            },
        );
        // written by an `include!`, so in no file of the text: external, named by its own crate
        assert_eq!(
            call_owners(&on, "src/lib.rs"),
            [(
                "generated".to_owned(),
                external("scripts", "scripts::generated")
            )]
        );
    }

    /// §3, proc macros: `#[derive(Hello)]` gives `Thing::hello`, which a call finds only when the proc-macro server ran.
    #[test]
    fn a_derive_expands_only_when_asked() {
        let copy = common::copy_of_fixture("derive", "derive_effect");
        let manifest = copy.join("app/Cargo.toml");

        let off = open(&copy, &manifest);
        assert_eq!(call_owners(&off, "app/src/lib.rs"), []);

        let on = open_with(
            &copy,
            &manifest,
            &LoadOptions {
                build_scripts: false,
                proc_macros: true,
            },
        );
        assert_eq!(
            call_owners(&on, "app/src/lib.rs"),
            [("hello".to_owned(), external("app", "app::Thing::hello"))]
        );
    }

    /// ENGINE.md, S-RS3: a load that went on with `--no-deps` keeps `std` and loses every call between the workspace's own crates.
    #[test]
    fn without_dependencies_std_resolves_and_a_call_into_a_sibling_member_does_not() {
        let copy = common::copy_of_fixture("offline", "offline_effect");
        let loaded = open(&copy, &copy.join("app/Cargo.toml"));
        assert!(
            loaded.dependencies_error().is_some(),
            "the fixture must make cargo metadata fail"
        );
        assert_eq!(
            call_owners(&loaded, "app/src/lib.rs"),
            [
                (
                    "String::new".to_owned(),
                    external("alloc", "alloc::string::String::new")
                ),
                (
                    "len".to_owned(),
                    external("alloc", "alloc::string::String::len")
                ),
            ],
            "member::in_member is not listed: it did not resolve"
        );
    }

    /// The old-toolchain measurement (RS3 §7), by hand: `fixtures/measure/` has a project method reached through `Option::unwrap` and through
    /// `Vec::iter`. Started by a shell loop with `RS_WORKER=1 RUSTUP_TOOLCHAIN=<toolchain>`, it prints one line: did the load work, was `std` found, did
    /// both project calls resolve, and how long the load took. It asserts nothing: a toolchain that cannot load is a result.
    #[test]
    #[ignore = "by hand: RS_WORKER=1 RUSTUP_TOOLCHAIN=<toolchain> cargo test ... worker_measure -- --ignored --nocapture"]
    fn worker_measure() {
        if std::env::var_os("RS_WORKER").is_none() {
            return;
        }
        let copy = common::copy_of_fixture("measure", "measure");
        let root = crate::paths::absolute(&copy).unwrap();
        let manifest = root.join("Cargo.toml");
        let started = std::time::Instant::now();
        let loaded = match load(&root, &manifest, &LoadOptions::default()) {
            Ok(loaded) => loaded,
            Err(error) => {
                println!(
                    "MEASURE loaded=no error={:?}",
                    error.lines().next().unwrap_or("")
                );
                return;
            }
        };
        let load_ms = started.elapsed().as_millis();
        let owners = call_owners(&loaded, "src/lib.rs");
        let project_calls = owners
            .iter()
            .filter(|(text, owner)| text == "run" && matches!(owner, Owner::Project { .. }))
            .count();
        let std_calls = owners
            .iter()
            .filter(|(text, owner)| {
                (text == "unwrap" || text == "iter") && matches!(owner, Owner::External { .. })
            })
            .count();
        println!(
            "MEASURE loaded=yes load_ms={load_ms} std_source={} std_calls={std_calls}/2 project_calls={project_calls}/2 no_std_source={:?} dependencies_error={} toolchain_warning={:?}",
            if loaded.no_std_source().is_none() {
                "found"
            } else {
                "missing"
            },
            loaded.no_std_source(),
            loaded.dependencies_error().is_some(),
            loaded.toolchain_warning(),
        );
    }
}
