# 04.04.01 RS0: spikes

```
RS0
├── 1. why, and the rule for spike code
├── S-RS1  does it build as a dependency, and what does it cost?
├── S-RS2  load a real workspace and resolve its calls
├── S-RS3  what does the user's machine need?
├── S-RS4  can we find and edit the doc comment of every kind of def?
└── 2. what RS0 hands to the next phases
```

## 1. Why, and the rule

Everything in this folder was read from source and never run. TS0 took one working session and changed the TypeScript plan in four places
(the resolver the plan relied on was private; freeing memory needed regions; the JSDoc range came from the scanner). RS0 is the same
insurance. Spike code lives in `languages/rust/spikes/` as a workspace of its own, is never depended on, and is **deleted** when its answer
is in `languages/rust/ENGINE.md`.

S-RS1 and S-RS4 are all `rust-syntax` needs, so RS1 can start after those two.

## S-RS1: does it build as a dependency?

| Question | Write down |
|---|---|
| Which `ra_ap_*` version on crates.io is the clone's commit `2a362a4`, or the nearest one after it? | the version, as `=` pins, and the commit it was published from |
| Does a crate with only `ra_ap_syntax` build on 1.99.1? Does `cargo tree` show `salsa`? | yes / no; cold build time; binary size **[measure]** |
| Does a crate with `ra_ap_hir`, `ra_ap_ide_db`, `ra_ap_load-cargo`, `ra_ap_project_model`, `ra_ap_vfs` build? With `ra_ap_ide` added? | cold build time and binary size of each **[measure]**: this decides whether `ide` stays out of the release binary ([`../01-what-rust-analyzer-gives.md`](../01-what-rust-analyzer-gives.md) §2) |
| Is `ra_ap_syntax`'s edition type usable from outside (`ra_ap_parser::Edition` or a re-export)? | the path |
| Linux and Windows | through CI, as TS0's still-open item taught: do not leave it for later |

The engine's `rust-version` is 1.98 and its edition 2024; our toolchain is 1.99.1, so this should pass. **[check]**

## S-RS2: load a real workspace and resolve its calls

Load, with build scripts, the proc-macro server and cache prefill **off**:

1. this repo's `core/` (small, no proc macros of its own);
2. `languages/typescript/` (a real dependency graph, a git dependency);
3. one outside project with 100+ crates in its graph.

For each: load time, peak memory, and the same again for a second `Semantics` on the loaded database **[measure]**.

Then, on a hand-written file, ask the engine about each row and write the answer down. This table is the first draft of the policy table,
as S-TS3 was:

| Shape | Ask | The plan assumes |
|---|---|---|
| `json.write()` on a concrete type, method from a trait impl | `resolve_method_call_fallback` | the method in `impl Writer for JsonWriter`, and a substitution |
| `w.write()` with `w: &W`, `W: Writer` | the same | the trait's declaration |
| `w.write()` with `w: &dyn Writer`; with `w: Box<dyn Writer>` | the same | the trait's declaration |
| `w.write()` with `w: impl Writer`; on the result of `fn make() -> impl Writer` | the same | the trait's declaration |
| `self.step()` in a trait's default method | the same | the trait's declaration of `step` |
| `Foo::new()`, `W::new()`, `<Foo as Default>::default()`, `Default::default()` with the type inferred | `type_of_expr(callee).as_callable()`; `resolve_path_with_subst` | the function; for `W::new()` the trait's; for the inferred one **unknown: write down what comes back** |
| `Foo(a, b)`, `Enum::Variant(a)`, `Foo { a, b }`, `Self { .. }`, `Self(..)` | `as_callable().kind()`; `resolve_record_literal` or `resolve_path` on the record's path | `TupleStruct`, `TupleEnumVariant`, the struct |
| `f()` where `f` is a closure in a local; `(self.callback)(x)`; `f()` with `f: fn()`; `f()` with `f: F`, `F: Fn()` | `as_callable().kind()` | `Closure`, `FnPtr` (through a field), `FnPtr`, `FnImpl` |
| a function named as a value: `xs.iter().map(parse)`, `spawn(worker)`, `.map(Self::helper)`, `.map(Foo::new)`, `.map(Wrapper)` | `resolve_path` on the path expression | `PathResolution::Def(Function)`; for `Wrapper` the struct |
| `println!("{}", f())`, `vec![g()]` | walk the expansion; `original_range` / `original_range_opt` of the call | the call maps to the `f()` written in the file |
| a call that is in the **body** of a project `macro_rules!` | the same | the call maps to nothing smaller than the macro call. What exactly `original_range_opt` returns decides RS4 §3's rule |
| a call to a function a `macro_rules!` generated | `HasSource::source(db)` of the target | `file_id` is a macro file |
| a function under `#[tokio::main]`-style attribute, proc macros off | the calls of its body | found, from the original body |
| `a + b`, `x?`, `*p`, `for x in it`, `.await` | (not asked in layer 1) | only confirm that none of them is an `ast::CallExpr` / `MethodCallExpr` |

Also from this spike:

- **`hir::attach_db`.** The clone's `hir` imports `attach_db`; some type queries need a database "attached" to the thread. Find out whether
  a caller of `Semantics` must do it, and where. Write the rule into `ENGINE.md`, because forgetting it is a panic, not an error.
- **Cancellation.** How a query that is cancelled reports it to a caller outside `ide` (an unwind with `Cancelled`; is there a public
  `catch`?). RS5 depends on the answer; RS4 does not (one thread, no writer).
- **The oracle.** Call `ide::Analysis::outgoing_calls` on five functions and confirm its ranges are comparable with ours (the name of a
  method call, the callee expression of a call).
- **Scenario cost.** Loading a Cargo package runs `cargo metadata` and loads the sysroot. Measure one tiny package: if it is seconds, the
  thirteen scenarios load thirteen times. Options to compare with numbers: accept it; one workspace for all scenarios; a sysroot-less load
  for programs that use nothing from `std` (not possible for the `Box<dyn …>` ones).
- **`Cargo.lock`.** Does `cargo metadata` write a lockfile into a fixture folder? (Rule 6: no output files in git. Either the lockfiles are
  committed on purpose, or the load passes `--locked` / `--offline` and the fixtures have no dependencies.)

## S-RS3: what does the user's machine need?

Run the load of S-RS2 (1) with no `rust-src` component, with no `cargo` on the path, with a `rust-toolchain.toml` naming a toolchain that
is not installed, and offline with a dependency that is not downloaded. Write the exact error text of each: they become the variants of
`OpenError` in RS3, and the messages the driver shows.

One more: with `rust-src` missing, **does the load fail or succeed without `std`?** If it succeeds, every `String` and `Vec` method is
unresolved, silently. That must be an error or a loud counter, never a quiet half answer (rule 8).

## S-RS4: can we find and edit the doc comment of every kind of def?

Parser only. For each kind of def of RS1's table (function, method in an inherent impl, method in a trait impl, trait method with and
without a body, struct, enum, union, trait, inline module, type alias, const, static, `macro_rules!`, a function in a function, a foreign
function in an `extern` block):

| Question | The plan assumes |
|---|---|
| are the `///` lines above the item children of the item's node? In which order with attributes? | yes; doc comments and attributes interleave in source order |
| does a blank line between a `///` block and the item detach it? Between two `///` lines? | **unknown**. The first plan said "a blank line detaches a comment"; `n_attached_trivias` in `crates/parser/src/shortcuts.rs` is the rule to read and then to test, because for doc comments `rustc` itself does not care about blank lines |
| a plain `//` comment directly above: in the node or not? | in the node (so `range` must skip it by hand) |
| `/** … */`, `#[doc = "…"]`, `//!` inside a body, `////` | the first is a doc comment token; the second an attribute; the third an inner comment of the body; the fourth a plain comment |
| does `rustfmt` change a `/// ID: …` line? With `wrap_comments = true` and `normalize_doc_attributes = true`? | no; no; the last one turns `#[doc]` attributes into `///` lines, which is harmless |

## 2. What RS0 hands on

| To | What |
|---|---|
| RS1 | the engine pin; the edition type; the answers of S-RS4 about node ranges |
| RS2 | S-RS4's table: it *is* the reading rule |
| RS3 | the load numbers; the error texts; the `attach_db` rule; the lockfile answer |
| RS4 | the first policy table, observed; the macro-mapping answer; the scenario cost |
| RC0 | nothing yet. RC0 runs after RS4, on numbers RS4's counters produce |

`ENGINE.md` gets a section per spike, with the date, as TypeScript's has. Where an answer differs from what this plan assumed, the plan's
file is fixed in the same commit (CLAUDE.md, "How to work here").

Next: [`02-rs1-outline.md`](02-rs1-outline.md)
