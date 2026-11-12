# 04.01 What rust-analyzer gives, and what needs our work

```
what rust-analyzer gives
├── 1. what rust-analyzer is, as a library
├── 2. the crates we use
├── 3. exported: use as it is
├── 4. needs our work
├── 5. four facts that shape the design
├── 6. risks
└── 7. what the context layer will ask of it
```

Read from `../language_clone/rust-analyzer` at commit `2a362a4`.

## 1. What rust-analyzer is, as a library

The Rust language server (`rust-lang/rust-analyzer`, MIT OR Apache-2.0). Unlike tsrs, it is **layered on purpose**, and its top layer, the
`ide` crate, is a designed API: plain data in, plain data out, no LSP types. Its crates are published to crates.io every week with an `ra_ap_`
prefix. The API is not stable (any week may break it), but it is meant to be called.

It is built on `salsa`, like ty: an incremental database. Change one file, and only what depended on it is recomputed. Cancellation is built in.

## 2. The crates we use

```
rust-syntax      ra_ap_syntax  (+ ra_ap_parser, rowan)                     the lossless syntax tree. No salsa.

rust-analysis    + ra_ap_hir            Semantics: resolve a call, a path, a method; types; impls. THE layer we call
                 + ra_ap_ide_db         RootDatabase (the salsa database `Semantics` reads)
                 + ra_ap_load-cargo     load a Cargo workspace into a database
                 + ra_ap_project_model  cargo metadata, the sysroot, finding the manifest
                 + ra_ap_vfs            paths ↔ file ids
                 (dev only) ra_ap_ide   `Analysis::outgoing_calls`, as the cross-check of RS4's proof
```

The first plan called `ide` "the designed API" and meant to stay on it. Reading `crates/ide/src/call_hierarchy.rs` changed that: its
`outgoing_calls` is about 60 lines over `hir::Semantics`, it **drops** every site it cannot resolve and every construction, and it gives no
type arguments. We need all three, in layer 1 already (rule 8) and for layer 2. So `sem.rs` is written on `hir`, and `ide` stays as a test
oracle: for every fixture, the sites the engine's own call hierarchy reports must be a subset of ours ([RS4](04-default-resolver/05-rs4-calls.md) §8).
Whether leaving `ra_ap_ide` out of the release binary makes it smaller is S-RS1's **[measure]**.

## 3. Exported: use as it is

### For `rust-syntax`

| We need | rust-analyzer gives | Notes |
|---|---|---|
| parse one file | `syntax::SourceFile::parse(text, edition) -> Parse<SourceFile>` | error-tolerant; `Parse::errors()` |
| walk the tree | typed nodes: `ast::Fn`, `ast::Impl`, `ast::Trait`, `ast::Struct`, `ast::Enum`, `ast::Module`, `ast::Const`, `ast::Static`, `ast::MacroRules`; `HasName`, `HasAttrs`, `HasDocComments`, `HasVisibility` | generated from the grammar (`ungrammar`) |
| ranges | `node.syntax().text_range()`, UTF-8 bytes | attributes, doc comments **and plain comments directly above an item** (no blank line between) are part of the item's node. The rule is `n_attached_trivias` in `crates/parser/src/shortcuts.rs` |
| comments above an item | the item node's first tokens (`SyntaxKind::COMMENT`, `WHITESPACE`) | the tree is lossless: every byte is in it. The parser's "a blank line detaches a comment" rule is the same as our marker rule, so the item's doc lines (where the marker lives) are simply the node's first children |
| memory | ordinary Rust ownership: `Parse` is dropped, memory is freed | no arenas to manage |

### For `rust-analysis`

| We need | rust-analyzer gives | Notes |
|---|---|---|
| load a project | `load_cargo::load_workspace_at(root, &cargo_config, &load_config, &progress) -> (RootDatabase, Vfs, Option<ProcMacroClient>)` | runs `cargo metadata`, finds the sysroot |
| the query API | `hir::Semantics::new(&db)` over the `RootDatabase` that loading returned. (`ide::AnalysisHost` / `Analysis` wrap the same database; we use them only in the oracle test) | `Analysis` is a snapshot; every method returns `Cancellable<T>` |
| an outline, if wanted | `Analysis::file_structure(&config, file_id) -> Vec<StructureNode>` | we use our own rule (TS1's reasoning applies); this is a cross-check |
| **what a function calls** | `Analysis::outgoing_calls(&config, FilePosition) -> Option<Vec<CallItem>>` | the engine's own call hierarchy. **Our test oracle, not our resolver** (§2). `CallItem { target: NavigationTarget, ranges: Vec<FileRange> }` |
| the same, by hand | `hir::Semantics::resolve_method_call(&ast::MethodCallExpr) -> Option<hir::Function>`; `sema.type_of_expr(&expr)` → `.as_callable(db)` → `CallableKind::{Function, TupleStruct, TupleEnumVariant, Closure, FnPtr, FnImpl}`; `sema.resolve_path` | this is what `outgoing_calls` is made of (`crates/ide/src/call_hierarchy.rs`, about 60 lines). **This is what we call** |
| the same, with the type arguments of the call | `resolve_method_call_fallback(&call) -> Option<(Either<Function, Field>, Option<GenericSubstitution>)>`; `resolve_path_with_subst(&path)` | the plain functions are these with the substitution thrown away (`semantics.rs`). Layer 1 calls these and ignores the substitution; RC1 reads it |
| a trait method → the impl's method | inside `resolve_method_call`: `SourceAnalyzer::resolve_impl_method_or_trait_def` asks `lookup_impl_method` with the call's substitution. So `json_writer.write()` gives the method in `impl Writer for JsonWriter`, and `w.write()` with `w: &T` gives the trait's declaration (read from the source; **[check]** in S-RS2) | answers the first plan's open question |
| a declaration's place | `NavigationTarget { file_id, full_range, focus_range, name, kind, container_name }`; `hir::Function::module(db)`, `.name(db)`, `HasSource` | |
| project code or external | the file's `SourceRoot::is_library` | registry crates and the sysroot are library roots |
| macros | `sema.descend_into_macros(token)`: calls *inside* a macro call's arguments are found (`println!("{}", f())`) | built into `outgoing_calls` |
| edits and unsaved text | `host.apply_change(change)` with the new file text | incremental for free |
| cancel | applying a change cancels running queries; `Cancellable<T>` returns `Err(Cancelled)` | built in |

## 4. Needs our work

| # | Work | Why | Phase |
|---|---|---|---|
| 1 | **the outline rule and qualnames** | `impl` blocks have no name: `impl Foo { fn bar }` must become `Foo::bar`, and `impl Display for Foo { fn fmt }` must not collide with `impl Debug for Foo { fn fmt }` | RS1 |
| 2 | **the marker** | a `/// ID: …` line in the item's doc comment (D6): reading it, and placing it after the last doc line without gluing it to the user's paragraph | RS2 |
| 3 | **per-site results in source order** | `outgoing_calls` groups by callee. Each `CallItem` keeps its `ranges`, so regrouping by range is easy; or write the 60-line walk ourselves on `Semantics` to also get construction (`Foo { .. }`) and unresolved sites, which `outgoing_calls` silently drops | RS4 |
| 4 | **`NavigationTarget` / `hir::Function` → our identity** | the bridge to the outline, by file + range, as in TypeScript | RS4 |
| 5 | **loading that is fast enough and honest about what it skipped** | build scripts and proc macros off by default; say so in `Stats` | RS3 |
| 6 | **the facade and contract tests** | weekly breaking releases | every phase |

Smaller than TypeScript's list: no memory life cycle to design, cancellation and incrementality come with salsa, and the default resolver is a
public method.

## 5. Four facts that shape the design

### a) The user must have a Rust toolchain

Loading a Cargo workspace runs `cargo metadata` and reads the standard library's source from the sysroot (`rustup component add rust-src`).
Unlike tsrs (which bundles `lib.d.ts`), nothing is bundled. For Rust users that is a fair requirement, but it is a **user-machine dependency**:
`Project::open` must return a clear `OpenError::NoCargo` / `NoSysroot`, and tier 1–2 (`rust-syntax`) must never need it.

### b) Macros hide code from the parser

`rust-syntax` sees text. A function generated by a macro (`macro_rules!` that expands to `fn`s, a derive, an attribute macro that rewrites a
function) is **not in the outline**. That is correct for V-NOC: you cannot attach a doc or write a marker on code that is not in the file.
`rust-analysis` *does* see expanded code, so a call may resolve to a function that only exists after expansion. Policy: the target is reported
at the macro call that produced it when rust-analyzer can map it back (`original_range`), otherwise it is `External`-like: named, not entered.

### c) What loading costs depends on three switches

`LoadCargoConfig { load_out_dirs_from_check, with_proc_macro_server, prefill_caches, .. }`:

| Switch | Off (phase 1 default) | On |
|---|---|---|
| run build scripts (`load_out_dirs_from_check`) | fast; code under `include!(concat!(env!("OUT_DIR"), …))` is missing | runs `cargo check`: slow, may compile the project |
| proc-macro server | derives and attribute macros do not expand; calls inside `#[tokio::main]`-style rewritten functions are still found (the original body is used) **[check]** | needs the toolchain's proc-macro server; more complete |
| prefill caches | first query is slower | slower start |

Start with all three off, count what was skipped, and let real projects say which to turn on.

### d) `salsa` here is not Python's `salsa`

rust-analyzer pins `salsa 0.28.5` with its own feature set. This is the concrete reason for one workspace per language
([`01-monorepo/02`](../01-monorepo/02-workspaces-and-toolchain.md) §1). It never meets ruff's copy.

## 6. Risks

| Risk | How likely | What limits the damage |
|---|---|---|
| a weekly release breaks an item we call | certain | `=` pins; `sem.rs`; using `ide` (the most stable layer) wherever it is enough |
| the driver binary is large, and a cold load of a big workspace takes many seconds | certain | tier 1–2 does not link it; V-NOC's per-language extension; `Session` keeps the database alive |
| the user's toolchain is missing or broken | medium | fact (a): clear errors, and tier 1–2 unaffected |
| trait dispatch looks "wrong" to users (`x.fmt()` on a generic goes to the trait's declaration) | certain | the policy table says so; "show implementations" ([`02-tree-rules/03`](../02-tree-rules/03-the-seam.md) §7) is cheap here, rust-analyzer has go-to-implementations |
| our outline and rust-analyzer disagree about an item under `#[cfg(...)]` | medium | the outline lists what is in the text; a cfg-disabled function is in the outline and simply has no calls. Fixture: `cfg.rs` |

## 7. What the context layer will ask of it

Read from the clone for [`05-context-layer/`](05-context-layer/01-design.md); none of it is called in layer 1. Each line is a public item of
`ra_ap_hir` at `2a362a4`, and each is a **[check]** of [RC0](05-context-layer/02-rc0-spikes.md).

| Layer 2 needs | The engine has | Notes |
|---|---|---|
| the type arguments at a call | `GenericSubstitution::types(db) -> Vec<(Symbol, Type)>`, `all_types(db)` (also the unnamed `impl Trait` parameters; its comment says it is kept for external tools) | the parent impl's or trait's parameters come first, then the function's own |
| a caller's type written with the caller's parameters → the same type with them filled in | `Type::instantiate(args) -> Type` | "A `Type` remembers its origin. Trying to do anything (except comparing) with types of different origins will cause errors or panics. Instead, use the `instantiate` methods" (the doc comment of `hir::Type`) |
| a trait's method + concrete types → the impl's method | `Semantics::resolve_trait_impl_method(env: Type, trait_, func, subst) -> Option<Function>` | public, with a `FIXME: better api for the trait environment`: likely to move in a bump |
| is this type a parameter, a closure, a `dyn`, a function item? | `Type::as_type_param`, `as_closure`, `as_dyn_trait`, `as_callable` → `CallableKind`, `as_adt`, `type_arguments`, `as_reference`, `tuple_fields` | enough to walk a type's structure from outside |
| where a `dyn Trait` or a `fn` pointer is **born** | `Semantics::expr_adjustments(&expr) -> Option<Vec<Adjustment { source, target, kind }>>`, with `Adjust::Pointer(PointerCast::Unsize \| ReifyFnPointer \| ClosureFnPointer)` | the coercion the compiler inserted: `source` is `Box<JsonWriter>`, `target` is `Box<dyn Writer>`. This is the one fact value tracking starts from |
| the implementations of a trait | `Impl::all_for_trait(db, trait_)`, `Impl::all_for_type(db, ty)` | for the "possible targets" option |
| types as keys | `Type` implements `Hash` and `Eq` by hand | whether two spellings of one type compare equal (lifetimes, aliases) is S-RC1 |

Next: [`02-architecture.md`](02-architecture.md)
