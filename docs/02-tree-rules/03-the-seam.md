# 02.03 The seam: a default resolver now, context sensitivity later

```
the seam
├── 1. the goal, in one sentence
├── 2. seam one: the output      (a hard guarantee)
├── 3. seam two: inside a language crate   (same modules as pylspt, some empty)
├── 4. seam three: the solver    (the `Lang` trait and the flat solver)
├── 5. the test that already knows the future: two expected trees
├── 6. what the upgrade will touch, and what it will not
└── 7. two cheaper steps between "default" and "context-sensitive"
```

## 1. The goal

> TypeScript and Rust start with the engine's default resolver. When one of them becomes context-sensitive, **nothing outside that language's
> `-analysis` crate changes**, and inside it the work is filling in modules that already exist.

"A seam" is a place where two pieces of code meet through a small, stable shape, so that one side can be replaced. There are three here, from the
outside in. The first is a guarantee. The second is a habit. The third is a trait.

## 2. Seam one: the output

From [`01-graph-and-tree.md`](01-graph-and-tree.md) §1: a call graph is a context graph with one context per function. So:

| | Default resolver | Context-sensitive |
|---|---|---|
| return type | `Graph` | `Graph` |
| contexts per function | 1 | 1 or more |
| `ContextId::digest` | zeros | a hash of the key |
| renderer, `to_text`, CLI output, driver conversion, V-NOC's wire and UI | unchanged | unchanged |

Everything that consumes the answer is written against `Graph` and already handles "several contexts of one function at one site" (rule 7),
because Python needs it on day one. **This seam cannot be got wrong later, because Python tests it now.**

## 3. Seam two: inside a language crate

pylspt's `interp/` has a shape. A default-resolver language uses the **same module names with the same jobs**, and leaves the parts it does not
need yet as the smallest thing that type-checks.

| Module | Job (from pylspt) | Default resolver | Context-sensitive |
|---|---|---|---|
| `sem.rs` | the only code that talks to the engine | parse, find a def, list call sites, **resolve a callee** | the same, plus types of expressions and member lookup |
| `walk.rs` | visit one body, produce its sites | visit call expressions in source order; for each, `resolve(site, &env)` | the same loop, now also executing assignments into `env` |
| `env.rs` | names → values on this path | `pub(crate) struct Env;` (empty) | a persistent map |
| `key.rs` | what a context holds | `pub(crate) type Key = ();` | fingerprints of the arguments |
| `calls.rs` | the public entry; implements `Lang` | as written in §4 | unchanged |

And one function is written in its final form from the first day:

```rust
// walk.rs — the same in both columns of the table above
fn resolve(&self, site: &RawSite, env: &Env) -> Vec<Target> {
    self.resolve_from_env(site, env)                          // default resolver: always None
        .unwrap_or_else(|| self.sem.callee(site).into())      // THE default resolver
}

fn resolve_from_env(&self, _site: &RawSite, _env: &Env) -> Option<Vec<Target>> {
    None            // nothing is tracked yet. Context sensitivity starts by making this return Some.
}
```

This looks like ceremony for a function that returns `None`. It is the cheapest insurance in the plan: the call sites of `resolve` already pass
an `env`, the walk already has a place where one would be updated, and the day `resolve_from_env` learns one case (say, "a variable assigned
from `new Foo()` in this body"), that case works without touching anything else.

## 4. Seam three: the solver

TypeScript and Rust need the same small machine: start at an entry, walk a body, follow each edge to a context not seen before, stop at limits,
build the `Graph`. That machine lives once, in `analyzer-core::solve`, and a language plugs into it through one trait.

```rust
// core/analyzer-core/src/solve.rs
pub trait Lang {
    /// The engine's handle for a function or class. Only valid during one request.
    type Func: Copy + Eq + Hash;
    /// What a context holds besides the function. `()` for a default resolver.
    type Key: Clone + Eq + Hash;

    /// Name, file, range, kind, stable identity: a row of `graph::Graph::defs`.
    fn describe(&self, f: Self::Func) -> DefInfo;
    /// The 16 bytes of `ContextId::digest`. Zeros for `()`.
    fn digest(&self, key: &Self::Key) -> [u8; 16];
    /// Walk ONE body under ONE key. No recursion into callees: the solver does that.
    fn walk(&mut self, f: Self::Func, key: &Self::Key, cancel: &Cancel) -> Summary<Self>;
}

pub struct Summary<L: Lang + ?Sized> {
    pub sites: Vec<SiteCalls<L>>,      // in source order
    pub complete: bool,                // false: the walk was cut (cancelled, deadline)
}
pub struct SiteCalls<L: Lang + ?Sized> { pub site: Site, pub targets: Vec<Target<L>> }

pub enum Target<L: Lang + ?Sized> {
    Call(L::Func, L::Key),
    Construct { class: L::Func, init: Option<(L::Func, L::Key)> },
    External(External),
    Unresolved,
}

pub struct Solver<L: Lang> { /* lang, memo: HashMap<(L::Func, L::Key), Slot<L>>, limits, stats */ }

impl<L: Lang> Solver<L> {
    pub fn new(lang: L, limits: Limits) -> Self;
    /// One body, memoized. This is what "expand one node" costs.
    pub fn summary(&mut self, f: L::Func, key: &L::Key, cancel: &Cancel) -> &Summary<L>;
    /// Everything reachable from the entries, until a limit. Sorted, stable ids.
    pub fn graph(&mut self, entries: &[(L::Func, L::Key)], cancel: &Cancel) -> Graph;
}
```

> **🦀 Rust note: associated types, and why `()` is enough.** `type Func` and `type Key` are *associated types*: each implementation of `Lang`
> picks them once, and the solver is written against the names. They are not generic parameters (`Lang<F, K>`) because a language has exactly one
> choice of each; with associated types the solver is `Solver<L>` and never has to repeat `F, K` everywhere.
>
> The unit type `()` has exactly one value and already implements `Clone`, `Eq` and `Hash`. So a memo table keyed by `(Func, ())` is a table keyed
> by function, with no special case in the solver:
> ```rust
> impl Lang for TypeScript {
>     type Func = sem::Func;
>     type Key = ();                                    // one context per function
>     fn digest(&self, _: &()) -> [u8; 16] { [0; 16] }
>     fn walk(&mut self, f: sem::Func, _: &(), cancel: &Cancel) -> Summary<Self> { walk::body(&self.sem, f, &Env, cancel) }
>     …
> }
> ```
> Changing `type Key = ();` to a real type later is one line here, and the solver compiles unchanged. `?Sized` on `Summary<L: Lang + ?Sized>` is
> only there so the types can be named inside the trait itself; you can ignore it when reading.

Why the trait has `walk` (one body) and not `call_graph` (everything):

- **Laziness.** V-NOC's tier 3 asks for "expand one node on demand" and "stop at a budget and remember where you stopped" (`09/02` §4, and its
  warning that adding this afterwards was a batch job for Python). With one-body walks, a budget is a counter in the solver, and expanding a cut
  node is `summary()` on its context. This is designed in [`03-typescript/07-ts5-driver-needs.md`](../03-typescript/07-ts5-driver-needs.md).
- **Cancel.** The solver checks `cancel` between bodies; `walk` checks it between statements. No language writes its own cancel loop.
- **Caching.** A summary is per `(function, key)`, so a cache across requests is "keep the memo, drop the entries of files that changed".

**Python does not implement `Lang`.** Its solver feeds return values and effects from callee summaries back into callers and resolves recursion
with a fixpoint; the flat solver has none of that. Python produces the same `Graph` through its own path. That is fine: the guarantee is seam
one. Seam three is a convenience shared by the languages that are simple today.

## 5. The test that already knows the future

Each folder in `scenarios/` is one small program, written once per language, with the tree it must give. Where a default resolver and a
context-sensitive solver give different trees, the folder has **both** answers:

```
scenarios/two-callers/
├── README.md                 what this scenario shows, and which rule it pins
├── python/main.py
├── typescript/main.ts
├── rust/src/main.rs
├── expected.default.tree     main → read_json → emit → Writer.write
│                                  → read_csv  → emit → Writer.write
└── expected.precise.tree     main → read_json → emit → JsonWriter.write
                                   → read_csv  → emit → CsvWriter.write
```

Each language says which level it is at, in one line of its test:

```rust
#[test] fn scenarios() { analyzer_testkit::scenarios::run(&TS, Level::Default); }     // typescript today
#[test] fn scenarios() { analyzer_testkit::scenarios::run(&PY, Level::Precise); }     // python
```

The day TypeScript becomes context-sensitive, its line changes to `Level::Precise`, and **the definition of "done" already exists**, written
by Python. A scenario where the two levels agree has a single `expected.tree`.

A starting list of scenarios, each pinning a rule from the last file:

| Scenario | Pins |
|---|---|
| `straight-line` | rules 3, 7: order of children, one node per site |
| `two-callers` | the difference between the two levels (rule 2) |
| `same-callee-twice` | two calls to one function from one body are two nodes (V-NOC merges; we do not) |
| `recursion-direct`, `recursion-mutual` | rule 8 |
| `constructor` | `Construct` and its `init` (rule 5) |
| `method-on-class` | a method call on a concrete type |
| `interface-dispatch` | the dynamic-dispatch row of the policy table |
| `callback-argument` | rule 3's anonymous-function row |
| `external-call` | rule 5: an external is a leaf and is not entered; rule 9 |
| `unresolved-call` | rule 6: reported, not guessed |
| `syntax-error-in-callee` | rule 6: a partial answer |
| `deep-chain` (generated, 2,000 levels) | rule 10: the stack limit cuts and marks |

The pylspt fixture tests (41 of them) stay Python-only. A scenario is added only when the same program makes sense in all three languages.

## 6. What the upgrade will touch

When TypeScript (or Rust) becomes context-sensitive:

| Piece | Changes? |
|---|---|
| `analyzer-core`: `graph`, `render`, `text`, `edit`, `outline` | no |
| `analyzer-testkit`, `scenarios/` | no (one line: the level) |
| `typescript-syntax` | no |
| `typescript-cli`, the V-NOC driver, V-NOC's wire, stored `.vnoc/` data | no. Call trees are recomputed; context ids change from zeros to hashes, so a UI selection keyed by context id is lost once |
| `typescript-analysis/src/sem.rs` | **grows**: type of an expression, member lookup on a type, "what does this class construct" |
| `…/env.rs`, `…/key.rs` | **filled in** (they exist, empty) |
| `…/walk.rs` | **grows**: executes assignments and attribute writes into `env`; `resolve_from_env` returns `Some` |
| `…/calls.rs` | `type Key` changes; `digest` hashes it |
| `analyzer-core::solve` | **this is the open part.** A context-sensitive solver needs callee summaries to carry return values, and recursion needs a fixpoint. The flat solver has neither. At that point the rule of two applies: Python has that machinery, TypeScript needs it, so the language-independent part of pylspt's `solver.rs` / `summary.rs` / `defer.rs` moves into `core`. See [`03-typescript/08`](../03-typescript/08-later-context-sensitive.md) |

So the honest summary: **the upgrade is additive everywhere except the solver, and the solver's upgrade is a move of existing Python code, not
an invention.** How much of pylspt's solver really is language-independent is unknown until someone tries; that is a direction, not a decision.

## 7. Two cheaper steps in between

Full context sensitivity is months of work (it was for Python). Two steps give much of the visible value earlier and fit the seams as they are.
Neither is planned in a phase; they are options for after TS5.

| Step | What the user sees | How it fits |
|---|---|---|
| **show implementations** | under an interface or trait method, the classes that implement it, as children marked "possible" | no new key. `sem` gains `implementations(func)`; the renderer gets an option. This is class-hierarchy analysis; both engines have "go to implementations" |
| **local values only** | `const w = new JsonWriter(); w.write()` resolves to `JsonWriter.write` inside one function | `env` is filled within one body and never crosses a call, so `Key` stays `()`. In typed TypeScript and Rust the type checker already does this; it helps mostly for plain JavaScript |

Next: [`04-the-defs-table.md`](04-the-defs-table.md)
