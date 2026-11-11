# 04.02 Rust: the phases

```
rust phases
├── 1. the phases at a glance
├── RS0  spikes
├── RS1  outline: what is a def, qualnames
├── RS2  markers
├── RS3  project loading
├── RS4  call graph: the walk, the policy table
└── RS5  what the driver needs
```

Rust follows the same phases as TypeScript. This file only says what is **different**; where a section is short, the TypeScript file with the
same number is the full version.

## 1. At a glance

| Phase | Builds | Crate | Proof | Size next to TypeScript's |
|---|---|---|---|---|
| **RS0** | three spikes | scratch | answers in `ENGINE.md` | smaller |
| **RS1** | `outline(text, edition)` | `rust-syntax` | snapshots; no `salsa` in `cargo tree` | similar: `impl` naming is the hard part |
| **RS2** | `marker`, `plan_marker`, header | `rust-syntax` | round trip, never panics | smaller: the core helper exists; the tree attaches comments for us |
| | **tag `rust-v0.1.0`** | | | |
| **RS3** | `Project::open(root)` | `rust-analysis` | a workspace loads; clear errors without a toolchain | smaller: `load_workspace_at` does it |
| **RS4** | `call_graph` | `rust-analysis`, `rust-cli` | scenarios at `Level::Default`; a fixture per policy row | smaller: the resolver is public, the solver is in core |
| | **tag `rust-v0.2.0`** | | | |
| **RS5** | `Session` | `rust-analysis` | tier-3 conformance | much smaller: budget, expand and cancel are in core after TS5; edits are `apply_change` |

Rust is planned **after** TypeScript's TS4 for one reason: by then `analyzer-core`'s solver, the testkit and the scenarios have had one real
user, and Rust is the second. If Rust needs something the core cannot express, that is the rule of two finding the real shared shape.

## RS0: spikes

| Spike | Question | Write down |
|---|---|---|
| **S-RS1** | Which `ra_ap_*` version matches the clone's commit, and does a crate depending on `ra_ap_ide` + `ra_ap_load-cargo` build on 1.99.1 on three operating systems? | the version number; cold build time; binary size **[measure]** |
| **S-RS2** | Load a real workspace (this repo's own `core/` is a fine first target, then something with 100+ crates in its dependency graph) with the three switches off. Call `outgoing_calls` on five functions | load time and memory **[measure]**; what each of these resolves to: a method on a concrete type, a method through `dyn Trait`, through a generic `T: Trait`, a closure being called, a function pointer, an associated function `Foo::new()`, a tuple-struct constructor, a call inside `println!`, a call to a macro-generated function |
| **S-RS3** | What does the user's machine need? Run the same load with no `rust-src` component, and with no `cargo` on the path | the exact error each case gives, to turn into `OpenError` variants |

S-RS2's output is the first draft of the policy table.

## RS1: the outline

```rust
// rust-syntax/src/lib.rs
pub fn outline(text: &str, edition: Edition, style: MarkerStyle<'_>) -> Outline;
pub enum Edition { E2015, E2018, E2021, E2024 }        // the driver reads it from Cargo.toml; default: the newest
```

### What is a def

| Syntax | Def? | Kind | Example qualname |
|---|---|---|---|
| `fn f()` at module level | yes | function | `f` |
| `struct S`, `enum E`, `union U` | yes | struct, enum, struct | `S`, `E` |
| `trait T`, and its methods (with or without a default body) | yes | trait, method | `T`, `T::m` |
| a method or associated function in `impl S { }` | yes | method | `S::m` |
| a method in `impl T for S { }` | yes | method | `<S as T>::m` |
| `mod m { }` (inline) | yes | module | `m`; its items are `m::f` |
| `type A = …`, `const C`, `static X` | yes | type, other(`const`), other(`static`) | `A`, `C` |
| `macro_rules! m` | yes | other(`macro`) | `m!` |
| a function inside a function | yes | function | `outer::{fn}::inner` |
| an `impl` block itself | **no** | | it has no name; it is only a naming context for its items |
| a closure | no | | rule 3: its calls belong to the enclosing def |
| enum variants, struct fields | no for now | | adding later is safe |
| items produced by a macro | no | | not in the text (fact b of the last file) |

### Qualnames

Frozen at `rust-v0.1.0` (rule 12). Rust has its own path syntax, so the joiner is `::`, and the names are the ones `rustc` itself prints in
error messages:

| Case | Qualname | Why this form |
|---|---|---|
| inherent impl | `S::m` | how you would call it |
| trait impl | `<S as T>::m` | Rust's own "fully qualified syntax"; `impl Display for S` and `impl Debug for S` both have `fmt`, and this tells them apart |
| generic type | `S::m` for `impl<T> S<T>`; the type's generic arguments are dropped | `impl S<u8>` and `impl S<u16>` with the same method name are true duplicates here; `occurrence` separates them. Simple and stable beats exact |
| impl for a reference, slice, tuple, `dyn Trait` | the type's source text, whitespace normalised: `<&S as T>::m`, `<[T] as Sum>::sum` | no better name exists |
| nested module | `a::b::f` for inline modules. A file module (`mod a;`) adds **nothing**: the file is already the `module` part of `StableDef::Path` | a moved file must keep its qualnames (V-NOC plan `09/02`) |
| local item | `outer::{fn}::inner` | the function-body joiner, as `.<locals>.` in Python and TypeScript, written in Rust's style. Cannot collide with a real path |
| `#[cfg]` twins | the same qualname twice; `occurrence` separates them | |

Decided (D10): `<S as T>::m`. It is displayed in V-NOC's UI and stored in `.vnoc/`.

### Ranges

`range` is the item node's range **without** its leading plain comments, starting at the first doc comment or attribute: here the driver
contract's wording ("from its first decorator / attribute / doc comment") fits Rust exactly, because doc comments are attributes in Rust. The
marker is one of those doc lines (RS2), so it is inside the range.

## RS2: markers

**D6: the marker lives in the doc comment** (the same decision as Python's docstring and TypeScript's JSDoc,
[`03-typescript/04-ts2-markers.md`](../03-typescript/04-ts2-markers.md) §2). The comment-above placement is the later option described there in §8.

```rust
/// Parses an order.
///
/// ID: 3RGJ8M2K…                ← the marker: the last line of the item's doc comment
#[inline]
pub fn parse_order(raw: &str) -> Order {
```

| Point | Rule |
|---|---|
| reading | among the item's own outer doc comments (`///` lines, or one `/** */` block), the first line that reads `<label>: <text>` after the `///` (or the leading `*`) is the marker. The parser already attached exactly these comments to the item node (last file, §3), so this is a loop over the node's first children |
| planning, the item has `///` lines | add `/// <label>: <text>` after the **last** doc line. If that last line is not empty, add an empty `///` line first: in Markdown a line directly under a paragraph joins it, and the marker would be glued to the user's sentence in rustdoc |
| planning, the item has a `/** */` block | the same text work as TypeScript's JSDoc: the last line before `*/`. This is the second user of that helper, so it moves to `analyzer-core` now (rule of two) |
| planning, no doc comment | one new line `/// <label>: <text>` directly above the item's first attribute or keyword |
| doc comments written as attributes (`#[doc = "…"]`) | not read as a marker, and not edited. A new `///` line is added above them |
| inner doc comments (`//!` inside a module or function body) | belong to the enclosing item, not to this one. Never read, never written |
| `////` (four slashes) and plain `//` comments | not doc comments in Rust. Not read |
| `rustdoc` and `#![warn(missing_docs)]` | the marker line is published in the API docs: the accepted cost of D6. A side effect to know: an item with only a marker line now "has docs" for the `missing_docs` lint |
| `rustfmt` | does not reflow `///` lines unless `wrap_comments` is set, and a short line is never wrapped. Test: run `rustfmt` on every marked fixture and assert the marker line is unchanged **[check]** |
| items inside a macro call (`lazy_static! { … }`, `cfg_if! { … }`) | not defs, so never marked |
| `Skip` | the same reasons as TypeScript: `NotOnOwnLine`, `SyntaxError`, `AlreadyMarked`, `UnsafeDocComment`, `PlacementNotSupported` |

**The file header** is a plain comment, as in every language: `// <label>: <text>` after a leading licence comment and **before** inner
attributes and inner doc comments (`#![…]`, `//!`), which must come before any item but may follow a plain comment. It uses the core's
`comment_marker` with the prefix `//`.

## RS3: project loading

```rust
pub struct Project { /* an AnalysisHost + the Vfs */ }
impl Project {
    pub fn open(root: &Path) -> Result<Project, OpenError>;      // the Cargo workspace at or above `root`
    pub fn environment_inputs(&self) -> Vec<String>;             // **/Cargo.toml, Cargo.lock, rust-toolchain(.toml), .cargo/config.toml
}
#[non_exhaustive]
pub enum OpenError { NoManifest, NoCargo, NoSysroot, Metadata(String) }
```

- `load_workspace_at` with build scripts, proc macros and cache prefill **off**; `Stats.language` records `build_scripts_skipped`,
  `proc_macros_skipped`.
- `sem.rs` holds the `Analysis` snapshot. Handles (`Func`) wrap `hir::Function` and carry the snapshot's lifetime; salsa's types already have
  `'db`, so there is no `PhantomData` trick to write.
- A folder with a single `.rs` file and no `Cargo.toml`: not supported for call graphs in phase 1 (`NoManifest`). Outline and markers work on
  any file.

## RS4: the call graph

The walk is rust-analyzer's own `outgoing_calls` (`../language_clone/rust-analyzer/crates/ide/src/call_hierarchy.rs`) rewritten to keep each site
separate and to keep what it drops:

```
for each ast::CallableExpr in the body's descendants, in source order
    Call(expr)        →  sema.type_of_expr(callee)?.as_callable(db)  →  Function · TupleStruct · TupleEnumVariant · Closure/other
    MethodCall(expr)  →  sema.resolve_method_call(&expr)
    nothing resolved  →  Unresolved          (outgoing_calls returns None here and the site disappears; we keep it — rule 6)
plus: ast::RecordExpr  `Foo { .. }`          →  Construct { class: Foo, init: None }
plus: ast::MacroCall                         →  descend into its expansion, map sites back with `original_range`
```

### The policy table (`languages/rust/docs/policy.md`)

| Feature | Phase-1 policy |
|---|---|
| a method on a concrete type | the method in that type's `impl` |
| a method through a trait object or a generic bound (`x: &dyn Writer`, `T: Writer`) | the **trait's** method declaration. A leaf if it has no default body. Not the implementations |
| a trait method called on a concrete type (`json_writer.write()`) | the method in `impl Writer for JsonWriter` when rust-analyzer resolves the impl, else the trait's declaration **[check in S-RS2 which it gives]** |
| `Foo::new()` | a `Call` to the function `Foo::new`. Rust has no constructors; `new` is an ordinary function |
| `Foo { a, b }`, `Foo(a, b)`, `Enum::Variant(a)` | `Construct { class, init: None }` |
| closures | their calls belong to the enclosing def; calling a closure value is `Unresolved` |
| function pointers, `impl Fn` parameters | `Unresolved` |
| operators (`a + b` → `Add::add`), `?`, `Deref`, `Drop`, `for` (`IntoIterator`) | **not sites.** They are calls in the compiler's eyes, but nobody reads `a + b` as a call, and showing them would bury the tree. Later option |
| `.await` | transparent |
| macros: `println!`, `vec!`, `format!` | the macro itself is not a node; calls written in its arguments are sites of the enclosing def |
| a call to a function defined by a macro | `External`-like leaf, named, not entered (fact b) |
| `#[test]` functions | ordinary defs. An option to hide them is the driver's, not ours |
| generics | the generic function; no instantiation |
| branches, loops, `match` | every call expression in the body is a site |
| project code | the file's source root is not a library root: workspace members and path dependencies inside the workspace are project code; registry crates, git dependencies and the sysroot are external |
| external | `External { package: <crate name>, name: <path> }` |

## RS5: what the driver needs

After TS5, `Budget`, `Token`, lazy render, paging and `Cancel` are in the core. Rust's part:

| Need | Rust |
|---|---|
| unsaved text, edits | `AnalysisHost::apply_change` with the file's new text. Incremental by construction |
| handles after a change | a new `Analysis` snapshot; the solver's memo is dropped (the same "way 1" as TypeScript) |
| cancel | connect `Cancel` to the snapshot: applying a change, or dropping the host's snapshot, makes running queries return `Cancelled`; `sem` maps that to "cut" |
| `Cargo.toml` changed | `reload()`: load the workspace again |

Next: [`../05-python-migration/01-what-changes.md`](../05-python-migration/01-what-changes.md)
