# 04.04.02 RS1: the outline

```
RS1
├── 1. the public function
├── 2. what is a def
├── 3. qualnames  (frozen at rust-v0.1.0)
├── 4. occurrence, and why a qualname is not unique
├── 5. ranges
├── 6. the crate, file by file
├── 7. tests
└── 8. steps
```

## 1. The public function

```rust
// rust-syntax/src/lib.rs
pub fn outline(text: &str, edition: Edition, style: MarkerStyle<'_>) -> Outline;

#[non_exhaustive]
pub enum Edition { E2015, E2018, E2021, E2024 }
impl Edition { pub const LATEST: Edition = Edition::E2024; }
```

Text in, data out (rule 2). The edition changes what parses (`async`, `dyn`, `gen` are keywords in some and not others), so it is an
input; the driver reads it from `Cargo.toml` and falls back to `LATEST`. `Outline`, `Def` and `DefKind` are core's.

## 2. What is a def

| Syntax | Def? | Kind | Example qualname |
|---|---|---|---|
| `fn f()` at module level | yes | `Function` | `f` |
| `struct S`, `union U` | yes | `Struct` | `S` |
| `enum E` | yes | `Enum` | `E` |
| `trait T` | yes | `Trait` | `T` |
| a method or associated function of a trait, with or without a default body | yes | `Method` | `T::m` |
| a method or associated function in `impl S { }` | yes | `Method` | `S::m` |
| a method in `impl T for S { }` | yes | `Method` | `<S as T>::m` |
| `mod m { }` (inline) | yes | `Module` | `m`; its items are `m::f` |
| `mod m;` (a declaration of a file module) | **no** | | it names a file; the file is the def's home |
| `type A = …` at module level | yes | `Type` | `A` |
| `const C`, `static X` at module level or in an impl or trait | yes | `Other("const")`, `Other("static")` | `C`, `S::C`, `<S as T>::C` |
| an associated `type` in an impl or trait | **no** for now | | nothing calls it and nothing is attached to it; adding later is safe |
| `macro_rules! m` | yes | `Other("macro")` | `m!` |
| a function declared in an `extern "C" { }` block | yes | `Function` | `f`. It has no body: a leaf |
| a function, struct, impl… inside a function body | yes | as above | `outer::{fn}::inner` |
| an `impl` block itself | **no** | | no name; a naming context for its items |
| `const _: () = { … };` | **no**, and it adds nothing to the names inside | | the Rust form of D15's "an anonymous container adds nothing" |
| a closure, an `async` block | no | | rule 3: its calls belong to the enclosing def |
| enum variants, struct fields | no for now | | `Enum::Variant(a)` is a construction of the enum (RS4) |
| items a macro call produces (`lazy_static! { … }`, a derive) | no | | not in the text ([`../01`](../01-what-rust-analyzer-gives.md) §5b) |
| an item under `#[cfg(...)]` | yes, always | | the outline lists what is in the text; whether it is compiled is the engine's business |

Adding a kind later is safe; removing one orphans docs. So the two "no for now" rows are deliberate: start narrow.

## 3. Qualnames

**Frozen at `rust-v0.1.0`.** They are stored in users' files. The joiner is `::` and the forms are the ones `rustc` prints.

| Case | Qualname | Why this form |
|---|---|---|
| inherent impl | `S::m` | how you call it |
| trait impl | `<S as T>::m` | Rust's own fully qualified syntax (D10) |
| the self type is a path with generic arguments | the arguments are **dropped**: `impl<T> S<T>` and `impl S<u8>` both give `S::m` | `impl<T> S<T>` is by far the common case, and `S<T>::m` would change when someone renames `T`. Two concrete impls with one method name are separated by `occurrence` |
| the self type is a path with a module prefix (`impl fmt::Display for a::b::S`) | the **last segment** only: `<S as Display>::fmt` | a `use` added or removed must not rename a def |
| the trait has generic arguments (`impl From<A> for S`, `impl From<B> for S`) | **kept**, as written, whitespace normalised: `<S as From<A>>::from` | this is the common way to have two same-named methods on one type. Dropping them would leave both as `<S as From>::from`, told apart only by their order in the file, so reordering two impls would swap the docs attached to them. **Q-RS1** |
| the self type is not a path (`&S`, `[T]`, `(A, B)`, `dyn Tr`, `fn()`) | the type's source text, whitespace normalised, in angle brackets: `<&S as T>::m`, `<[T]>::len`, `<dyn Tr>::m` | no better name exists; the brackets keep `dyn Tr::m` from reading as a path |
| a blanket impl (`impl<T: Display> Tr for T`) | `<T as Tr>::m` | it follows from the rows above |
| inline modules | `a::b::f` | |
| a file module | adds **nothing**: `src/a/b.rs` holding `fn f` gives `f` | the file is already the `module` part of `StableDef::Path`; a moved file keeps its qualnames |
| an item inside a function | `outer::{fn}::inner`; a method of a local impl: `outer::{fn}::S::m` | the function-body joiner, as `.<locals>.` in Python and TypeScript. `{fn}` cannot be an identifier, so it cannot collide with a real path |
| an item inside a `const` or `static` initializer, or a closure, or a block | named under the **nearest enclosing def**, with the `{fn}` joiner if that def is a function | closures and blocks are not naming frames (D21, rule 4: a new kind of container must not move existing names) |
| a macro | `m!` | how you call it |
| `#[cfg]` twins, two `impl S<u8>` / `impl S<u16>` with one method name | the same qualname; `occurrence` separates them (§4) | |
| a raw identifier (`r#type`) | without `r#`: `type` | the name is `type`; `r#` is spelling |

> **🦀 Rust note: why `<S as T>::m` and not `S::m` for both.** A type can implement many traits, and two traits can each have a method
> called `fmt` (`Display` and `Debug` do). `S::fmt` would then name two functions. Rust's own answer, used in compiler errors and in code
> when a call is ambiguous, is *fully qualified syntax*: `<S as Display>::fmt`. We borrow it because it is unique where Rust needs it to
> be, and because a Rust reader already knows it.

The rule, with one fixture per row, is `languages/rust/docs/qualnames.md`. The fixtures' snapshots are the frozen form.

## 4. Occurrence

`occurrence` is 0-based: the number of defs with the same qualname before this one in the file, in source order (D18). A qualname is
**not unique in a file** (D15), so anything that takes a qualname from a person lists the matches and asks; it never picks the first.
`rust-cli` does this from RS1 (its `outline` subcommand prints the occurrence when it is not 0).

## 5. Ranges

| Field | Is |
|---|---|
| `range` | from the item's first **doc comment or attribute** to its last byte. Plain `//` comments directly above are in the engine's node (S-RS4) and are **not** in the range |
| `name_range` | the name token; for a macro, the name without `!` |
| `params` | parameter patterns as written, `self` included as `self` / `&self` / `&mut self` |
| `marker` | RS2 |
| `syntax_error` | a parse error's range overlaps the def's node |

The driver contract's wording ("from its first decorator / attribute / doc comment") fits exactly, because in Rust a doc comment *is* an
attribute. The marker is one of those lines, so it is inside the range, unlike TypeScript (D10's Q-M5), and nobody has to extend it.

## 6. The crate

```
rust-syntax/src/
├── lib.rs        the public names of §1 and RS2; nothing else
├── sem.rs        the ONLY file that imports ra_ap_syntax: parse; an owned, engine-free list of items (kind, name, ranges, doc lines, parent)
├── outline.rs    items → Outline: the naming rule of §3, occurrence
├── marker.rs     RS2
└── header.rs     RS2
```

`sem.rs` returns owned data; no syntax node leaves it. That is not needed for memory safety here (rust-analyzer's trees are ordinary
reference-counted values), it is rule 4, and it keeps an engine bump inside one file.

## 7. Tests

| Test | From |
|---|---|
| a snapshot per fixture, one fixture per row of §2 and §3 | `analyzer_testkit::syntax::snapshot_every_fixture` |
| the outline of a file cut every 50 bytes never panics | `cut_every_50_bytes` |
| `cargo tree -p rust-syntax` has no `salsa`, no `ra_ap_hir`, no `ra_ap_ide` | a line in `scripts/check-all.sh`, as for the other languages |
| existing names never move | from the first tag on: a test fails if a snapshot row changes, and passes if rows are added (D21's rule 4) |
| a real file: every def of this repo's own `core/analyzer-core/src/*.rs` has a qualname, and (qualname, occurrence) is unique per file | a test over the repo's own source: free, large, and always present |

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | the engine pin in `languages/rust/Cargo.toml` and `ENGINE.md` (from S-RS1); `sem.rs` parses and lists top-level functions | `cargo test -p rust-syntax`; the `cargo tree` check |
| 2 | every kind of §2 at module level, inline modules | snapshots |
| 3 | impls and traits: the naming rows of §3 | snapshots; `languages/rust/docs/qualnames.md` written **before** the code, reviewed |
| 4 | items in function bodies, in `const _`, in closures and blocks | snapshots |
| 5 | occurrence; `syntax_error`; the cut test; the repo's-own-source test | all of §7 |
| 6 | `rust-cli outline <file>` | prints the outline of a file of this repo |

Q-RS1 is answered before step 3. Nothing is tagged until RS2.

Next: [`03-rs2-markers.md`](03-rs2-markers.md)
