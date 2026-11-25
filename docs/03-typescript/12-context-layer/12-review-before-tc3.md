# 03.12.12 A review of what is built, before TC3 (2026-11-25)

```
the review
├── 1. wrong answers ─────────────── run, each with a program that shows it
│   ├── 1.1 `void 0` is read as "something was passed"
│   ├── 1.2 the same request gives two graphs, by what was asked before it
│   └── 1.3 a parameter typed as data that holds a function
├── 2. design points ─────────────── read from the code; each costs more after TC3
│   ├── 2.1 counters are side effects of a walk
│   ├── 2.2 every edge has one stamp, and TC3 needs a second
│   ├── 2.3 no cap on the contexts of one function
│   └── 2.4 parts of the table nothing reads
├── 3. the tests ─────────────────── what is good, and five habits to change
├── 4. small things
├── 5. the decisions, made and assumed
└── 6. an order to take them in
```

**What this is.** The owner asked for a review of the built layer (H17, TC0, TCL, TC1, TC2; branch `ts-context-layer` at `81485ce`)
before TC3 starts: the decisions made or assumed, how the tests are written, quick patches that will hurt at scale, duplicates, and what
the reviewer would have done differently. **Nothing here is fixed.** Each finding says what happens, shows an example, says why, and says
what I would do; whether and when is the owner's and the manager's.

**How each finding was checked.** "Run" means I ran the analyzer on a small program written for the finding and the output is quoted.
"Read" means it follows from the code and I did not run it. A finding that is only read can be wrong: check it before depending on it.

**What I read:** `value.rs`, `key.rs`, `flow.rs`, `eval.rs`, `env.rs`, `caps.rs`, `walk.rs`, `calls/keyed.rs`, the changes of `calls.rs`,
`sem/flow.rs`, `Sem::fits`, `tests/context.rs`, `tests/common/context.rs`, core's `solve.rs` loop and `graph::digest`.
**What I did not read:** `tests/sweep.rs`, `tests/precise.rs`, `sem/flow/tests.rs`, most of `sem/resolve.rs`, `sem/handed.rs`, the TCL code.

---

## 1. Wrong answers

### 1.1 `void 0` is read as "something was passed" (run)

**The program.**

```ts
function run(cb: () => void = fallback) {
    cb();
}
function fallback() {}
function handler() {}

function viaVoid(flag: boolean) {
    run(flag ? handler : void 0);
}
function viaUndefined(flag: boolean) {
    run(flag ? handler : undefined);
}
```

In JavaScript `void 0` **is** `undefined`, so both functions do the same thing: when `flag` is false nothing is passed, the default
applies, and `cb()` calls `fallback`.

**What the analyzer says** (`typescript-cli calls … --precise`):

| Entry | Calls of `cb` drawn | Unresolved left |
|---|---|---|
| `viaUndefined` | `fallback`, `handler` | 1 (`function_typed_value`) |
| `viaVoid` | `handler` **only** | **0** |

For `viaVoid` the call of `fallback` is missing, and the mark that said "something here is not known" is gone too. That is the one thing
the layer promised never to do (README, rule 2: never fewer edges; `CLAUDE.md` rule 8: never guess).

**Why.** `sem/flow.rs` `expr_of` has a list of kinds that are "data" (nothing callable). `Kind::VoidExpression` is in it (line 391).
"Data" means *a value was passed, and it is not a function*, so the default is not used (`eval::param`: "`null` was passed: a default is
for `undefined` only"). The conditional is then `{handler} ∪ data = {handler}`, closed, and the layer speaks.

The name `undefined` goes another way: it is an identifier with no function behind it, so it is `Unknown`, which keeps the value open.
That is why `viaUndefined` is right, and also why it still has one unresolved edge it does not need.

**Why it matters.** `void 0` is rare in hand-written TypeScript and common in compiled JavaScript: it is what `tsc` and Babel write for
`undefined`. A project that has built files inside its root meets it.

**What I would do.** Give "nothing was passed" its own expression, `Expr::Undefined`, for `void <anything>` and for the name `undefined`
when the checker says it is the global one. `eval` maps it to `{Undefined}`. Then both entries above draw `fallback` and `handler` with
no unresolved edge. The small fix, if that is too much now: move `VoidExpression` out of the data list (it becomes `Unknown`).

**The test that was missing.** The data list has 17 kinds and the table of rows has a fixture for none of them by kind. A wrong entry in
that list cannot be seen by any test today. One row whose fixture passes each kind to a parameter with a default would have caught this.

### 1.2 The same request gives two graphs, by what was asked before it (run)

**The program** (three files):

```ts
// a.ts
export function fa(g: () => void) { g(); }
// b.ts
export function fb(g: () => void) { g(); }
// main.ts
import { fa } from "./a";
import { fb } from "./b";
function leaf() {}
function run(cb: (g: () => void) => void) {
    cb(leaf);
}
export function pick(flag: boolean) {
    run(flag ? fb : fa);
}
export function touchA() {
    fa(leaf);
}
```

**What I ran.** The graph of `pick`, `Precision::Precise`, with a limit of 5 walks (`Limits::new(5, None)`), twice: once on a project
just opened, once on a project that was first asked for `touchA`.

| | `fa` under the key `{leaf}` | `fb` under the key `{leaf}` |
|---|---|---|
| `pick` on a fresh project | `Incomplete(MaxContexts)` | `Finished` |
| `pick` after `touchA` | `Finished` | `Incomplete(MaxContexts)` |

Same project, same entry, same options, two graphs. With limits 1–4 and 6–8 the two are equal: the difference shows only where the
limit lands between the two contexts. This breaks rule 9 ("same input, same output").

**Why.** Three facts, each fine alone:

1. A `Value` keeps its members sorted by `DefId`, and a `DefId` is (program, **file number**, def number).
2. A file gets its number the first time one of its defs is asked for (`Sem::def_id`, `sem/flow.rs`), and the numbers live as long as
   the program is loaded: across requests. A fresh project meets `b.ts` first (it is written first in `flag ? fb : fa`); after `touchA`,
   `a.ts` already has a number.
3. `walk::resolve_from_env` pushes one target per member **in the value's order**, and core's solver walks contexts in the order it
   discovers them. A limit by count (`Limits::max_contexts`, `Budget::max_contexts`, the CLI's `--max-contexts`) stops at whichever
   comes later.

So the order of files in a number that "never reaches an output" does reach one, through the order of work under a limit.

**Why no test saw it.** `every_context_row_gives_the_same_text_twice` opens a fresh project for each render and asks the entries in the
same order, so both runs give the files the same numbers. And without a count limit the graphs are equal (`Graph::new` sorts).

**What I would do.** Sort the functions of a value by their stable id (`describe(func).id`, the id the graph uses) before pushing the
targets in `resolve_from_env`: one sort of at most 8 items. The digest already does this for the same reason (`key::canon`: "sorted as
`Canon`, never by `DefId`"). And one test: the request above in the two histories, under each limit from 1 to 8, graphs equal.

🦀 *Why a sort and not "number the files in a fixed order":* numbering files by, say, their place in the program would also work, but
it makes every later use of `DefId` order safe by a fact far from where it is used. Sorting at the one place the order leaves the layer
keeps the reason next to the code.

### 1.3 A parameter typed as data that holds a function (run)

**The program.**

```ts
function handler() {}
function noop() {}
function lied(cb: string, other: () => void) {
    ((cb as any) || other)();
}
function liar() {
    lied(handler as any, noop);
}
```

At run time `cb` holds `handler` (a function is truthy), so the call is of `handler`.

| | Calls drawn in `lied` | Unresolved left |
|---|---|---|
| layer off | none | 1 (`any_or_untyped`) |
| layer on | `noop` | **0** |

The layer on draws the function that is **not** called and removes the mark.

**Why.** TC2 step 5f decided: a parameter whose declared type cannot hold a function is `data` in every context (`Param::capable`,
`eval::param`). `cb: string` is data, so the callee is `data ∪ {noop} = {noop}`, closed. The tripwire (D61) does not see it: it checks
each function that **is** drawn, and `noop` fits.

**How bad.** It needs a lie in two places: the caller casts a function to pass it as a `string`, and the callee casts it back to call
it. That is rare. But it is a place where the layer trusts a declared type, TypeScript's types can be bypassed with `as any`, and the
policy file does not say so.

**What I would do.** One of two, the owner's choice:

- **Say it.** A row in `policy.md` with this program as its fixture: "a parameter declared as data is taken as data; code that passes a
  function through `as any` loses that edge". An assumption stated and shown is fine; one that is silent is not.
- **Or narrow it.** Where the engine's own answer for the site is `any_or_untyped` (types were bypassed **at this call**), a parameter
  that is not capable is `unknown`, not `data`. The program above then keeps its unresolved edge beside `noop`.

---

## 2. Design points

### 2.1 Counters are side effects of a walk (read)

**How it is built.** `TypeScript` holds a map of counters behind a `RefCell`. While a body is walked, code calls `note_walk(key)` (a fact
of this context: the tripwire, `callee_undefined`, the split of `function_typed_value`) or `note_body(key)` (a fact of the body: counted
only when the flag `first_walk` says this is the def's first walk in the request). `eval` counts a widened set by calling
`World::widened()` at the join.

🦀 *A side effect* here means: the walk returns its `Summary`, and **also** changes a map somewhere else. Nothing in the returned value
says what was counted. If the same walk runs twice, the map is changed twice.

**Why it works today.** Core's solver walks each `(function, key)` exactly once per request and keeps the summary. One walk, one count.

**Where it stops working.**

1. **TC3's memo of variable values.** The handoff for TC3 plans "a memo of variable values per walk". Take:

   ```ts
   function pick(n: number) {
       const h = n === 1 ? a1 : n === 2 ? a2 : /* … */ a9;   // nine functions: past the cap of 8
       run(h);
       other(h);
       h();
   }
   ```

   `h` is read three times. Without a memo the join runs three times and `value_widened_set` is 3; with a memo it is 1. Neither number
   is wrong by a rule, because no rule says whether the counter counts *places in the code* or *evaluations*. Today it counts
   evaluations: `walk::keyed` calls `key_of` once **per target** of a site, so one argument at a site with two targets is evaluated, and
   counted, twice (`tests/context.rs` says it in a comment: "the argument is evaluated once, for the one target `run`").
2. **TC6's rounds.** [10](10-tc6-return-values.md) §3.1 walks the contexts of a cycle again each round until nothing changes. Every
   `note_walk` counter of such a context is then multiplied by its rounds: a `callee_undefined` of 1 becomes 3.
3. **The property that leans on a counter.** `never_fewer_edges` allows as many emptied sites as `callee_undefined` says (§3.4). Each
   extra count makes that test allow one more lost site.
4. **A choice at every call.** Each new counter must pick `note_body` or `note_walk`. A wrong pick compiles, passes on fixtures with one
   context per def, and miscounts on real projects.

**What I would do.** Keep the notes of a walk **with the walk**, not in one shared map:

```rust
/// What one walk of one context said. A second walk of the same context replaces it.
#[derive(Default)]
struct Notes {
    /// facts of the body: the same in every context of the def
    body: BTreeMap<&'static str, u64>,
    /// facts of this context
    context: BTreeMap<&'static str, u64>,
}

notes: RefCell<HashMap<(DefId, KeyId), Notes>>,
```

`Lang::walk` starts an empty `Notes`, the walk fills it, and it is **stored under** `(def, key)` (inserted, so a re-walk overwrites).
When the graph is made: `context` is summed over the contexts in the graph, `body` is taken once per def. The `first_walk` flag and the
`defs_walked` set go away, and walking a context twice changes nothing. It needs no change in core. And write down, in
[03](03-design.md), that a counter counts places, with a test of a site that has two targets.

### 2.2 Every edge has one stamp, and TC3 needs a second (read)

**How it is built.** Core's `Basis` says what an edge rests on (`Path`, `Type`, `Name`, `Likely`). The flat solver stamps every edge
with the language's one constant (`solve.rs`: `EdgeTo::Context(id, L::BASIS)`), and TypeScript's is `Basis::Type` (`calls.rs` line 200).
A `Target` has no place for a basis of its own.

**Is it wrong today?** No. Python decides the basis per edge (`interp/call.rs`, the `match receiver` before `targets.push`), and its
rule for what TC2 draws is `Type`: a call that rests on *an input of this context, which the key gave* is the engine's answer through a
type. `run → a` in `two-callers` is that case. So TC2's stamp agrees with the reference, by luck of having only one.

**Where it stops being right.** The same rule in Python says `Path` when *the walk itself holds what the call rests on*: "an object it
built here, or a closure it made and called through a variable". That is TC3's first row:

```ts
function main() {
    const h = handler;   // a local the walk reads
    h();                 // Python: Basis::Path. TypeScript's solver can only say Type
}
```

and TC5's (`new Impl()` built in this body, then `x.method()`). No decision row covers it; the one constant was never chosen for the
layer, it is what the solver had.

**What I would do.** Before TC3 draws its first edge: let `Target::Call` (and `Construct`) carry a `Basis`, the engine's answers
passing `L::BASIS` as now. It is a change in core's solver, so every language's tests must stay green (rule 5); TC6 already plans one
core change, but this one is needed earlier. If the owner prefers to keep one stamp, write the row ("TypeScript's edges are all
`Type`") so that it is a decision and the difference from Python is stated.

### 2.3 No cap on the contexts of one function (read; the numbers are the project's own)

**What was decided.** TC2 step 4b measured contexts per function (N6) and decided: no cut. The table (`engine/09-tc2.md`):

| Entry | Greatest number of contexts of one function | Functions with 64 or more |
|---|---|---|
| V-NOC, three entries | 2 | 0 |
| the private codebase, `main` | 19 | 0 |
| the compiler, `createProgram` | **111** (`visitNode`) | **6** |

**Why I would not leave it.** Python cuts a function at 64 contexts (`contexts_per_func`), marks it and counts it. With **only**
parameters in a key, the compiler already has six functions past that line. TC3 adds captured variables to a key, TC4 objects, TC5 the
receiver: each multiplies. Think of one component drawn from 300 places, each with a different props object: 300 contexts of it, and of
everything it passes those props to.

**What I would do.** Build the cap before TC4, as [03](03-design.md) §12 planned it for TC2 ("contexts per function (64, Python's
number): later keys of that function are cut, marked `Incomplete`", a new `Cut` in core) and step 4b put off. Built while it rarely
fires, it is a small step with one fixture. Built after a real project takes minutes, it is done in a hurry.

### 2.4 Parts of the table nothing reads (read)

`flow.rs` marks five fields `#[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]`: `Flow::anons`, `Flow::handed`,
`SiteFlow::span`, `SiteFlow::kind`, `SiteFlow::anon`.

🦀 `#[expect(dead_code)]` tells the compiler "this is unused, and I know". Unlike `#[allow]`, it **fails** the build once the field is
used. So the build passing proves that, outside tests, nothing reads these fields.

TC2 is closed, so the reason text is no longer true. And they are not free: to fill `Flow::handed`, `Sem::flow` calls `handed_over` for
every table, and `walk::body_of` calls `handed_over` again for the same def to draw the hand-over edges. Two descriptions of one thing,
one of them unused.

**What I would do.** At TC3 step 1, decide per field: TC3 reads it (then the walk takes hand-overs **from the table** and the second
call goes), or it is removed until a phase needs it. Fix the reason texts either way.

---

## 3. The tests

**What is good, and worth keeping as the pattern.**

- The laws of a value are checked over **every** value of a small universe (`value.rs`: all subsets of five members, open and closed).
- `eval` asks the outside through a trait (`World`), so its tests answer by hand and need no engine.
- The two graph properties (`never_fewer_edges`, `a_context_is_a_function_of_its_key`) have a test that **breaks a graph on purpose** to
  show each one finds what it is for (`each_property_finds_what_it_is_for`).
- `each_context_walked_alone_has_the_edges_of_the_graph`: every context is walked again by a language with no memory.
- The table in `policy.md` drives the fixtures, and a test names a fixture with no row and a snapshot no row builds.

### 3.1 Pinned counts that only detect change

```rust
assert_eq!(seen, 60, "every entry of every row");          // tests/context.rs, twice
assert_eq!(entries.len(), 60);                              // src/calls/keyed.rs
assert_eq!(of("callback-two-callers"), 4);                  // …and 23 more lines like it
```

Adding **one** row to the table made these fail, and updating them was a coder step of its own (TC2 step 10b, "the pins learn the row,
58 → 60"). A test that fails on every correct addition, and is fixed by typing the new number, teaches people to type the new number.
What it guards ("the table was read, and whole") is already held by `every_context_row_has_a_fixture_and_a_snapshot`.

**What I would do.** Compare with what the table says, not with a literal: `assert_eq!(seen, context_entries().len())`, and
`assert!(seen > 0)`. Drop the per-row list.

### 3.2 The same thing written twice

| What | Where | Cost when it changes |
|---|---|---|
| the parser of `policy.md` rows | `tests/common` (`rows`, `split_policy`) and `calls/keyed.rs` (`context_entries`): two parsers | a new column must be taught twice; they can disagree on what a row is |
| eight digest byte arrays (`A`, `B`, `HANDLER`, …) | `key.rs` tests and `calls/keyed.rs` | a bump of `CONTEXT_ID_VERSION` edits both |
| `const STYLE: MarkerStyle` | 15 files | none today; noise |

A unit test (in `src/`) cannot use `tests/common`, which is why the second parser exists. `analyzer-testkit` is already a dev-dependency
of both kinds of test: the row parser can live there once. The digest pins can be one `#[cfg(test)]` module in the crate.

### 3.3 Tools written as tests

- 13 tests are `#[ignore]`, driven by 18 environment variables (`TS_SWEEP_*`, `TS_MEASURE_*`, `TS_REFRESH_*`, …). `tests/sweep.rs` is
  835 lines. `measure_flow` is 110 lines inside `calls.rs`'s test module.
- The N6 measure put a **prototype of relevance** in test code: `Measure { relevance, bound }`, `reads()`, and a hook in the shipped
  path:

  ```rust
  let data = bind(&flow.params, args, |expr| eval(lang, env, expr));
  #[cfg(test)]
  let data = lang.measured(lang.sem.def_id(callee), &flow, data);
  lang.intern_key(data)
  ```

🦀 `#[cfg(test)]` compiles a line only in test builds. So the `key_of` that tests run is **not** the `key_of` that ships: it has one
more step. Here the step is harmless when `relevance` is off, but each such line is a place where "the tests pass" and "the product
works" can come apart.

Two problems. The numbers in the N6 table "with relevance" come from `reads()`, which its own doc comment calls "an approximation of
relevance". When TC3 builds the real one there are two, and the measured one is the one that is deleted. And a measure that needs environment variables and `--ignored --nocapture` can be run only by someone who has read its
doc comment.

**What I would do.** N6 is answered: delete `Measure`, `reads` and the hook now. Move sweeps and measures to a subcommand of
`typescript-cli` (or an `examples/` program), with arguments in place of environment variables; `scripts/ts-try.sh` then calls that.
Tests stay tests: they pass or fail on their own.

### 3.4 An allowance by count, not by site

`never_fewer_edges` has one stated exception: a site whose callee is only `undefined` has no edge. It is checked like this:

```rust
let mut allowed = precise.stats.language.get("callee_undefined").copied().unwrap_or(0);
for (report, only_unresolved) in lost.into_values() {
    if only_unresolved && allowed > 0 { allowed -= 1; } else { found.push(report); }
}
```

So if the layer counted 3, **any** 3 sites that had only unresolved edges may vanish, not the 3 it meant. A bug that empties the wrong
site passes whenever some other site in the graph was honestly counted. And the counter is per walk (§2.1), so it grows with contexts.

**What I would do.** Let the test know *which* sites: for example, a test-visible list of `(def, site)` the layer emptied, compared
exactly with the sites that lost every edge.

### 3.5 Fixtures that are missing

| Missing | Would have caught |
|---|---|
| one row per kind in `expr_of`'s data list | §1.1 |
| one request in two session histories, under a count limit | §1.2 |
| a site with two targets and one widened argument | what §2.1's counter counts |
| a parameter typed as data, called through a cast | §1.3 (as a stated row) |

---

## 4. Small things

| What | Where | Why it matters | What I would do |
|---|---|---|---|
| an `expect` that panics | `calls.rs` `intern_key`: `.expect("a value names a def of this request's program")` | 🦀 `expect` stops the program when the value is `None`. `CLAUDE.md` rule 8: an analysis step never fails | fall back: a key that names an unknown def is the open key |
| a linear search inside a loop | `sem/flow.rs` `written_symbols`: `!written.contains(&symbol)` on a `Vec` | 🦀 `Vec::contains` looks at every item, so n written names cost n² steps in one very large function. The release gate passed "at the edge" on the compiler (step 4d); I did not measure this line's share | `IndexSet` (already in the lockfile): same order, one step per lookup |
| a check that is quadratic in debug builds | `key.rs` `intern`: `debug_assert!(!self.digests.contains(&made))` | only in debug, but that is how tests run | a `HashSet` beside the `Vec`, under `#[cfg(debug_assertions)]` |
| a comment that is no longer true | `calls.rs` module doc: "TypeScript is not context sensitive yet" as the first sentence | it is, with `Precise` | reword |
| `(flag ? runA : runB)(x)` | met while probing (run); the same with the layer off and on | the answer is one `no_declaration`, though both branches are plain functions. Not the layer's doing | a row, or a note for TC3 (the callee is a `OneOf` the table can already read) |

---

## 5. The decisions, made and assumed

"Decided" has a row or a step that says so. "Assumed" was never asked: it fell out of how the code was written.

| What the code does | Decided or assumed | My read |
|---|---|---|
| an arrow in a key is "an arrow", whichever it is (`AnonAny`) | decided ([03](03-design.md) §6; Python does the same) | right |
| a value lists at most 8 members, then it is wide (`MAX_SET`) | decided ([03](03-design.md) §12) | right; counted |
| arguments bind by position; a spread makes its slot and later ones unknown; extra arguments are not read | decided ([06](06-tc2-functions-through-parameters.md) §5) | right |
| the tripwire counts **and** marks a function that does not fit (D61) | decided | right |
| a site whose callee is only `undefined` has no edge | decided (step 5a) | right for the user; the test's allowance is weak (§3.4) |
| body counters once per def, context counters once per walk | decided (step 6) | works today; breaks with a memo or rounds (§2.1) |
| no cap on contexts per function (N6) | decided, on parameters only | revisit before TC4 (§2.3) |
| relevance moved from TC2 to TC3 | decided (owner) | fine; delete the prototype (§3.3) |
| a parameter declared as data is data (step 5f) | decided, but not stated to users as an assumption | state it or narrow it (§1.3) |
| the kinds in the data list are all "a value was passed" | **assumed** | one is wrong (§1.1) |
| the order of a value's members is never seen outside | **assumed** | it is, under a count limit (§1.2) |
| every edge is `Basis::Type`, the layer's too | **assumed** | right for TC2 by Python's rule; not for TC3's locals (§2.2) |
| a counter counts evaluations, not places | **assumed** | write the rule (§2.1) |
| the table keeps hand-overs, anonymous functions and site kinds for later phases | **assumed** to be read "from TC2 on" | decide per field at TC3 step 1 (§2.4) |

---

## 6. An order to take them in

Not a plan, a suggestion; the manager session owns the steps.

1. **Before TC3 step 1:** §1.1 (`void 0`) and §1.2 (the order). Both are wrong answers, both are small, and each brings a fixture.
2. **As TC3 step 1's first part:** §2.1 (notes kept per context). TC3's memo is the first thing that breaks the current counters, so
   this must be in before the memo, not after.
3. **Questions for the owner:** §1.3 (state the assumption, or narrow it) and §2.2 (a basis per edge, as Python has, before TC3
   draws an edge from a local; or one stamp, written down as a decision).
4. **With TC3's own cleanup:** §2.4 (the unread fields), §3.1–§3.3 (pins, duplicates, the relevance prototype).
5. **Before TC4:** §2.3 (the cap), §3.4 (the allowance by site).

---

## 7. Where each finding stands (manager, 2026-11-26, at `9fc7a50`, after TC3 steps 1–6)

The manager read this review only at TC3's close, not before step 1 as §6 suggested: it was committed while TC3 was being built. Every
finding was checked again against the code of `9fc7a50`.

| Finding | Still true? | What happens |
|---|---|---|
| §1.1 `void 0` is data | yes (`sem/flow.rs`, the data list) | **fixed in TC3 step 8b**: `Expr::Undefined` for `void …` and the global `undefined` (the review's first fix), a row per kind of the data list |
| §1.2 the order of a value's members reaches the output under a count limit | yes (`walk::members` pushes in `DefId` order) | **fixed in 8b**: the members sorted by stable id before targets are made; the review's test, two histories, limits 1–8 |
| §1.3 a parameter typed as data that holds a function | yes, and TC3 applies the same rule to variables | **the owner's** (README "What needs your yes"); recommended: an expression cast to `any` or `unknown` is `Unknown` (narrow, sound, local) |
| §2.1 counters are side effects of a walk | partly: TC3's solve counts once per walk (its last round), but `key_of` still evaluates per target, and TC6's rounds will multiply | scheduled **before TC6** (notes kept per `(def, key)`); the rule "a counter counts places" written then |
| §2.2 one `Basis` for every edge | yes: TC3 now draws edges from locals and closures with `Basis::Type`, where Python says `Path` | **the owner's**: a basis per edge (a core change) before TC4, or one stamp written down as a decision |
| §2.3 no cap on the contexts of one function | yes | scheduled **before TC4** (03 §12's cap, 64, marked and counted) |
| §2.4 parts of the table nothing reads | partly: `Flow::handed` is read now (needs, relevance); four fields are still `expect(dead_code)` with a stale reason | **8b**: each removed or its reason corrected; the second `handed_over` call per def taken from the table if it is the same |
| §3.1 literal pins | yes (94, raised at every TC3 step) | **8b**: the count read from the table |
| §3.2 two row parsers, duplicated digest pins | yes | later, with the tools of §3.3 |
| §3.3 the relevance prototype in shipped code | **fixed** in TC3 step 6 (`Measure`, `reads`, the hook are gone) | tools as tests: later |
| §3.4 the allowance by count | yes | scheduled **before TC4** |
| §4 `expect` in `intern_key`; the quadratic `debug_assert`; the stale module doc | yes | **8b** |
| §4 `written_symbols`' linear search | **gone**: step 1's `written_identifiers` keeps a sorted list | — |
