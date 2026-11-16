# Rust qualnames

This is the naming rule of `rust-syntax`. **It is frozen at the first tag (`rust-v0.1.0`).** A qualname is stored in users' files: adding a kind
of def is safe, changing how an existing def is named is not (rule 10). Every row below is pinned by a snapshot in
`crates/rust-syntax/tests/snapshots/`; when this file and a snapshot disagree, the snapshot is what the code does. The plan it was written from
is `docs/04-rust/04-default-resolver/02-rs1-outline.md` §2, §3 and §3.1 (decisions D10, D15, D29).

## What a qualname is

```
qualname  =  the enclosing defs' names, then the def's own name, joined by "::"

            joiner    "::"       between a def and what is declared in it     outer::inner::f · T::m
                      "::{fn}::" between a function and an item declared in    outer::{fn}::inner
                                 its body (RS1 step 4, not built yet)
```

The qualname never contains the file path: a file module adds **nothing** (`src/a/b.rs` holding `fn f` gives `f`), because the file is already
the `module` part of the stored def. Moving a file does not rename its defs. Only an **inline** `mod m { }` is a name.

A name is the identifier as written, **without `r#`** (`r#type` is `type`: `r#` is spelling).

**An `impl` block is not a def.** It has no name and no entry in the outline. It is a *naming context*: the items inside get the impl's
header as a prefix (rows I1 to I9). The `parent` of an item in an impl is the def that encloses the impl (a module, or nothing at file level),
never the type the impl is for, because that type may live in another file. The `parent` of an item in a trait is the trait's def.

**A qualname is not unique in a file** (D15): two `impl S { fn m() }` blocks, `#[cfg]` twins, and `fn f` in two functions' `use` scopes all
share one. `occurrence` (0-based, source order, RS1 step 5) separates them, and the marker is what tells them apart for good. A tool that
takes a qualname from a person lists the matches and asks; it never picks the first.

## What is a def

| # | Syntax | Kind | Qualname | Fixture |
|---|---|---|---|---|
| D1 | `fn f()` at module level (also `async`, `const`, `unsafe`, `extern "C"`) | Function | `f` | `fn_names.rs`, `fn_forms.rs` |
| D2 | `struct S`, `union U` | Struct | `S` | `item_kinds.rs` |
| D3 | `enum E` | Enum | `E` | `item_kinds.rs` |
| D4 | `trait T` | Trait | `T`. Its own generics are **not** in the name: `trait T<A>` is `T` | `item_kinds.rs`, `trait_items.rs` |
| D5 | `type A = …` at module level | Type | `A` | `item_kinds.rs` |
| D6 | `const C`, `static X` at module level | `Other("const")`, `Other("static")` | `C`, `X` | `item_kinds.rs` |
| D7 | `mod m { }` (inline) | Module | `m`; its items are `m::f` | `item_modules.rs` |
| D8 | `mod m;` | **not a def** | it names a file; the file is the def's home | `item_modules.rs` |
| D9 | `macro_rules! m` | `Other("macro")` | `m!` at file level. In a module it is named **by its module path plus `m!`, like every other item**, not the way it is called: `outer::inner::m!`. `#[macro_export]` does not move it (the macro is callable at the crate root, but the name follows where it is written) | `item_kinds.rs`, `item_modules.rs`, `macro_in_module.rs` |
| D10 | a function declared in an `extern "C" { }` block | Function | `f`. The block is not a def and adds nothing to the name. No body: a leaf | `item_extern_block.rs` |
| D11 | a function, method or associated function of a trait, with or without a default body | Method | `T::m` | `trait_items.rs` |
| D12 | a function in `impl S { }` or `impl T for S { }` | Method | rows I1 to I9 | `impl_*.rs` |
| D13 | `const C` in a trait or an impl | `Other("const")` | `T::C`, `S::C`, `<S as T>::C` | `trait_items.rs`, `impl_inherent.rs`, `impl_trait.rs` |
| D14 | an associated `type` in a trait or an impl | **not a def, for now** | nothing calls it, nothing is attached to it; adding it later is safe | `trait_items.rs`, `impl_trait.rs` |
| D15 | a macro call in a trait or an impl, a derive, anything a macro produces | **not a def** | not in the text | |
| D16 | an `impl` block itself | **not a def** | a naming context | `impl_inherent.rs` |
| D17 | `const _: () = { … };` | **not a def**, and it adds nothing to the names inside | `_` is not a name | `item_kinds.rs` |
| D18 | an enum variant, a struct field | **not a def, for now** | | |
| D19 | a closure, an `async` block | **not a def** | its calls belong to the enclosing def | |
| D20 | an item under `#[cfg(…)]` | a def, always | the outline lists what is in the text; whether it compiles is the engine's business | `item_modules.rs` |
| D21 | an item that has no name the parser could read | **not a def** | `fn` with no name, `struct` with no name: nothing to name | `item_broken.rs` |

Every row of this section is built (RS1 steps 1 to 3).

## Naming an impl's items

The prefix of an item in an impl is built from the impl header, which is read from **tokens only**: nothing is looked up, expanded or
resolved, because the outline sees one file's text and nothing else (rule 2). Generics of the impl itself (`impl<T: Clone>`), `where` clauses,
and the keywords `unsafe`, `default`, `const` and `!` of the header are **not** part of the name.

| # | The impl | Prefix, then `::name` | Example | Why |
|---|---|---|---|---|
| I1 | inherent, the self type is a path | the path's **last segment** with its generic arguments, as type text (§ Type text) | `impl S` → `S::m` | how you call it |
| I2 | inherent, the self type has generic arguments | the arguments are **kept** (D29) | `impl<T> S<T>` → `S<T>::m`; `impl S<u8>` → `S<u8>::m`; `impl S<u16>` → `S<u16>::m` | two impls of one type with one method name get two names, so reordering them cannot swap the docs attached to them. The cost, accepted: renaming `T` renames the def |
| I3 | the self type has only lifetime arguments | lifetimes are not arguments here, they are **dropped** | `impl<'a> P<'a>` → `P::m`; `impl<'a, T> W<'a, T>` → `W<T>::m` | `impl Tr for S<'a>` and `impl Tr for S<'static>` overlap (E0119), so a lifetime never tells two impls apart; and clippy's `needless_lifetimes` rewrites them (`'a` to `'_`): a lint fix must not rename a def |
| I4 | the self type is a path with a module prefix | only the head is shortened: the **last segment**, arguments whole | `impl a::b::S` → `S::m`; `impl a::S<b::T>` → `S<b::T>::m` | a `use` added or removed must not rename a def. Inside the arguments the prefix stays, because there it is what tells impls apart |
| I5 | trait impl | `<self as Trait>` | `impl T for S` → `<S as T>::m` | Rust's own fully qualified syntax (D10) |
| I6 | trait impl, the trait is a path with a module prefix | the trait's **last segment** | `impl fmt::Display for S` → `<S as Display>::fmt` | as I4 |
| I7 | trait impl, the trait has generic arguments | **kept** (D29), as type text | `impl From<A> for S` → `<S as From<A>>::from`; `impl From<io::Error> for a::S<b::T>` → `<S<b::T> as From<io::Error>>::from` | the common way to have two same-named methods on one type: `From<A>` and `From<B>` must not both be `<S as From>::from` |
| I8 | the self type is **not a path** (`&S`, `[T]`, `(A, B)`, `dyn Tr`, `fn()`, `*const T`, `!`, a macro in type position, a path with a `<A as B>::` qualifier) | the whole type as type text, in angle brackets when inherent | inherent `impl [T]` → `<[T]>::len`; `impl dyn Tr` → `<dyn Tr>::m`; trait impl `impl T for &S` → `<&S as T>::m` | no better name exists; the brackets keep `dyn Tr::m` from reading as a path. In a trait impl the angle brackets are the form's own |
| I9 | a blanket impl | follows from I5 and I8: the self type is the parameter's name | `impl<T: Display> Tr for T` → `<T as Tr>::m` | |
| I10 | the header cannot be read: a parse error lies in it (from `impl` to the `{` of its items), or it has no self type, or `for` has no type on one side (a file being typed) | the impl's items are **not defs** | `impl for S { fn m() {} }`, `impl Tr for { … }`, `impl S<u8 { … }` list nothing | as D21: nothing to name from. The engine recovers from `impl for S` by reading `for S` as a higher-ranked type, so a header with an error is not read at all: a guess would be stored in a file |
| I11 | `impl` twins: two impls whose headers spell the same type (`impl S` twice, each with `fn m`) | the same qualname, told apart by `occurrence` | `S::m`, `S::m` | `impl S<u8>` and `impl S<u16>` are **not** twins: their names differ (I2) |
| I12 | an impl in an inline module | the module path, then the prefix | `mod a { mod b { impl S {} } }` → `a::b::S::m`; the trait form: `a::b::<S as T>::m` | the module path is a def's name like any other |

> **🦀 Rust note: why `<S as T>::m` and not `S::m` for both.** A type can implement many traits, and two traits can each have a method called
> `fmt` (`Display` and `Debug` do). `S::fmt` would then name two functions. Rust's own answer, used in compiler errors and in code when a call
> is ambiguous, is *fully qualified syntax*: `<S as Display>::fmt`. We borrow it because it is unique where Rust needs it to be.

## Type text

Every type that appears in a qualname is printed by **one rule**: the self type's arguments, the trait's arguments, and a whole self type
that is not a path (I8).

**A type is named the way it is written.** Nothing is looked up, expanded or shortened.

| # | Written | In the name | Not | Why |
|---|---|---|---|---|
| T1 | `impl From<Bytes> for S`, with `type Bytes = Vec<u8>;` | `Bytes` | `Vec<u8>` | the outline cannot know what an alias means (it may be in another file), and editing one alias would rename defs in every file that uses it |
| T2 | `impl From<io::Error> for S` | `io::Error` | `Error`, `std::io::Error` | a path inside the arguments is kept as written (I4) |
| T3 | `impl Add<Self> for S` | `Self` | `S` | |
| T4 | `impl<T> From<T> for S<T>` | `T` | | a type parameter keeps its name; renaming it renames the def (D29's accepted cost) |
| T5 | `impl From<::std::io::Error> for S` | `::std::io::Error` | | the leading `::` is written, so it is kept |

> **🦀 Rust note: why spelling is safe (coherence).** `Bytes` and `Vec<u8>` are one type under two names, but Rust forbids two impls of one
> trait for one type (E0119), and two inherent methods of one name on one type (E0592), however the types are spelled. So in code that
> compiles, one impl never needs two names. Two *different* types spelled the same (an `Error` imported from `io` in one module and from `fmt`
> in another) land in two modules, so the module part of the name already differs. What is left needs local `use` items in two function bodies,
> and `occurrence` separates it.

**The printing rule.** The type's tokens are printed in order, and:

| # | Rule | Written | Type text |
|---|---|---|---|
| P1 | **Comments and whitespace** are dropped | `S< u8 >`, `S</* why */ u8>` | `S<u8>` |
| P2 | **Lifetimes** are dropped, from every list they stand in, and the list's `,` or `+` goes with them: a lifetime argument, the lifetime of a reference, a lifetime bound, a `for<'a>` binder. A list of **angle** brackets that is left empty loses its brackets (P3) | `Cow<'a, str>`; `S<T, 'a>`; `S<'a>`, `S<'_>`; `&'a mut T`; `dyn Tr + 'static`; `dyn 'a + Tr + Send`; `for<'a> fn(&'a str) -> &'a str` | `Cow<str>`; `S<T>`; `S`; `&mut T`; `dyn Tr`; `dyn Tr + Send`; `fn(&str) -> &str` |
| P3 | **Empty angle brackets** go, written or left empty by P2. **Parentheses never go**: an empty `()`, `fn()` or `Fn()` is a unit type, a function pointer or a callable bound, and keeps them | `S<>`; `From<()>`; `From<fn()>`; `From<Box<dyn Fn()>>` | `S`; `From<()>`; `From<fn()>`; `From<Box<dyn Fn()>>` |
| P4 | The **`::` before `<`** in a type path is dropped | `Vec::<u8>` | `Vec<u8>` |
| P5 | A **trailing comma** in a list is dropped, **except** the comma of a one-element tuple, which makes it a tuple | `HashMap<K,V,>`; `(A, B,)`; `fn(A,)`; `(A,)` | `HashMap<K, V>`; `(A, B)`; `fn(A)`; `(A,)` |
| P6 | **`r#`** is dropped from a raw identifier | `From<r#type>` | `From<type>` |
| P7 | **One space** after `,`, `;` and a single `:` (the token `:`, never inside `::`); on both sides of `+`, `=`, `->` and `as`; after the keywords `dyn`, `impl`, `mut`, `const`, `unsafe`, `extern`, and after an ABI string (`"C"`); and, as a **backstop**, one space between any two word tokens (identifiers, keywords, literals) that would otherwise touch. **Nowhere else** | `&'a mut [u8 ;4]`; `Box<dyn Fn(u8)->u8+Send+'static>`; `dyn Iterator<Item=u8>`; `<T as Tr>::Out`; `fn(a:u8)`; `*const T`; `unsafe extern "C" fn(i32)`; `a::B`, `a :: B` | `&mut [u8; 4]`; `Box<dyn Fn(u8) -> u8 + Send>`; `dyn Iterator<Item = u8>`; `<T as Tr>::Out`; `fn(a: u8)`; `*const T`; `unsafe extern "C" fn(i32)`; `a::B`, `a::B` |
| P8 | A **const argument in braces** and a **macro in a type position** follow P1 to P7 like every other token. **No exception** | `S<{ N+1 }>`, `S<{N + 1}>`, `S<{ N + /* c */ 1 }>`; `m!( a  b )`, `m!(a\nb)` | `S<{N + 1}>`; `m!(a b)` |

**What P8 means at the edges** (all pinned by `type_text_braces.rs`). The inside of a macro call is a token tree, not a type, so what P2 to P5 say
about types does not apply to it: a lifetime stays (`m!('a x)`), a trailing comma stays (`m!(a,)`). P1, P6 and P7 apply to every token. P7 spaces
only the tokens it names, so `{ N+1 }` prints `{N + 1}` but `{ N-1 }` prints `{N-1}`: a name is never a re-formatted expression.

(P7 has `:` and `as` and the backstop because without them `fn(a: u8)` and `<T as Tr>::Out` would print as `fn(a:u8)` and `<TasTr>::Out`, and a keyword the list forgot would merge into the next word. The backstop is why `m!(a b)` keeps its space.)

So `impl<'a, T: Clone> From<&'a [T]> for Wrapper<'a, T>` is named `<Wrapper<T> as From<&[T]>>::from`.

**What still renames a def**, on purpose: changing a written type (renaming `T`, replacing `Bytes` with `Vec<u8>`, writing `std::io::Error` where
`io::Error` was). Each is an edit of the impl header, as renaming the trait or the struct is. **What never does:** spacing, line breaks, comments,
lifetimes, the impl's own generics and bounds, the `where` clause. This holds everywhere in the header, braced const arguments and macros in types included (P8).

## Items in bodies

An item can stand in a function body, in the initializer of a `const` or a `static`, or in a block, a closure or an `async` block inside any of
those. It is a def like any other and is named under the **nearest enclosing def**. The `parent` of such an item is that def (never a module
that is further out), and the joiner after a **function** is `{fn}`; after anything else it is `::`.

| # | Syntax | Qualname | Why | Fixture |
|---|---|---|---|---|
| B1 | a fn, struct, enum, union, trait, type, const, static, `mod` or `extern` block inside a function body | `outer::{fn}::inner`; a `mod` in it: `outer::{fn}::m::f`; in a function in a function: `outer::{fn}::inner::{fn}::deep` | the function-body joiner (as `.<locals>.` in Python and TypeScript). `{fn}` cannot be an identifier, so it cannot collide with a real path | `body_items.rs` |
| B2 | an `impl` or a `trait` inside a function body | a method of a local impl: `outer::{fn}::S::m`, `outer::{fn}::<S as T>::m`; of a local trait: `outer::{fn}::T::m` | B1 followed by I1 to I9 or D11. The impl is still no def | `body_items.rs` |
| B3 | a `macro_rules!` in a function body | `outer::{fn}::m!` | D9 with B1 | `body_macros.rs` |
| B4 | an item in a method, in a trait's default method, or in a trait impl's method | `S::m::{fn}::inner`, `T::m::{fn}::inner`, `<S as T>::m::{fn}::inner` | the method's qualname is a function's qualname, so B1 applies to it unchanged | `body_items.rs` |
| B5 | an item in a **closure**, an `async` block, a `const { }` block, or any block (`if`, `loop`, `match` arm, `unsafe`, labelled) | `outer::{fn}::inner`, as if the closure or block were not there | closures and blocks are not naming frames (D21, rule 4: a new kind of container must not move existing names) | `body_frames.rs` |
| B6 | an item in the initializer of a **named** `const` or `static` (module level, or an associated const of an impl or a trait) | `C::helper`, `X::helper`, `S::C::helper`, `T::C::helper`: the const is the enclosing def, and only a function takes `{fn}` | the const is a def (D6, D13), so it is the nearest one | `body_const.rs` |
| B7 | an item in the initializer of `const _: () = { … };` | **named as if the block were not there**: `f` at module level, `m::f` in a module, `outer::{fn}::f` in a function, `<S as T>::m` for an impl in it | `const _` is no def and adds nothing to the names inside (D17). It is the form derive macros and `static_assertions` write | `body_const.rs` |
| B8 | the same name twice in one function (two blocks, two closures) | one qualname, told apart by `occurrence` | D15 | `body_twins.rs` |
| B9 | an item in a place that is not a body: an array length or a const argument (`[u8; { fn f() {} 1 }]`), an enum discriminant, a field type, a default | **not looked at**, so not a def | nothing is attached to code that cannot be written there in practice; adding it later is safe | `body_not_looked_at.rs` |
| B10 | a function, or a const that is not `const _`, with no name the parser could read, with items in its body | **not defs**, as D21 | nothing to name them under | `body_not_looked_at.rs` |

## Fixtures

One file per row group under `crates/rust-syntax/tests/fixtures/`, all parsed as edition 2024. **Step 3 adds** `impl_inherent.rs` (D12, D13,
D16, I1 to I3), `impl_module_prefix.rs` (I4, I6, I12), `impl_trait.rs` (I5, D14), `impl_trait_args.rs` (I7), `impl_non_path.rs` (I8),
`impl_blanket.rs` (I9), `impl_broken.rs` (I10), `impl_twins.rs` (I11), `trait_items.rs` (D4, D11, D13, D14), `macro_in_module.rs` (D9),
`type_text_spelling.rs` (T1 to T5), `type_text_lifetimes.rs` (P2, P3, with `S<>`, `()`, `fn()` and `Fn()`), `type_text_punctuation.rs` (P4 to P6), `type_text_spacing.rs` (P7),
`type_text_braces.rs` (P8: `S<{ N+1 }>` and `S<{N + 1}>` side by side, a macro written two ways) and `impl_header_noise.rs` (a header written with extra spaces, line breaks, comments, a `where` clause and
changed lifetimes beside its clean twin: the two outlines must carry the same qualnames, checked by a unit test on the printer as well, with no exclusion: not even for braces or macros).

**Step 4 adds** `body_items.rs` (B1, B2, B4), `body_macros.rs` (B3), `body_frames.rs` (B5), `body_const.rs` (B6, B7), `body_twins.rs` (B8) and
`body_not_looked_at.rs` (B9, B10).
