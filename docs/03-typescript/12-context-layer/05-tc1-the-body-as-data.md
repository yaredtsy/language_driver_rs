# 03.12.05 TC1: the body as data

```
TC1
├── 1. what it builds, and what it must not change
├── 2. `flow.rs`: the types
├── 3. `sem/flow.rs`: reading a body
├── 4. the switch: `Options::precision`
├── 5. the empty layer: `Key`, `Value`, `Env`, `eval`
├── 6. the harness every later phase uses
├── 7. tests
└── 8. steps
```

Depends on TC0 (S-TC1, S-TC2, S-TC3). Changes no graph.

## 1. What it builds

The floor of the layer: the table of a body ([03](03-design.md) §4), the switch that turns the layer on, the types with nothing in
them yet, and the tests that every later phase must keep green. After TC1 the layer exists and answers "unknown" to everything.

**It must not change**: any graph, tree, wire, counter or snapshot, with the switch off **or on**. With it on, every value is unknown,
so every site gets the engine's answer (rule 5) and every key is the open key (digest zeros). That equality is TC1's proof.

## 2. `flow.rs`: the types

The types of [03](03-design.md) §4, in a file outside `sem/` that imports nothing from tsrs. TC1 defines all of `Flow` and fills part
of it; a later phase fills more and adds `Expr` variants. `Flow` and `Expr` are `pub(crate)` and `#[non_exhaustive]` is not needed
(nothing outside the crate sees them).

| Filled in TC1 | Left empty until |
|---|---|
| `sites`: for each site, its span, its kind (call, `new`, JSX, tagged, decorator, accessor), the callee as an `Expr`, the arguments as `Expr`s with a `spread` flag, the anonymous function it is written in (if any) | — |
| `Expr::Unknown`, `Data`, `Def`, `Anon`, `OneOf`, and the look-through forms (parentheses, `as`, `!`, `satisfies`) | `Param` (TC2), `Var` (TC3), `Member`, `Object` (TC4), `New`, `This` (TC5), `Call` (TC6) |
| `handed`: the hand-overs, as `(span, Expr)` | — |
| `anons`: span and number, in source order | their parameters and captures (TC7) |
| `params`: how many, which is a rest parameter | `capable` (TC2) |
| | `vars`, `needs` (TC3), `returns` (TC6) |

A JSX element's attributes are one argument. In TC1 it is `Expr::Unknown`; TC4 makes it an `Object`.

## 3. `sem/flow.rs`: reading a body

```rust
impl Sem {
    /// The table of `func`'s body. `None` for a def with no node in the program's tree (as `call_sites`). Read once and kept, like `body`.
    pub(crate) fn flow(&self, def: DefId, style: MarkerStyle<'_>) -> Option<Arc<Flow>>;
    /// A def as a number, and back. The number is good while this program is loaded.
    pub(crate) fn def_id(&self, func: Func<'_>) -> DefId;
    pub(crate) fn func(&self, def: DefId) -> Func<'_>;
}
```

It reuses what exists: `Collector` (`sem/collect.rs`) already finds the sites, the names in a hand-over position and the object
literals; `sem/handed.rs` already asks the checker what a name is (`functions_named`) and walks binding patterns. `flow` converts each
callee and argument **node** to an `Expr`:

```
to_expr(node)
    look through parentheses, `as`, `!`, `satisfies`, a type assertion
    an identifier or a property access the checker resolves to a def with a body, or to a class   → Def(func)
    an arrow or a function expression that is not a def of the outline                            → Anon(its number)
    `c ? a : b`, `a || b`, `a ?? b`, `a && b`                                                     → OneOf([…])
    a literal, arithmetic, a comparison, `typeof`, a JSX element used as a value                  → Data
    anything else                                                                                 → Unknown
```

Everything is done inside one entered region with one held checker, and what leaves is owned (the tsrs memory rule of `CLAUDE.md`).

> **🦀 Rust note: `Arc<T>` for a table that several readers hold.** `flow` is asked for by the walk of the def, and again by every
> caller that builds a key for it (to know how many parameters it has). Returning a `Flow` by value would copy the table each time;
> returning `&Flow` would tie the borrow of the memo map to the caller for as long as it holds the table, and the next `flow` call
> (which needs to insert into the map) would not compile. `Arc<Flow>` is a shared pointer with a counter: `Arc::clone` copies the
> pointer and adds one, the table is freed when the last holder drops it. It must be `Arc` and not `Rc`: an `Rc` counts without a lock, so
> Rust forbids sending it to another thread, and **a struct that holds an `Rc` cannot be sent either**. The table is kept inside `Sem`, a
> `Session` holds the `Sem`, and a `Session` must be able to move to a worker thread (the doctests of `proof` pin that `Sem` is `Send`).
> `Arc` pays an atomic add for it, which is nothing here. `Sem::bodies` already holds `Arc<Body>` for the same reason. And the table has no
> lifetime: it holds numbers, not `Func<'p>` ([03](03-design.md) §4).

## 4. The switch

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Precision {
    /// The engine's resolver alone: what every release so far gives.
    #[default]
    Default,
    /// The context layer on.
    Precise,
}
// Options gains: pub precision: Precision
```

`typescript-cli calls` gains `--precise`. The default stays `Default` until Q-TC4 is answered. With `Default`, `sem::flow` is never
called: the layer costs nothing when it is off. `Precise` stands for a private set of capability bits ([03](03-design.md) §15.2). TC1 defines the set; no bit does anything yet.

## 5. The empty layer

| File | TC1 |
|---|---|
| `key.rs` | `KeyId` (a number), `KeyData` with the three fields of [03](03-design.md) §6, all empty, `Keys` (the interner, on `indexmap`), `KeyId::OPEN`, and the one function that puts a key in its normal form |
| `walk.rs`, `calls.rs` | the five `()` become `KeyId::OPEN` ([01](01-where-we-start.md) §2) |
| `calls.rs` | `Lang::digest`: zeros for the open key; for any other, the function S-TC3 settled. No other key can exist yet, and a unit test builds one by hand to pin the bytes |
| `value.rs` | `Value`, `One` (no variant used yet), `Value::unknown()`, `Value::data()`, `Value::Wide`, `join`. No import from `sem` |
| `env.rs` | `Env { flow: Arc<Flow>, key: KeyId }`, built in `Lang::walk` when the switch is on |
| `eval.rs` | `eval(&Expr, &Env) -> Value`: unknown for every `Expr` |
| `walk.rs` | `resolve_from_env` calls `eval` on the site's callee and returns `None` when the value has no known member (always, in TC1) |

## 6. The harness

`tests/context.rs` and `tests/fixtures/context/`, built now so that TC2 starts by flipping a snapshot, not by writing a test runner.

- **A context fixture** is a small project. Its snapshot (`tests/snapshots/context/<name>.txt`) holds, for each entry it names: the
  default tree, the precise tree, and the edges of both in the form the policy snapshots use (`from | callee text | what it is`),
  with a context's digest printed as its first 8 hex digits when it is not zeros.
- **The precise policy table**: `languages/typescript/docs/policy.md` gains a second table, "With the context layer", whose rows name a
  context fixture. `tests/policy.rs` reads it as it reads the first: a row without a fixture, or a fixture without a row, fails.
  It is empty at the end of TC1 but for the tour.
- **The three properties**, as functions any test can call on a pair of graphs: `off_is_identical`, `never_fewer_edges`,
  `a_context_is_a_function_of_its_key` (each context of the graph is walked again alone and gives the same sites; a context reached
  from two different entries has equal edges).
- **The tour** ([03](03-design.md) §2) as the first fixture, with every line an entry of its own and `main` as one more.

## 7. Tests

| Test | |
|---|---|
| every existing test, switch off | unchanged, no snapshot touched |
| every existing graph test **again with the switch on** | equal graphs (TC1's proof) |
| `Sem::call_sites` equals the spans of `flow.sites`, for every def of every fixture and of the V-NOC frontend | the table has every site, in order |
| the defs `Sem::handed_over` finds equal the `Def`s of `flow.handed` | the table sees what D22 sees |
| a text snapshot of the tour's tables (`tests/snapshots/context/tour.flow.txt`) | reviewed by reading it against the source |
| `eval` on hand-written tables | unknown, for now; the test file exists |
| the laws, on generated values (`proptest`, a dev-dependency): `join` in any order and twice gives the same; `Wide` absorbs; data changes nothing; equal keys have equal digests and unequal keys unequal ones; a key survives its canonical form and back | the value and the key are sound before any phase leans on them |
| a second program loaded in the same test: no number of the first is read through the second | the rule that replaces the lifetime ([03](03-design.md) §4) |
| the sweep with the switch on, V-NOC and the compiler | no panic, `engine_kind_unhandled` 0, equal counters; time and memory beside the switch-off run **[measure]** |

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 0 | **Built** (477c4bd, 5362028; vscode's sweep still to run). **H17 first** ([`../10-review-fixes.md`](../10-review-fixes.md) §6.3, revised 2026-11-23): every engine accessor and downcast that can panic on a kind is banned outside `sem/view/`. `sem/flow.rs` is then written against the views from its first line, and adds a view by kind for each kind it reads | clippy `-D warnings` with the ban; the four sweeps: `engine_kind_unhandled` 0, one hash per graph equal before and after |
| 1 | `Precision`, the capability set, `Options::precision`, `--precise`; `KeyId`, `Keys`, the five literals | all tests green; `cargo tree`: `indexmap`, `smallvec`, `bitflags`, `rustc-hash` become direct dependencies, and no crate is new to the lockfile but `proptest` (tests only) |
| 2 | `flow.rs`; `sem/flow.rs` for sites and arguments | the `call_sites` equality |
| 3 | `handed` in the table | the `handed_over` equality |
| 4 | `value.rs`, `env.rs`, `eval.rs` (behind the `World` trait), `resolve_from_env` wired | equal graphs with the switch on; the laws |
| 5 | the harness, the tour, its default snapshot | the tour's "today" column of [03](03-design.md) §2 confirmed or corrected, cell by cell |
| 6 | the sweep with the switch on | the numbers in `ENGINE.md` |
| 7 | (mine) `ENGINE.md`: the engine items `sem/flow.rs` calls; `policy.md`: the second table's header | reviewed |

Next: [`06-tc2-functions-through-parameters.md`](06-tc2-functions-through-parameters.md)
