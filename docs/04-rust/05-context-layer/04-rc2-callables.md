# 04.05.04 RC2: callables as type arguments

```
RC2
├── 1. what the user sees
├── 2. why this is RC1 again, not a new mechanism
├── 3. function items
├── 4. closures  (under Q-RC1's answer A)
├── 5. what stays open after RC2
├── 6. tests
└── 7. steps
```

## 1. What the user sees

| Code | Layer 1 | RC2 |
|---|---|---|
| `fn each<F: Fn()>(f: F) { f() }` · `each(encode)` | `each` with nothing under it; `encode` `[Passed]` under the caller | `each` → `encode`; the `[Passed]` edge stays |
| `fn retry(op: impl Fn() -> bool)` · `retry(check)` | the same | the same as above |
| `struct Handler<F: Fn()> { f: F }` · `(self.f)()` in a method · `Handler { f: work }.fire()` | `Handler::fire` with nothing under it | `Handler::fire` → `work` |
| `each(Json::write_all)`, `each(W::flush)` under a closed key | nothing | the method; through the impl when it is a trait's |
| `each(\|\| encode())` | `encode` under the caller, where the closure is written; `each` with nothing under it | **unchanged tree** (answer A of Q-RC1). The `Unresolved` edge at `f()` is gone: the site is now known to call that closure, whose calls are already listed |
| `let f = \|\| work(); f();` | `work` at the closure; an `Unresolved` edge at `f()` | `work` at the closure; no `Unresolved` edge |

So for named functions the tree gains real children, and for closures the tree keeps its shape and loses false `Unresolved` counts.

## 2. Why this is RC1 again

> **🦀 Rust note: every function and every closure has a type of its own.** In Rust, `encode` (a function's name used as a value) does
> not have the type `fn()`. It has a unique, unnameable type the compiler calls a *function item type* ("`fn() {encode}`"), which holds
> no data and means exactly "this one function". Every closure expression likewise gets its own anonymous type. So when you call
> `each(encode)`, the type parameter `F` is inferred to be *the type of `encode`*, and `each::<{encode}>` is monomorphized like any
> other generic: inside that copy, `f()` is a direct call to `encode`. That is why Rust iterators and callbacks are as fast as
> hand-written loops, and it is why we need no new idea here: the callable is **already in the key**, as a type, since RC1.
> (A `fn()` *pointer* is the erased version, like `dyn`: RC3 and RC4.)

`F` is a slot of `each`; RC1's `callee_key` already fills it with whatever type the site passes. RC2 is: `sem` learns to look inside
such a type, and `resolve_from_env` answers `FnParam` sites.

## 3. Function items

```
resolve_from_env(f, site, env)   — open = FnParam
    callee_ty = sem.close(type of the callee expression, env.types)?       not closed → None
    match sem.callable(callee_ty):
        a function item (g, its type arguments)  → as RC1 §4: through sem.impl_method if g is a trait's item;
                                                   Target::Call(g, key) · External · …
        a tuple struct or variant constructor     → Target::Construct / External       (`.map(Wrapper)` inside a walked body)
        a closure                                 → §4
        a function pointer, a `dyn Fn`            → None: still open (FnPointer / Dyn), RC3–RC4
```

`sem.callable` is `Type::as_callable` → `CallableKind` on a closed type. The function item's own type arguments (`parse::<u8>` passed
as a value) come with it and go through `callee_key` unchanged.

Relevance (RC1 §5, clause a) now includes "a slot whose type is called".

## 4. Closures

With answer A ([design §8](01-design.md)):

- A site that calls a value whose closed type is a closure draws **no edge** and is counted as the note `closure_call_known`. It is not
  an `Unresolved` edge any more, because nothing about it is unknown: the calls it stands for are the closure's sites, which belong to
  the def the closure is written in and are in the tree there.
- This covers `FnParam` sites (the closure came in as a type argument) and `Closure` sites (it is in a local in the same body). For the
  second, no key is involved at all: the callee expression's type says which closure it is.
- A closure's own sites are resolved in the **writer's** context, as always: `main`'s `env.types` apply inside the closures `main`
  writes. Nothing to build.
- A closure is named, in a key's canonical text, by its enclosing def and its number in source order (RS4's `RawSite.closure`).

What answer B would add later is one function: "the sites of closure `n` of def `d`, under context `(d, key)`", which `sem.sites` can
already answer by filtering on the tag. Nothing in RC2 closes that door.

## 5. What stays open after RC2

| Still open | Why | Phase |
|---|---|---|
| `f: fn()`, `f: &dyn Fn()`, `Box<dyn FnMut()>`, a `Vec<Box<dyn Fn()>>` of handlers | erased on purpose: the type no longer says which function | RC3–RC6 |
| `fn make() -> impl Fn()` · `make()()` | an opaque return type | RC4 (the hidden type is a closure or a function item; then §3 or §4 applies) |
| a callable passed to **external** code (`iter.for_each(work)`, `thread::spawn(job)`) | the external body is never entered (rule 5) | stays. The `[Passed]` edge (D22) is the whole answer, as in layer 1 |
| `async` functions and futures (`spawn(task())`, `.await`) | awaiting is not a site; a future's `poll` is the compiler's | not planned. `task()` itself is an ordinary call and is in the tree |

## 6. Tests

| Test | |
|---|---|
| `tests/fixtures/context/`: `callback-fn`, `callback-method`, `callback-in-struct`, `callback-closure`, `closure-local`, `callback-external`, `callback-constructor` | snapshots flip; `Stats.unresolved` falls by exactly the `FnParam` and `Closure` sites that closed |
| the scenario `callback-argument` | matches `expected.precise.tree` **if** that tree is the one answer A draws. Python's precise tree puts the callback's calls under the function that calls it (answer B). So under A, Rust keeps this scenario at `Default` and says why in the scenario's README; this is the visible price of A, and part of what Q-RC1 decides |
| passed and called | `each(encode)`: one `[Passed]` node under the caller and one called node under `each`; the merged wire keeps them apart (D22: `passed` is in the merge key) |
| the design's §11 list | |

## 7. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `sem.callable` on closed types; `FnParam` for function items | `callback-fn`, `callback-method`, `callback-in-struct` |
| 2 | constructors as callables | `callback-constructor` |
| 3 | closures: `closure_call_known`, for `FnParam` and `Closure` sites | `callback-closure`, `closure-local`; the unresolved counts |
| 4 | relevance clause (a) for callables | RC1's relevance test still passes with these fixtures added |
| 5 | `policy.md`, `ENGINE.md`; the real-project numbers | reviewed |

RC2 is small **because** RC1 was built on types. If Q-RC1 is answered B, RC2 keeps these steps and a new phase follows it; it is
designed in `02-tree-rules/` first, for TypeScript and Rust together.

Next: [`05-rc3-local-values.md`](05-rc3-local-values.md)
