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

**Measured (BC0, 2026-11-21, `languages/bend/ENGINE.md` §BC0): the table below had four ways; the sites show six.** The two missing: a lambda's
own binder (`k => s => k(x, s)`, a CPS continuation; 1 of the demos' 55 sites, 27 in the corpus) and a parameter the callers **never pass at
all** (partial application: `Unit <- drop(2n)` applies `drop` to one of its three arguments and `P.bind` supplies the rest later; 10 of the
demos' 32 parameter sites). And a template argument is not only "a def or a lambda": it is any **closed** term, and the demos pass a nullary
def called at compile time (`~World()`), a constructor of defs (`~App{Fly.view, Fly.tick}`) and, 330 times out of 387, **the caller's own `~`
parameter passed along**. So `Known` must hold a closed term (a def, a lambda, a call, a constructor), and the key must **flow down** through
callers from the first commit, or BC1 resolves 1 of the 16 template sites instead of 16.

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

**Done (2026-11-21, 6f4cff0): the numbers are in `languages/bend/ENGINE.md` §BC0, checked two ways (the spike's site list equals the crate's
`walked()` spans over 78,814 defs; its totals are the sweep's).** The demos: 55 `function_typed_value` sites in 4,323 call sites (1.3%): 16
template parameters, 32 plain parameters, 3 closures from a call's result (all `f(a)(k)`, none a `let`), 3 constructor fields, 0 do binds, 0
lambdas in place, 1 lambda's binder. Following callers up names the function for all 16 templates and 12 of the 32 parameters; the 20 others
are 17 in one program (`pure_hvm5_mini`'s CPS parser: partial applications and primitives handed to `P.bind`) and 3 with no caller but the
sugar. Base: 35 of 820 sites, 91% templates and parameters. The corpus's 545 are 440 checker probes in `tests/`. **Verdict:** BC1 and BC2 are
one mechanism and pay (28 of 55 sites, 0.65 points of all call sites, exact for templates); BC3, BC4 and BC5 do not pay today (3, 0 and 3
sites) and wait for a program of the owner's that has them. Whether to build BC1+BC2 at all is Q-BD8 (05 §3).

A prior, from a regex count another reader made on the demos at the same commit (2026-11-20, not ours, **[measure]** in BC0): about 5,900
call sites, 64 with a local variable as head; in Base, 39 of about 1,000. If that holds, layer 1 resolves about 99% of sites by name and the
context layer is about the last 1%, which is why it is designed small and scheduled last.

One script, in `languages/bend/spikes/`, over the demos and `tests/`, with layer 1 built: for every `function_typed_value` site, which of the
four ways it is, and for the first two, whether the caller's argument is a def, a lambda or a parameter. Written into `ENGINE.md`. Go / no-go:
if templates and parameters together are most of the sites (the guide says templates are "how `List.map` is written in Base", and user code
calls Base's higher-order defs far more than it writes its own), BC1 and BC2 pay; if most sites are closures through lets, BC3 first.

## 3. BC1: templates (built together with BC2 as one step, per BC0)

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

## 5. BC3–BC5 (not worth building today: BC0's numbers; each stays a fixture of the known limits)

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
- **partial application in a CPS monad** (`Unit <- drop(2n)`, the continuation supplied later by `P.bind`) and a primitive handed to the
  monad's bind (`eof`, `peek`, `pop`): the function is applied by Base-like glue the caller never names; 17 of the demos' 55 sites, all in
  `pure_hvm5_mini`. A limit, with a fixture.
- a lambda's own binder called (`k => s => k(x, s)`): the value comes from whoever calls the lambda; a limit, with a fixture.

These are listed so the owner can read the limits as a list of fixtures, one each, as RC6 does.

Next: [`05-decisions-and-questions.md`](05-decisions-and-questions.md)
