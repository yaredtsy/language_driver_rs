# 03.12 TypeScript: the context layer

```
the context layer
├── where we start ──────────────── what the code, Python and tsrs give ──────────── 01-where-we-start.md
├── the patterns ────────────────── what unlocks a call tree, in build order ────── 02-patterns.md
├── the design ──────────────────── values · the body as data · the key · the rules ─ 03-design.md
├── TC0  counts and spikes ──────── eight counts, four spikes, before any code ───── 04-tc0-counts-and-spikes.md
├── TC1  the body as data ───────── `sem::flow`; no graph changes ───────────────── 05-tc1-the-body-as-data.md
├── TC2  functions through parameters  the first real key ───────────────────────── 06-tc2-functions-through-parameters.md
├── TC3  variables and captures ─── locals, nested handlers, module constants ────── 07-tc3-variables-and-captures.md
├── TC4  objects and props ──────── options bags, JSX attributes, destructuring ──── 08-tc4-objects-and-props.md
├── TC5  instances ──────────────── which class is behind an interface ──────────── 09-tc5-instances.md
├── TC6  return values ──────────── factories, custom hooks, curry ──────────────── 10-tc6-return-values.md
└── later: TC7–TC10 ─────────────── anonymous functions · fields · recursion · library models ─ 11-later.md
```

Written 2026-11-20, after reading Python's `interp/` (the layer that exists), TypeScript's `typescript-analysis` as built through TS5,
core's `solve.rs`, and tsrs at the pinned commit `13a31a4`. It replaces the direction file
[`../08-later-context-sensitive.md`](../08-later-context-sensitive.md): that file's steps C1–C6 are mapped to the phases here in
[`01-where-we-start.md`](01-where-we-start.md) §6.

**Nothing here is built.** Every `[check]` and `[measure]` is a claim to verify; TC0 exists to verify the ones the design rests on.

## The short version

1. **We can start.** The seam of TS4 held: `walk::resolve` already asks `resolve_from_env` first, `Env` and `Key` exist by name, and
   the solver in core is already keyed by `(function, key)`. Five small things are in the way; none is a rewrite
   ([01](01-where-we-start.md) §5).
2. **TypeScript's layer follows values, not types.** The checker erases type arguments, and tsrs does not let us read the ones it
   inferred. So the key holds *what was passed*: which function, which object, which class. That is Python's idea, not Rust's RC1.
3. **It is far smaller than Python's.** Python executes a body statement by statement, with objects that have an identity and writes
   that have effects. TypeScript's layer **does not execute**. `sem` turns a body into a small table of plain data (which expression
   each variable gets, what each call site passes), and the layer answers questions about that table. A variable has **one value: the
   join of everything assigned to it**. That is the right model for a language where a closure captures the variable, not its value,
   and it is what makes a nested handler see its component's props without any machinery.
4. **Every phase is released on its own.** The layer is off by default (`Options::precision`). With it off, every graph is byte for
   byte what it is today. With it on, a phase can only replace an "unresolved" answer by a better one; it never removes an edge the
   engine gave.
5. **The order follows what was measured**: parameters first (17% to 48% of the unresolved function-typed calls, by project), then what
   a call returns (14% to 77%), then `this` (0% to 47%). No single phase answers most of every project.

## The phases at a glance

| Phase | Adds | The key holds | What the user sees | Core change | Size |
|---|---|---|---|---|---|
| **TC0** | counts on the four projects, four spikes | — | numbers in `ENGINE.md` that confirm or reorder the list below | none | S |
| **TC1** | `sem::flow`: a body as plain data; `Options::precision`; the test harness of the layer | nothing (the open key only) | nothing: every graph identical | none | M |
| **TC2** | a function passed to a parameter | `Fn` per parameter | `run(handler)` shows `handler` under `run`; two callers of `run` are two contexts | the scenario level per scenario; a cut per function | M |
| **TC3** | locals, variables a nested def captures, module constants | + captured variables | a nested handler that calls its component's callback resolves | none | M |
| **TC4** | object literals, JSX attributes, destructuring, spread | + objects, by member | `<Panel onClose={f} />` and `open({ onDone: f })` resolve inside the callee; a component passed as a prop | none | L |
| **TC5** | instances: `new C()`, the static class of an argument, `this` | + instances | `emit(new JsonWriter())` shows `JsonWriter.write`; plain JavaScript method calls | none | M |
| **TC6** | what a call returns | — | factories, custom hooks, `make()()`, a higher-order component | **the solver hands a return value to the caller** (one note, shared with Rust's RC4) | L |
| TC7 | an anonymous function shown under the function that calls it | + what a closure captured | `each(() => work())` shows `work` under `each` | none | M, **needs Q-TC1** |
| TC8 | fields set by a constructor | + fields of an instance | `this.writer.write()` resolves | none | L |
| TC9 | values around a recursive cycle; arrays and maps of functions | — | completeness | the fixpoint, if measured | L |
| TC10 | a table of library functions that return their argument (`useCallback`, `memo`) | — | the React idioms no value can reach | none | S, **needs Q-TC2** |

TC0–TC6 have a file each. TC7–TC10 are one file of sketches: they are designed when reached, from the numbers the earlier phases give.

## What V-NOC gets, and when

```
after TC2   a callback passed by name resolves in the function that calls it
after TC4   React props resolve: the tree under a parent shows what its children do with the handlers it gave them
after TC5   a call through an interface shows the class that was passed (the scenario two-callers, precise)
after TC6   factories and custom hooks
```

A tag is the owner's. The first phase that changes a graph is TC2, so the first tag that can carry the layer is the one after TC2; I
would tag after TC4, when React code visibly improves. Which tag turns the layer **on by default** is a decision of its own (Q-TC4).

## The rule for every phase

Each phase ends with these, and is not done without them:

1. **Off is identical.** Every existing snapshot (policy, calls, scenarios, wire) passes untouched with `Precision::Default`.
2. **Never fewer edges.** For every fixture, each edge of the default graph from a site the layer does not speak at is in the precise
   graph, and no site that had a target has none.
3. **No-reuse mode agrees.** Solving with the memo off gives the same edge set (a summary is a function of its key).
4. **Same twice.** `assert_deterministic` on the precise graphs; digests equal across two processes.
5. **Its fixtures flip on purpose.** Each row the phase adds to the precise policy table has a fixture and a reviewed snapshot.
6. **Numbers.** Contexts, walks, time and the open-site counts on the four projects, before and after, in `ENGINE.md`.
7. `lib.rs` is still short; no engine type left `sem`; `cargo fmt`, `clippy -D warnings` on the crates touched.

## What needs your yes

| | Question | My recommendation | Blocks |
|---|---|---|---|
| **Q-TC1** | An anonymous function that a project function calls (`each(() => work())`): do its calls stay under the function it is **written** in (today's rule), or move under the function that **calls** it? The same question as Rust's Q-RC1 | stay, through TC6; move in TC7, designed once for both languages ([03](03-design.md) §9) | TC7 only |
| **Q-TC2** | (a) May the analyzer hold a small table of library functions that return their argument (`useCallback(f)` is `f`, `memo(C)` is `C`)? It is a statement about a library, not about the user's code. (b) Is a call of a function that a library made (`setOpen` from `useState`) still "unresolved", or an `External` named after the call that made it? | (a) yes, as data, each row with a fixture; (b) `External`; both later, with the counts of TC6 ([11](11-later.md) §4) | TC10 only |
| **Q-TC3** | A node the budget stopped at is continued today by asking for its **def** as a new entry. With contexts that loses the key: the continued subtree would be less precise than the same subtree drawn in one request. The request must be able to name a **context** | `Session` keeps the keys of the contexts it listed, and a request may start from a context id; an unknown id answers as today ([03](03-design.md) §10) | the first tag after TC2 (a driver contract change) |
| **Q-TC4** | When is the layer on by default? | after TC5, when the shared scenarios pass at the precise level | no phase |
| **Q-TC5** | "Show the classes that implement this interface" (the old C2) | not built, as D36 decided for Rust: tsrs has no such query below its LSP layer, and an edge keeps one meaning, "is called" | nothing |

Next: [`01-where-we-start.md`](01-where-we-start.md)
