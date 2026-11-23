# 03.12.04 TC0: counts and spikes

```
TC0
├── 1. why count first
├── 2. the eight counts
├── 3. the four spikes
├── 4. what each number can change in the plan
├── 5. where the code lives, and where the answers go
└── 6. steps
```

Nothing of the layer is built in TC0. It ends with numbers and yes/no answers in `languages/typescript/ENGINE.md`, and with this
folder corrected where a number disagrees with it.

## 1. Why count first

The sweep of 2026-11-19 says where a function-typed value is **called** (a parameter, a call's result, `this`, other). The layer
resolves such a call only if a caller **passes** something it can follow. Nobody has counted that end. The order TC3 → TC4 → TC5, the
"one value per variable" model and the limits of [03](03-design.md) §12 all rest on guesses about it. Each count below replaces one
guess, and costs a day, not a phase.

The corpus is the four projects of the sweep: the V-NOC frontend, the private CLI codebase, TypeScript 5.9.3's `src/compiler`, and
vscode's `src`.

## 2. The eight counts

All of them read the source and today's graph. None depends on the layer.

| | Count | Per project, the share of… | Guess it replaces |
|---|---|---|---|
| **N1** | **the other end** | arguments that land in a parameter whose type can hold a function or an object, at a site whose target is project code, by what the argument **is**: a def by name · an anonymous function · an object literal with a function member · a parameter passed on · a local · `new C(…)` · an expression whose static type is a class · a call's result (project / external) · `this.x` · other | how much each of TC2–TC6 has to put in a key |
| **N2** | own or captured | open sites of the "parameter" column whose leftmost name is: the def's **own** parameter · a parameter of an **enclosing** def (pattern A8) · reached through a pattern or a member path of depth 1, 2, more | whether captures (TC3) come before objects (TC4), and whether depth 2 is enough |
| **N3** | called or called on | open sites of the "parameter" column that **call** the value (`cb()`, `props.onClose()`) against those that call a **method on** it (`w.write()`) | whether functions (TC2, TC4) or instances (TC5) are the larger half |
| **N4** | assigned twice | variables whose type can hold a tracked value: declared `const` · `let`/`var` assigned once · assigned more than once | the price of one value per variable |
| **N5** | written through | objects (a parameter, a local holding a literal) that are written through in the body that holds them · handed to a project function · handed to an external function · put in an array literal (a hook's dependency list) · given a second name (`const o = x`) · held by a `const` at the top of a file, exported or not | how often the two "open" rows of the policy fire, so whether TC4's rule is too careful |
| **N6** | contexts | for the sample entries (S-TC4): the number of distinct argument tuples per callee, as a distribution (median, 95th, greatest), **counted four ways**: telling one anonymous function from another or not, with relevance or without | whether 64 contexts per function is a limit or a wall |
| **N7** | fields | open sites of the `this` column, `this.x.m()` or `this.x()`: `x` is a parameter property · assigned only in the constructor · assigned elsewhere too; and whether the class is built by `new` in project code | whether TC8 is worth its size, and for which projects |
| **N8** | whose result | open sites of the "call's result" column: the call that made the value is to a project function · to a package, by package and export (`react` `useState`, …), the top 20; and of the package half, how many are a name bound straight from the call (`const [a, setA] = useState()`): TCL's forecast | TC6's forecast, and the rows TC10's table would need |

The breakdown of today (`function_typed_value_from_*`) is the model: a key in `Stats.language`, a line in the breakdown, a row in
`ENGINE.md`.

## 3. The four spikes

| | Question | Done when |
|---|---|---|
| **S-TC1** | **Does tsrs answer what [01](01-where-we-start.md) §4 says it does?** Each public function named there is called once, on the tour program ([03](03-design.md) §2): the resolved signature of each call and its parameter symbols in order (for `function f(this: T, cb)` the list must hold `cb` alone; for a generic call each parameter symbol must still lead to the declaration's parameter; the type of each slot by `get_type_at_position_exported`, which must be `boolean` and not `T` for `useState(false)`) · `is_type_assignable_to` on a function and the type of the parameter it is passed to · for each identifier, which parameter or variable declaration it names, through destructuring · the static class of each argument · `write` looked up on `Json` and on a subclass that inherits it · the writes of one property in a file | a table in `ENGINE.md`: function, what it returned, as expected or not. A "no" names the phase it hits |
| **S-TC2** | **Can a body be read into `Flow` in one pass, and what does it cost?** A spike reader that produces, for one def: its sites with the callee and the arguments as `Expr`, and every assignment of every variable it declares, the ones made inside nested functions included | the tour's tables, printed and read; time and memory for every def of V-NOC and of vscode **[measure]**; a list of syntax the reader met and had no `Expr` for, by count; **the table holds numbers, is kept in `Sem` behind an `Arc`, and `Sem` is still `Send`** (the doctests of `proof` pass) |
| **S-TC3** | **The digest.** A canonical key written and hashed as Python's `interp/canon.rs` does. Is the result equal in two processes and on two machines; `xxhash-rust` is already in the TypeScript lockfile (tsrs depends on it); the function goes to core in TC2's first step (D57; Python and Bend each hold a copy, and switch in a session of their own) | the answer, and where the function lives |
| **S-TC4** | **The measure and the sample.** The per-site measure of [03](03-design.md) §10, on today's graphs: for a fixed list of entries per project, the open sites reached, by reason. And the list itself: which entries (for V-NOC `TaskDetailPanel` and the page components; for the others the files' own code, `Entry::File`, of their entry points), such that all of them finish in minutes on the 8 GB machine | `tests/sweep.rs` can print the measure; the baseline table in `ENGINE.md`, to which every phase adds a column |

### 3.1 S-TC1, case by case (written 2026-11-23, before it ran)

The program is the tour ([03](03-design.md) §2) and one more file, `extra.ts`, in a folder with a `tsconfig.json` (`"jsx": "preserve"`,
`"strict": true`, and a three-line `declare namespace JSX` meant to make the elements type-check; it does not: the file has an `export`, so it is a module and the namespace is local to it, and `<button>` is TS7026. A fixture that needs JSX types writes them in `declare global { … }`. No case reads `<button>`). Every expectation below was written before the
spike ran; the spike prints `got` beside it.

| # | Code | Ask | Expected |
|---|---|---|---|
| E1 | `function withThis(this: Ctx, cb: () => void) {}` and `withThis(handler)` | `get_resolved_signature_exported(call)`: `parameters()`, `this_parameter()` | parameters `[cb]`; this parameter `this` |
| E2 | `function run<T>(x: T, cb: (v: T) => void) {}` and `run(1, handler)` | each of `parameters()`: its `value_declaration()` | the `Parameter` nodes `x` and `cb` of `run`'s own declaration (same position in the file) |
| E3 | `declare function useState<S>(initial: S): [S, (next: S) => void]` and `const [open, setOpen] = useState(false)` | `get_type_at_position_exported(sig, 0)`, printed by `type_to_string_exported` | `boolean` |
| E4 | `function many(first: number, ...fs: (() => void)[]) {}` and `many(1, handler, handler)` | `has_rest_parameter()`, `min_argument_count()`, `parameters().len()`, the type at positions 1, 2, 3 | `true`, `1`, `2`, `() => void` three times |
| E5 | E4's call, and the tour's `each(encode)` | `is_type_assignable_to_exported(type of argument, type at its position)`, the argument's type by `get_type_at_location` | `encode` to `each`'s slot 0: `true`; `1` to `many`'s slot 1: `false` |
| E6 | the tour: `onClose()` in `back`, `f()` in `each`, `h()` in `main` | the callee's `get_symbol_at_location_exported`, its `value_declaration()` kind, and `get_root_declaration` of it | `BindingElement` → root `Parameter` of `Panel`; `Parameter` of `each`; `VariableDeclaration` in `main` |
| E7 | the tour's `send(new Json())`, and `const w: Writer = new Json(); send(w)` in `extra.ts` | the argument's `get_type_at_location`: `object_flags()` has `Class`? `symbol()` | `new Json()`: class, `Json`; `w`: not a class (an interface), `Writer` |
| E8 | `class Sub extends Json {}` and `new Sub()` | `get_property_of_type_exported(instance type, "write")`, its `value_declaration()` | for `Json` and for `Sub`: `Json.write`'s `MethodDeclaration` |
| E9 | `class Box { x = 1; set() { this.x = 2; } read() { return this.x; } }` | `get_references_to_symbol_in_file(file, x)`, and `ast::is_write_access` of each | three references: the declaration's name `false`, `this.x = 2` `true`, `return this.x` `false` |
| E10 | the tour's `<Panel onClose={join} />` | `get_resolved_signature_exported(element)`: `declaration()`, `parameters()` | `Panel`, one parameter |
| E11 | `let k = encode; k = join; ({ k } = o); [k] = xs; k ||= join; for (k of xs) {} k();` | the references to `k` in the file, and `ast::is_write_access` of each | the declaration's name `false`; the five writes `true`; `k()` `false` |

### 3.2 S-TC4, as decided (2026-11-23)

**The site.** A call site is named by what does not move between runs and phases: the def it is in (its file, qualname and occurrence,
from `Graph::defs` by the edge's `from.func`) and its `Site` (start and end, relative to the def). A context **reaches** a site when it
has an edge there (every site has one: rule 8). A site is **open in a context** when that context has an `Unresolved` edge there.

**The measure** of one run (every graph the run made, contexts told apart by `ContextId`): for every site open in at least one context,
how many contexts reach it and in how many it is open, and its reasons. A site is *open in every context* or *open in some*. Written as
one line per site to a file, and summed by reason on the screen.

**Against a baseline.** Given the file of an earlier run, every site of it is one of: still open in every context that reaches it, open in
some, open in **none** (answered everywhere), or **not reached** by this run. That last split is the one the layer reports from TC2 on;
TC0's baseline is the same code run against its own file (every site "every", as one context per function allows nothing else).

**The sample**, measured by the reviewer on 2026-11-23 (debug CLI, whole graph, no cut):

| Project | Entries | Contexts | Graph time | Peak footprint |
|---|---|---|---|---|
| V-NOC frontend | `Dashboard` (`src/pages/Dashboard.tsx`), `HomePage` (`src/pages/HomePage.tsx`), `TaskDetailPanel` | 541, 62, 90 | 245, 83, 53 ms | 143 MB |
| the private codebase | its entry file's `main` (kept outside git, in the reviewer's notes) | 5,923 | 0.8 s | 222 MB |
| TypeScript 5.9.3 `src/compiler` | `createProgram` (`program.ts`) | 6,949 | 0.55 s | 131 MB |
| vscode `src` | chosen at the reviewer's first vscode run (the machine's swap was full on 2026-11-23) | | | |

V-NOC's own entry file reaches 2 contexts and its route files 1 (the router is a library that holds the pages as data), so the pages
are entries of their own.

A "no" is written in `ENGINE.md` with the phase it hits: E1, E2, E4, E5, E10 hit TC2 (binding and the tripwire), E3 nothing yet
(types in a key are later), E6 and E11 TC3 (variables, and the completeness rule of [03](03-design.md) §5), E7 and E8 TC5, E9 TC8.

S-TC2 is the one that can change the design: if assignments inside nested functions cannot be attributed to the variable's scope in one
pass, "a value is complete when every assignment was read" does not hold as written, and [03](03-design.md) §5 needs another rule.

## 4. What each number can change

| If… | Then… |
|---|---|
| N1: "a def by name" and "a parameter passed on" are a small share, anonymous functions a large one | TC7 (D54) matters more than this plan assumes: it moves up, before TC5 |
| N2: enclosing-def parameters are rare | TC3 moves after TC4 |
| N2: paths deeper than 2 are common | `max_depth` is 3, and N6 is read again with it |
| N3: "called on" is the larger half | TC5 moves before TC4 |
| N4: more than a few percent are assigned more than once | the refinement of [01](01-where-we-start.md) §3.1 (a value per place in the body) is planned as a phase, not left as an option |
| N5: objects handed to external functions are common, and written through rarely | TC4's library half becomes a question for you: assume a library leaves an object's members alone, as a stated exception to "never guess" |
| N6: the 95th percentile is near 64 | relevance (the second row of [03](03-design.md) §12) is built inside TC2, not after it |
| N7: most `this.x` are parameter properties of classes built by `new` | TC8 moves before TC6 for class-heavy projects; if they are built by a container, TC8 is dropped from the plan |
| N8: project functions are a small share | TC6 is smaller in effect than its column suggests; TC10's table is where the rest is |
| N6: telling anonymous functions apart multiplies the contexts of a helper and changes no edge | confirmed: a key writes `AnonAny` ([03](03-design.md) §6). If the four counts are close, the rule stays anyway: it is Python's |
| N5: an object in an array literal is common (dependency lists) | that row of the policy is relaxed for an array literal passed straight to an external call, as a stated exception, and it is a question for you |
| the forecast (step 7) says a phase answers less than a few percent of the open sites its entries reach | the phase is not built before the ones after it; TCL, which needs no layer, is built first |

## 5. Where the code lives, and where the answers go

- **Spikes** (S-TC1 – S-TC3): `languages/typescript/spikes/`, a workspace of its own, deleted when the answer is written
  (`CLAUDE.md`, "How to work here"). **It shares the TypeScript workspace's build**: a copy of `languages/typescript/Cargo.lock`, the
  same `[profile.dev]` lines, the same `rev` for every `tsrs_*`, and `CARGO_TARGET_DIR=../../target` when built. Then no engine crate
  is compiled again (measured 2026-11-23 with a probe: only the probe itself compiled), where a target of its own would cost about
  2 GB of a disk that has 3.5 GB free.
- **Counts** (N1 – N8): they need what `sem` knows, so they are counters in the analyzer, as the origin split was. A count that a later
  phase reads again stays (N6 becomes a line of the breakdown); the others are removed in the commit that writes their numbers down.
- **The measure** (S-TC4): stays, in `tests/sweep.rs`.
- **Answers**: `languages/typescript/ENGINE.md`, a section "TC0: what was counted". Corrections to this folder in the same commit as
  the number that causes them.

Memory: the machine has 8 GB. One project at a time, `-j2`, `--test-threads=2`, never two sweeps at once.

## 6. Steps

| # | Do | Proof |
|---|---|---|
| 1 | S-TC1. **Done** (spike 9b91422, reviewed 2026-11-23: 40 of 40 as expected, and seven more shapes probed by the reviewer) | the table in `ENGINE.md` |
| 2 | S-TC4: the sample entries and the per-site measure, on today's code. **Built** (`b0f21fb`; reviewed 2026-11-23, baseline in `ENGINE.md`; a fix for its memory on a full sweep follows; vscode's row waits) | the baseline table |
| 3 | N2, N3, N8: they split counters that exist | three tables |
| 4 | N1, N4, N5, N7: they read arguments and assignments, which is most of S-TC2's reader | four tables; S-TC2's list of syntax with no `Expr` |
| 5 | S-TC2's cost, S-TC3 | the numbers; where the hash lives |
| 6 | N6, from the arguments N1 found | the distribution |
| 7 | this folder corrected by §4; the order of TC3–TC5 confirmed or changed; **a forecast per phase**: of the open sites the sample entries reach, how many each of TCL, TC2–TC6 can answer | reviewed with you |

Next: [`05-tc1-the-body-as-data.md`](05-tc1-the-body-as-data.md)
