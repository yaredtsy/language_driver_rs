# 04.04.02 RS1: the outline

```
RS1
├── 1. the public function
├── 2. what is a def
├── 3. qualnames  (frozen at rust-v0.1.0)
│   └── 3.1 type text: how a type is spelled in a name
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
| the self type is a path with generic arguments | the arguments are **kept**, spelled as type text (§3.1): `impl<T> S<T>` gives `S<T>::m`, `impl S<u8>` gives `S<u8>::m`, `impl From<A> for S<u8>` gives `<S<u8> as From<A>>::from`. Lifetimes are not arguments here: `impl<'a> S<'a>` gives `S::m` | **D29.** Two impls of one type with one method name (`impl S<u8>`, `impl S<u16>`) get two names, so reordering them cannot swap the docs attached to them. The cost, accepted: renaming `T` renames the def. It is also the form `std::any::type_name` prints (`S<u8>::new`, `<S<u8> as From<A>>::from`) |
| the self type or the trait is a path with a module prefix (`impl fmt::Display for a::b::S`) | the **last segment** only: `<S as Display>::fmt`. Only the head is shortened: the generic arguments are type text, whole (`impl From<io::Error> for a::S<b::T>` gives `<S<b::T> as From<io::Error>>::from`) | a `use` added or removed must not rename a def. Inside the brackets the prefix stays, because there it is what tells impls apart: `From<io::Error>` and `From<fmt::Error>` on one error type is a common pair, and both would be `From<Error>` |
| the trait has generic arguments (`impl From<A> for S`, `impl From<B> for S`) | **kept**, spelled as type text (§3.1): `<S as From<A>>::from` | **D29.** This is the common way to have two same-named methods on one type. Dropping them would leave both as `<S as From>::from`, told apart only by their order in the file, so reordering two impls would swap the docs attached to them |
| the self type is not a path (`&S`, `[T]`, `(A, B)`, `dyn Tr`, `fn()`) | the whole type as type text (§3.1), in angle brackets: `<&S as T>::m`, `<[T]>::len`, `<dyn Tr>::m` | no better name exists; the brackets keep `dyn Tr::m` from reading as a path |
| a blanket impl (`impl<T: Display> Tr for T`) | `<T as Tr>::m` | it follows from the rows above |
| inline modules | `a::b::f` | |
| a file module | adds **nothing**: `src/a/b.rs` holding `fn f` gives `f` | the file is already the `module` part of `StableDef::Path`; a moved file keeps its qualnames |
| an item inside a function | `outer::{fn}::inner`; a method of a local impl: `outer::{fn}::S::m` | the function-body joiner, as `.<locals>.` in Python and TypeScript. `{fn}` cannot be an identifier, so it cannot collide with a real path |
| an item inside a `const` or `static` initializer, or a closure, or a block | named under the **nearest enclosing def**, with the `{fn}` joiner if that def is a function | closures and blocks are not naming frames (D21, rule 4: a new kind of container must not move existing names). **Accepted cost:** after a const or static the joiner is `::`, so `const C: u8 = { fn helper() {} 1 };` and a type-namespace item spelled the same (`mod C`, `struct C {}`, `trait C`, each with a `helper`) both give `C::helper`; so do `fn f` and a `fn f` inside `const _` (B7). Legal Rust, rare (it needs a const with helper items and a same-spelled type or module); `occurrence` separates them (D15). Measured 2026-11-16 on d603a8e |
| a macro | `m!` | how you call it |
| `#[cfg]` twins; two impls whose headers spell the same type (`impl S` twice, each with an `fn m`) | the same qualname; `occurrence` separates them (§4) | `impl S<u8>` and `impl S<u16>` are not twins: their names differ (D29) |
| a raw identifier (`r#type`) | without `r#`: `type` | the name is `type`; `r#` is spelling |

> **🦀 Rust note: why `<S as T>::m` and not `S::m` for both.** A type can implement many traits, and two traits can each have a method
> called `fmt` (`Display` and `Debug` do). `S::fmt` would then name two functions. Rust's own answer, used in compiler errors and in code
> when a call is ambiguous, is *fully qualified syntax*: `<S as Display>::fmt`. We borrow it because it is unique where Rust needs it to
> be, and because a Rust reader already knows it.

The rule, with one fixture per row, is `languages/rust/docs/qualnames.md`. The fixtures' snapshots are the frozen form.

### 3.1 Type text

Every type that appears in a qualname is printed by **one rule** (D29): the self type's generic arguments, the trait's generic arguments,
and a whole self type that is not a path. The rule reads the impl header's tokens and nothing else, because `outline` sees one file's text
and nothing else (rule 2).

**Spelling, not meaning.** A type is named the way it is written. Nothing is looked up, expanded or shortened:

| Written | In the name | Not | Why |
|---|---|---|---|
| `impl From<Bytes> for S`, with `type Bytes = Vec<u8>;` | `Bytes` | `Vec<u8>` | the outline cannot know what an alias means (it may be defined in another file), and if it could, editing one alias would rename defs in every file that uses it |
| `impl From<io::Error> for S` | `io::Error` | `Error`, `std::io::Error` | a path in the brackets is kept as written (§3, the module-prefix row) |
| `impl Add<Self> for S` | `Self` | `S` | |
| `impl<T> From<T> for S<T>` | `T` | | a type parameter keeps its name; renaming it renames the def (D29's accepted cost) |
| `impl From<::std::io::Error> for S` | `::std::io::Error` | | the leading `::` is written, so it is kept |

> **🦀 Rust note: why spelling is safe here (coherence).** You might worry that `Bytes` and `Vec<u8>` are the same type under two names.
> Rust's *coherence* rule forbids two impls of one trait for one type (error E0119), and two inherent methods with one name on one type
> (E0592), however the types are spelled. So in code that compiles, one impl never needs two names. The opposite case, two different
> types spelled the same (an `Error` imported from `io` in one module and from `fmt` in another), lands in two different modules, so the
> module part of the name already differs. What is left (one spelling, one module, two meanings) needs local `use` items in two function
> bodies, and `occurrence` separates it, as for any repeated qualname (D15).

**The printing rule.** The type's tokens are printed in order, and:

1. **Comments and whitespace** are dropped.
2. **Lifetimes** are dropped, with the `,` or `+` that joined them: a lifetime argument (`S<'a, T>` is `S<T>`; `S<'a>` and `S<'_>`
   are `S`: empty **angle** brackets go, written or left empty, and parentheses never do: `()`, `fn()`, `Fn()` stay), the lifetime of a reference (`&'a mut T` is `&mut T`), a lifetime bound (`dyn Tr + 'static` is
   `dyn Tr`), and a `for<'a>` binder.
3. **Optional punctuation** is dropped: the `::` before `<` in a type path (`Vec::<u8>` is `Vec<u8>`), and a trailing comma in a list
   (`<A, B,>`, `(A, B,)`), **except** the comma of a one-element tuple, which makes it a tuple: `(A,)` stays `(A,)`.
4. **`r#`** is dropped from a raw identifier, as in §3's last row.
5. **One space** is put after `,`, `;` and a single `:` (the token `:`, never inside `::`); on both sides of `+`, `=`, `->` and `as`;
   after the keywords `dyn`, `impl`, `mut`, `const`, `unsafe`, `extern` and after an ABI string (`"C"`); and, as a backstop, between
   any two word tokens (identifiers, keywords, literals) that would otherwise touch. Nowhere else. (`:` and `as` were added on
   2026-11-16, from `qualnames.md`: without them `fn(a: u8)` and `<T as Tr>::Out` print as `fn(a:u8)` and `<TasTr>::Out`. The backstop
   makes a keyword this list forgot print with a space instead of merging into the next word.)
6. A **const argument in braces** (`S<{ N + 1 }>`) and a **macro in a type position** follow rules 1 to 5 like every other token: no
   exception (changed 2026-11-16; the first draft kept their own text). `S<{ N+1 }>` and `S<{N + 1}>` are both `S<{N + 1}>`, and
   `m!( a  b )` is `m!(a b)`. So spacing, line breaks and comments never rename a def, anywhere.

| Written | Type text |
|---|---|
| `S< u8 >`, `S</* why */ u8>` | `S<u8>` |
| `HashMap<K,V,>` | `HashMap<K, V>` |
| `Vec::<u8>` | `Vec<u8>` |
| `Cow<'a, str>` | `Cow<str>` |
| `&'a mut [u8 ; 4]` | `&mut [u8; 4]` |
| `Box<dyn Fn(u8)->u8+Send+'static>` | `Box<dyn Fn(u8) -> u8 + Send>` |
| `for<'a> fn(&'a str) -> &'a str` | `fn(&str) -> &str` |
| `dyn Iterator<Item=u8>` | `dyn Iterator<Item = u8>` |
| `( A , B , )`, `(A,)` | `(A, B)`, `(A,)` |
| `*const T`, `unsafe extern "C" fn(i32)` | `*const T`, `unsafe extern "C" fn(i32)` |
| `S<{ N+1 }>`, `S<{N + 1}>` | `S<{N + 1}>` |
| `<T as Tr>::Out`, `fn(a:u8)` | `<T as Tr>::Out`, `fn(a: u8)` |
| `S<>`, `fn()`, `()` | `S`, `fn()`, `()` |

So `impl<'a, T: Clone> From<&'a [T]> for Wrapper<'a, T>` is named `<Wrapper<T> as From<&[T]>>::from`.

**Why lifetimes go** (proposed by the reviewer, accepted by the owner on 2026-11-16 as part of D29). They never tell two impls apart:
`impl Tr for S<'a>` and `impl Tr for S<'static>` overlap, and Rust refuses that (E0119). And tools rewrite them: clippy's
`needless_lifetimes` fix turns `impl<'a> Tr for S<'a>` into `impl Tr for S<'_>`. A lint fix must not rename a def. Dropping them
also keeps the most common lifetime-carrying impl, `impl<'a> Parser<'a>`, named `Parser::m`.

**What still renames a def**, on purpose: changing a written type (renaming `T`, replacing `Bytes` with `Vec<u8>`, writing
`std::io::Error` where `io::Error` was). Each is an edit of the impl header, as renaming the trait or the struct is.

## 4. Occurrence

`occurrence` is 0-based: the number of defs with the same qualname before this one in the file, in source order (D18). A qualname is
**not unique in a file** (D15), so anything that takes a qualname from a person lists the matches and asks; it never picks the first.
`rust-cli` does this from RS1 (its `outline` subcommand prints the occurrence when it is not 0).

## 5. Ranges

| Field | Is |
|---|---|
| `range` | from the item's first **doc comment or attribute** to its last byte. Plain `//` comments directly above are in the engine's node (S-RS4) and are **not** in the range. Nor is a trailing `// c` from the previous item's line, which the engine also puts at the start of the next item's node: the start is the first `DOC_COMMENT` or `ATTR` child, else the first child that is not a `COMMENT` or `WHITESPACE`, never `text_range().start()` |
| `name_range` | the name token; for a macro, the name without `!` |
| `params` | parameter patterns as written. The receiver comes first and is one of `self`, `mut self`, `&self`, `&mut self`: a lifetime is dropped (`&'a self` is `&self`), and a typed receiver (`self: Box<Self>`, `mut self: Rc<Self>`) is `self` |
| `marker` | RS2 |
| `syntax_error` | a parse error **touches the def's `range`** (the row above, doc comments and attributes included, not the bare node). An error with a range overlaps it half-open (`error.start < def.end && error.end > def.start`). An **empty** error is a missing token, and counts when `def.start <= error.start <= def.end`, both ends included: the engine puts the missing `}` of a never-closed `mod m {` or `fn f() {` at the def's end offset, and the text's trailing whitespace is outside the node, so the half-open test misses it (measured in RS1 step 5 on `item_broken.rs`). **Accepted cost:** the last complete def inside an unclosed parent ends at that same offset and is flagged with it ("when unsure, flag more": RS2 then skips marking it, nothing breaks). Each language has its own rule here, all erring toward flagging: Python flags any touching range (ruff's closed `intersect`), TypeScript widens an empty error to one byte at its start |
| a byte-order mark | not a syntax error: the engine's parser does not skip a leading U+FEFF (`expected an item` at `[0..0]`, found in RS2 step 4), so `sem` parses the text after it and adds its 3 bytes back to every offset. Ranges stay offsets into the file as given. Fixture: a BOM file whose outline has no `syntax error` line |

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
| a snapshot per fixture, one fixture per row of §2, §3 and §3.1's two tables | `analyzer_testkit::syntax::snapshot_every_fixture` |
| the outline of a file cut every 50 bytes never panics | `cut_every_50_bytes` |
| type text depends on tokens only: one header written with extra spaces, line breaks and comments gives the same qualname | a unit test on `outline.rs`'s type printer |
| `cargo tree -p rust-syntax` has no `salsa`, no `ra_ap_hir`, no `ra_ap_ide` | a line in `scripts/check-all.sh`, as for the other languages |
| existing names never move | from the first tag on: a test fails if a snapshot row changes, and passes if rows are added (D21's rule 4) |
| a real file: every def of this repo's own `core/analyzer-core/src/*.rs` has a qualname, and (qualname, occurrence) is unique per file | a test over the repo's own source: free, large, and always present |

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | the engine pin in `languages/rust/Cargo.toml` and `ENGINE.md` (from S-RS1); `sem.rs` parses and lists top-level functions | `cargo test -p rust-syntax`; the `cargo tree` check |
| 2 | every kind of §2 at module level, inline modules | snapshots |
| 3 | impls and traits: the naming rows of §3, the type text of §3.1 | snapshots; `languages/rust/docs/qualnames.md` written **before** the code, reviewed |
| 4 | items in function bodies, in `const _`, in closures and blocks | snapshots |
| 5 | occurrence; `syntax_error`; the cut test; the repo's-own-source test | all of §7 |
| 6 | `rust-cli outline <file> [--label L] [--edition 2015\|2018\|2021\|2024]` (default `Edition::LATEST`: the CLI reads no `Cargo.toml`) | prints the outline of a file of this repo. ✅ 8d38b5b |

Q-RS1 is answered (D29): step 3 builds §3 and §3.1 as written. Nothing is tagged until RS2.

Next: [`03-rs2-markers.md`](03-rs2-markers.md)
