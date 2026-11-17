//! Who owns a def: project code, or external (RS3 §4, RS4 §5). Owned data comes out; no engine type does.

use analyzer_core::text::Span;
use ra_ap_hir::{
    Adt, AsAssocItem, AssocItemContainer, Const, Crate, Function, HasSource, HirDisplay, InFile,
    Macro, Module, Semantics, Static, Variant,
};
use ra_ap_ide_db::RootDatabase;
use ra_ap_syntax::{AstNode, TextRange, ast::HasName};

use super::load::Loaded;

/// A def that can be a function of the graph or the class of a construction (RS4 §5). A variant is not one: it is owned by, and built as, its enum.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Item {
    Function(Function),
    Const(Const),
    Static(Static),
    Adt(Adt),
}

impl Item {
    /// The def a variant stands for: its enum (`Enum::V(a)` builds an `Enum`), or the struct or union itself (`hir::Variant` covers all three).
    pub(crate) fn of_variant(db: &RootDatabase, variant: Variant) -> Item {
        Item::Adt(variant.adt(db))
    }
}

/// Who owns a def. Everything the engine knows about a def that is not the user's own code is `External`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Owner {
    /// Written in a file under the root, in a crate of the project: the file, relative to the root with `/`, and where the def's name is in it
    /// (`None` for a def with no name: `const _`).
    Project { file: String, name: Option<Span> },
    /// `package` is the crate's name (`alloc`, `serde`, `member`), `name` the path of the def **from the crate's root, the crate's own name left
    /// out** (`string::String::len`): core draws an external as `package.name`.
    External {
        package: String,
        name: String,
        cause: Cause,
    },
}

/// Why a def is `External` (§5). The first is the ordinary case; the other three are defs of a **local** crate that are not entered, each a body that
/// could have been walked and was not, so each is counted (rule 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cause {
    /// The crate is not local: `std`, `core`, `alloc`, a registry or git dependency.
    Dependency,
    /// The def is in a macro file (a `macro_rules!` made it, or an `include!` of build-script output) or has no source: it is in no file of the text.
    MacroFile,
    /// The def's source is in another crate's file: a built-in derive's method, whose source is the trait's declaration in `core`.
    OtherCrate,
    /// The def is in a file of its own crate that is not under the root: a path dependency or workspace member above or beside the root (RS3 §2).
    OutsideRoot,
}

/// Where a def of a local crate is written, as far as the cause needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Written {
    /// In no file: a macro file, or no source at all.
    Nowhere,
    /// In a file. `under_root` is the root test of RS3 §2; `own_crate` whether that file is a module of the def's own crate.
    InFile { under_root: bool, own_crate: bool },
}

/// §4 and §5 as a table: `None` is project code, `Some` is the reason it is external. `local` is `CrateOrigin::Local`: a workspace member, or a path
/// dependency. The other origins (`Library`: registry and git; `Lang`: `std`, `core`, `alloc`; `Rustc`) are never project code, even when their files
/// sit under the root (a vendored crate's do).
fn cause_of(local: bool, written: Written) -> Option<Cause> {
    if !local {
        return Some(Cause::Dependency);
    }
    match written {
        Written::Nowhere => Some(Cause::MacroFile),
        Written::InFile {
            under_root: true, ..
        } => None,
        Written::InFile {
            own_crate: true, ..
        } => Some(Cause::OutsideRoot),
        Written::InFile { .. } => Some(Cause::OtherCrate),
    }
}

/// Where a def is declared, and where its name is: what `HasSource` says, for each kind of def, read down to what `Owner` needs.
struct Source {
    written: Written,
    /// The file relative to the root, when the def is in one under it.
    relative: Option<String>,
    name: Option<TextRange>,
}

impl Loaded {
    /// The owner of a def (a call's target, RS4 §3). Called inside `attach_db`, with the `Semantics` that lives there.
    pub(super) fn owner_of(&self, sema: &Semantics<'_, RootDatabase>, item: Item) -> Owner {
        let db = self.db();
        let (module, source, path) = match item {
            Item::Function(function) => {
                let module = function.module(db);
                let mut path = module_path(db, module);
                push_container(
                    db,
                    module.krate(db),
                    function.as_assoc_item(db).map(|i| i.container(db)),
                    &mut path,
                );
                path.push(function.name(db).as_str().to_owned());
                (
                    module,
                    function.source(db).map(|s| self.source_of(sema, module, s)),
                    path,
                )
            }
            Item::Const(konst) => {
                let module = konst.module(db);
                let mut path = module_path(db, module);
                push_container(
                    db,
                    module.krate(db),
                    konst.as_assoc_item(db).map(|i| i.container(db)),
                    &mut path,
                );
                // `const _: () = …` has no name: the engine has none either
                path.push(
                    konst
                        .name(db)
                        .map_or_else(|| "_".to_owned(), |n| n.as_str().to_owned()),
                );
                (
                    module,
                    konst.source(db).map(|s| self.source_of(sema, module, s)),
                    path,
                )
            }
            Item::Static(stat) => {
                let module = stat.module(db);
                let mut path = module_path(db, module);
                path.push(stat.name(db).as_str().to_owned());
                (
                    module,
                    stat.source(db).map(|s| self.source_of(sema, module, s)),
                    path,
                )
            }
            Item::Adt(adt) => {
                let module = adt.module(db);
                let mut path = module_path(db, module);
                path.push(adt.name(db).as_str().to_owned());
                (
                    module,
                    adt.source(db).map(|s| self.source_of(sema, module, s)),
                    path,
                )
            }
        };
        self.owner(module, source, path)
    }

    /// The owner of a function: `owner_of` for the most common def. The tests of this file ask it.
    #[cfg(test)]
    pub(super) fn owner_of_function(
        &self,
        sema: &Semantics<'_, RootDatabase>,
        function: Function,
    ) -> Owner {
        self.owner_of(sema, Item::Function(function))
    }

    /// The owner of a variant: its enum's (a struct or union is its own), because the outline has no variant def and a project variant is built as its
    /// enum. An **external** variant keeps its name (`option::Option::Some`, `result::Result::Err`): it has no outline row, and `Ok(x)` and `Err(e)` both named
    /// `result::Result` would be one target of the merged wire (RS4 §5).
    pub(super) fn owner_of_variant(
        &self,
        sema: &Semantics<'_, RootDatabase>,
        variant: Variant,
    ) -> Owner {
        let db = self.db();
        let owner = self.owner_of(sema, Item::of_variant(db, variant));
        match (owner, variant) {
            (
                Owner::External {
                    package,
                    name,
                    cause,
                },
                Variant::EnumVariant(of_enum),
            ) => Owner::External {
                package,
                name: format!("{name}::{}", of_enum.name(db).as_str()),
                cause,
            },
            (owner, _) => owner,
        }
    }

    /// Whether a macro is the project's own (RS3 §4): written in a file under the root, in a crate of the project. `println!` and `vec!` are not; a
    /// `macro_rules!` of the project is, whichever crate of the project uses it.
    pub(super) fn is_project_macro(&self, sema: &Semantics<'_, RootDatabase>, mac: Macro) -> bool {
        let db = self.db();
        let module = mac.module(db);
        let written = mac
            .source(db)
            .map(|source| self.source_of(sema, module, source).written);
        cause_of(
            module.krate(db).origin(db).is_local(),
            written.unwrap_or(Written::Nowhere),
        )
        .is_none()
    }

    /// Where the def is written. `file_id()` is `None` for a macro file: the def is not in the text. A def with no source at all (a derived impl's
    /// methods may have none) is nowhere either, which `owner` reads from the `Option`.
    fn source_of<N: HasName>(
        &self,
        sema: &Semantics<'_, RootDatabase>,
        module: Module,
        source: InFile<N>,
    ) -> Source {
        let db = self.db();
        let krate: Crate = module.krate(db);
        let file = source.file_id.file_id().map(|file| file.file_id(db));
        let relative = file.and_then(|file| self.root_relative(self.vfs().file_path(file)));
        let written = match file {
            None => Written::Nowhere,
            Some(file) => Written::InFile {
                under_root: relative.is_some(),
                // only asked when it matters: a file outside the root of a local crate
                own_crate: relative.is_none()
                    && sema
                        .file_to_module_defs(file)
                        .any(|module| module.krate(db) == krate),
            },
        };
        Source {
            written,
            relative,
            name: source.value.name().map(|name| name.syntax().text_range()),
        }
    }

    /// `module`'s crate decides `local`; where the def is written decides the rest (`cause_of`).
    fn owner(&self, module: Module, source: Option<Source>, path: Vec<String>) -> Owner {
        let db = self.db();
        let krate = module.krate(db);
        let written = source.as_ref().map_or(Written::Nowhere, |s| s.written);
        match cause_of(krate.origin(db).is_local(), written) {
            None => {
                let source = source.expect("project code has a source");
                Owner::Project {
                    file: source
                        .relative
                        .expect("project code is in a file under the root"),
                    name: source
                        .name
                        .map(|range| Span::new(range.start().into(), range.end().into())),
                }
            }
            Some(cause) => {
                // a crate with no name (a `rust-project.json` crate may have none): the engine has nothing to call it by
                let package = krate
                    .display_name(db)
                    .map_or_else(|| "unnamed".to_owned(), |name| name.to_string());
                Owner::External {
                    package,
                    name: path.join("::"),
                    cause,
                }
            }
        }
    }
}

/// The segment before an associated item's name (RS4 §5's table): a trait's own item has the trait; an inherent impl of a struct, enum or union has the
/// type's name, without generic arguments; an inherent impl of any other type has the self type as the engine displays it (`u8`, `[T]`); a trait impl
/// has `<self type as trait>`, both as displayed and with generic arguments (`<String as From<&str>>`). It is never stored, so it only has to be unique
/// and the same on every run (rule 9): impls on builtin types and the several trait impls of one type must not share a name, for the merged wire counts
/// one name as one target.
fn push_container(
    db: &RootDatabase,
    krate: Crate,
    container: Option<AssocItemContainer>,
    path: &mut Vec<String>,
) {
    let target = krate.to_display_target(db);
    match container {
        Some(AssocItemContainer::Trait(t)) => path.push(t.name(db).as_str().to_owned()),
        Some(AssocItemContainer::Impl(i)) => {
            let self_ty = i.self_ty(db);
            match i.trait_ref(db) {
                None => path.push(match self_ty.as_adt() {
                    Some(adt) => adt.name(db).as_str().to_owned(),
                    None => self_ty.display(db, target).to_string(),
                }),
                Some(trait_ref) => {
                    let mut trait_text = trait_ref.trait_().name(db).as_str().to_owned();
                    // the trait's own arguments after `Self`, which is argument 0: `From<&str>`
                    let args: Vec<String> = (1..)
                        .map_while(|at| trait_ref.get_type_argument(at))
                        .map(|arg| arg.display(db, target).to_string())
                        .collect();
                    if !args.is_empty() {
                        trait_text = format!("{trait_text}<{}>", args.join(", "));
                    }
                    path.push(format!("<{} as {trait_text}>", self_ty.display(db, target)));
                }
            }
        }
        None => {}
    }
}

/// The names of the modules from the crate's root down to `module`, the crate's own name left out.
fn module_path(db: &RootDatabase, module: Module) -> Vec<String> {
    let mut names: Vec<String> = module
        .path_to_root(db)
        .into_iter()
        .filter_map(|module| module.name(db))
        .map(|name| name.as_str().to_owned())
        .collect();
    names.reverse();
    names
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ra_ap_hir::{CallableKind, attach_db};
    use ra_ap_syntax::ast;

    use super::*;
    use crate::sem::common;
    use crate::sem::{Rung, load};
    use crate::{LoadOptions, Project, Warning};

    #[test]
    fn what_is_project_code_row_by_row() {
        let in_file = |under_root, own_crate| Written::InFile {
            under_root,
            own_crate,
        };
        // a Local crate, with the def written in a file under the root: project code
        assert_eq!(cause_of(true, in_file(true, true)), None);
        // a Local crate, no file: a macro made it
        assert_eq!(cause_of(true, Written::Nowhere), Some(Cause::MacroFile));
        // a Local crate, a file of its own crate outside the root: a path dependency or a member beside the root
        assert_eq!(
            cause_of(true, in_file(false, true)),
            Some(Cause::OutsideRoot)
        );
        // a Local crate, a file of another crate: a built-in derive's method has the trait's declaration as its source
        assert_eq!(
            cause_of(true, in_file(false, false)),
            Some(Cause::OtherCrate)
        );
        // Library, Lang and Rustc are `local == false`: external whatever else is true, a vendored crate under the root included
        for written in [
            Written::Nowhere,
            in_file(true, true),
            in_file(false, true),
            in_file(false, false),
        ] {
            assert_eq!(cause_of(false, written), Some(Cause::Dependency));
        }
    }

    /// Every call written in `file` (relative to the root), as (the text of the callee's name, who owns the target), in source order. Only calls that
    /// resolve to a function: this is the part of RS4's site walk that the fixture needs.
    fn call_owners(loaded: &Loaded, file: &str) -> Vec<(String, Owner)> {
        attach_db(loaded.db(), || {
            let sema = ra_ap_hir::Semantics::new(loaded.db());
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
                    out.push((text, without_name(loaded.owner_of_function(&sema, f))));
                }
            }
            out
        })
    }

    /// The file tests compare a project def by its file; where its name is has tests of its own.
    fn without_name(owner: Owner) -> Owner {
        match owner {
            Owner::Project { file, .. } => Owner::Project { file, name: None },
            external => external,
        }
    }

    fn project(file: &str) -> Owner {
        Owner::Project {
            file: file.to_owned(),
            name: None,
        }
    }

    /// An external of a crate that is not local.
    fn external(package: &str, name: &str) -> Owner {
        external_because(package, name, Cause::Dependency)
    }

    fn external_because(package: &str, name: &str, cause: Cause) -> Owner {
        Owner::External {
            package: package.to_owned(),
            name: name.to_owned(),
            cause,
        }
    }

    fn open(root: &Path, manifest: &Path) -> Loaded {
        open_with(root, manifest, &LoadOptions::default())
    }

    fn open_with(root: &Path, manifest: &Path, opts: &LoadOptions) -> Loaded {
        let root = crate::paths::absolute(root).unwrap();
        let manifest = crate::paths::absolute(manifest).unwrap();
        load(&root, Some(&manifest), &manifest, opts).expect("the fixture loads")
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
                    external_because("member", "generated", Cause::MacroFile)
                ),
                // a path dependency inside the root
                ("inside::in_inside".to_owned(), project("inside/src/lib.rs")),
                // std: `Lang` origin, whatever the root
                (
                    "String::new".to_owned(),
                    external("alloc", "string::String::new")
                ),
                ("len".to_owned(), external("alloc", "string::String::len")),
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
                    external_because("member", "in_member", Cause::OutsideRoot)
                ),
                (
                    "member::generated".to_owned(),
                    external_because("member", "generated", Cause::MacroFile)
                ),
                (
                    "inside::in_inside".to_owned(),
                    external_because("inside", "in_inside", Cause::OutsideRoot)
                ),
                (
                    "String::new".to_owned(),
                    external("alloc", "string::String::new")
                ),
                ("len".to_owned(), external("alloc", "string::String::len")),
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
                external_because("scripts", "generated", Cause::MacroFile)
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
            [(
                "hello".to_owned(),
                external_because("app", "Thing::hello", Cause::MacroFile)
            )]
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
                    external("alloc", "string::String::new")
                ),
                ("len".to_owned(), external("alloc", "string::String::len")),
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
        let loaded = match load(&root, Some(&manifest), &manifest, &LoadOptions::default()) {
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

    /// Rung 4: a file with no manifest is a crate of its own, and a call into its `mod` child resolves.
    #[test]
    fn a_lone_file_resolves_a_call_into_its_mod_child() {
        let copy = common::copy_of_fixture("lone", "origin_lone");
        let root = crate::paths::absolute(&copy).unwrap();
        let loaded = load(&root, None, &root.join("main.rs"), &LoadOptions::default())
            .expect("a lone file loads");
        assert_eq!(loaded.rung(), Rung::Lone);
        assert!(loaded.no_manifest());
        assert_eq!(
            call_owners(&loaded, "main.rs"),
            [("child::go".to_owned(), project("child.rs"))]
        );
    }

    /// Every tool is found the way the engine finds it (RS3 §6, ENGINE.md "Finding a tool"): with `PATH` set to an empty folder and `HOME`, `CARGO_HOME` and
    /// `RUSTUP_HOME` kept, a driver started the way a macOS GUI app starts one loads at rung 1 **with `std`**, and says nothing. A `rustc` found by `PATH`
    /// alone is not there, the sysroot is unknown, and the load has no `std` and a warning that tells the user to install `rust-src`. In a worker, for the
    /// reason of the test below.
    #[test]
    fn with_cargo_off_the_path_the_engines_rule_finds_it_and_std_resolves() {
        let nothing = common::tmp_root().join("empty_path");
        let _ = std::fs::remove_dir_all(&nothing);
        std::fs::create_dir_all(&nothing).unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "sem::origin::tests::worker_empty_path",
                "--nocapture",
            ])
            .env_clear()
            .env("RS_WORKER", "1")
            .env("PATH", &nothing);
        // where the toolchain lives stays; only `PATH` goes
        for key in [
            "HOME",
            "CARGO_HOME",
            "RUSTUP_HOME",
            "USERPROFILE",
            "SystemRoot",
            "TMP",
            "TEMP",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let out = command.output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.status.success() && text.contains("1 passed"),
            "the worker failed:\n{text}"
        );
    }

    #[test]
    #[ignore = "run by with_cargo_off_the_path_the_engines_rule_finds_it_and_std_resolves"]
    fn worker_empty_path() {
        if std::env::var_os("RS_WORKER").is_none() {
            return;
        }
        let copy = common::copy_of_fixture("owner", "origin_empty_path");
        let root = crate::paths::absolute(&copy).unwrap();
        let file = root.join("ws/app/src/lib.rs");
        let opened = Project::open(&root, &file, &LoadOptions::default()).unwrap();
        assert_eq!(opened.warnings(), [], "no warning: the toolchain was found");
        assert_eq!(opened.loaded().rung(), Rung::Cargo);
        let manifest = root.join("ws/app/Cargo.toml");
        let loaded = load(&root, Some(&manifest), &manifest, &LoadOptions::default()).unwrap();
        assert!(
            loaded.no_std_source().is_none(),
            "std was found: {:?}",
            loaded.no_std_source()
        );
        // `String::len` resolves, to `alloc`: there is a `std`
        assert!(
            call_owners(&loaded, "ws/app/src/lib.rs")
                .contains(&("len".to_owned(), external("alloc", "string::String::len")))
        );
    }

    /// Rung 3 with no cargo and no rustc on `PATH` and an empty `HOME` (RS3 §6, S-RS3: the engine otherwise finds `~/.cargo/bin/cargo`), in a worker:
    /// `cargo test` itself puts the toolchain on the path of its children, and this crate may not call `set_var`.
    #[test]
    fn without_cargo_the_readers_crate_graph_resolves_the_projects_own_calls() {
        let nothing = common::tmp_root().join("no_tools");
        let _ = std::fs::remove_dir_all(&nothing);
        std::fs::create_dir_all(&nothing).unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "sem::origin::tests::worker_without_cargo",
                "--nocapture",
            ])
            .env_clear()
            .env("RS_WORKER", "1")
            .env("PATH", &nothing)
            .env("HOME", &nothing)
            .env("USERPROFILE", &nothing);
        // a Windows process needs its system folder to start
        for key in ["SystemRoot", "TMP", "TEMP"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let out = command.output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.status.success() && text.contains("1 passed"),
            "the worker failed:\n{text}"
        );
    }

    #[test]
    #[ignore = "run by without_cargo_the_readers_crate_graph_resolves_the_projects_own_calls"]
    fn worker_without_cargo() {
        if std::env::var_os("RS_WORKER").is_none() {
            return;
        }
        let copy = common::copy_of_fixture("rung3", "origin_rung3");
        let root = crate::paths::absolute(&copy).unwrap();
        let file = root.join("app/src/lib.rs");

        // the public view: no cargo is a warning and not a failure, and the project's files include the module outside `src/`
        let opened = Project::open(&root, &file, &LoadOptions::default())
            .expect("a load with no cargo is a load");
        assert_eq!(opened.warnings(), [Warning::NoToolchain]);
        assert_eq!(opened.manifest(), "app/Cargo.toml");
        assert!(
            opened.files().iter().any(|f| f == "shop/extra/more.rs"),
            "a #[path] module outside src/ is loaded: {:?}",
            opened.files()
        );
        assert_eq!(opened.skipped(), []);

        // the engine's view: the calls of §6's row resolve, across the two crates too, with no `std`
        let loaded = load(
            &root,
            Some(&root.join("app/Cargo.toml")),
            &file,
            &LoadOptions::default(),
        )
        .unwrap();
        assert_eq!(loaded.rung(), Rung::Reader);
        assert!(loaded.no_toolchain());
        let shop = "shop/src/lib.rs";
        assert_eq!(
            call_owners(&loaded, "app/src/lib.rs"),
            [
                ("shop_core::helper".to_owned(), project(shop)),
                // the outer call comes first in the walk: `.submit()` of `Order::new().submit()`
                ("submit".to_owned(), project(shop)),
                ("shop_core::Order::new".to_owned(), project(shop)),
            ]
        );
        assert_eq!(
            call_owners(&loaded, shop),
            [
                // `Self::new()` and `self.validate()`
                ("Self::new".to_owned(), project(shop)),
                ("validate".to_owned(), project(shop)),
                // the one `#[cfg(...)]` function of this machine: the host's cfgs are set although no `rustc` could say them
                ("helper".to_owned(), project(shop)),
                ("more::more_fn".to_owned(), project("shop/extra/more.rs")),
            ]
        );
    }
}
