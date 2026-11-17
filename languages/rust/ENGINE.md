# Engine: rust

The RS0 spikes below were run on 2026-11-16 (macOS, Darwin 23.6.0, Rust 1.99.1) and their code deleted when RS0 closed; it is in git history at commit `31864ad`
(`languages/rust/spikes/`: `s1-*` S-RS1/S-RS2 loaders, `s2-policy`, `s2-oracle`, `s2-cancel`, `s4-docs`, `fixtures/policy`).

- Upstream URL: https://github.com/rust-lang/rust-analyzer (read from `../language_clone/rust-analyzer` at `2a362a4`; **built from crates.io**, not git)
- Pinned commit / version: `ra_ap_* =0.0.358` for **every** `ra_ap_*` crate we name (published 2026-10-12), written once in `languages/rust/Cargo.toml` (RS1 step 1). See S-RS1 for why one number and why not newer.
- Engine crates used: `rust-syntax`: `ra_ap_syntax` (its `ra_ap_parser`, `rowan` come with it). `rust-analysis`: + `ra_ap_hir`, `ra_ap_ide_db`,
  `ra_ap_load-cargo`, `ra_ap_project_model`, `ra_ap_vfs`. Dev only: `ra_ap_ide` (the oracle). Final list confirmed when RS3 and RS4 are built.
- Patches: none
- How to bump: an engine bump is a change of its own (only the pins, the lockfile, the `sem` module and this file).
  1. Look for a newer set: `cargo info ra_ap_hir`, `ra_ap_ide`, `ra_ap_ide_db`, `ra_ap_load-cargo`, `ra_ap_project_model` (these five were the ones that stopped at
     0.0.358). Move **every** `ra_ap_*` line of `[workspace.dependencies]` in `languages/rust/Cargo.toml` to one `=` version that all of them have.
  2. `cargo update`, then check the pair that broke a fresh lock (S-RS1): `grep -A1 'name = "unicode-' Cargo.lock`. `ra_ap_hir_def` pins `unicode-ident` exactly; if
     `unicode-properties` is a different Unicode version, `ra-ap-rustc_lexer` does not compile: `cargo update -p unicode-properties --precise <the matching one>`. Commit the lockfile.
  3. `cargo test`, then `cargo test -p rust-syntax` alone, and `scripts/check-all.sh --static` (its `cargo tree` line: `rust-syntax` has no `salsa`, `ra_ap_hir` or `ra_ap_ide`).
  4. Re-check every item this file names ("The engine items we call" below, the S-RS2 policy table, the `attach_db` rule, `Sysroot::discover`'s `rustup` call, the `set_test` default, the doc-comment node shapes
     of S-RS4) against the new source; fix `sem` only.
  5. Write the new version and the date here.
- Licence notice: rust-analyzer is MIT OR Apache-2.0. (Notice text to be copied from the published crates when RS3 ships a binary.)

## The engine items we call

What `sem` imports and calls, read from the code (RS2 step 6, 2026-11-16, `ra_ap_syntax =0.0.358`). An engine bump re-checks every row. A
new call into the engine adds its row in the same commit (the reviewer writes it, from the code).

**`rust-syntax`** (`crates/rust-syntax/src/sem.rs`, the only file that names `ra_ap_syntax`):

| Item | What we rely on |
|---|---|
| `Edition` (`Edition2015`, `2018`, `2021`, `2024`) | mapped one to one from our `Edition`; never `Edition::DEFAULT` (2015, see S-RS1) |
| `SourceFile::parse(&str, Edition) -> Parse<SourceFile>` | never fails: a broken file still gives a tree. Does **not** skip a byte-order mark (an error at 0), so `sem` parses after it and shifts every offset back |
| `Parse::errors()`, `SyntaxError::range()` | one range per error; an empty range can sit at the end of the text, or one byte past it (clamped by `sem`) |
| `Parse::tree()`, `Parse::syntax_node()` | the typed root, and the untyped root for the token walk of the file header |
| `ast::HasModuleItem::items()` | the items of a `SourceFile` and of a `mod`'s `ItemList` |
| `ast::Item::{Fn, Struct, Union, Enum, Trait, Impl, TypeAlias, Const, Static, MacroRules, Module, ExternBlock}`, `ast::Item::cast` | the kinds of def (RS1 §2); `cast` finds an item inside a body |
| `Trait::assoc_item_list()`, `Impl::assoc_item_list()`, `AssocItemList::assoc_items()`, `ast::AssocItem::{Fn, Const}` | the items of a trait and an impl |
| `Module::item_list()`; `ExternBlock::extern_item_list()`, `ExternItemList::extern_items()`, `ast::ExternItem::Fn` | an inline `mod`'s items (`None` for `mod m;`); a foreign `fn` |
| `Fn::body()`, `Const::body()`, `Static::body()`, `Const::underscore_token()` | where items in bodies are looked for (B1–B11); `const _` is no def |
| `Impl::self_ty()`, `Impl::trait_()`, `Impl::for_token()` | the impl header (I1–I12); a `for` with no type on one side, or a parse error in the header, and it is not read (I10) |
| `ast::Type::PathType`, `PathType::path()`, `Path::first_segment()`, `Path::segment()`, `PathSegment::type_anchor()` | a header type is a path whose last segment is its name, unless it has a `<A as B>::` qualifier |
| `ast::HasName::name()`, `Name::text()` | the def's name, `r#` still on (stripped by `sem`); `None` when the parser could not find one |
| `ast::HasAttrs::attrs()`, `Attr::kind()`, `Attr::meta()`, `ast::Meta::KeyValueMeta`, `Attr::simple_name()`, `AttrKind::Outer` | the `#[doc = …]` fragments for placing a marker (RS2 §4) |
| `ast::HasAttrs::doc_comments()`, `DocComment::kind()`, `DocComment::text_with_markers()` | the item's outer doc lines in source order, outer and inner mixed (`sem` keeps `Outer`); the text keeps `///`, `/**`, and a CRLF file's `\r` (S-RS4) |
| `Fn::param_list()`, `ParamList::self_param()`, `ParamList::params()`, `Param::pat()`; `SelfParam::mut_token()`, `amp_token()`, `ty()` | the params of the outline and the four receiver forms (RS1 §5) |
| `AstNode::syntax()`, `SyntaxNode::{kind, text, text_range, children_with_tokens, preorder, preorder_with_tokens}`, `Preorder::skip_subtree`, `WalkEvent::Enter`, `NodeOrToken` | the walks: a body's items (iterative, so deep nesting cannot overflow the stack), an item's range, the type-text printer, the header's token scan |
| `SyntaxToken::{kind, text, text_range, parent}`, `TextRange::{new, start, end}` | tokens and offsets (`TextSize` into `u32`) |
| `SyntaxKind`: `WHITESPACE`, `COMMENT`, `SHEBANG`, `DOC_COMMENT`, `OUTER_DOC_COMMENT`, `INNER_DOC_COMMENT`, `ATTR` | the tree shapes of S-RS4: a doc line is a `DOC_COMMENT` node around an `OUTER_`/`INNER_DOC_COMMENT` token; a plain comment is a `COMMENT` token and can open an item's node, so an item's range starts at its first `DOC_COMMENT` or `ATTR` child, else its first child that is not `COMMENT` or `WHITESPACE` |
| `SyntaxKind`: `LIFETIME`, `LIFETIME_ARG`, `FOR_BINDER`, `GENERIC_ARG_LIST`, `PARENTHESIZED_ARG_LIST`, `TUPLE_TYPE`, `PARAM_LIST`, `TYPE_BOUND_LIST`, `ABI`, `STRING`, `COLON2`, `COMMA`, `PLUS`, `EQ`, `THIN_ARROW`, `COLON`, `SEMICOLON`, `R_PAREN`, `R_BRACK`, `R_ANGLE`, `R_CURLY`, `AS_KW`, `DYN_KW`, `IMPL_KW`, `MUT_KW`, `CONST_KW`, `UNSAFE_KW`, `EXTERN_KW` | the type-text printer (`docs/qualnames.md` P1–P8). A renamed or split kind changes stored qualnames: the `the_printing_rule` test and the snapshots catch it |

**`rust-analysis`** (`crates/rust-analysis/src/sem/`, D35; as of RS3 step 7, 5fb73d8; the test-only calls are marked):

| Item | Where | What we rely on |
|---|---|---|
| `ra_ap_project_model::ProjectManifest::from_manifest_file(AbsPathBuf)` | `load.rs` | a `Cargo.toml` path becomes the engine's manifest; cargo finds the workspace root above a member |
| `ProjectWorkspace::load(manifest, &CargoConfig, progress)` | `load.rs` | runs `cargo metadata` and loads the sysroot; falls back to `--no-deps` metadata by itself when the full one fails (§ "In-project edges by load mode") |
| `CargoConfig { sysroot: Some(RustLibSource::Path(dir)), set_test: true, extra_env, .. }` | `load.rs` | `Path` looks for `rust-src` without `rustup` (§ "How `sem::load` finds std"); `set_test` makes `cfg(test)` code active (S-RS2); `extra_env` reaches every `cargo` the engine runs, the sysroot's included (`CARGO_RESOLVER_LOCKFILE_PATH`, D38) |
| `ra_ap_load_cargo::load_workspace(ws, &extra_env, &LoadCargoConfig { load_out_dirs_from_check: false, with_proc_macro_server: ProcMacroServerChoice::None, prefill_caches: false, num_worker_threads: 1, proc_macro_processes: 0 })` | `load.rs` | returns `(RootDatabase, Vfs, _)`; build scripts, proc macros and cache prefill stay off (RS3 §3) |
| `ra_ap_vfs::{Vfs::iter, Vfs::file_path, VfsPath::as_path, AbsPathBuf}` | `load.rs`, `origin.rs` | every loaded file with its path; a path is compared as engine text (`/`, drive-letter case ignored as the engine's own `VfsPath` encoding does, `vfs_path.rs` `paths_encoding`) |
| `ra_ap_ide_db::RootDatabase` | `load.rs`, `origin.rs` | owned by `Loaded`; dropping it frees the load |
| `ra_ap_hir::attach_db(&db, \|\| …)` | `origin.rs` | every `hir` call runs inside it, or the engine panics (S-RS2) |
| `Function::{module, source, name, as_assoc_item}`, `HasSource::source(..).file_id.file_id()`, `HirFileId → EditionedFileId::file_id(db)` | `origin.rs` | where a def is written; `file_id()` is `None` for a macro file, so a macro-made def is in no file (RS3 §4) |
| `AsAssocItem`, `AssocItemContainer::{Trait, Impl}`, `Impl::self_ty(..).as_adt()`, `Trait::name`, `Adt::name` | `origin.rs` | the type or trait segment of an external name; a self type that is not an ADT (`[T]`, `&T`) gives none |
| `Module::{krate, name, path_to_root}`, `Crate::{origin, display_name}`, `CrateOrigin::is_local` | `origin.rs` | `Local` is a workspace member or a path dependency; `Library`, `Lang`, `Rustc` are external; a crate with no display name is `unnamed` |
| `ProjectWorkspace::{run_build_scripts(&config, progress), set_build_scripts}`, `WorkspaceBuildScripts::error`, `CargoConfig::target_dir_config = TargetDirectoryConfig::UseSubdirectory` | `load.rs` | the build run (`cargo check`) when a switch is on (RS3 §3); its output goes to `<target-dir>/rust-analyzer`; a run that reports an error still hands back what it built |
| `LoadCargoConfig { load_out_dirs_from_check, with_proc_macro_server: ProcMacroServerChoice::Sysroot, proc_macro_processes }` | `load.rs` | `OUT_DIR` code after a build run; the toolchain's own proc-macro server, a pool of one; the client may be dropped after the load (each macro the database knows holds the pool) |
| `ProjectWorkspaceKind::Cargo { cargo, .. }`, `CargoWorkspace::{packages, manifest_path}`, `PackageData::{targets, is_member, is_local, manifest}`, `TargetData::kind`, `TargetKind::{BuildScript, Lib { is_proc_macro }}` | `load.rs` | what `skipped()` counts and which manifests `environment_inputs()` names |
| `ProjectWorkspace::sysroot.error()`, `ProjectWorkspaceKind::Cargo { error, .. }` | `load.rs` | read right after `ProjectWorkspace::load`: a missing `rust-src` (`NoStdSource`) and the `--no-deps` retry's cargo message (`DependenciesMissing`); the load itself says neither (RS3 §6, step 5a) |
| `ra_ap_project_model::project_json::{ProjectJsonData, ProjectJson::new}`, `ProjectWorkspace::load_inline(ProjectJson, &CargoConfig, progress)` | `load.rs` | rungs 3 and 4 (RS3 §6): our reader's crates as JSON (`display_name`, `root_module`, `edition`, `deps`, `is_workspace_member`, `cfg`, `is_proc_macro`, `source.include_dirs` / `exclude_dirs`), sysroot `None`; with no `rustc` the engine falls back to no cfgs and keeps the data-layout error ("Rung 3") |
| `Semantics::file_to_module_def(FileId)` (inside `attach_db`) | `load.rs` | `files()`: a loaded `.rs` file is listed only when some crate has a module for it (RS3 §2); the first call builds the module maps (seconds) |
| `Const::{module, source, name, as_assoc_item}`, `Static::{module, source, name}`, `Adt::{module, source, name}`, `Variant::adt`, `InFile<N: HasName>::value.name()` | `origin.rs` (RS4 step 1, e9e03ed) | the owner of every kind of `Func` and of a construction's class; a variant is owned by its enum; a project def's owner holds its **name** range, the outline's `name_range` |
| `Semantics::file_to_module_defs(file)`, `Module::krate` | `origin.rs` | whether a file outside the root is a module of the def's own crate (`OutsideRoot`) or of another (`OtherCrate`: a built-in derive's method, whose source is the trait's declaration in `core`) |
| `Semantics::{new, attach_first_edition_opt, parse}`, `EditionedFileId::edition`, `SyntaxNode::{text, covering_element, ancestors}` | `bridge.rs` | a project file opened once per request from the **engine's** text (so a byte-order mark shifts both outlines alike); a file no crate has a module for gives no edition, so no entry |
| `Semantics::to_def` on `ast::{Fn, Adt, Const, Static}` | `bridge.rs` | the outline's def → the engine's: the node whose name is exactly the outline's `name_range`; `None` in `#[cfg]`-disabled code |
| `hir::attach_db` once per request (`sem::attached`), the `Semantics` built inside it | `bridge.rs` | S-RS2's rule, for a whole request |
| `Semantics::expand_macro_call(&MacroCall) -> Option<InFile<SyntaxNode>>` | `bridge.rs` for `sites.rs` (RS4 step 2, 7edd537) | a macro call's expansion, walked in its place, recursively (depth ≤ 128); `None` both for an unknown macro and inside a disabled statement (RS4 §3 "As built") |
| `Semantics::original_range_opt(&SyntaxNode) -> Option<FileRange>` | `bridge.rs` | `Some`: the node is written in a file (a macro argument), `None`: a macro's own body (S-RS2). The `FileRange`'s file must be the def's (RS4 §3) |
| `Semantics::type_of_expr(&Expr).is_some()` | `bridge.rs` | "the engine lowered it into the body": `None` for an expression a `cfg` turns off inside an active body; never asked of a macro call |
| `Semantics::resolve_macro_call`, `Macro::{module, source}` | `bridge.rs`, `origin.rs` | which macro a call is, and whether it is a project macro (`cause_of`, as a def) |
| `Semantics::{resolve_path_with_subst, resolve_expr_as_callable, resolve_variant, resolve_method_call_fallback, resolve_path, resolve_type}`, `PathResolution::{Def(ModuleDef::{Function, Adt, EnumVariant, TypeAlias, BuiltinType}), TypeParam, SelfType}`, `CallableKind::*` | `resolve.rs` (RS4 step 3, a7af483) | §4's table: path first, then the callable type; a record by `resolve_variant`; the substitution is dropped (RS4 §4 rule 2), except to read `Self` for classifying (the step-3 review) |
| `Semantics::type_of_expr(..).original()`, `Type::{autoderef, as_type_param, as_dyn_trait, as_impl_traits, as_associated_type_parent_trait, is_unknown}`, `TypeParam::{parent, name, is_implicit}`, `Function::as_assoc_item` → `AssocItemContainer::Trait` | `resolve.rs` | `Open` of a trait's own item. **`Self` in a trait is an implicit type parameter** (`is_implicit` is true for it as for an argument-position `impl Trait`), so it is told apart by its parent and name first |
| `ast::Path::{qualifier, segment}`, `PathSegment::type_anchor`, `TypeAnchor::ty` | `resolve.rs` | the qualifier of `W::make()` and the type of `<W as Make>::make()` |
| `ast::{MacroCall, CallExpr, MethodCallExpr, RecordExpr, ClosureExpr, BlockExpr::async_token, Item, Attr::simple_name}`, `SyntaxNode::preorder`, `Preorder::skip_subtree` | `sites.rs` | the walk of a body and of an expansion: nested items skipped, closures and `async` blocks numbered, `#[cfg]` read by the tree |
| test only: `Semantics::{new, attach_first_edition_opt, parse, resolve_expr_as_callable, resolve_method_call_fallback}`, `CallableKind::Function`, `Either::left`; `ra_ap_syntax::{ast::CallExpr, ast::MethodCallExpr, AstNode}` | `origin.rs` tests | the fixture's call walk, the part of RS4's site walk RS3 needs |

## RS4: the bridge (step 1, e9e03ed, 2026-11-17)

Found while building it, on the policy fixture (`crates/rust-analysis/tests/fixtures/policy/`):

- **A built-in derive's method is named by the type.** `Derived::default()` (`#[derive(Default)]`) resolved **path first** is the derived impl's
  method: its `module` is the type's module, its source the trait's declaration in `core`, so `External { package: "policy", name:
  "shapes::Derived::default" }`, cause `OtherCrate`. `.clone()` on it has the same shape. The callable type first answers `core`'s
  `Default::default` instead: one more case where the path is the more specific answer (RS4 §4). Answers RS4 §5's [check].
- **Name resolution answers inside `#[cfg]`-disabled code.** `helper()` in a `#[cfg(any())] fn disabled` resolves by path, though the engine
  has no def for `disabled` (`to_def` is `None`) and the callable type does not resolve. So a site list must come from code the engine
  lowered, never from the file's text (RS4 §3).
- **`original_range_opt` is `None` for every call written in a macro's own body** (step 2's file fix, 41bf33f): plain, `$crate::` and
  method-chain callees alike, also for a macro defined in another module. So the check that a range's file is the def's is a guard; the
  `from_elsewhere!` fixture is its regression test.
- **A file with a byte-order mark joins**: the outline is built from the engine's text of the file, whose offsets are the ones `HasSource`
  reports.

## S-RS1: does it build as a dependency?

### Which version (2026-11-16)

There is **no published `ra_ap_*` set at the clone's commit `2a362a4`**, and no complete set after 0.0.358. `cargo info` on 2026-11-16:

| Crate | Latest published |
|---|---|
| `ra_ap_hir`, `ra_ap_ide`, `ra_ap_ide_db`, `ra_ap_load-cargo`, `ra_ap_project_model` | **0.0.358** (2026-10-12) |
| `ra_ap_syntax`, `ra_ap_parser`, `ra_ap_vfs` | 0.0.363 |

The publishing job stopped for the `hir`-family crates after 0.0.358 (guess: the new `hir-ide` crate is not published; not verified). Pin
**`=0.0.358` for every `ra_ap_*` crate**, `ra_ap_syntax` included: `rust-syntax` and `rust-analysis` share one lockfile, and mixing 0.0.363 with
0.0.358 would put two `rowan`-compatible but different `ra_ap_syntax` into one tree. Read the engine source from the registry copy
(`~/.cargo/registry/src/*/ra_ap_*-0.0.358/`), not from the clone: they differ (below, and S-RS4).

`rust-version` of every crate is 1.98, edition 2024; the toolchain 1.99.1 builds them with no flag and no patch.

### The lockfile trap: `unicode-properties` must be 0.1.4

A cold resolve **fails to compile** (`ra-ap-rustc_lexer`, `E0080`: "unicode-properties and unicode-ident must use the same Unicode version").
`ra_ap_hir_def 0.0.358` pins `unicode-ident = "=1.0.24"` (Unicode 17); the newest `unicode-properties` is 0.1.5 (Unicode 18); the lexer
checks that the two agree at compile time. Cargo picks 0.1.5, so a fresh `Cargo.lock` is broken. The fix is one line in the lockfile:
`cargo update -p unicode-properties --precise 0.1.4` (Unicode 17). Therefore **`languages/rust/Cargo.lock` is committed and a bump re-checks this pair.**
A crate with only `ra_ap_syntax` has no `hir_def` pin in its tree, so alone it would resolve `unicode-ident` 1.0.27 and 0.1.5 (Unicode 18, which agree);
inside our workspace the lock decides.

### Build cost, measured

`cargo build --release`, `debug = 0`, `CARGO_INCREMENTAL=0`, warm registry, empty target directory for the first two rows:

| Crate set | Cold build | Binary: names the API only | Binary: really loads a workspace and resolves calls |
|---|---|---|---|
| `ra_ap_syntax` (parse a string) | **9.5 s** | 1.0 MB | 1.0 MB |
| + `hir`, `ide_db`, `load-cargo`, `project_model`, `vfs` | **2 m 05 s** | 17.5 MB | **21.95 MB** |
| + `ra_ap_ide` (`AnalysisHost`, `outgoing_calls`) | +2 m 39 s (so ≈ 4 m 45 s cold) | 17.5 MB | **22.45 MB** |

- "names the API only" is a trap: the linker drops what is not called, so adding `ide` looked free. The right column is a program that loads a
  Cargo workspace, parses every file, resolves every call, and (for `ide`) calls `outgoing_calls`. **`ide` costs about 0.5 MB in the binary and
  2.5 minutes of cold release build.** Keeping it as a dev-dependency (the oracle) is worth it for the build time, not the size.
- `cargo tree`: the syntax-only crate has **no `salsa`** (0 matches). The `hir` set has `salsa 0.28.5`. Rule 4's "fast crate must not compile the
  engine's analysis" is therefore real: `cargo test -p rust-syntax` compiles 9.5 s of engine, not 2 minutes.
- Target directory after all three builds: about 0.9 GB (`debug = 0`). Without `debug = 0` expect several times that: the owner has ~12 GB free.
- 120 `ra_ap_*` lines in `cargo tree` for the `hir` set, 154 with `ide`.
- Not checked: Linux and Windows. **Open:** needs CI (the repo has `.github/workflows/ci.yml`, RS1 adds the job).

### The edition type

`ra_ap_syntax::Edition` (a re-export of `ra_ap_parser::Edition`, which is `ra_ap_edition::Edition`). Variants `Edition2015`, `2018`, `2021`, `2024`;
`Edition::CURRENT` and `LATEST` are 2024, **`Edition::DEFAULT` is 2015** (do not use it for a file whose edition is unknown: `rustc` defaults to
2015 only without `--edition`; Cargo always passes one). It implements `FromStr` (`"2024"`). `SourceFile::parse(text, edition) -> Parse<SourceFile>`.

## S-RS4: can we find and edit the doc comment of every kind of def?

Answer: **yes**, with five corrections to the plan. Run on `ra_ap_syntax =0.0.358`, `Edition::Edition2024`, with `languages/rust/spikes/s4-docs`
(about 45 cases; the parse of each had no errors).

**What the tree looks like (this differs from the plan).**

- A doc line is **a node**, not a token: `DOC_COMMENT` wrapping one token `OUTER_DOC_COMMENT` (`///`, `/** */`) or `INNER_DOC_COMMENT` (`//!`, `/*! */`).
  An attribute is the node `ATTR`. Both are direct children of the item's node, **interleaved in source order**
  (`/// a` · `#[inline]` · `/// b` · `#[must_use]` gives `DOC_COMMENT ATTR DOC_COMMENT ATTR`).
- **`HasDocComments` does not exist in 0.0.358** (the plan named it). Doc lines come from `ast::HasAttrs`: `doc_comments() -> AstChildren<ast::DocComment>`
  (outer **and** inner, in order), `attrs() -> AstChildren<ast::Attr>`, `attrs_with_doc() -> AstChildren<ast::AnyAttr>` (both kinds, source order: this
  is the one RS2 needs for "after the last doc comment, wherever it stands"). `ast::DocComment` has `.kind()` (`AttrKind::Outer | Inner`), `.token()`,
  `.text_with_markers()`, `.text()` (markers stripped).
- `//!` inside a body is a `DOC_COMMENT` child of the body's `STMT_LIST`, **not** of the `FN` node. For a `mod m { //! … }` it is under the module's `ITEM_LIST`.
  So the item's own `doc_comments()` never contains an inner doc line.

**The kinds of def.** For every kind below, a `///` above the item is a child of the item's own node, first in the node, and `doc_comments()` returns it:
`fn`, a method in an inherent `impl`, a method in a trait `impl`, a trait method with and without a body, `struct`, `enum`, `union`, `trait`,
inline `mod`, `type`, `const`, `static`, `macro_rules!` (also with `#[macro_export]` between the doc and the macro: the order is kept), a `fn` in a `fn`,
a foreign `fn` in an `unsafe extern "C" { }` block, and the `impl` block itself. An `impl` has no name (`name()` is `None`), as the plan said.
(Enum variants and record fields were not asked of RS1's table; the parser also attaches comments to them, `n_attached_trivias` names `VARIANT`,
`RECORD_FIELD`, `TUPLE_FIELD`.)

**Blank lines (the plan's "unknown").** Two mechanisms, and the first reading of this section mixed them up (corrected 2026-11-16, after a review
case that the first reading could not explain):

1. **Doc comments are not trivia in 0.0.358.** `SyntaxKind::is_trivia` is only `WHITESPACE | COMMENT`. `OUTER_DOC_COMMENT` and `INNER_DOC_COMMENT` are real
   tokens, and the grammar eats them like `#[…]`: `outer_attrs` in `ra_ap_parser/src/grammar/attributes.rs` wraps each in a `DOC_COMMENT` node as a child of
   the item. So **whatever stands between two doc lines (a blank line, a plain `//`) is trivia *inside* the item's node, and every outer doc line above an
   item is a child of it.** Nothing about blank lines is ever asked of a doc comment.
2. **Only plain `//` and `/* */` comments** go through `n_attached_trivias` (`ra_ap_parser/src/shortcuts.rs`): they are leading trivia of the item, and
   they are pulled into its node only while no blank line (`"\n\n"`) stands between them and the item. That function also has a branch for "the next comment
   up is an outer doc comment", but it matches the lexer's `COMMENT` kind, so for a doc line it never fires (a dead branch in this version).

The cases (all run on the real parser):

| Source above an item | In the item's node? |
|---|---|
| `/// a`, blank line, item | **yes**: a blank line does not detach an outer doc comment (as in `rustc`) |
| `/// a`, blank line, `/// b`, item | both, and the blank line between them |
| `/// a`, two blank lines, item | yes |
| `/// a`, blank line, `#[inline]`, item | yes |
| `// plain`, item (no blank) | **yes** (so `range` must skip it by hand) |
| `// plain`, blank line, item | **no** (LF files) |
| `// plain`, `/// d`, item | both |
| `/// d`, `// plain`, item | both; `doc_comments()` is just `/// d` |
| `/// d`, blank, `// plain`, item | all three |
| `// plain`, blank, `/// d`, item | only `/// d` and after: the plain comment is outside |
| `// header`, blank, `/// d`, item (file start) | only `/// d` |
| `//! crate doc`, blank, `/// d`, item | only `/// d`: an inner doc comment breaks attachment |
| `/// a`, `// plain`, blank, `/// b`, item (the review case) | **everything**: `/// a` is a `DOC_COMMENT` child of `f`, the plain comment a `COMMENT` child, the blank line the whitespace after it, then `/// b`. Nothing is left in the parent, so there is no stray token. `doc_comments()` is `["/// a", "/// b"]`, as `rustc` counts them. CRLF: the same |
| `/// a`, `// plain`, blank, item | `/// a`, the plain comment and the blank line are all in the node; `doc_comments()` is `["/// a"]` |
| `/// a`, `// plain`, blank, `#[inline]`, item | all in the node |
| `fn f() {} // c` then `/// d` `fn g() {}` | **`g`'s node starts at `// c`**, the trailing comment of the previous item's line |

So **an outer `///` or `/** */` line above an item is always a child of that item's node, however it is spaced or what stands between it and the item.**
A plain `//` before the *first* doc line or attribute is the item's only when it is directly above the item, or when it ends the run of comments that reaches the item
without a blank line. One caveat: a doc line the grammar cannot attach to an item (one before a `}`, say) is a parse error, which `SyntaxError` already covers. The consequence for RS1's `range` ("from the first doc comment or attribute"): do not use `text_range().start()`.
Use the first `DOC_COMMENT` or `ATTR` child, else the first child that is not a `COMMENT` or `WHITESPACE`. The trailing-comment case is the one the
plan missed: without the rule a def's range would begin on the previous def's last line.

**CRLF is a different tree.** A `\r\n` file keeps the `\r` **inside** the comment token (`"/// a\r"`; `.text()` ends in `\r`), and the whitespace
between two lines is `"\n\r\n"`, which does not contain `"\n\n"`. Therefore in a CRLF file **a plain `//` comment above a blank line stays attached**
(in an LF file it is detached). Doc lines are unaffected (they attach either way). RS2: strip a trailing `\r` when reading a doc line, copy the
file's line ending when writing (already in the plan).

**The file header (RS2).** The header is a plain `// <label>: <text>` comment "before the first item". Because of the rules above, **it can be a child of the first
item's node**, so RS2 must compare against RS1's range start (the first `DOC_COMMENT` or `ATTR` child, else the first child that is not `COMMENT` or `WHITESPACE`),
**never the item node's start**. Cases, with the node start and the range start (byte offsets):

| File | Where the header comment is | node start | range start |
|---|---|---|---|
| `// ID: x` directly above `fn f() {}` (LF) | a `COMMENT` child of `FN` | 0 | 9 |
| `// ID: x`, blank, `fn f() {}` (LF) | a `COMMENT` child of `SOURCE_FILE`, before the `FN` | 10 | 10 |
| `// ID: x`, blank, `fn f() {}` (**CRLF**) | a `COMMENT` child of **`FN`** (the CRLF rule above) | **0** | 12 |
| `// ID: x` directly above `fn f() {}` (CRLF) | child of `FN` | 0 | 10 |
| `// ID: x`, blank, `/// d`, `fn f() {}` (CRLF) | child of `FN`, before the `DOC_COMMENT` | 0 | 12 (the doc line) |
| `// ID: x`, blank, `#[inline]`, `fn f() {}` (CRLF) | child of `FN`, before the `ATTR` | 0 | 12 (the attribute) |
| `#!/usr/bin/env rust-script`, `// ID: x`, blank, `fn f() {}` | `SHEBANG` then a `COMMENT`, both children of `SOURCE_FILE` | 37 | 37 |
| `// ID: x`, blank, `//! crate`, blank, `fn f() {}` | child of `SOURCE_FILE`; the inner doc line is a `DOC_COMMENT` child of `SOURCE_FILE` too | 21 | 21 |

So in a CRLF file with a blank line between header and item, `node start` is the start of the file while the header is "before the first item": the rule to
write is "the header is the comment run that ends before the first item's **range start**, found by scanning tokens from the start of the file, whichever node
holds them", not "a child of `SOURCE_FILE`".

**The other rows of the table.**

| Source | Result |
|---|---|
| `/** d */` and a multi-line `/**\n * d\n * ID: x\n */` | one `DOC_COMMENT` each; `.text_with_markers()` is the whole block |
| `#[doc = "d"]` | an `ATTR`, **not** a `DOC_COMMENT`: `doc_comments()` is empty, `attrs()` has it. It is not a place for the marker |
| `////` (four slashes), `/*** */` | a plain `COMMENT`, as the plan said |
| `//!` in a body | see above |

**`rustfmt`** (`rustfmt 1.10.0-stable` of 1.99.1), a file with a `/// ID: …` line, a `/** … ID: … */` block and `#[doc]` attributes:
default config: **file unchanged**. With `wrap_comments = true` and `normalize_doc_attributes = true` (both took effect on this stable rustfmt, no
`unstable_features` needed): a 140-column summary line is re-wrapped, `#[doc = "from attribute"]` became `///from attribute`, and **every `ID:` line, the
`ID:` line inside the `/** */` block, and the odd ones (`///ID: …`, `///   ID: …`) were byte-identical.** `rustfmt` does not touch a short `/// ID: <id>` line.
Caveat for RS2's test: `normalize_doc_attributes` makes `#[doc]` into a doc *line*, so a marker placed after such a line goes after it, harmlessly.

## S-RS3: what does the user's machine need? (partly run, 2026-11-16)

Run on `core/` (and on scratch packages) with `load_workspace_at`, build scripts off, proc-macro server `None`, `RustLibSource::Discover`.

| Case | What happens | Where the text is |
|---|---|---|
| **no `rust-src`** | the load **succeeds**. No error, no progress message. `Sysroot::error()` says `can't load standard library from sysroot\n<sysroot>\n(discovered via `rustc --print sysroot`)\ntry installing `rust-src` the same way you installed `rustc`\n`; `is_rust_lib_src_empty()` is `true`; `num_packages()` is 0. Calls then resolve **silently half**: on `core/` 634 of 978 calls and **74 of 914 method calls** (with `std`: 978/978 and 914/914) | `ProjectWorkspace.sysroot`, which `load_workspace_at` does not return: RS3 must load through `ProjectWorkspace::load` and `load_workspace` (see below) |
| **no `cargo`** (`HOME` empty, minimal `PATH`) | `Err`: `Failed to load the project at <manifest>: Failed to read Cargo metadata from Cargo.toml file <manifest>, None: Failed to run `cd "<dir>" && RUSTUP_AUTO_INSTALL="0" "cargo" "metadata" "--format-version" "1" "--manifest-path" "<manifest>"`: No such file or directory (os error 2)`. **With only a minimal `PATH` it still works**: the engine falls back to `~/.cargo/bin/cargo` | the returned `Err` |
| **`rust-toolchain.toml` names a toolchain that is not installed** | `Err`: `… `cargo metadata` exited with an error: error: toolchain '1.50.0-aarch64-apple-darwin' is not installed\nhelp: run `rustup toolchain install` to install it\n`. The engine sets `RUSTUP_AUTO_INSTALL=0` itself, so it never downloads a toolchain | the returned `Err` |
| **offline, a dependency not downloaded** | the load **succeeds**. `cargo metadata` fails, the engine retries with `--no-deps`, and calls into the dependency are unresolved (0 of 1 calls, 0 of 2 method calls on a one-line fixture). The cause is in `ProjectWorkspace.kind`: `Cargo { error: Some(..) }`, text `` `cargo metadata` exited with an error: error: no matching package named `itoa` found\nlocation searched: crates.io index\nrequired by package `withdep v0.1.0 (<dir>)`\nAs a reminder, you're using offline mode (--offline) … `` | `ProjectWorkspaceKind::Cargo { error }` (not returned by `load_workspace_at`) |

Two of the four are a **quiet half answer** (rule 8): a missing `rust-src` and an unreachable dependency. Both are visible only on the
`ProjectWorkspace`. RS3's rule: load through `ProjectWorkspace::load`, read `sysroot.error()`, `sysroot.is_rust_lib_src_empty()` and `kind.Cargo.error`,
turn them into `OpenError` variants (or a loud counter in `Stats`) and only then call `load_workspace`.

**`Sysroot::discover` installs `rust-src` by itself.** `RustLibSource::Discover` (with no `sysroot_src`) goes through
`discover_rust_lib_src_dir_or_add_component`, which runs **`rustup component add rust-src`** when the source is missing. My first load on this machine did
exactly that (5.4 s instead of 0.7 s, and `rustlib/src` appeared). A library that changes the user's toolchain breaks "text in, data out". RS3 must not call
`Sysroot::discover`; it finds the sysroot with `rustc --print sysroot`, checks `lib/rustlib/src/rust/library` itself, and passes `sysroot_src` so that
`discover_with_src_override` is used (no `rustup`).
(To reproduce the clean no-`rust-src` case I removed the component the engine had added, ran the cases above with `RUSTUP_DIST_SERVER` pointing at a
closed port, and added it back for the rest of RS0.)

**Which toolchain's `std`?** The sysroot is whatever `rustc --print sysroot` prints **in the project's folder**. In this repo `rust-toolchain.toml` selects 1.99.1;
a fixture outside the repo gets the default `stable`. Scenarios live inside the repo, so they get 1.99.1; a user's project gets its own.

## S-RS2: load a real workspace and resolve its calls (run 2026-11-16)

The loader is `languages/rust/spikes/s1-hir`; the policy table is `s2-policy` over `spikes/fixtures/policy` (one function per row); the oracle is `s2-oracle`;
cancellation is `s2-cancel`. All on `=0.0.358`, release build, `LoadCargoConfig { load_out_dirs_from_check: false, with_proc_macro_server: None,
prefill_caches: false, num_worker_threads: 1, proc_macro_processes: 0 }`, `RustLibSource::Discover` with `rust-src` installed (see S-RS3 for why RS3 does not use
`Discover`), `set_test: true`.

### Rules the engine imposes (each is a panic or a silent miss if forgotten)

- **`hir::attach_db` is required.** `Semantics::resolve_method_call_fallback` outside `hir::attach_db(&db, || …)` **panics**: `Tried to use attached db, but
  no db is attached` (`ra_ap_hir_ty-0.0.358/src/next_solver/interner.rs:2491`). `ide::Analysis` does it itself (`attach_db_allow_change` inside `Cancelled::catch`).
  `sem.rs` has one entry that attaches, and every walk happens inside it. A `Semantics` created and used outside it is a bug.
- **`set_test: true` is needed.** `CargoConfig::set_test` ("Gate `#[test]` behind `#[cfg(test)]`") defaults to `false`: `cfg(test)` is off, the
  `#[cfg(test)]` modules are inactive, and **not one call inside them resolves**. On `core/`: 151 of 953 calls and 490 of 863 method calls without it,
  **978/978 and 914/914 with it**. Calls in code that a `cfg` switches off are always `Unresolved`; RS4 counts them.
- **A call inside a macro's arguments is not in the file's tree.** `Semantics::parse` gives the written file; `println!("{}", f())` holds `f()` only as tokens.
  The call is found by `sema.expand_macro_call(&MacroCall) -> Option<InFile<SyntaxNode>>` and walking the expansion (recursively: a nested macro call is a
  `MacroCall` node in the expansion). `Analysis::outgoing_calls` does **not** do this (below), so the plan's "built into `outgoing_calls`" was wrong.

### The policy table, observed (first draft of RS4's table)

`R` = `resolve_path_with_subst` / `resolve_method_call_fallback`, `C` = `type_of_expr(callee).original.as_callable(db).kind()`. "container" is
`Function::as_assoc_item(db)?.container(db)`. "subst" is whether the second element of the pair is `Some`.

| Shape | What came back | The plan assumed | Verdict |
|---|---|---|---|
| `json.write()`, `json: JsonWriter`, method from `impl Writer for JsonWriter` | the **impl's** method, subst `Some` | the impl's method + subst | as planned |
| `w.write()`, `w: &W` or `w: W`, `W: Writer` | the **trait's** declaration, subst `Some` | the trait's | as planned |
| `w.write()`, `w: &dyn Writer`; `w: Box<dyn Writer>` | the trait's declaration | the trait's | as planned |
| `w.write()`, `w: impl Writer` (argument) | the trait's declaration | the trait's | as planned |
| `make().write()`, `fn make() -> impl Writer` | the **impl's** method (`impl Writer for JsonWriter`), not the trait's: the engine sees through a return-position `impl Trait` | the trait's | **differs**: an opaque return type is resolved to its hidden type |
| `self.step()` in a trait's default method | the trait's declaration of `step`, subst `Some` | the trait's | as planned |
| `Foo::new()` (inherent) | `R`: the function, container "inherent impl", subst `Some`; `C`: `Function` | the function | as planned |
| `W::make()`, `W: Make` | `R`: the **trait's** declaration; `C`: the same | the trait's | as planned |
| `<JsonWriter as Make>::make()` | `R`: the **impl's** `make`; `C`: the **trait's** `make` | (not stated) | **`R` and `C` disagree**: `R` is the more specific |
| `<Bar as Default>::default()` and `Default::default()` with `let b: Bar = …` (hand-written `impl Default for Bar`) | `R`: the impl's `default`, subst `Some`; `C`: the trait's `Default::default` | "unknown: write down what comes back" | **answer**: with the type known or inferred, `R` gives the impl's method; `C` gives the trait's. The inferred case is resolved |
| the same on `#[derive(Default)] struct Foo` | `R` and `C`: a `Function` whose container is an `impl Default for _` but whose `HasSource` is **the trait's declaration in `core`** (not a macro file), subst `None` | (not stated) | **a derived impl has no source of its own**: `source()` points at the trait item. A call to a derived method is External or "derived"; do not identify the callee by `source()` |
| `Pair(1, 2)` | `R`: `Adt Pair`; `C`: `TupleStruct(Pair)` | `TupleStruct` | as planned |
| `E::V(1)` | `R`: `Variant V`; `C`: `TupleEnumVariant(V)` | `TupleEnumVariant` | as planned |
| `Self(1, 2)` in `impl Pair` | `R`: **`SelfType`** (not the struct); `C`: `TupleStruct(Pair)` | the struct | **differs** for `R`; use `C` |
| `Rec { a: 1 }`, `E::R { a: 1 }`, `Self { a: 1 }` | `resolve_path`: `Adt Rec`, `Variant R`, **`SelfType`**; **`resolve_variant(RecordExpr)`**: `Variant Rec`, `Variant R`, `Variant Rec` (`hir::Variant` covers a struct too) | `resolve_path` on the path | **use `resolve_variant`**: it also answers `Self { .. }` |
| `f(1)`, `let f = \|x\| x` | `R`: `Local`; `C`: `Closure` | `Closure` | as planned |
| `(h.cb)(1)`, `cb: fn(u8) -> u8` field | no path (a `ParenExpr`); `C`: `FnPtr` | `FnPtr` | as planned |
| `f()`, `f: fn()` | `R`: `Local`; `C`: `FnPtr` | `FnPtr` | as planned |
| `f()`, `f: F`, `F: Fn()` | `C`: `FnImpl(Fn)` | `FnImpl` | as planned |
| `f(1)`, `f: &dyn Fn(u8) -> u8` | `C`: `FnImpl(Fn)` | (not stated) | `FnImpl`, not a different kind |
| `xs.into_iter().map(parse)`, `spawn(worker)` | `resolve_path` on the path expression: `Function` (free) | `Def(Function)` | as planned |
| `.map(Self::helper)`, `.map(Foo::helper)` | `resolve_path`: `Function`, container "inherent impl" | `Def(Function)` | as planned |
| `.map(Wrapper)` (tuple struct as a function) | `resolve_path`: `Adt Wrapper` | the struct | as planned |
| `println!("{}", f())` | `MacroCall` expands (56 bytes). In the expansion: `$crate::io::_print(…)` (the macro's own call, **`original_range_opt` = `None`**), `format_args_nl!` (expands again), and `f()` with `original_range_opt` = exactly `f()` | the call maps to the `f()` written in the file | as planned |
| `vec![g()]` | the expansion holds three calls of `vec!`'s own (`box_assume_init_into_vec_unsafe`, `write_box_via_move`, `Box::new_uninit`, all `None`) and `g()` (exact) | the same | as planned |
| a call in the **body** of a project `macro_rules!` (`inner_call()`) | the call resolves; **`original_range_opt` = `None`** | "maps to nothing smaller than the macro call" | **answer to RS4 §3**: a call written in a macro body has no original range. The call site to report is the **macro call's** range (the `body_call!()` in the file) |
| a call to a function a `macro_rules!` generated (`generated()`) | `R` and `C`: `Function`, `HasSource::source` has `file_id.is_macro() == true` | a macro file | as planned |
| a function under `#[tokio::main]`, proc macros off | the body is the original body: `work()` resolves, `original_range_opt` exact | found, from the original body | as planned |
| `a + b` (a user `Add`), `x?`, `*p`, `for y in it`, `.await` | **no** `CallExpr` and no `MethodCallExpr` node for any of them | none is a call | as planned |

The rule these rows give RS4: **a site's range is `original_range_opt` of the callee expression (call) or of the name (method call); `Some` means written in the
file; `None` means the call comes from a macro's own body, and the macro call's range is the site.** For a path call prefer `resolve_path_with_subst`; when it is
`None` or returns a local or a closure, use `as_callable().kind()`; for a record use `resolve_variant`.

### The oracle: `Analysis::outgoing_calls`

Called on eight functions of the fixture (a concrete method, an impl-return chain, an inherent associated call, a tuple struct, an iterator chain with a
closure, `println!`, a macro-generated callee, a `#[tokio::main]` body). **Its ranges are a subset of ours on all eight**, and they are comparable: the oracle's
range is the **name** of a method call and the **whole callee expression** of a call (`Foo::new`, not just `new`), the same two ranges the table above uses.
Where the sets differ: the oracle never reports `f()` inside `println!(…)` (ours does), because it walks `fn_body.syntax().descendants()` and never expands macros.
It also drops a call it cannot turn into a navigation target (no target, no entry). So the subset check is the right oracle test, and "the oracle finds
everything" is not.

### Cancellation (RS5 depends on this)

A query running in another thread is cancelled by unwinding with a `salsa::Cancelled` payload. `ra_ap_ide_db::base_db::salsa::Cancelled` is public; its
`Cancelled::catch(f)` takes an `UnwindSafe` closure (use `AssertUnwindSafe`) and returns `Result<T, Cancelled>`. `Cancelled` is `Local`, `PendingWrite` or
`PropagatedPanic`. Two ways to cause it, measured on a cold query (the first walk, about 2.5 s) cancelled 400 ms in:

| How | What the reader gets | Time from request to return | The same handle afterwards |
|---|---|---|---|
| `db.cancellation_token()` (per handle, `Clone + Send`), then `token.cancel()` from any thread | `Err(Local)` | **0.9 ms** | **usable**: a second walk on it returned `Ok` (the token is reset when the next top-level query starts) |
| a writer: `db.trigger_cancellation()` (or `apply_change`) with `&mut RootDatabase`; it blocks until every other handle is dropped | `Err(PendingWrite)` | 1.6 ms | **not usable**: the second walk failed with `PendingWrite` again; the handle must be dropped and a new one cloned |
| neither | `Ok` after 2.35 s | | |

RS5's per-request handle is `db.clone()` (`RootDatabase: Clone`, a cheap storage handle) plus its token, and a cancelled request ends in `Err(Local)` with the handle
still good. A text change is a writer: the old handles die with `PendingWrite`. Everything outside `ide` is `attach_db` + `Cancelled::catch` around the walk.

### Load numbers (three projects, release, `core/`-style loads)

| Project | Packages in the graph (workspace + sysroot) | Load | First walk (all project files) | Second walk, a new `Semantics` on the same database | Peak memory |
|---|---|---|---|---|---|
| this repo's `core/` (13 files walked) | 39 (26 are the sysroot) | 0.72 s | 3.3 s | **8.7 ms** | 474 MB |
| `languages/typescript/` (37 files, a git dependency) | 113 | 0.87 s | 6.6 s | 22.8 ms | 767 MB |
| `languages/rust/spikes/` (9 files; the outside project, 232 packages) | **232** | 0.94 s | 7.3 s | 4.7 ms | 840 MB |

(Peak memory is `/usr/bin/time -l`; `ps` RSS fell during the third run because macOS compresses memory, so it is not used.) A second `Semantics` on a loaded
database costs milliseconds and no memory: the work is in the database, not in `Semantics`. **Everything 100%-resolved**: 978/978 calls and 914/914 method calls on `core/`,
1762/1762 and 2997/2997 on `typescript/`, 140/140 and 307/307 on `spikes/` (calls that are written outside macros).

### Scenario cost (decides how RS4's scenarios load)

Thirteen one-package programs, each with a `Box<dyn Writer>`, a `vec!` and a `println!`:

| Option | Time for 13 | Resolves `std`? |
|---|---|---|
| accept: one load per scenario | **39 s** (3.0 s each: 0.3 s load + 2.5 s first query, which is `std` being analysed) | yes |
| **one workspace for all** (a generated `Cargo.toml` with `members = ["s*"]`) | **3.0 to 3.5 s** in total, 445 MB | yes |
| a sysroot-less load, one per scenario | 3.0 s | **no**: 0 of 2 method calls resolve (`Box`, `Vec`, `String` are unknown) |

The `std` cost is paid once per process, not once per package. Choose **one shared workspace**, built in a temporary folder by the test harness (it copies the
scenario folders in and writes the `[workspace]` manifest there). The sysroot-less option is a trap and is not an option. The repo currently has six scenarios; the
numbers scale with the process, not with the count.

### `Cargo.lock`: what a load writes, and the three checks of RS3 §5

Confirmed on `ra_ap_project_model 0.0.358` with `toolchain 1.99.1`, 1.98.0, 1.94.1 and 1.74.1 (the rows are one-package fixtures with an `itoa` dependency):

| Fixture | Toolchain (decided by `cargo --version` run in the project's folder) | Result |
|---|---|---|
| `core/` (lock committed) | 1.99.1 | `Cargo.lock` byte-identical, mtime unchanged, `git status` clean |
| a one-package fixture, lock from an earlier load | 1.99.1 | byte-identical |
| **stale** lock (the manifest has `itoa`, the lock does not) | 1.99.1 | **byte-identical**, and `itoa` still resolves (the engine worked on a copy) |
| stale lock | 1.94.1 (≥ 1.82, < 1.95: `--lockfile-path`) | byte-identical |
| stale lock | **1.74.1 (< 1.82)** | **rewritten in the project**: `itoa` entries added. No copy |
| **no lock** | 1.99.1 | `Cargo.lock` **written into the project** (365 bytes with `itoa`) |

So: a project **with** a lock is never touched by a toolchain ≥ 1.82; a project **without** one gets a file, whatever the toolchain.

The three checks of RS3 §5, with `CARGO_RESOLVER_LOCKFILE_PATH=<temp>/Cargo.lock` put in `CargoConfig::extra_env`:

1. **The sysroot still loads, and a project with a lock still uses the engine's copy.** With the variable set on 1.99.1: `sysroot error: None`, 26 sysroot packages, calls resolve
   (`itoa` too). With a stale lock the project's lock stayed byte-identical **and the redirect path stayed empty**: the engine sets its own value for the variable after `extra_env`, so
   its copy wins.
2. **Cargo creates the lock at the path when no file is there.** Yes: with no lock, on 1.99.1 and on 1.98.0, `<temp>/Cargo.lock` was created (with the `itoa` entries) and **the
   project had no `Cargo.lock` afterwards.** The control run without the variable wrote the file into the project.
3. **Which toolchain version decides: the project's.** The engine runs `cargo --version` with the manifest's folder as the working directory
   (`toolchain_info/version.rs`), so a `rust-toolchain.toml` in the folder counts. Observed: **1.98.0 honours the variable** (file created at the path, none in the project);
   **1.94.1 ignores it** (the lock was written into the project), also when `rust-toolchain.toml` in the project selected 1.94.1 while the loader ran under another toolchain.
   1.97 itself (the plan's boundary, from `make_lockfile_copy`) is not installed here: only "1.98.0 yes, 1.94.1 no" is observed. So option B works from 1.97 (observed ≥ 1.98), and below it the fallback is the
   rule-2 exception reported by `wrote_lockfile()`.

### Smaller facts

- **Load writes nothing else** into a project that has a lock (checked by `git status` and hashes). The sysroot's own metadata uses `--lockfile-path` (cargo's deprecation warning for the flag shows up in the sysroot's `metadata_error`
  when offline).
- **API names that exist in 0.0.358** (checked by compiling and running): `Semantics::new`, `parse`, `attach_first_edition_opt`, `resolve_expr_as_callable`,
  `resolve_method_call`, `resolve_method_call_fallback`, `resolve_path`, `resolve_path_with_subst`, `resolve_variant(RecordExpr) -> Option<Variant>`, `type_of_expr`,
  `expand_macro_call`, `original_range`, `original_range_opt`; `hir::CallableKind::{Function, TupleStruct, TupleEnumVariant, Closure, FnPtr, FnImpl}` (note `TupleEnumVariant`
  holds an `EnumVariant`); `AnalysisHost::with_database`, `Analysis::outgoing_calls(&CallHierarchyConfig, FilePosition)`. **`resolve_record_literal` does not exist.**

## RS3: loading a workspace

### How `sem::load` finds the standard library (step 1, f4064e4)

`CargoConfig.sysroot = RustLibSource::Path(<rustc --print sysroot>)`, run in the manifest's folder (so a `rust-toolchain.toml` there counts)
with `RUSTUP_AUTO_INSTALL=0`. `ProjectWorkspace::load` maps `(Path(dir), None)` to `Sysroot::discover_rust_lib_src_dir(dir)`
(`ra_ap_project_model-0.0.358/src/workspace.rs` ~237), which looks for `lib/rustlib/src/rust/library` and records "can't find standard
library sources" in `sysroot.error()` when it is not there; it never runs `rustup`. Only `RustLibSource::Discover` goes through
`discover_rust_lib_src_dir_or_add_component` (`sysroot.rs` ~467), which runs `rustup component add rust-src`. So `sem` does not check the
folder itself, and `sysroot_src` stays `None`. The engine sets `RUSTUP_AUTO_INSTALL=0` on its own `cargo` and `rustc` commands
(`ra_ap_toolchain`, `NO_RUSTUP_AUTO_INSTALL_ENV`). A manifest path is made absolute and cleaned of `.` and `..` by text:
`std::path::absolute` keeps `..`, and then no loaded file starts with the root and `files()` is silently empty.

### In-project edges by load mode (step 1, 2026-11-16)

The measurement RS3 §5 asked for before Q-RS6. A throwaway probe (`languages/rust/spikes/rs3-probe/`, deleted after this was written down),
release, `set_test` on, one process per load. Every call site written outside a macro, resolved as in S-RS2. A target is **project code**
when its crate is `CrateOrigin::Local` and its file is under the root; a constructor counts as a target; a def made by a macro counts as
external. Modes: **a** full; **b** `CargoConfig::no_deps = true`; **c** no `std` (sysroot `None`); **d** both (close to the ladder's
rung 3). Re-counted by the reviewer from the probe's per-site dumps; a site's class was compared between modes by its position.

| Project | Mode | Sites | Same crate | Same package, other target | Cross-package | External | Function-typed | Unresolved | Load | Peak memory |
|---|---|---|---|---|---|---|---|---|---|---|
| `core/` | a | 2035 | 781 | – | 45 | 1182 | 27 | 0 | 0.95 s | 474 MB |
| | b | 2035 | 768 | – | **0** | 179 | 25 | 1063 | 0.50 s | 351 MB |
| | c | 2035 | 686 | – | 34 | 4 | 11 | 1300 | 0.22 s | 85 MB |
| | d | 2035 | 686 | – | 0 | 0 | 11 | 1338 | 0.10 s | 35 MB |
| `languages/typescript/` | a | 4654 | 1152 | 105 | 16 | 3284 | 97 | 0 | 0.95 s | 763 MB |
| | b | 4654 | 1095 | 104 | **0** | 373 | 96 | 2986 | 0.64 s | 350 MB |
| | c | 4654 | 862 | 81 | 9 | 302 | 88 | 3312 | 0.48 s | 374 MB |
| | d | 4654 | 862 | 81 | 0 | 0 | 88 | 3623 | 0.42 s | 36 MB |
| the outside project of S-RS2 (232 packages) | a | 463 | 19 | 2 | 7 | 421 | 14 | 0 | 4.3 s | 854 MB |
| | b | 463 | 19 | 2 | **0** | 45 | 14 | 383 | 0.76 s | 375 MB |
| | c | 463 | 8 | 1 | 7 | 50 | 14 | 383 | 2.2 s | 588 MB |
| | d | 463 | 8 | 1 | 0 | 0 | 14 | 440 | 0.18 s | 35 MB |

Site totals differ from S-RS2's calls plus method calls (1892 on `core/`, 4759 on `typescript/`; up on one, down on the other): the code
changed since and the two probes count sites differently. Not reconciled; the counts that decide are each mode against mode a of the same
run. Mode a has 0 unresolved on all three, as S-RS2 found.

**In-project edges lost, against mode a** (core 826, typescript 1273, outside 28):

| Mode | `core/` | `languages/typescript/` | outside |
|---|---|---|---|
| b `no_deps` | −58: **all 45 cross-package**, 13 same-crate | −74: **all 16 cross-package**, 57 same-crate, 1 same-package | −7: **all 7 cross-package** |
| c no `std` | −106: 11 cross-package, 95 same-crate | −321 | −12 |

- **Cross-package (the [check] of RS3 §5, option D): a call from one workspace member into another does not resolve under `no_deps`.**
  Every one of the 45 / 16 / 7 becomes `Unresolved`, and every one is a plain call by path (`EdgeTo::External(..)`, `render(&graph, ..)`,
  `ContextId::of(..)` from `analyzer-testkit` into `analyzer-core`; `Dialect::from_path` from the CLI and tests; the spike crates into
  `s1_hir::load`). Not a type question: `--no-deps` leaves `metadata.resolve` empty, and the engine builds every crate's dependencies from
  it (`cargo_workspace.rs`), so the members have no dependency edge between them.
- **Same-crate losses are `std`-typed**: `line_at(text, at)?.slice(text)` (the `?`), `all.iter().map(|c| c.as_str())` (a closure
  parameter of an adapter), `for child in &node.children { child.edge_marks() }`, `Project::open(..).unwrap().sem.call_sites(..)`. No lost
  edge needed a third-party crate's type.
- **`no_deps` also cripples `std`.** `ProjectWorkspace::load` passes `config.no_deps` to the sysroot's own `cargo metadata`
  (`workspace.rs` lines 410, 514, 525, 587 into `Sysroot::load_workspace`), so `std`, `alloc` and `core` lose their links to each other:
  under b on `core/`, 649 `core`, 291 `alloc` and 20 `std` calls went unresolved, and only 179 external calls resolve. A "no project
  dependencies, working `std`" load cannot be asked for through `CargoConfig`.
- **The engine's own offline fallback loses the same cross-package edges, but keeps `std`** (read from the source, not measured):
  `FetchMetadata` always pre-fetches `--no-deps` metadata, and when the full `cargo metadata` fails it returns that one
  (`cargo_workspace.rs` ~831, "This makes r-a still work partially when offline"). That `no_deps` is local to the project's fetch; the
  sysroot is loaded with `config.no_deps`, which stays `false`. So under `DependenciesMissing` (D37, the ladder's rung 2) `std` works,
  and **every call between the workspace's own crates is `Unresolved`**.

### Old toolchains: what a load loses (step 5b, 2026-11-17)

Run once by hand (`tests/toolchains.rs`, `worker_measure`) on `fixtures/measure/`: edition 2021 (cargo 1.74 cannot read 2024), a project
method `run` reached through `Option::unwrap` and through `Vec::iter`. Two runs each; the second is shown (the first paid cold caches).

| Toolchain | Loads | `std` found | `unwrap`, `iter` resolved | `run` resolved | Load |
|---|---|---|---|---|---|
| 1.74.1 | yes | yes | 2/2 | 2/2 | 604 ms |
| 1.94.1 | yes | yes | 2/2 | 2/2 | 362 ms |
| 1.98.0 | yes | yes | 2/2 | 2/2 | 889 ms |
| 1.99.1 | yes | yes | 2/2 | 2/2 | 600 ms |
| 1.74.1 substituted by 1.99.1 | yes | yes | 2/2 | 2/2 | 732 ms (1117 ms first) |

**Nothing is lost for calls down to 1.74.1** (one small project and two `std` paths, not a broad sample). What changes below a version is
the lockfile, and that sets the minimum: **1.82**. Below it the engine makes no copy of the lock, and cargo rewrites a stale committed
`Cargo.lock` in place (S-RS2, "Cargo.lock"), a write into the user's tree that nothing else prevents. From 1.82 a project with a lock is
never touched; below 1.97 a project with none gets one, said by `LockfileWritten` (D38). Substitution adds `rustup toolchain list` and one
`cargo --version` per toolchain, about 100 to 400 ms, only when the project's toolchain is too old or not installed. 1.74.1, 1.94.1 and
1.98.0 were uninstalled after the run.

### rustup facts (`rustup 1.29.3`, found in step 5b)

- **`rustup toolchain list` installs a missing toolchain** when it runs in a folder whose `rust-toolchain.toml` names one that is not
  installed (rustup 1.28+: the directory override is resolved first). Found by doing it by hand: it started downloading 1.50.0, which was
  stopped and removed. So `toolchain::installed` runs it from a neutral folder with `RUSTUP_AUTO_INSTALL=0`, and every `cargo` and `rustc`
  we start sets `RUSTUP_AUTO_INSTALL=0` (`toolchain::command`). Any new `rustup` call needs the same.
- A default that is also the active toolchain is printed `(active, default)`, not `(default)`: read the words inside the parentheses.
- A pinned toolchain that is not installed makes `cargo --version` fail with `error: toolchain '<name>' is not installed` on stderr;
  `toolchain::probe` reads the name from that text (English, rustup's wording: a change makes the probe `Unreadable`, and the load then
  fails as `OpenError::Load` until step 7).
- `RUSTUP_TOOLCHAIN` in `CargoConfig::extra_env` is left alone by the engine (`Sysroot::tool`), so one variable moves every tool, ours and
  the engine's, to the chosen toolchain.

### Rung 3: `load_inline` with no toolchain (step 7 checks, 2026-11-17)

A throwaway probe (`languages/rust/spikes/rung3/`, deleted after this was written down) turned each `Cargo.toml` into a `ProjectJson` and
loaded it with `ProjectWorkspace::load_inline`, sysroot `None`.

1. **No `rustc`, no `cargo`, an empty `HOME`: the engine falls back, it does not fail.** Run with `env -i PATH=/usr/bin:/bin HOME=<empty>`:
   loaded in 4 ms, no panic. In `load_inline` (`workspace.rs` ~489) `version::get(..).ok().flatten()` gives no toolchain,
   `rustc_cfg::get` logs and returns `vec![]` when `rustc --print cfg` fails (`toolchain_info/rustc_cfg.rs`), and `target_data::get` keeps
   its error in `workspace.target` (a `Result`: "unable to fetch target-data-layout"). **The cost is the empty cfg list**: `#[cfg(unix)]`
   and `debug_assertions` code is inactive, so a call inside a `#[cfg(unix)]` function did not resolve. Giving each crate the host cfgs in
   the JSON (`"cfg": ["unix", "debug_assertions", "target_os=\"macos\"", …]`) made it resolve. `cfg(test)` works without that:
   `set_test` applies to JSON crates too. With `rustc` on `PATH` (still no sysroot) the engine reads the cfgs itself (65 atoms), in 366 ms.
2. **Without `std`, project types are enough.** On two hand-built crates, 14 of 14 sites resolved: `self.validate()`, `Self::new()`,
   `Order::new().submit()`, `let o: Order = …; o.submit()`, a function by path, and `shop::Order::new().submit()`, `shop::helper()` across
   the crates, in code that uses no `Option`, `String` or `Vec`.
3. **In-project edges, rung 3 against rung 1** (an edge is a call site resolved to project code under the root):

| Project | Resolution | Rung 1 | Rung 3 | Lost |
|---|---|---|---|---|
| `core/` | callable only (step 1's probe) | 826 | 720 | 106 (13%) |
| `core/` | path first (RS4's policy) | 881 | 763 | 118 (13%) |
| `languages/rust/` | callable only | 437 | 342 | 95 (22%) |
| `languages/rust/` | path first | 437 | 416 | 21 (5%) |

   Rung 3 never gained an edge, and its callable-only numbers on `core/` (686 same-crate, 34 cross-package) equal step 1's "no `std`"
   column exactly. What it loses is receivers typed through `std`: `line.slice(text)?`, `all.iter().map(|c| c.as_str())`,
   `loaded.no_std_source().map(str::to_owned)`, `for x in …`, `let Some(..) = … else`, a call inside a `Vec::push(…)` argument. Loads took
   18 to 76 ms on the real workspaces against about 900 ms for rung 1.
   **Step 1's table undercounts**: its probe resolved a call by path only through `resolve_expr_as_callable`, which fails when the
   function's signature names `Option` or `String`; `resolve_path` first (RS4's policy) lifts rung 1 on `core/` from 826 to 881. D38 does
   not change: under `no_deps` every cross-package edge was lost whatever the method.
4. **A JSON crate sees only its root module's folder** by default (`src/`): a `#[path = "../../tests/common/mod.rs"]` module was not
   loaded, and six edges were lost, until each crate's `source.include_dirs` was set to its whole package folder (without `target/`).
