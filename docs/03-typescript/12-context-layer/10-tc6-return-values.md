# 03.12.10 TC6: return values

```
TC6
├── 1. what the user sees
├── 2. what a context returns
├── 3. what that asks of the solver  (the note this phase writes first)
├── 4. `Expr::Call`: reading a call's value
├── 5. module constants made by a call
├── 6. the last rows: a getter's result, a decorator factory
├── 7. fixtures and tests
└── 8. steps
```

Depends on TC2–TC5 for the values it returns. **The one phase that changes core.** On V-NOC it addresses the largest column (77% of
the unresolved function-typed calls come from a call's result), and N8 says how much of that column is project code at all.

## 1. What the user sees

Patterns D1–D7 of [02](02-patterns.md); line g of the tour.

| Code | Before | TC6 |
|---|---|---|
| `function pick(): Writer { return new Csv(); }` · `pick().write()` | unresolved | `Csv.write` |
| `function ident(x) { return x; }` · `ident(handler)()` | `Unresolved(no_declaration)` | `handler` |
| `function useSaver() { const save = () => {…}; return { save }; }` · `const { save } = useSaver(); save()` | unresolved | `save`, with what it captured inside `useSaver` |
| `function withAuth(C) { return function Guard(p) { return <C {...p} />; }; }` · `const Page = withAuth(Home)` · `<Page />` | unresolved | `Guard` → `Home` |
| `const w = await makeWriter(); w.write()` | unresolved | the class `makeWriter` builds |
| `twice(work)()` | `Unresolved(no_declaration)` | `Unresolved(anonymous_function_called)`: known to be the arrow in `twice`, whose calls TC2 already drew under `twice` |
| `const [open, setOpen] = useState(false); setOpen(true)` | unresolved | **unresolved**: the function is made inside a library ([02](02-patterns.md) F1) |
| a recursive function's own result | unresolved | unresolved, counted `return_in_cycle` (TC9) |

## 2. What a context returns

`flow.returns`: the expression of every `return e` of the def's **own** body (not of the functions written inside it), and the body of
an arrow that is an expression. Then:

```
returns(g, key) = join(eval(e) for e in flow(g).returns), in the env of (g, key)
```

- It is a `Value`, so it is closed: it names no variable of `g`. A nested def that is returned carries its captures
  (TC3), which is exactly what a custom hook's `save` needs.
- An `async` function returns what its `return` gives, and `await e` is `e`'s value. A `Promise` built by hand is unknown.
- A generator returns unknown. A function with no `return` returns the empty value.
- A def with no body (a signature, a `declare function`) and external code return unknown.

## 3. What that asks of the solver

Core's flat solver calls `walk(f, key)` and gets sites back. `walk` cannot ask anything about a callee. To evaluate `pick().write()`
in `main`, the walk of `main` needs `returns(pick, key)` **during** its own walk.

Rust's plan reached the same wall and compared two shapes
([`04-rust/05-context-layer/06-rc4-values-across-calls.md`](../../04-rust/05-context-layer/06-rc4-values-across-calls.md) §5). In short:

| | (a) the language keeps its own table of return values | (b) the solver answers "what does this context return?" |
|---|---|---|
| core | unchanged | `Lang` gains `type Ret` (`()` for a flat language); `Summary` gains `returns: L::Ret`; `walk` receives a handle with `returns(g, &key) -> Option<&L::Ret>` |
| limits, cancel, counters | the hidden evaluations are outside them | one counter, one cut, one cancel |
| recursion | the language detects it | the solver sees "asked for while in progress" and answers `None` |
| who benefits | one language | TypeScript now, Rust at RC4 |

**(b), as Rust's plan recommends**, and for one more reason on this side: TypeScript's evaluation is lazy, so a return value can pull
in a chain of other contexts. Those walks must be the solver's, or `max_contexts` and the deadline stop meaning what they say.

This phase **starts with the note** in `docs/02-tree-rules/` (as passed edges got theirs), for your approval before any code. Whichever
of TypeScript and Rust gets there first writes it; it is one note. What it must settle:

| Question | My answer today |
|---|---|
| a context asked for while it is being walked | `None`; the language counts it (`return_in_cycle`) and uses unknown. A summary that got a `None` is still complete: unknown is a sound value. TC9 replaces `None` by a fixpoint if the counter asks |
| a walk made only for a value, under a **budget** | it counts toward the cost limits and not toward the budget. A budget says how much of the tree the request wants; a caller's own edges must not depend on it (rule 1) |
| the order of visits | depth-first where a value is needed, so which contexts a cost limit cuts can differ from today **for a language that uses the handle**. Still deterministic (source order). Flat languages do not change |
| a callee that was cut | its `returns` is not used: unknown, and the asking context is marked as having read a cut (Python's `reader_read_cut`) |
| the proof for flat languages | TypeScript with the switch off, Rust layer 1 and the toy languages give byte-identical graphs before and after |
| lifting Python's solver into core instead | no. Python's `solver.rs` is built around parked walks and replayed operations, which neither TypeScript nor Rust has. The handle is a small growth of the flat solver; the fixpoint is a separate question for TC9 / RC6 |

> **🦀 Rust note: why `walk` gets a handle and not the solver.** The solver owns the language (`Solver { lang: L, memo: … }`) and calls
> `self.lang.walk(…)`. While that call runs, `self.lang` is mutably borrowed, so `walk` cannot also receive `&mut self` (the whole
> solver): that would be two mutable borrows of the same thing, which Rust forbids. The way out is to lend `walk` only the *other*
> fields. Rust lets a function borrow two different fields of a struct at once ("disjoint borrows"), so the solver can build a small
> struct holding `&mut self.memo` and the limits, and pass that. The handle's `returns` then needs to walk a callee, which needs the
> language again, while the outer `walk` still holds it. Two ways are open: `walk` takes `&self` (a shared borrow can be handed out
> twice; the language then keeps its counters in `RefCell`s, as TypeScript's already does), or the walk lends itself back for the
> inner call, `callees.returns(self, g, &key)` (a *reborrow*: the outer `&mut` is paused while the inner one is used). That choice is
> the technical heart of the note.

## 4. `Expr::Call`

`sem/flow.rs` turns a call expression used as a value into `Expr::Call(site)`. Evaluating it:

```
eval(Call(i)) = join over the targets of site i (the layer's own, so a callee resolved through a parameter counts):
    Call(g, key)       callees.returns(g, key), or unknown when it answers None
    Construct          as Expr::New
    External           unknown        (counted `external_call_result`, by package, in the sweep: the list TC10 reads)
    Unresolved         unknown
```

The targets of a site are computed once per walk and kept, so `f()()` (the second call's callee is the first call's value) costs one
resolution of each. `relevant` (TC2 §8) gains its rule (c) for real: a parameter that can reach a `return` is relevant.

`const { save } = useSaver()` needs nothing new: the variable's assignment is `Member(Call(i), save)` (TC4's patterns).

## 5. Module constants made by a call

`export const api = createApi();` is a module variable whose value is now computable. It is evaluated through the same handle, by
whichever walk asks first. Because the value is a function of the code alone, who asks does not matter, with one exception to
handle: a value computed while a context it depends on was in progress got a `None`, and must not be kept. `[check]` in the note: the
memo of module values keeps only values that met no `None`.

## 6. The last rows

Small, each behind a counter that exists, done last and only if N8 or the sweep shows them:

- **a getter's result** (`b.fn()` with `fn` a getter): the getter's return value is called. Removes `uncovered_getter_result_calls`.
- **a decorator factory** (`@factory(1)`): the function `factory` returns is called with what is decorated. Replaces
  `Unresolved(decorator_result)`.
- **a tagged template whose tag is a call** (``make()`x` ``).

## 7. Fixtures and tests

`return-instance` · `return-argument` · `return-either` (two `return`s) · `return-object` · `return-hook` · `return-nested-def` ·
`return-hoc` · `return-arrow` · `return-await` · `return-chain` (three functions deep) · `return-external` (unknown, counted) ·
`return-recursive` (counted) · `return-none` · `return-generator` · `return-module-const` · `return-getter` ·
`return-decorator-factory`.

| Test | |
|---|---|
| core: a toy language with return values | the handle memoizes; a callee asked for mid-walk is walked once; in progress → `None`; the limits count it; a budget does not |
| flat languages unchanged | every TypeScript snapshot with the switch off, every Rust and toy snapshot: identical |
| the three properties on every fixture, with return values compared too | |
| a tree cut by a budget and continued equals the whole tree | the budget row of §3 |
| the tour's line g; every line but i is in its final column | |
| the four projects | the per-site measure; `return_in_cycle`, `external_call_result` by package |

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | the note; your decision | approved |
| 2 | core: `Ret`, `Summary::returns`, the handle, the toy tests | core green; flat languages byte-identical |
| 3 | `sem/flow.rs`: `returns`, `Expr::Call`, `await` | table snapshots |
| 4 | `returns(g, key)`; `eval(Call)` | `return-instance`, `return-argument`, `return-object`, `return-hook`, `return-chain` |
| 5 | recursion and cuts: `None`, the counters | `return-recursive`; the cut test |
| 6 | module constants | `return-module-const` |
| 7 | relevance rule (c) | equal edges, fewer contexts |
| 8 | §6's rows, if counted | their fixtures |
| 9 | the four projects | `ENGINE.md`; the forecast of TC10 from `external_call_result` |
| 10 | (mine) `policy.md`, `ENGINE.md`, [`02-tree-rules/03-the-seam.md`](../../02-tree-rules/03-the-seam.md) §6: "the open part" is closed | reviewed |

Next: [`11-later.md`](11-later.md)
