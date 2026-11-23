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
| a recursive function's own result (`build(n)` returns `leaf` or `build(n - 1)`) | unresolved | `leaf`: the value of a cycle is worked out by rounds (§3.1) |

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
| recursion | the language detects it, and must iterate by itself | the solver sees "asked for while in progress", answers the value so far, and works the group out by rounds (§3.1) |
| who benefits | one language | TypeScript now, Rust at RC4 |

**(b), as Rust's plan recommends**, and for one more reason on this side: TypeScript's evaluation is lazy, so a return value can pull
in a chain of other contexts. Those walks must be the solver's, or `max_contexts` and the deadline stop meaning what they say.

This phase **starts with the note** in `docs/02-tree-rules/` (as passed edges got theirs), for your approval before any code. Whichever
of TypeScript and Rust gets there first writes it; it is one note. What it must settle:

| Question | My answer today |
|---|---|
| a context asked for while it is being walked | **its value so far, which starts as data ("nothing yet"), never unknown; then the group is worked out by rounds** (§3.1). It said `None` and unknown before the review of 2026-11-23: that makes the answer depend on which function the request entered first |
| a walk made only for a value, under a **budget** | it counts toward the cost limits and not toward the budget. A budget says how much of the tree the request wants; a caller's own edges must not depend on it (rule 1) |
| the order of visits | depth-first where a value is needed, so which contexts a cost limit cuts can differ from today **for a language that uses the handle**. Still deterministic (source order). Flat languages do not change |
| a callee that was cut | its `returns` is not used: unknown, and the asking context is marked as having read a cut (Python's `reader_read_cut`) |
| the proof for flat languages | TypeScript with the switch off, Rust layer 1 and the toy languages give byte-identical graphs before and after |
| lifting Python's solver into core instead | no to its machine, yes to its rule. Python's `solver.rs` parks walks and replays recorded operations because its walk executes statements and cannot simply be run again. A `Flow` is data: evaluating a context again is the whole replay. So core gets the rule (start empty, repeat until nothing changes, keep nothing before the group is final) in a few dozen lines, and none of the parking |

> **🦀 Rust note: why `walk` gets a handle and not the solver.** The solver owns the language (`Solver { lang: L, memo: … }`) and calls
> `self.lang.walk(…)`. While that call runs, `self.lang` is mutably borrowed, so `walk` cannot also receive `&mut self` (the whole
> solver): that would be two mutable borrows of the same thing, which Rust forbids. The way out is to lend `walk` only the *other*
> fields. Rust lets a function borrow two different fields of a struct at once ("disjoint borrows"), so the solver can build a small
> struct holding `&mut self.memo` and the limits, and pass that. The handle's `returns` then needs to walk a callee, which needs the
> language again, while the outer `walk` still holds it. Two ways are open: `walk` takes `&self` (a shared borrow can be handed out
> twice; the language then keeps its counters in `RefCell`s, as TypeScript's already does), or the walk lends itself back for the
> inner call, `callees.returns(self, g, &key)` (a *reborrow*: the outer `&mut` is paused while the inner one is used). That choice is
> the technical heart of the note.

### 3.1 A value around a cycle

```ts
function a(n) { return n ? b(n - 1) : work; }
function b(n) { return n ? a(n - 1) : other; }
```

If "still being walked" were answered with unknown: a request that enters at `a` walks `b` while `a` is unfinished, so `b` is kept as
`{other}` and open. A request that enters at `b` keeps `b` as `{work, other}` and open. The same context, two answers. Both are sound and
neither is wrong, but rule 8 of the design is broken: a tree cut by a budget and continued from `b` disagrees with the tree asked for
whole, and the README's rule 9 fails. Python never answers that way. Its rule, in the form a `Flow` allows:

| | Rule | Python |
|---|---|---|
| 1 | A context asked for while it is being walked answers **its value so far**. That value starts as **data**: empty and closed, "nothing yet" | `defer.rs` `no_values`: "every pending value empty: round 0" |
| 2 | The asker, and every context between it and the one asked, form a **group**. When the first of them finishes, the members are evaluated again, in the order they were met, until no member's return value and no member's sites change | `solver.rs` `resolve_group`, `Slot::set_current` |
| 3 | Nothing computed inside a group is kept in the memo before the group is final | `State::Parked`, `State::Resolving` |
| 4 | **Reading "nothing yet" must answer "nothing yet"**: a call of data returns data, a member of data is data. Unknown is only for what the layer cannot read. An early unknown would stick (a set only grows), and whether it happens would depend on the order | a replay starts every pending value empty |
| 5 | It ends because a value is finite: a set of at most `max_set`, nested at most `max_depth`, and `Wide` absorbs. A backstop of 32 rounds: past it every member's value is `Wide`, the group's contexts are marked and `return_rounds_capped` is counted | `max_rounds` 32, `Gaps::CYCLE`, `rounds_capped` |
| 6 | A cost limit and the cancel are looked at **before every walk made for a value**, as before every other walk. A value walk nested deeper than a cap is cut, counted, and its value is not used (the stack is finite: a 2 MiB stack already aborts on deep code, [`../10-review-fixes.md`](../10-review-fixes.md)) | `cost_limit`; `max_stack` 256, `Cut::Stack` |
| 7 | Who reads a cut value is marked as having read a cut, and is not reused | `reader_read_cut`, `settle`: "if any member read a cut, every member is cut" |

With this, `a` and `b` both return `{work, other}`, complete, from either entry, and `build(3)()` resolves to `leaf` in TC6 and not "one
day". How a group is found (the depth of the lowest running context a walk read, with a union-find, as Python does; or the strongly
connected parts of "who read whose value", with `petgraph`) is chosen in the note: [03](03-design.md) §15.5.

> **🦀 Rust note: why "start empty and repeat" gives one answer.** Think of each context's return value as a box that can only *gain*
> members. Start every box empty. Evaluate; a box may gain something; anyone who read that box is evaluated again. Because boxes only
> gain and there are finitely many possible members, this stops. The boxes it stops at are the *smallest* ones that agree with the code,
> and there is only one smallest answer, so the order of evaluation cannot change it. Starting a box as "unknown" breaks the argument:
> unknown is already the largest value, so whoever happens to be read first gets it for good.

## 4. `Expr::Call`

`sem/flow.rs` turns a call expression used as a value into `Expr::Call(site)`. Evaluating it:

```
eval(Call(i)) = join over the targets of site i (the layer's own, so a callee resolved through a parameter counts):
    Call(g, key)       callees.returns(g, key): final, or inside a cycle the value so far (§3.1); unknown only for a cut
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
handle: a value computed while a context it depends on was unfinished read a value so far, and must not be kept before that group is final. `[check]` in the note: the
memo of module values keeps only values that read no unfinished context.

## 6. The last rows

Small, each behind a counter that exists, done last and only if N8 or the sweep shows them:

- **a getter's result** (`b.fn()` with `fn` a getter): the getter's return value is called. Removes `uncovered_getter_result_calls`.
- **a decorator factory** (`@factory(1)`): the function `factory` returns is called with what is decorated. Replaces
  `Unresolved(decorator_result)`.
- **a tagged template whose tag is a call** (``make()`x` ``).

## 7. Fixtures and tests

`return-instance` · `return-argument` · `return-either` (two `return`s) · `return-object` · `return-hook` · `return-nested-def` ·
`return-hoc` · `return-arrow` · `return-await` · `return-chain` (three functions deep) · `return-external` (unknown, counted) ·
`return-recursive` (resolves) · `return-cycle` (`a` and `b` above, entered from each: the same value) · `return-rounds-capped` · `return-none` · `return-generator` · `return-module-const` · `return-getter` ·
`return-decorator-factory`.

| Test | |
|---|---|
| core: a toy language with return values | the handle memoizes; a callee asked for mid-walk is walked once; a ring of 2, 3 and 9 functions gives the same values from every entry; nothing of a group is kept before it is final; the limits count a value walk; a budget does not |
| flat languages unchanged | every TypeScript snapshot with the switch off, every Rust and toy snapshot: identical |
| the three properties on every fixture, with return values compared too | |
| a tree cut by a budget and continued equals the whole tree | the budget row of §3 |
| the tour's line g; every line but i is in its final column | |
| the four projects | the per-site measure; `return_rounds_capped`, the largest group and its rounds, `external_call_result` by package; the time of one expand request |

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | the note; your decision | approved |
| 2 | core: `Ret`, `Summary::returns`, the handle, the toy tests | core green; flat languages byte-identical |
| 3 | `sem/flow.rs`: `returns`, `Expr::Call`, `await` | table snapshots |
| 4 | `returns(g, key)`; `eval(Call)` | `return-instance`, `return-argument`, `return-object`, `return-hook`, `return-chain` |
| 5 | cycles: the value so far, the rounds, nothing kept before a group is final; cuts; the nesting cap | `return-recursive`, `return-cycle` from both entries; the cut test |
| 6 | module constants | `return-module-const` |
| 7 | relevance rule (c) | equal edges, fewer contexts |
| 8 | §6's rows, if counted | their fixtures |
| 9 | the four projects | `ENGINE.md`; the forecast of TC10 from `external_call_result`; **the time of one "expand this node" request** on the sample entries, which is the number that says whether a memo must outlive a request ([03](03-design.md) §15.1) |
| 10 | (mine) `policy.md`, `ENGINE.md`, [`02-tree-rules/03-the-seam.md`](../../02-tree-rules/03-the-seam.md) §6: "the open part" is closed | reviewed |

Next: [`11-later.md`](11-later.md)
