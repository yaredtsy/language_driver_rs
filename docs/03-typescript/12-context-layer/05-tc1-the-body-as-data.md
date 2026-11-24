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
| `calls.rs` | `Lang::digest`: zeros for the open key, read from `Keys`. For any other, core's function, at TC2 step 1 (D57), with the test that pins its bytes (§8, step 1) |
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
| 1 | **Built** (`c2452bb`, reviewed: `languages/typescript/engine/08-tc1.md`). `Precision`, `Options::precision`, `--precise`; `KeyId`, `KeyData` (no field yet), `Keys`, the five literals. **As decided for the build** below | all tests green, no snapshot touched; `--precise` prints byte for byte what no flag prints; `Cargo.lock` gains only the edge `typescript-analysis → indexmap` |
| 2 | **Built** (`bac93f9`; gate passed on three projects, vscode owed: `languages/typescript/engine/08-tc1.md`). `flow.rs`; `sem/flow.rs` for sites and arguments | the `call_sites` equality; **S-TC2's cost, the gate** (moved from TC0): the tables of every def of the four projects built in at most the time of a depth-1 sweep of the same project and at most 25% more peak memory, measured in the release build. Over that, stop and report before step 3 |
| 3 | **Built** (`6b7c2ef`, reviewed: `languages/typescript/engine/08-tc1.md`). `handed` in the table | the `handed_over` equality |
| 4 | `value.rs`, `env.rs`, `eval.rs` (behind the `World` trait), `resolve_from_env` wired; **the capability set** (`Caps`, [03](03-design.md) §15.2, moved here from step 1: the first step that reads it, to decide whether an `Env` is built); `KeyData`'s three fields | equal graphs with the switch on; the laws |
| 5 | the harness, the tour, its default snapshot | the tour's "today" column of [03](03-design.md) §2 confirmed or corrected, cell by cell |
| 6 | the sweep with the switch on | the numbers in `ENGINE.md` |
| 7 | (mine) `ENGINE.md`: the engine items `sem/flow.rs` calls; `policy.md`: the second table's header | reviewed |

**Step 1, as decided for the build (2026-11-24).** A type or a crate comes in the step that first uses it, so no code waits behind an
`allow(dead_code)` and no dependency sits unused:

- `Precision` (§4) lives in `calls.rs` beside `Options`, exported by name from `lib.rs`. `Options::new` sets `Precision::Default`.
  **Nothing reads it in step 1**: step 4 is where `Precise` starts building an `Env`. So `--precise` changes nothing yet, and a CLI test
  says so.
- `key.rs`: `KeyId(u32)` with `KeyId::OPEN = KeyId(0)`; `KeyData`, **with no field** until step 4 (so every `KeyData` is the open one);
  `KeyData::normal` (the one normal form, the identity while there is no field); `Keys { table: IndexSet<KeyData>, digests: Vec<[u8; 16]> }`
  with `Keys::new()` (interns the open key, number 0, digest zeros), `intern` and `digest`. `type Key = KeyId`.
- `TypeScript` holds `keys: RefCell<Keys>`, and `Lang::digest` reads it. The digest of a key that is not the open one is core's function,
  **at TC2 step 1** (D57), with the test that pins its bytes: before `KeyData` has a field no such key can be built, so that row of §5
  moves there.
- `indexmap = "2"` (default features, as tsrs takes it: nothing is rebuilt). `smallvec` (it must take the feature `union`, as tsrs does,
  or tsrs is rebuilt), `rustc-hash` and `bitflags` come in with their first use; `proptest` at step 4.

**Step 2, as decided for the build (2026-11-24).**

- **The handles.** `flow.rs` holds `DefId { program, file, def }` (three `u32`: the program's number, the file's number, the outline index + 1,
  or 0 for the file's own code). `sem/flow.rs` holds `Flows`, one field of `Sem`: the program's number (from a global `AtomicU32`, taken
  when the `Sem` is made), the files as an `IndexSet<P<Node>>` (a file's number is its place, given on first use) and the memo
  `HashMap<DefId, Arc<Flow>>`. `Sem::def_id(func)`, `Sem::func(def) -> Option<Func>` (`None` for a number of another program, or a file
  number never given) and `Sem::flow(def, style) -> Option<Arc<Flow>>`. The program's number is what makes "no number of one program is
  read through another" a test and not a hope.
- **What step 2 fills.** `Flow { params, sites, anons }`; `handed` is step 3, the rest later phases. `Param { rest }` for each parameter of
  the def's own declaration, without a `this` parameter (`ast::is_this_parameter`); empty for the file's code, a class and any def that is not
  function-like. `SiteFlow { span, kind, callee, args, anon }`, index for index with `Body::sites`: `kind` is call, new (a `Call` site
  whose node is a `NewExpression`), JSX, tagged, decorator or accessor; `args` is `Some` for a call or a `new` (its arguments; a
  `SpreadElement` is `Arg { spread: true }` of its expression) and for JSX (one `Unknown`: the attributes, TC4), and `None` for the other
  three: **not read**, which a later phase must take as unknown, never as "no argument". `anon` is the innermost anonymous function whose
  span holds the site. `AnonFlow { span }`: each `ArrowFunction` or `FunctionExpression` the `Collector` walks into that is not a def of the
  outline, in the order it meets them (source order); `Body` gains the list.
- **`to_expr`**, after `unwrap_expression`: a node that is a def of this file's outline is `Def` (a class: `Unknown`, TC5); an
  anonymous function is `Anon(i)` (its span in the list); an `Identifier` or a `PropertyAccessExpression` is what `functions_named`
  answers: one function `Def`, several `OneOf` of `Def`s in its order, any class or nothing `Unknown`; `c ? a : b` and `a || b`, `a ?? b`,
  `a && b` are `OneOf` (a nested `OneOf` is flattened); `Data` is a string, number, bigint, regular-expression or template literal, `true`,
  `false`, `null`, a prefix or postfix unary, `typeof`, `void`, `delete`, a binary arithmetic, bitwise, shift, comparison, equality,
  `instanceof` or `in` (and their compound assignments, `+=` …), and a JSX element or fragment; **everything else is `Unknown`** (an
  assignment, `||=`, a comma, an array, an object, a call, a `new`, `this`, `await`, an element access).
- **A hole H17 missed, closed here.** The engine's `Node::parameters()` is `function_like_data().unwrap()`: it panics on a node that is not
  function-like, and it is not in the ban (H14 listed the accessors that end in `panic!`). A scan of `ast.rs` at the pin for accessors
  with `unwrap`, `expect` or `panic!` found it alone among those `sem` can reach. Step 2 adds the views `FunctionLike` (`parameters`) and
  `Parameter` (`is_rest`), bans `Node::parameters` and `Node::parameter_list`, and moves `collect.rs`'s two calls to the view.
- **The gate** is an ignored in-crate test, `measure_flow` (as `measure_handed_over`): on a fresh project, the depth-1 graph of every
  project def in one request (time `T_sweep`, memory after `M_sweep`), then the table of every def, checking each against `call_sites`
  (time `T_after`, memory `M_flows`); on a second fresh project, the tables alone (`T_cold`). It passes when `T_cold ≤ T_sweep` and
  `M_flows ≤ 1.25 × M_sweep`, release build, on the four projects (run by the reviewer: the coder cannot see them).

**Step 3, as decided for the build (2026-11-24).** `Flow` gains `handed: Box<[HandedFlow]>`, `HandedFlow { at: Option<Span>, value: Expr,
written: bool }`, sorted by `at` (start, then end), ties in the order made. It is made from two sources, so that the table can never
see a hand-over D22 does not:

- **every `Handed` of `Sem::handed_over(func)`**, as `HandedFlow { at, value: Def(def_id(func)), written }`. `handed_over` is the one
  place that knows how a name in a hand-over position stands for a def (a literal's members, a destructured member, an alias); the table
  does not resolve it a second way;
- **every name of `Body::refs` whose span no `Handed` has**, as `HandedFlow { at: Some(span), value: expr_of(node), written: false }`: a
  name `handed_over` found no def for. Today that is a parameter, a local, a class, a declared function: `Unknown`. TC2 makes a parameter
  `Param`, TC3 a local `Var`; that is why they are in the table at all.

`handed_over` is called before the checker is taken (it takes its own). Its `uncovered` and `signatures_passed` are the walk's to count,
not the table's. **The proof** (§7, "the table sees what D22 sees"): for every def of the fixtures and of the real projects, the
`(at, def)` pairs of `handed_over` equal the `Def`s of `flow.handed`, **and no entry of the second kind holds a `Def`** (inside a `OneOf`
either). A failure there means `expr_of` and `handed_over` disagree on what a name is; it is reported, not patched over.

Next: [`06-tc2-functions-through-parameters.md`](06-tc2-functions-through-parameters.md)
