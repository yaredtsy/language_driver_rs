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
├── later: TC7–TC10 ─────────────── anonymous functions · fields · recursion · library models ─ 11-later.md
└── a review before TC3 ─────────── what is built through TC2: findings, nothing fixed ──── 12-review-before-tc3.md
```

Written 2026-11-20, after reading Python's `interp/` (the layer that exists), TypeScript's `typescript-analysis` as built through TS5,
core's `solve.rs`, and tsrs at the pinned commit `13a31a4`. It replaces the direction file
[`../08-later-context-sensitive.md`](../08-later-context-sensitive.md): that file's steps C1–C6 are mapped to the phases here in
[`01-where-we-start.md`](01-where-we-start.md) §6.

**Revised 2026-11-23**, after a review against Python's `interp/` (the layer that is built, tested and trusted) and against tsrs at the
pin, function by function. What changed, and in which file, is the last section of this page.

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
   a call returns (14% to 77%), then `this` (0% to 47%). No single phase answers most of every project. Those are upper bounds: TC2,
   a parameter's value called as it is, measured 0% (V-NOC), 1.8% (the private codebase) and 3.4% (the compiler) of the function-typed
   sites of the samples, against a forecast of at most 1%, 15% and 6%.
6. **Where the design had to choose, it takes what Python does.** A key is a number (interned), its digest is made once. A value that
   holds nothing callable ("data") is not the same as a value the layer knows nothing about. A return value around a cycle starts
   empty and is worked out again until it stops changing: answering "unknown" there would make a tree depend on where the request
   started ([10](10-tc6-return-values.md) §3).
7. **It is built to grow** ([03](03-design.md) §15): numbers and not borrowed handles inside the layer; one private switch per phase,
   so each phase is proved against the one before; a version for context ids; the files that know nothing of tsrs written so that a
   move to core is a move; and a crate before our own code wherever one fits (five of them are already in the lockfile).
8. **The engine gives more than the first draft said.** The signature a call resolves to, its parameters without `this`, the type of
   each slot and "is this type assignable to that one" are all public at the pin ([01](01-where-we-start.md) §4). So arguments are
   bound through the checker's signature, not through the syntax, and nothing has to be asked of upstream to start.

## The phases at a glance

| Phase | Adds | The key holds | What the user sees | Core change | Size |
|---|---|---|---|---|---|
| **TC0** | counts on the four projects, four spikes | — | numbers in `ENGINE.md` that confirm or reorder the list below | none | S |
| **TCL** | a function a library made is named, not "unresolved" (`setOpen` from `useState`). **Not part of the layer**: no key, no value, no table. Any time after TC0 | — | on `TaskDetailPanel`, 56 of the 86 unresolved calls become `External` (measured; the plan said 43) | none | S, **done** (`aa32042`, `fbee1b4`) |
| **TC1** | `sem::flow`: a body as plain data, held by number; the interned key; the capability set; `Options::precision`; the test harness of the layer | nothing (the open key only) | nothing: every graph identical | none | M |
| **TC2** | a function passed to a parameter, bound through the resolved signature | `Fn` per parameter | `run(handler)` shows `handler` under `run`; two callers of `run` are two contexts | the scenario level per scenario; the digest (no cut per function: N6 did not ask for one) | M, **done** (step 9 measured, step 10 closed it) |
| **TC3** | locals, variables a nested def captures, module constants | + captured variables | a nested handler that calls its component's callback resolves | none | M, **done** (2026-11-26: steps 1–8b; `engine/10-tc3.md`; step 2b not built, D67) |
| **TC4** | object literals, JSX attributes, destructuring, spread | + objects, by member | `<Panel onClose={f} />` and `open({ onDone: f })` resolve inside the callee; a component passed as a prop | none | L |
| **TC5** | instances: `new C()`, the static class of an argument, `this` | + instances | `emit(new JsonWriter())` shows `JsonWriter.write`; plain JavaScript method calls | none | M |
| **TC6** | what a call returns, **cycles included** | — | factories, custom hooks, `make()()`, a higher-order component, a recursive factory | **the solver hands a return value to the caller, and works a cycle out by rounds** (one note, shared with Rust's RC4; Python's rule) | L |
| TC7 | an anonymous function shown under the function that calls it (D54) | + what a closure captured | `each(() => work())` shows `work` under `each` | a tree rule, if recursion by function hides calls ([03](03-design.md) §9) | M, **not scheduled** (D58); it needs the two answers of 03 §9 first |
| TC8 | fields set by a constructor | + fields of an instance | `this.writer.write()` resolves | none | L |
| TC9 | arrays and maps of functions; a value per place in a body | — | completeness | none | L |
| TC10 | a table of library functions that return their argument (`useCallback`, `memo`) | — | the React idioms no value can reach | none | S, **needs Q-TC2** |

TC0–TC6 have a file each. TC7–TC10 are one file of sketches: they are designed when reached, from the numbers the earlier phases give.

## What V-NOC gets, and when

```
after TCL   a call of a function a library made (`setOpen`) has a name: about half of what reads "unresolved" in a React component
after TC2   a callback passed by name resolves in the function that calls it (measured at TC2 step 9: no site of V-NOC's samples changes)
after TC4   React props resolve: the tree under a parent shows what its children do with the handlers it gave them
after TC5   a call through an interface shows the class that was passed (the scenario two-callers, precise)
after TC6   factories and custom hooks
```

A tag is the owner's. TC2 is the first phase that changes a graph, and it has **no tag** (D64): the first tag that carries the layer is
TC5's, and it turns the layer **on by default** (Q-TC4, D64).

## The rule for every phase

Each phase ends with these, and is not done without them:

1. **Off is identical.** Every existing snapshot (policy, calls, scenarios, wire) passes untouched with `Precision::Default`.
2. **Never fewer edges.** For every fixture, each edge of the default graph from a site the layer does not speak at is in the precise
   graph, and no site that had a call target has none. **And on the four projects**: the sites the default graph draws and the precise
   graph does not are counted in the sweep, and the count is 0 (a cut is the one way to lose a subtree, and no fixture reaches a cap), with
   one allowance: a site whose callee the layer knows is only `undefined` has no edge, counted `callee_undefined` ([06](06-tc2-functions-through-parameters.md) §3).
3. **No-reuse mode agrees.** Solving with the memo off gives the same edge set (a summary is a function of its key); for core's flat solver, which walks a context once per request, the check is each context walked alone in a fresh request, [06](06-tc2-functions-through-parameters.md) §9).
4. **Same twice.** `assert_deterministic` on the precise graphs; digests equal across two processes.
5. **Its fixtures flip on purpose.** Each row the phase adds to the precise policy table has a fixture and a reviewed snapshot.
6. **Numbers.** Contexts, walks, time and the open-site counts on the four projects, before and after, in `ENGINE.md`.
7. `lib.rs` is still short; no engine type left `sem`; `cargo fmt`, `clippy -D warnings` on the crates touched.
8. **The phase before, from this one.** With the phase's own capability bit off, every graph is byte for byte the previous phase's
   ([03](03-design.md) §15.2).
9. **The same from any entry.** A context reached from two entries has equal edges; a tree cut by a budget and continued from the cut
   node equals the tree asked for whole. (The second half is postponed with D63: a cut node is continued from its def, so the continued
   subtree can be less precise; it holds again when a request can resume from a context.)
10. **The cost, measured** (D62, from TC3). On the projects, release build: the same sweep with the layer off and on, its time and peak
   memory and the two ratios, written in `ENGINE.md`. A line to watch (2× the time, +25% the peak), not a pass/fail.

## What needs your yes

| | Question | My recommendation | Blocks |
|---|---|---|---|
| **Q-TC1** | An anonymous function that a project function calls (`each(() => work())`): do its calls stay under the function it is **written** in (today's rule), or move under the function that **calls** it? The same question as Rust's Q-RC1 | **answered by D54 (2026-11-21): move.** It was decided while Bend was built, and Python's layer leaves a lambda unknown, so it was put to you again. **Answered 2026-11-23 (D58): the easy part for now.** An arrow's calls stay with the def that writes it, TC7 is not scheduled, and D54 is looked at again before it ever is. Either way nothing moves before TC7, and TC7 needs two more answers ([03](03-design.md) §9): when an arrow moves, and what the tree does when recursion-by-function hides its calls | TC7 only |
| **Q-TC2** (answered: (a) D65, (b) D56) | (a) May the analyzer hold a small table of library functions that return their argument (`useCallback(f)` is `f`, `memo(C)` is `C`)? It is a statement about a library, not about the user's code. (b) Is a call of a function that a library made (`setOpen` from `useState`) still "unresolved", or an `External` named after the call that made it? | (a) yes, as data, each row with a fixture, later, with the counts of TC6 ([11](11-later.md) §4). (b) `External`, **and now**: it needs nothing of the layer, and it is half of what a React user sees as "unresolved" (phase TCL). **(b) answered 2026-11-23 (D56):** shown as called, since it has no body; kept inside the analyzer as a kind of its own, so the display is one function to change later and nothing is done twice; no more time spent on it | (a) TC10, still open; (b) decided |
| **Q-TC3** (answered: D63) | A node the budget stopped at is continued today by asking for its **def** as a new entry. With contexts that loses the key: the continued subtree would be less precise than the same subtree drawn in one request. The request must be able to name a **context** | `Session` keeps the keys of the contexts a request listed and did not finish, and a request may start from a context id; an unknown id answers as today ([03](03-design.md) §10). V-NOC has no TypeScript driver yet, so the driver can be written to this from its first version | the first tag after TC2 (a driver contract change) |
| **Q-TC4** (answered: D64) | When is the layer on by default? | after TC5, when the shared scenarios pass at the precise level | no phase |
| **Q-TC5** | "Show the classes that implement this interface" (the old C2) | not built, as D36 decided for Rust: tsrs has no such query below its LSP layer, and an edge keeps one meaning, "is called" | nothing |
| **Q-TC6** | May `core/` take two small crates, `xxhash-rust` and `indexmap`? Python and Bend each hold a copy of the digest function today, and TypeScript would be the third: by the rule of two it belongs in core, and core has only `serde` and `serde_json`, with a rule to stay small | yes for `xxhash-rust` (the digest, when TC2 needs it); `indexmap` only when a second language interns its keys through core. **Answered 2026-11-23 (D57): yes for `xxhash-rust`.** The digest function goes into core in TC2's first step; Python and Bend switch to it in a session of their own | decided |
| **Q-TC7** (answered: D66) | May TC4 assume that a library function leaves an object's members alone (a stated exception to "never guess")? Asked by TC0's N5 | no: keep the sound rule; the share it costs is 2–9% of object parameters, and local literals are written through more often than they are handed to a library. Revisit with TC4's sweep | TC4 |

## What the review of 2026-11-23 changed

Checked against Python's `interp/` and against tsrs at the pin `13a31a4`. Bend's layer was built fast and is **not** used as evidence here.

| # | Change | Why | Where |
|---|---|---|---|
| 1 | a return value around a cycle starts empty and is worked out by rounds; it was "unknown, counted, a fixpoint later if measured" | with "unknown", the same context has different edges depending on the entry, and a tree continued from a cut node disagrees with the whole tree. Python starts every pending value empty and replays the group (`defer.rs` `no_values`, `solver.rs` `resolve_group`, 32 rounds, then marked) | [03](03-design.md) §8 rule 8, [10](10-tc6-return-values.md) §3, [11](11-later.md) §3 |
| 2 | three named values: unknown, **data**, **wide** | `ok && handler` must not stay open for ever; a cap that answers "unknown" makes a join depend on its order. Python keeps `Ty` apart from `Unknown` | [03](03-design.md) §3, §11, §12 |
| 3 | a key is interned (a number), put in one normal form, its digest made once | two keys equal by digest and not in memory would be merged silently by `Graph::new`; the solver asks for a digest at every edge. Python interns (`Interner<Context, ContextId>`) | [03](03-design.md) §6 |
| 4 | no anonymous function is told from another in a key, before TC7 | it changes no edge and multiplies contexts. Python has no lambda in a key | [03](03-design.md) §6, [06](06-tc2-functions-through-parameters.md) §3 |
| 5 | the table of a body holds numbers, is kept in `Sem` as `Arc<Flow>` | `Flow<'p>` cannot be stored in the `Sem` it borrows from; an `Rc` would stop a `Session` from moving to a worker thread | [03](03-design.md) §4, [05](05-tc1-the-body-as-data.md) §3 |
| 6 | arguments are bound through the resolved signature | public at the pin; `Signature::parameters()` has no `this`, so the off-by-one of `function f(this: T, cb)` cannot happen, and the overload is the checker's | [01](01-where-we-start.md) §4, [03](03-design.md) §7, [06](06-tc2-functions-through-parameters.md) §2 |
| 7 | a variable is complete only if every write the **engine** sees is a modelled assignment | our list of write forms can miss one; then an edge would be dropped with no counter | [03](03-design.md) §5, [07](07-tc3-variables-and-captures.md) §2 |
| 8 | the trust rule of objects covers a second name, a module constant, and an array literal | each was a way for a written-through object to stay "complete" | [03](03-design.md) §11, [08](08-tc4-objects-and-props.md) §5 |
| 9 | an instance says whether it is exact | a followed `new` and a static class are different facts; adding the flag later moves every digest. Python: `is_built_here` | [03](03-design.md) §3, [09](09-tc5-instances.md) §3 |
| 10 | "never fewer edges" is also counted on the four projects | a cut loses a subtree the default tree shows, and no fixture reaches a cap | this page, [03](03-design.md) §12 |
| 11 | a capability bit per phase; a version for context ids; neutral files; crates | so that each phase is an addition | [03](03-design.md) §15 |
| 12 | Q-TC1 is answered (D54); TCL is pulled out of TC10; Q-TC6 is new. Answered by you the same day: D56 (TCL), D57 (`xxhash-rust` in core), D58 (arrows stay, TC7 not scheduled) | | this page, `docs/07` |
| 13 | H17 (no call outside `sem/view/` that can panic on a node's kind; revised 2026-11-23: the engine's downcasts banned, the type ban dropped) goes before TC1 | `sem/flow.rs` reads more kinds of node than all of `sem` does today, and H14 was a panic of exactly that kind | [05](05-tc1-the-body-as-data.md) §8 |

Next: [`01-where-we-start.md`](01-where-we-start.md)
