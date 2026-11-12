# 04.05.06 RC4: values across calls

```
RC4
├── 1. what the user sees
├── 2. `-> impl Trait`: the half that needs no values
├── 3. going in: erased parameters in the key
├── 4. coming out: return values
├── 5. what that asks of the solver  (the note this phase writes first)
├── 6. relevant parameters, again
├── 7. tests
└── 8. steps
```

This is where a trait object passed to a function resolves, the case people mean when they say "context-sensitive". It is also the
first phase that cannot run on core's flat solver as it is (§5).

## 1. What the user sees

| Code | Before | RC4 |
|---|---|---|
| `fn send(w: &dyn Writer) { w.write() }` · `send(&Json)` in one caller, `send(&Csv)` in another | `send` → `Writer::write`, the same under both | two contexts of `send`; each caller sees its own impl |
| `fn run(job: fn()) { job() }` · `run(work)` | `run` with nothing under it | `run` → `work` |
| `fn pick() -> Box<dyn Writer> { Box::new(Csv) }` · `pick().write()` | `Writer::write` | `<Csv as Writer>::write` |
| `fn make() -> impl Writer { Json }` · `make().write()` | `Writer::write` | `<Json as Writer>::write` (§2) |
| `fn wrap(w: Box<dyn Writer>) -> Box<dyn Writer> { w }` · `wrap(Box::new(Json)).write()` | the declaration | `Json`'s: in, through, and out |
| a chain: `a(&Json)` → `b(w)` → `c(w)` → `w.write()` | declarations | resolved at the end of the chain |
| a recursive function's return value | the declaration | **the declaration** until RC6 |

## 2. `-> impl Trait`: the half that needs no values

> **🦀 Rust note: an opaque type is not an erased type.** `fn make() -> impl Writer` looks like `-> Box<dyn Writer>`, and it is the
> opposite. With `impl Writer` the function returns **one concrete type**, fixed at compile time, and merely refuses to say its name
> to callers; the compiler still knows it and monomorphizes through it. The type the caller sees is called *opaque*: "the return type
> of `make`". Nothing was forgotten at run time, so nothing has to be followed: the answer is in the callee's body, as a type.

An `Open::Opaque` site has a receiver whose type is "the return type of `g`" (with `g`'s type arguments). That is true wherever the
value travelled in the caller, because the type itself names `g`. So:

```
resolve_from_env  — open = Opaque
    (g, args) = sem.opaque_origin(type of the receiver)          which function's return type this is   [check: the public form]
    key_g     = the key built from args, closed with env.types   (RC1 §3)
    hidden    = sem.hidden_type(g, key_g)?                       the type g's body returns, closed under key_g
    → RC1 §4's lookup with self_ty = hidden
```

`sem.hidden_type` asks the engine for the type of the function's returned expressions (**[check]** whether inference exposes the
hidden type directly; otherwise the type of the tail expression, which Rust requires to be the same type as every `return`). It is
memoized in `sem` per (function, key) and involves no walk and no env. If the hidden type is a closure or a function item, RC2
answers the call. This is step 1 of the phase and needs nothing from core.

## 3. Going in: erased parameters in the key

`Key.erased` gets one slot per parameter of the function whose declared type holds an erased type, the receiver first.

```
callee_key(g, site, env)        — RC1 filled `types`; now also:
    for each erased slot j of g:
        key.erased[j] = policy.admit(g, j, eval(the argument's Expr at this site, env))      // Unknown if not relevant (§6)

walk(g, key)                    — at the start:
    env.locals[parameter j] = key.erased[j]
```

`sem.flow` attaches to each `Site` the `Expr` of every argument that goes into an erased slot (it knows the callee's signature from
the site's resolution). Arguments are evaluated **eagerly**, once, in the caller's env at the call, as pylspt does: a chain
`a → b → c` flattens by itself, because `b` evaluates its argument `w` in an env where `w` already holds what `a` passed.

The entry is walked under the open key: its erased parameters are `Unknown`, and its `dyn` sites show the declaration. That is
correct (nobody told us), and it is why a tree asked for at `main` is richer than one asked for at `send`.

## 4. Coming out: return values

A context's **return value** is the join of the values of its returned expressions (every `return e` and the tail), evaluated in the
env the event interpreter has at that point. It is `Unknown` unless the function's return type holds an erased type, so most functions
never compute one.

Because parameters start from the key (§3), a function that returns its own parameter returns the key's value: no "this value is
parameter 0" symbols are needed. This is the simplification over pylspt's summaries ("values written relative to the inputs"), and it
holds because our values have no identity ([design §4](01-design.md)): passing a `Behind([Json])` in and getting it back is the same
fact as building it there.

In the caller, `Expr::Call(site)` evaluates to: the join of the return values of the site's `Target::Call` contexts; `Unknown` if
any target is external, unresolved, cut, or still being computed (recursion; RC6).

## 5. What that asks of the solver

To evaluate `pick().write()` while walking the caller, the walk must **ask for a callee's result in the middle of a body**. Core's
flat solver cannot: `Lang::walk` returns sites and the solver visits callees afterwards, breadth-first.
[`02-tree-rules/03`](../../02-tree-rules/03-the-seam.md) §6 names this "the open part", and
[`03-typescript/08`](../../03-typescript/08-later-context-sensitive.md) §4 sketches the growth. RC4 **starts by writing a design note**
(`docs/02-tree-rules/`, the way passed edges got theirs), for your approval, before any code. Its content, as I see it today:

| | (a) the language keeps its own table of return values | (b) the solver answers "what does this context return?" |
|---|---|---|
| shape | `Rust` holds `HashMap<(Func, Key), Value>` and, inside `walk`, interprets a callee's body itself when it needs its value | `walk` receives a handle: `callees.returns(g, &key) -> Option<&L::Ret>`. The solver walks `(g, key)` then and there if it has not, memoizes it, and hands back its `Summary::returns` |
| core changes | none | `Lang` gains `type Ret` (unit for a flat language) and the handle parameter; `Summary` gains `returns` |
| limits and stats | the hidden walks are not counted, not cut, not cancelled by the solver: `max_contexts` stops meaning what it says | every walk goes through the solver: one counter, one cut, one cancel |
| bodies walked | twice (once for the value, once for the sites), or a second memo next to the solver's | once |
| recursion | the language must detect it itself | the solver sees "asked for while in progress" and answers `None`; RC6 turns that into the fixpoint |
| Python, TypeScript | no help to either | this *is* the part of pylspt's solver that is not about Python; TypeScript's C4 needs the same thing |

I recommend **(b)**. (a) is the quick way and it breaks rule 8 quietly (limits that no longer count what they limit). (b) is a real
change to `core/`, so: a toy language with return values tests it first; TypeScript and Rust layer 1 never call the handle and must
produce byte-identical graphs before and after; Python is untouched (it does not implement `Lang`).

One consequence to state in the note: asking for a callee on demand makes the visiting order depth-first where a value is needed, so
*which* contexts a cost limit cuts can differ from today's breadth-first order **for a language that uses the handle**. It stays
deterministic (source order), and flat languages are unaffected.

The note also answers whether this is the moment to lift pylspt's `solver.rs` into core instead of growing the flat one (the rule of
two is met: Python has it, Rust needs it). That depends on how much of that file is free of ty's types, which nobody has measured.

## 6. Relevant parameters, again

RC1 §5's idea, extended: an erased slot is **relevant** when its value can change a target or the return value:

```
(c) the parameter reaches the receiver or callee of an open Dyn / FnPointer site of the body
(d) the parameter is passed into a relevant erased slot of a project callee
(e) the parameter can reach a returned expression, and the return type holds an erased type
```

A slot that is not relevant is `Unknown` in the key, so `fn log(w: &dyn Writer) { println!("writing") }` has one context however many
writers it is called with. Like RC1's, it comes after the phase works without it, with an equal-edges test.

`policy.admit` also applies the caps of [design §7](01-design.md) (`max_behind`).

## 7. Tests

| Test | |
|---|---|
| core: the toy language with return values | the handle memoizes; a callee asked for mid-walk is walked once; asked while in progress → `None`; limits count it |
| flat languages unchanged | every TypeScript snapshot and every Rust layer-1 snapshot identical before and after the core change |
| `tests/fixtures/context/`: `opaque-return`, `opaque-closure`, `dyn-param`, `dyn-param-two-callers`, `fnptr-param`, `dyn-factory`, `dyn-through`, `dyn-chain`, `dyn-return-branch` (two types), `dyn-return-external` (stays), `dyn-return-recursive` (stays, counted) | snapshots flip |
| scenarios | the `dyn` form of `interface-dispatch` matches `expected.precise.tree` |
| the tour ([design §2](01-design.md)) | lines a–f are in their final column |
| the design's §11 list | with no-reuse mode now also comparing return values |

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `Open::Opaque`: `sem.opaque_origin`, `sem.hidden_type` | `opaque-return`, `opaque-closure`. No core change, releasable alone |
| 2 | the design note for §5; your decision | approved |
| 3 | core: `Ret`, the handle, the toy tests | core green; flat languages byte-identical |
| 4 | `Key.erased`; arguments into the key; parameters from the key | `dyn-param`, `dyn-param-two-callers`, `fnptr-param`, `dyn-chain` (no return values yet) |
| 5 | `ret.rs`: return values; `Expr::Call` | `dyn-factory`, `dyn-through`, `dyn-return-branch` |
| 6 | relevance (c)–(e); the caps | equal edges, fewer contexts; real-project numbers |
| 7 | `policy.md`, `ENGINE.md` | reviewed |

Next: [`07-rc5-fields.md`](07-rc5-fields.md)
