# 08.04 Phase 3: the context layer

```
phase 3
├── 1. what "context" means in Bend
├── 2. BC0: the measurement
├── 3. BC1: templates
├── 4. BC2: function-typed parameters
├── 5. BC3–BC5: values through lets and returns · constructor fields · the fixpoint
└── 6. what stays unresolved, by design
```

Written against the TypeScript context layer's plan ([`../03-typescript/12-context-layer/`](../03-typescript/12-context-layer/README.md),
TC0–TC10, 2026-11-20) and Rust's ([`../04-rust/05-context-layer/01-design.md`](../04-rust/05-context-layer/01-design.md)). Bend's layer is a
subset of both, because the language removed the hard cases: no mutation, no fields that change, no classes, no dispatch, closures used at
most once. **Not scheduled**: designed now so that layer 1 is built on the right shapes (`key.rs`, `env.rs`, `resolve(site, &env)`), built
after BC0's numbers, as the other two tracks do.

## 1. What "context" means in Bend

Layer 1 leaves exactly one kind of site unresolved: **a call whose callee is a variable** (`function_typed_value`). A variable holds a function
in four ways:

| Way | Example | Who knows the value |
|---|---|---|
| a **template parameter** `~f` | `def twice(~f: U32 -> U32, x)`: `f(f(x))`; called as `twice(~g, 40)` | the **compiler**: a template argument must be closed (top-level defs and lambdas only), and each distinct set of `~` arguments is its own copy `twice~n`. The call site always has the answer |
| a **function-typed parameter** | `def apply(f: U32 -> U32, x: U32)`: `f(x)`; called as `apply(g, 1)` or `apply(x => h(x), 1)` | the caller, when it passes a name or a lambda; not when it passes a parameter of its own |
| a **closure from a call** | `add2 = adder(2)`; `add2(1)` | `adder`'s body: it returns `x => (x + k : U32)` |
| a **closure in a constructor field** | `App{+s => (s, Pix{s}), tick}` handed to `App.run` | the constructor site; the consumer is Base, so the chain stops |

A **context** is a def plus what is known about its function-typed parameters: `Key = BTreeMap<ParamIndex, Known>` where `Known` is a
project def (by `DeclId`) or a lambda (by its span in its def; its calls are the enclosing def's anyway, so a lambda as a value resolves to
"the calls inside it", which are already in the enclosing def's context: see §6). The digest is a hash of the map, zeros for the empty map, so
layer 1's context ids do not move until a key is non-empty (as RC1 says for Rust).

## 2. BC0: the measurement

One script, in `languages/bend/spikes/`, over the demos and `tests/`, with layer 1 built: for every `function_typed_value` site, which of the
four ways it is, and for the first two, whether the caller's argument is a def, a lambda or a parameter. Written into `ENGINE.md`. Go / no-go:
if templates and parameters together are most of the sites (the guide says templates are "how `List.map` is written in Base", and user code
calls Base's higher-order defs far more than it writes its own), BC1 and BC2 pay; if most sites are closures through lets, BC3 first.

## 3. BC1: templates

- `walk(f, key)` reads a `Term::Call` whose callee is a `~` parameter as a `Call` to `key[param]` when the key has it; `Unresolved` otherwise.
- At a call `t(~g, a)` in the caller's walk, the target is `Call(t, key')` with `key' = {0 ↦ g}`: the solver opens a context of `t` per distinct
  key, as it does for Rust's type arguments (RC1).
- A `~(x => …)` argument: `key' = {0 ↦ Lambda(span)}`; inside `t`, `f(x)` becomes a `Call` to… nothing nameable: the lambda has no def. Row
  BC-D1: the site is `Resolved-inline`: the lambda's own sites are **inlined** into `t`'s context as if written there. **[check]** against
  TC's decision on anonymous functions as values (Q-TC on lambdas); use the same answer. If TC chooses "a lambda is a def of its own", Bend's
  lambda gets a synthetic def `t.<lambda@offset>`? No: rule 10 forbids new qualnames of existing defs, but a *new kind* of def is allowed. Decide
  with TC, not alone.
- Proof: scenario `callback-argument` in template form matches `expected.precise.tree`; the `template-arg` policy row flips from "`f` is a Call
  from the enclosing def" to "`f` is a Call inside `twice`'s context for that caller".

Exact by construction: Bend itself refuses an open template argument, so there is no approximation in this phase.

## 4. BC2: function-typed parameters

The same key, for non-template parameters whose argument at the call site is a bare project def name or a lambda. An argument that is the
caller's own parameter stays unknown **unless the caller's key knows it** (the key flows down: `apply(f, x)` inside `twice(~f, …)` passes
`key[f]` along). An argument that is a `let` of a call result is BC3's. The rule set is TC2/TC3's with the mutation cases removed.

Proof: `two-callers` and `callback-argument` (closure form) match the precise trees; `variable` row splits into `resolved_by_key` and the
remaining `function_typed_value`.

## 5. BC3–BC5

| Phase | Adds | The Bend case | Limit |
|---|---|---|---|
| BC3 | `Env` inside one body: a `let` bound to a lambda, or to a call whose callee's summary says what it returns | `add2 = adder(2); add2(1)`: `adder`'s return summary is "a lambda at span s" → inline its sites (BC-D1) | a `match` that returns different closures per case: union of the cases (every target drawn; the renderer already shows several targets per site) |
| BC4 | return values across calls | `make(k)` returns `adder(k)` returns a lambda: two hops | depth-bounded, counted `env_depth` |
| BC5 | constructor fields | `App{view, tick}` → a `match` on `App{v, t}` in a project def and `t(events)` there | only when the constructor site and the match are both project code; Base consumers (`App.run`) stop the chain. The fixpoint for recursion through a field is RC6's shape; Bend's no-mutual-recursion rule means a def never reaches itself through another def's field, so the fixpoint is small |

## 6. What stays unresolved, by design

- a closure passed into Base and called by Base (`List.map(~f, xs)`'s `f`): the call happens in Base; we draw the passed edge and nothing more.
  This is the same as TypeScript's `arr.map(f)` today.
- a function read from a `do` block's bind (`f : U32 -> U32 <- IO.try(…)`): a value from an effect; `function_typed_value`.
- an `Array<U32 -> U32>` element called: `Array.get` is Base; unresolved.
- hub packages: external, whole.

These are listed so the owner can read the limits as a list of fixtures, one each, as RC6 does.

Next: [`05-decisions-and-questions.md`](05-decisions-and-questions.md)
