# 03.03 TS1: the outline

```
TS1
├── 1. the public function
├── 2. what is a def (the decision sheet)
├── 3. qualnames
├── 4. ranges
├── 5. the crate, file by file
├── 6. fixtures
└── 7. steps
```

## 1. The public function

```rust
// typescript-syntax/src/lib.rs
pub use analyzer_core::outline::{Def, DefKind, Outline, SyntaxError};
pub use analyzer_core::text::Span;

/// Which grammar to parse with. From the file name: `Dialect::from_path("a.tsx")`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dialect { Ts, Tsx, Js, Jsx }

pub fn outline(text: &str, dialect: Dialect) -> Outline;
```

Text in, plain data out. No file reads, no project, no checker. The dialect is a parameter because the same text parses differently as `.ts` and
`.tsx` (`<T>x` is a cast in one and the start of an element in the other).

## 2. What is a def

The V-NOC plan's guide: "things a person would want to write a note on and that have a stable name". For TypeScript there is one more
requirement: **every function a call can resolve to must be a def**, or a call tree would contain nodes that the outline does not know.

The engine's default resolver answers with the declarations that TypeScript's call hierarchy accepts
(`is_valid_call_hierarchy_declaration` in `tsrs_ls/src/callhierarchy.rs`). So the outline's function-like defs are **exactly that list**, plus the
type-like things worth documenting:

| Syntax | Def? | Kind | Example qualname |
|---|---|---|---|
| `function f() {}` | yes | function | `f` |
| `class C {}` | yes | class | `C` |
| a method, a `constructor`, `get x()`, `set x()`, `static m()` in a class | yes | method | `C.m`, `C.constructor`, `C.x` |
| `const f = () => {}`, `const f = function () {}`, `const C = class {}` | yes: the variable names it | function / class | `f`, `C` |
| a class property holding a function: `handle = () => {}` | yes | method | `C.handle` |
| a named function expression: `run(function step() {})` | yes | function | `outer.<locals>.step` |
| a method in an object literal: `const api = { load() {} }` | **yes** (D9: the engine resolves calls to them) | method | `api.load` |
| an interface, its method signatures | yes | interface, method | `Writer`, `Writer.write` |
| `enum E {}`, `type T = …`, `namespace N {}` | yes | enum, type, namespace | `E`, `T`, `N` |
| `let f = () => {}` / `var f = …` | **no** | | the engine's rule accepts only `const`: a `let` can be reassigned, so its name is not stable |
| an anonymous function passed as an argument: `xs.map(x => …)` | **no** | | no name. Its calls belong to the enclosing def (rule 3) |
| a property in an object literal holding an arrow: `{ load: () => {} }` | no for now | | the engine's rule does not accept it. Adding it later is safe |
| a local variable, a field without a function, a parameter | no | | |
| the file itself | not in `defs` | | the file is already a node in V-NOC |

Removing a kind later orphans docs; adding one is safe. So where I was unsure, the answer above is "no for now".

Overloads: several signatures followed by one implementation are **one def** (the implementation). In a `.d.ts`, where there is no
implementation, the signatures are separate defs with the same qualname, and the core's `occurrence` tells them apart, as it does for true
duplicates in Python.

## 3. Qualnames

The rule is frozen at the first tag (rule 12), so it is written here in full.

```
qualname  =  the names of the enclosing defs, joined, then the def's own name

joiner    "."            between a container and its member          C.m · N.f · api.load · Writer.write
          ".<locals>."   between a function and something declared   outer.<locals>.inner
                         inside its body
```

`.<locals>.` is Python's convention, used here on purpose: V-NOC's UI already shows it, and it cannot collide with a member path (`f.x` is a
property of `f`; `f.<locals>.x` is a local of `f`).

| Case | Name used |
|---|---|
| identifier name | as written: `parseOrder` |
| private name | with the hash: `C.#secret` |
| string or number literal name: `"my-key"() {}` | the source text, with its quotes: `C."my-key"`. A string that is a valid identifier loses them (`"load"() {}` is `C.load`: it declares the same member as `load() {}`); also for `declare module "lodash"` (owner, 2026-11-11) |
| computed name: `[Symbol.iterator]() {}` | the source text in brackets: `C.[Symbol.iterator]` |
| `export default function () {}` / `export default class {}` | `default` |
| `export default function f() {}`, `export default class Foo {}` | `f`, `Foo` and `Foo.m` (the export has a name; `default` only when it has none) |
| getter and setter of one property | both `C.x`; told apart by `occurrence` (and by `params`: a setter has one) |
| a `static` member | the same as an instance member: `C.m`. A class cannot have a static and an instance member that V-NOC needs to tell apart by name alone; if both exist, `occurrence` does it |
| a namespace written `namespace A.B {}` | two levels: `A`, `A.B` |
| a def inside a block (`if (x) { function f() {} }`) | the block adds nothing: `f` |

The qualname never contains the file path and never contains `export`.

> **🦀 Rust note: building qualnames with a stack.** The walk goes down the tree; keep the path as you go and never rebuild it from parents.
> ```rust
> struct Walker<'a> { stack: Vec<Frame>, out: Vec<Def>, text: &'a str }
> struct Frame { qualname: String, index: usize, is_function: bool }       // index into `out`
>
> fn enter(&mut self, name: &str, kind: DefKind, is_function: bool, range: Span) {
>     let qualname = match self.stack.last() {
>         None => name.to_owned(),
>         Some(p) if p.is_function => format!("{}.<locals>.{}", p.qualname, name),
>         Some(p) => format!("{}.{}", p.qualname, name),
>     };
>     let parent = self.stack.last().map(|p| p.index);
>     self.out.push(Def { qualname: qualname.clone(), name: name.to_owned(), kind, parent, range, /* … */ });
>     self.stack.push(Frame { qualname, index: self.out.len() - 1, is_function });
> }
> fn leave(&mut self) { self.stack.pop(); }
> ```
> Pushing to `out` *before* visiting children gives "parents before children, in source order" for free: it is just pre-order. pylspt's
> `DefScanner` and the Python driver's `outline.rs` do the same thing; this is that walk with a second joiner.

## 4. Ranges

| Field | Covers |
|---|---|
| `range` | from the first byte of the def's first **decorator**, modifier (`export`, `async`, `static`) or keyword, to its last byte. For `const f = () => {}`: the whole `const` statement when it declares one variable, otherwise just the declarator. For an overloaded function: from the first overload signature |
| `name_range` | the name token |

Comments above the def (the JSDoc, which holds the marker) are **not** in `range`. That differs from the driver contract's wording ("from its first
decorator / attribute / doc comment"), and the difference is deliberate: in TypeScript `node.pos()` includes all leading trivia, even a licence
header at the top of the file, so "the node's own start" is unusable, and "the JSDoc's start" needs a rule about blank lines. The *driver* can
extend the range upward over the JSDoc for display, using `marker.rs`'s knowledge of what is directly above. Decided (D10): the analyzer's `range` excludes the JSDoc. It matters because it
changes what a card shows.

`syntax_error` is true when a diagnostic's range overlaps the def's range. The V-NOC rule is "when unsure, flag more".

## 5. The crate, file by file

```
typescript-syntax/src/
├── lib.rs        pub use …; pub fn outline, marker, plan_marker, header_slot; Dialect
├── sem.rs        parse-in-a-region; RawNode (an opaque handle); the kind tests; name/range/params of a node
├── outline.rs    the walker from the Rust note; the table of §2 as one `match`; the rules of §3
├── marker.rs     TS2
└── header.rs     TS2
```

`sem.rs` exposes the tree to `outline.rs` through **a callback**, so that no tsrs pointer can leave the region:

```rust
// sem.rs
pub(crate) fn with_parsed<R>(text: &str, dialect: Dialect, f: impl FnOnce(&Parsed<'_>) -> R) -> R {
    let region = tsrs_core::arena::Region::new_scratch(CHUNK);
    let _scope = region.enter();
    let file = tsrs_parser::parse_source_file_owned(opts(dialect), text.to_string(), kind(dialect));   // not parse_source_file (ENGINE.md, S-TS2)
    f(&Parsed { file, text, _not_send: PhantomData })
    // `region` is dropped here: everything tsrs allocated is gone
}
```

> **🦀 Rust note: a closure as a fence.** `with_parsed` gives the tree to `f` and takes it back when `f` returns. `R` is whatever `f` builds:
> our `Outline`, made of `String`s and `Vec`s that own their data. Could `f` smuggle a tsrs pointer out inside `R`? With ordinary references the
> borrow checker would stop it, because `&Parsed<'_>` cannot outlive the call. tsrs's `P<Node>` is `'static` and `Copy`, so here the compiler
> does **not** stop it. Two things do: `RawNode`'s field is private to `sem.rs`, so `outline.rs` cannot store what it cannot name outside a
> `&Parsed` method; and the S-TS2 loop test runs in CI and would see memory grow or crash. This pattern ("scoped access through a closure") is
> common in Rust wherever a resource must be released at a known point: `std::thread::scope` is the standard library's example.

## 6. Fixtures

One file per case under `tests/fixtures/`; the testkit snapshots each one. The names say what they pin:

```
functions.ts            decl · async · generator · overloads · default export
arrows.ts               const arrow · let arrow (not a def) · class property arrow · callback (not a def)
classes.ts              methods · static · accessors · private names · constructor · decorators · abstract
object_literals.ts      method shorthand · nested objects · arrow properties (not defs)
nested.ts               function in function · class in function · function in method   (<locals>)
names.ts                string · number · computed · private
types.ts                interface + signatures · enum · type alias · namespace · namespace A.B
declarations.d.ts       overload signatures with no implementation
component.tsx           a function component · a class component · generics next to JSX
plain.js / module.mjs / common.cjs / view.jsx      the JavaScript dialects; `module.exports = function () {}`
syntax_error_mid_file.ts     the defs before and after the error are still listed; the one around it is flagged
emoji_and_cjk.ts        multi-byte characters before a def (the UTF-16 trap, tested at the driver; here: byte ranges are right)
crlf.ts  tabs.ts  bom.ts  shebang.js
empty.ts  only_comments.ts
```

## 7. Steps

| # | Do | Proof |
|---|---|---|
| 1 | copy `languages/_template` to `languages/typescript`; pin tsrs at TS0's `rev`; fill `ENGINE.md` | `cargo build` in the workspace |
| 2 | `sem.rs`: `with_parsed`, kind tests, `name`, `range`, `params`, syntax errors | a unit test parses each dialect |
| 3 | `outline.rs`: functions and classes only | `functions.ts`, `classes.ts` snapshots reviewed |
| 4 | the rest of §2's table, one row at a time, each with its fixture | all snapshots reviewed. **This review is the qualname rule being frozen: read every line** |
| 5 | syntax errors; the never-panics test | `cut_every_50_bytes` passes |
| 6 | the facade; make modules private; `#![warn(unreachable_pub)]` | `cargo doc` shows the short list of §1 |
| 7 | write `languages/typescript/docs/qualnames.md` from §2–§3 | the doc and the snapshots agree |
| 8 | `typescript-cli outline <file>` (20 lines) | you can try any file by hand |

Check at the end: `cargo tree -p typescript-syntax` lists no `tsrs_checker`, `tsrs_compiler` or `tsrs_ls`.

Next: [`04-ts2-markers.md`](04-ts2-markers.md)
