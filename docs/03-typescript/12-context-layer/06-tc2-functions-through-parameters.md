# 03.12.06 TC2: functions through parameters

```
TC2
├── 1. what the user sees
├── 2. `sem`: which parameter a name is, and which parameters can matter
├── 3. values: `Fn`, `Anon`, `Undefined`
├── 4. the key gets its first slots
├── 5. `resolve_from_env` answers
├── 6. what a real key forces: counters per def, a request that starts from a context
├── 7. core: two small changes
├── 8. relevance, after it works
├── 9. fixtures and tests
└── 10. steps
```

Depends on TC1. The first phase that changes a graph, and the one that proves the whole path: a value, a key, a digest, two contexts of
one function, the tests. It is deliberately the smallest value there is: a function, passed by name, to a parameter that is called.

## 1. What the user sees

Patterns A1–A6, A11, R1, R2 of [02](02-patterns.md); lines a, b, h, i of the tour.

| Code | Before | TC2 |
|---|---|---|
| `function run(cb) { cb(); }` · `run(handler)` | `run` with nothing under it | `run` → `handler` |
| `run(a)` in `first`, `run(b)` in `second` | one `run`, empty | two contexts of `run`; `first` shows `a` under it, `second` shows `b` |
| `relay(handler)` → `each(f)` → `f()` | empty at the end | `handler` at the end |
| `run(flag ? a : b)` | empty | `a` and `b`, two nodes at one site, one context |
| `function run(cb = noop) { cb(); }` · `run()` | empty | `noop` |
| `run(api.load)` | empty | `Api.load` |
| `function twice(f) { return () => { f(); f(); }; }` · `twice(work)` | `twice`, empty | `twice` → `work`, `work` |
| `each(() => work())` | `each` empty; `work` beside it | the same tree; the call in `each` has the reason `anonymous_function_called` and no longer counts as `function_typed_value` |
| `run(handler)` asked for **at `run`** | empty | empty: an entry has the open key ([02](02-patterns.md) F4) |

In every row the `[Passed]` edge of the function under its caller stays.

## 2. `sem`: which parameter a name is

`sem/flow.rs` learns two things.

**`Expr::Param(i)`.** An identifier whose symbol's `value_declaration` is a `Parameter` node, a direct child of **this def's** node, declared by a plain
name. `i` is its place in the def's own **signature** (`Signature::parameters()`; for an overloaded function, the implementation's, [03](03-design.md) §7), not in the syntax: a `this` parameter is not in that
list, so `function f(this: Ctx, cb)` has `cb` at 0, and a call `f(handler)` binds `handler` to it ([03](03-design.md) §7). Not yet: a parameter of an enclosing def (TC3), a name bound by a
destructuring pattern (TC4), a parameter of an anonymous function inside the def (it is that function's, filled by whoever calls it:
unknown). The table also says, per parameter:

| `Param` field | Meaning | From |
|---|---|---|
| `rest` | `...args` | the syntax |
| `named` | declared by a plain name (`cb`), not a binding pattern (`{ onClose }`): only such a parameter is ever an `Expr::Param` (added at step 2's design) | the syntax |
| `default` | the default's `Expr`, if any | the syntax |
| `assigned` | the body assigns to the parameter (`cb = cb \|\| noop`), a nested def or an anonymous function included. Until TC3 reads assignments, such a parameter is **open**: its key value is joined with unknown. Looked for only on a named, capable parameter (`false` on the others: never in a key) | `ast::is_write_access` on its references, found by TC0's search (`names_in`, `writes_of`: the engine's own is private) |
| `capable` | its declared type can hold a function, an object or a class instance. `false` for `string`, `number`, `boolean`, `bigint`, `symbol`, `null`, `undefined`, `void`, `never`, literal and template literal types, enums, and unions and arrays of only those. `true` for everything else, `any` and an untyped JavaScript parameter included. **Decided at step 2's design:** a tuple is `true` (its element types come only through `get_type_arguments`, which downcasts with an assert), a recursive type is `true` where the look stops (depth 4), an intersection is `true`, and a binding pattern is `false` until TC4 reads patterns | `get_type_of_symbol` (public, `checker_08.rs`; `sem/handed.rs` already calls it), `TypeFlags`. Not one of S-TC1's cases: what each type gives is this phase's own fixtures, one per kind of the list |

`capable` is the first filter of [03](03-design.md) §12. It is computed from the declaration alone, so it is the same for every caller.

## 3. Values

| `One` | Made by | Called |
|---|---|---|
| `Fn(func)` | `Expr::Def` naming a def with a body | `Target::Call(func, key_of(func, site))` |
| `Anon(def, n)` | `Expr::Anon(n)` | `Target::Unresolved(anonymous_function_called)`: known, and drawn where it is written (until TC7, D54). In a key it is written `AnonAny`: which arrow it is changes no edge yet |
| `Undefined` | a parameter with no argument | nothing is called there, and the layer knows it: no edge, counted `callee_undefined`. (Rule 2 of the README is about call targets; this site had an `Unresolved` edge, which said "unknown", and it is no longer unknown) |

`Expr::Def` naming a **class** is still unknown here (TC4), and so is one naming a signature without a body (D43: nothing to enter).

`join(a, b)`: the union of the known members, sorted by a stable order (a def by file and offset, an anonymous function by its def and
number), open if either is. More than `max_set` members: `Wide`, which absorbs every later join ([03](03-design.md) §3), counted `value_widened_set`. Data joined with anything changes nothing.

A parameter with a default: where its value holds `Undefined`, that member is replaced by the default's value, evaluated in the
callee's own env (a default may name another parameter). Where its value is open, the default's value is joined in: the caller may
have passed nothing.

## 4. The key

`Key.params[i] = admit(g, i, value)`:

```
admit(g, i, v)
    flow(g).params[i] is not capable, or is a rest parameter   → unknown
    otherwise                                                   → v
then, in the one normal form (KeyData::normal, at interning):
    every anonymous function among a slot's members            → written AnonAny (one member however many arrows)
    unknown slots at the end are dropped
```

(Step 4's design moved `AnonAny` from `admit` to the normal form, where [03](03-design.md) §6 already had it: a key interned by any path is
then erased the same way.)

**The canonical form and the digest** (S-TC3's function, used for real for the first time):

```rust
#[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
enum Canon { Unknown, Data, Wide, Fn(StableDef), AnonAny, Undefined, Set { known: Vec<Canon>, open: bool } }
struct CanonKey { params: Vec<Canon> }          // TC3 adds `captured`, TC5 `this`; a field is skipped when empty,
                                                // so a key's digest does not change when a later phase adds a field it does not use
```

> **🦀 Rust note: why not `#[derive(Hash)]` for the digest.** `Hash` is for hash maps inside one run of one program. The standard
> library promises nothing about its output across versions of Rust, or even across runs (`HashMap`'s default hasher is seeded at
> random). A digest leaves the process: the UI stores it, the next run must produce the same 16 bytes. So we write the bytes
> ourselves (a canonical struct, serialized) and hash those with a function whose output is specified (`xxh3_128`). The derived
> `Hash` is still what the solver's memo uses, where it only has to agree with itself.

## 5. `resolve_from_env` answers

[03](03-design.md) §7, with the parts TC2 has. In `walk.rs`:

```
for each site, in order:
    engine = sem.callee_with_facts(…)                                       as today
    if the engine's answer is open (function_typed_value, any_or_untyped, no_declaration on a call's result):
        v = eval(site.callee)
        targets = [target_of(m) for m in v.known] + (engine's answer if v.open or v.known is empty)
    else:
        targets = engine's answer
    for each target that is a Call or a Construct into project code:
        its key = key_of(callee, site)          the arguments, evaluated here
```

The split counters follow the edges: a site that is answered no longer adds to `function_typed_value_from_parameter`.

**Binding** goes through the signature the checker resolved ([03](03-design.md) §7): argument `i` to `Signature::parameters()[i]`, which is `flow(g).params[i]`. A function target that is not assignable to the callee's static type is counted `value_type_mismatch` and the engine's answer is kept beside it. `new C(a, b)` binds to the constructor that runs (`init`). A tagged template, a
decorator and an accessor pass nothing in TC2 (every slot unknown). A JSX element passes nothing until TC4.

**Decided at step 4's design (2026-11-24): binding is by position, and asks the checker nothing.** At run time JavaScript binds argument `i`
to the `i`-th parameter of the function that runs, which is the implementation the target names; `Flow::params` is that list without `this`
(step 2), so an overload's own shape does not matter and a rest is the implementation's. What would make the engine's target differ from the
function the arguments go to was probed with the CLI at `36434b3`: `f.call(x, h)` and `f.apply(x, [h])` are an `External` (and
`uncovered_call_apply`), so they bind nothing; `f.bind(x, a)` gives a new function type and `g(h)` on it is `function_typed_value`; `f.bind(x)`
with no argument keeps `f`'s parameters and the engine names `f`, so position is right there too; `super(cb)` is a construction of the base
class whose `init` is the constructor that runs, and an inherited constructor receives the arguments as they are. So step 4 calls no
`get_resolved_signature_exported`; the checker comes in at step 5, for the tripwire (`value_type_mismatch`).

## 6. What a real key forces

Both are in [03](03-design.md) §10; both are built here because TC2 is the first phase that needs them.

**Counters per def.** `TypeScript::note*` are called from `walk::body`, once per walk. They move to a map keyed by the def's id, filled
the first time a def is walked in a request, and summed by `finish`. A test: a function with one `uncovered_bind`, reached under two
keys, counts 1.

**A request that starts from a context** (Q-TC3). `Session` keeps `HashMap<ContextId, CanonKey>` for the contexts its graphs listed and did not
finish (a frontier, a cut), cleared by `reload`. `Session::graph_from(&ContextId, budget, cancel)` turns the canonical key back into a `Key<'p>` (each
`StableDef` is looked up as an entry is) and starts the solver there. A digest it does not have, or a def that is gone: the open key,
and `context_not_found` in `Stats.language`. The test is the one that motivates it: a tree cut at depth 1 and continued from each cut
node equals the tree asked for whole.

Until you answer Q-TC3 the method exists and the driver does not call it; a tree continued the old way is correct and less precise.

## 7. Core: two small changes

Each gets a short note in `docs/02-tree-rules/` first, as passed edges did, and keeps every language green.

| Change | Why now | Shape |
|---|---|---|
| **the scenario level is per scenario** ([`04-rust/02-architecture.md`](../../04-rust/02-architecture.md) §9, row 2) | TC2 makes one scenario precise, not all | `analyzer_testkit::scenarios::run` takes a function from the scenario's name to its `Level`; a language lists the scenarios it is precise for |
| **a cut for "too many contexts of one function"** ([03](03-design.md) §12; Rust's design §7 leaves the same choice open) | a real key can multiply contexts. **Built only if N6, or a phase's sweep, shows a function near 64.** Until then core does not change for it, and the sweep's count of sites lost to a cut (README rule 2) is the alarm | `Limits::max_contexts_per_function: Option<u32>` (`None` by default, so flat languages do not change), a `Cut` value for it, counted like the others |

## 8. Relevance, after it works

Steps 1–6 put every capable parameter in the key. Then, with an equal-edges test before and after:

```
parameter i of g is relevant if its value can reach, in g's body (through locals, once TC3 has them):
    (a) the callee of a site                      cb()
    (b) an argument of a site, or a hand-over     each(cb), onClick={cb}
    (c) a `return`                                 (it matters from TC6; conservative now)
    (d) a nested def or an anonymous function      const later = () => cb()
```

This is Python's `relevant.rs` on the table instead of on the syntax tree. A parameter that is not relevant is unknown in every key,
so `function log(cb) { console.log("x") }` has one context however it is called. Whether this step is needed inside TC2 or can wait
is N6's answer. Relevance is a fact of the code: it is kept in `Flow::facts`, worked out once per program, never once per request.

## 9. Fixtures and tests

`tests/fixtures/context/`, one folder each, a row of the precise policy table each:

`callback-by-name` · `callback-two-callers` · `callback-passed-through` · `callback-either` · `callback-default` ·
`callback-not-passed` (`cb?.()` with no argument: no edge) · `callback-method` · `callback-anonymous` · `callback-returned-arrow`
(line h) · `callback-recursive` (R1) · `callback-mutual` (R2) · `callback-spread` (`run(...args)`: unknown) · `callback-rest` ·
`callback-reassigned` (open: the handler **and** the engine's answer) · `callback-not-capable` (a `string` parameter: one context) ·
`callback-entry` (asked at the callee: today's tree) · `callback-javascript` (the same in a `.js` file, untyped).
Added by the review: `callback-this-parameter` (`function f(this: Ctx, cb)`) · `callback-overload` (the checker picks the signature, the def
with the body is entered) · `callback-or-data` (`run(ok && handler)`: one edge, nothing unresolved beside it) · `callback-two-arrows` (two
callers pass two different arrows: **one** context of the callee).

| Test | |
|---|---|
| the harness's three properties on every fixture | [03](03-design.md) rules 1, 4, 5, 7 |
| the policy row `callback-param` (the default table) | **unchanged**: it is the default level's answer |
| a new shared scenario, `scenarios/callback-argument/typescript`, with `expected.default.tree` and `expected.precise.tree` | TypeScript is precise for it; the other languages' folders are theirs to add |
| digests: the same key from two entries, in two processes | equal |
| `contexts_per_function` set to 2 on `callback-two-callers` with three callers | the third is `Incomplete`, counted, and the first two are whole |
| a tree cut and continued (§6) | equals the whole tree |
| the capability bit `PARAMS` off | every graph is TC1's, byte for byte |
| the inline evaluator against the solver, on every fixture without a cycle | equal trees ([03](03-design.md) §14) |
| `value_type_mismatch`; sites drawn at `Default` and missing at `Precise` | 0 and 0, on the fixtures and on the four projects |
| the four projects | the per-site measure before and after; contexts, walks, time |

**No-reuse mode (the README's rule 3), to design in this phase.** Core's flat solver has no switch that turns its memo off (that is Python's own
solver's `Limits::reuse`). TC2 makes the first keys that differ, so it is the first phase where a summary could be reused under the wrong
key. Step 4 decides how the check is made (a test-only switch in core, or a test that walks each context of a fixture's graph alone with its
key and compares), and writes it before step 5. (Found closing TC1, 2026-11-24.)

**Decided at step 4's design (2026-11-24): a test that walks each context alone, and no switch in core.** Core's flat solver walks a
`(func, key)` at most once per request (`Run::seen` in `graph_within`); its memo is reused only across requests of one solver
(`reuse_hits`). So "the memo off" changes nothing inside a request, and what could make a summary depend on more than its key is the
language's own state: the `TypeScript` value, its `Keys`, the tables `Sem` keeps. The check removes all of it: for every context a graph
walked (a test-only record in `TypeScript`, `walked`), a **fresh** `TypeScript` on the same `Sem` interns the context's `KeyData` alone, walks
the def once, and its targets, written with stable ids and digests, must be exactly the graph's edges leaving that context. It is
`calls/keyed.rs` `each_context_walked_alone_has_the_edges_of_the_graph`; step 4 runs it on its three fixtures, and from step 5 on it runs on
every row of the context table.

## 10. Steps

| # | Do | Proof |
|---|---|---|
| 1 | the core note; core: the level per scenario; the digest function, with `xxhash-rust` (D57; its shape is in `ENGINE.md` "S-TC2, S-TC3, N6"); a test pinning the digest's bytes, run on CI's three systems | core green; every language's tests green; flat graphs byte-identical |
| 2 | `sem/flow.rs`: `Param`, the `Param` table | the tour's table snapshot changes as expected |
| 3 | `value.rs`: `Fn`, `Anon`, `Undefined`, `join`; `eval`: `Param`, `Def`, `Anon`, `OneOf`, defaults | unit tests on hand-written tables |
| 4 | `key.rs`: `params`, `admit`, the canonical form; `key_of` at every project target | `callback-two-callers`: two contexts, two digests, still no new edge |
| 4b | **N6, moved here from TC0**: on the four projects, the contexts per function the keys of step 4 make (median, 95th percentile, greatest), counted with and without telling anonymous functions apart and with and without relevance (capability bits). The per-function cut (03 §12) is built in this phase only if the 95th percentile comes near 64; relevance (step 8) moves before step 5 if it is what keeps the count down | the distribution in `ENGINE.md` |
| 5 | `resolve_from_env` answers at open sites | the fixtures flip; lines a, b, h, i of the tour |
| 6 | counters per def | the two-keys counter test |
| 7 | `Session::graph_from` | the cut-and-continue test |
| 8 | relevance | equal edges, fewer contexts |
| 9 | the four projects | `ENGINE.md` |
| 10 | (mine) `policy.md` rows, `ENGINE.md`, this folder's forecast corrected | reviewed |

After step 9 the layer can be tagged for the first time, off by default.

**Step 1, as decided for the build (2026-11-24); built as decided, `d36d7b9` ([`engine/09-tc2.md`](../../../languages/typescript/engine/09-tc2.md)).** A core step; the shape is the note
[`02-tree-rules/08`](../../02-tree-rules/08-the-digest-and-scenario-levels.md), with the pinned bytes in it.

- **`analyzer_core::graph::digest<T: Serialize + ?Sized>(&T) -> [u8; 16]`**, beside `ContextId`: `serde_json::to_vec`, `xxh3_128`,
  `to_le_bytes`, `expect` on a value that cannot be written. `serde_json` becomes a normal dependency of `analyzer-core`, `xxhash-rust 0.8`
  (`xxh3`) a new one. Nothing calls it in this step; TypeScript's first caller is step 4, Python and Bend switch in their own session (D57).
- **`analyzer_testkit::scenarios::Levels`** (`All(Level)`, `PreciseFor(&'static [&'static str])`, `From<Level>`), and `run` takes
  `impl Into<Levels>`: every existing call is unchanged. A listed name with no program in the subject's folder fails the run.
- **TypeScript's one new test**: the shared scenarios with `Precision::Precise`, `Levels::PreciseFor(&[])`: the layer on, every tree the
  default one (6 scenarios). Step 5 adds `callback-argument` to the list and its `typescript/` program.
- **Lockfiles**: core's and all four languages', updated by resolution only (`cargo metadata`); Rust gains the package `xxhash-rust`, the
  others only `analyzer-core`'s dependency list. Python's and Rust's tests are not run on this machine (no build directory, 5 GB free):
  their lockfiles are checked with `cargo tree --locked`, their tests run on CI when the owner pushes.

**Step 2, as decided for the build (2026-11-24).** `sem/flow.rs` learns which parameter a name is; nothing evaluates it yet (`eval` answers
unknown for `Expr::Param` until step 3), so no graph changes.

- **`Expr::Param(u32)`**: in `expr_of`, an `Identifier` whose symbol's `value_declaration` is one of this def's own parameter nodes (the
  syntax list without a `this` parameter, which is the implementation's signature for an overloaded function) is `Param(i)`, `i` its place in
  that list. Asked before `functions_named`. A name bound by a pattern has a `BindingElement` declaration, and a parameter of an anonymous
  function or of another def is not in the list: both fall through to today's answer (unknown). A parameter used inside an anonymous function
  of the body is the body's `Param` (`twice`'s `f`, line h); one used inside a nested **def** is that def's business (TC3).
- **`Param { rest, named, default, assigned, capable }`** (§2's table). `default` is `expr_of` of the initializer, read in the def's own
  table, so a default naming an earlier parameter is `Param(j)` and an arrow in a default is `Anon(n)` (the collector walks parameter
  initializers). `capable` is a function of `sem` on `get_type_of_symbol`: `holds_only_data(ty, depth 4)` = a union of members that all do;
  `TypeFlags::Primitive | Never` (a `void`, `null`, a literal, an enum, a template literal are in `Primitive`); an array
  (`is_array_type`) whose element (`get_element_type_of_array_type_exported`) does; nothing else. `assigned` brings back TC0's `names_in`
  and `writes_of` (`c123d0f` removed them), without the feature.
- **The rider** of handoff 04: the comment above `one_of` in `expr_of`'s name branch says overloads repeat a member; they do not (step 5's fix
  showed it). It says: a script's function merged with a library global of the same name.
- Fixtures: `tests/fixtures/flow/src/params.ts` (every row of `capable`'s rule, names, defaults, writes) and `tests/fixtures/flow-js/src/params.js`
  (untyped, no config). The tour's snapshot changes in four tables only (`each`, `relay`, `twice` gain `param#0`; every table prints its
  parameter rows). **Built as decided, `4bb4e98`** ([`engine/09-tc2.md`](../../../languages/typescript/engine/09-tc2.md)).

**Step 3, as decided for the build (2026-11-24); built as decided, `36434b3`.** Values and `eval` answer; the walk still throws the answer away (`resolve_from_env` returns
`None` until step 5), so no graph moves.

- **`One`** gains `Anon(DefId, u32)` (the def it is written in, its number there) and `Undefined`, after `Fn`: the derived order is `Fn` by
  def, then `Anon` by def and number, then `Undefined`. `AnonAny` is a key's way to write an anonymous function and comes with `admit`
  (step 4). **`Value`** gains `contains(One)` and `without(One)` (the members less one, open as it was, wide stays wide). The laws' universe
  becomes three functions, an anonymous function and `Undefined` (still five members, cap 3).
- **`Env { flow, def, key: KeyData }`**: the def whose body it is (for `Anon`) and what its key holds (for `Param`). `Lang::walk` fills them
  (`Keys::data(KeyId)`, new). Under the open key every slot is unknown.
- **`eval`**: `Unknown` → unknown; `Data` → data; `Def(d)` → `{Fn(d)}`; `Anon(n)` → `{Anon(def, n)}`; `OneOf` → the join of its members;
  `Param(i)` → the key's slot `i` (unknown when absent), unknown for a rest parameter, joined with unknown when `assigned`, and, when the
  slot holds `Undefined` or is open and the parameter has a default, the slot without `Undefined` joined with the default's value. **A default
  is evaluated seeing only the parameters before its own** (`eval_below(.., i)`: `Param(j)` for `j ≥ i` is unknown), which is the language's
  rule and keeps `a = a` from looping. `capable` is not read by `eval`: whether a slot may hold a value is `admit`'s (step 4). `eval` is pure;
  `value_widened_set` is counted where a value is used (step 5). **A guard no step-3 test reaches:** `Lang::walk` filling `Env::key` from the key
  (every key is the open one until step 4); step 4's `callback-two-callers` proof is the first to reach it, and its review mutates it.
  (Corrected at step 4's design: `run`'s body passes nothing on, so its summary is the same under any key; the fixture that reaches the guard
  is `callback-passed-through`, where `relay`'s key decides `each`'s.)

**Step 4, as decided for the build (2026-11-24); built as decided, `98d4c1f` ([`engine/09-tc2.md`](../../../languages/typescript/engine/09-tc2.md)).** Keys get their slots and their digests; the layer still answers nothing (`resolve_from_env`
is `None`), so every edge is the engine's and only the contexts split. **`Caps::BUILT` stays empty**: `Precision::Precise` makes no key yet,
every public graph is unchanged, and the step's proofs run in the crate with `with_layer(Some(Caps::PARAMS))`. `PARAMS` joins `BUILT` at step
5, with the replacement of `tests/precise.rs`.

- **`One::AnonAny`**, between `Anon` and `Undefined`: an anonymous function, which one not said. Made only by `KeyData::normal`, which writes
  every `Anon` of every slot as it (`Value::map`, new: each member through a function, in the normal form again; wide stays wide). Python has
  no lambda in a key at all; `AnonAny` keeps what a key needs (an arrow was passed: step 5 says `anonymous_function_called`, not
  `function_typed_value`) and nothing that would split contexts (§1's last-but-one row).
- **`key.rs`**: `admits(&Param)` (capable and not rest) and `bind(params, args, value_of) -> KeyData`: slot `i` is unknown when the parameter
  is not admitted (its argument is not evaluated), or when a spread is at an argument `≤ i`; else `admit` of the argument's value, or of
  `{Undefined}` when there is no argument `i`. Arguments past the last parameter are not read. `Keys::intern(data, stable)` takes the function
  from a def to its `StableDef`; the open key is put in by `Keys::new` with digest zeros and never hashed; every other key's digest is
  `analyzer_core::graph::digest(&CanonKey)`, made once, with a `debug_assert!` that no earlier key of the request has it (two normal forms,
  one canonical form, would be merged by `Graph::new` without a word; only a bug in `canon` can do it, never a user's text).
- **The canonical form** (private to `key.rs`): `enum Canon { Unknown, Data, Wide, Fn(StableDef), AnonAny, Undefined, Set { known: Vec<Canon>,
  open: bool } }` and `struct CanonKey { params: Vec<Canon> }`, deriving `Serialize, PartialEq, Eq, PartialOrd, Ord`. A slot is written:
  unknown → `Unknown`, data → `Data`, wide → `Wide`, one member and closed → that member, anything else → `Set` with the members **sorted as
  `Canon`** (by `StableDef`, never by `DefId`, whose file numbers follow the order files were first asked for). `StableDef` is core's derive, so
  a def writes `{"Path":{"module":…,"qname":…,"nth":…}}` or `{"Marked":…}`. No def is in `CanonKey`: `Lang::digest` sees the key alone, and the
  context id carries the def. `CONTEXT_ID_VERSION` (1) is exported at step 5, when `Precise` first makes a digest a user sees.
- **The pinned bytes**, computed by the reviewer before the build (`xxhash-rust` 0.8.19 and 0.8.21 × `serde_json` 1.0.151 and 1.0.154, all
  the same; `M` is `Path{"src/main.ts", qname, 0}`):

  | `params` | `digest` (first 8 hex) |
  |---|---|
  | `[Unknown, Fn(M a)]` | `406ed661` |
  | `[Data]` · `[Wide]` | `6eee0fda` · `14bd25d0` |
  | `[Fn(M a)]` · `[Fn(M b)]` · `[Fn(M handler)]` · `[Fn(M other)]` | `5d2e5760` · `ac15243e` · `3fa907f4` · `b6a599f6` |
  | `[Fn(Marked "abc")]` | `64232037` |
  | `[AnonAny]` · `[Undefined]` | `e33b6f72` · `600f3651` |
  | `[Set{[Fn(Marked abc), Fn(M a), Fn(M b), AnonAny, Undefined], open}]` | `87063c79` |
  | `[Set{[Fn(M a), Fn(M b)], closed}]` · `[Set{[Fn(M a)], open}]` | `37e1016d` · `811376e0` |

- **`walk.rs`**: after either answer (`resolve_from_env` or the engine's), when the layer knows `PARAMS`, every `Target::Call(g, _)` and every
  `Construct { init: Some((g, _)) }` gets `key_of(site, g)`: the open key when the site's arguments were not read (`args: None`: a tagged
  template, a decorator, an accessor) or `g` has no table, else `bind(flow(g).params, args, eval in the caller's env)`, interned. A JSX
  element's one argument is `Unknown`, so it passes the open key without a rule of its own. A hand-over stays under the open key (TC3 gives it
  its captures). A class entry starts under the open key (D60).
- **`calls.rs`**: `TypeScript::knows(Caps)`, `TypeScript::intern_key(KeyData)` (the `StableDef` of a def is `sem.describe(sem.func(def))`'s id),
  and a test-only record `walked` of every `(func, key)` the walk was asked for.
- **Fixtures and tests.** Two rows of the context table, written by the reviewer with this decision, whose snapshots are both halves the same
  until step 5: `callback-two-callers` (`run(a)` in `first` and `again`, `run(b)` in `second`) and `callback-passed-through` (`relay(handler)`,
  `relay(other)`, each `each(cb)`; `direct` calls `each(handler)`). One fixture outside the table, `tests/fixtures/keys`, with one entry per rule
  of `bind` (fewer arguments, a default, a conditional, a spread, a rest, a leading parameter that cannot hold a function, a tagged template,
  two arrows, `new`, an inherited constructor, `super`). In-crate tests in `src/calls/keyed.rs`: two contexts of `run` with the pinned
  digests and, digests set aside, the default graph's edges exactly; one context of `each` per function passed, `handler`'s reached through
  `relay` and directly (the guard of step 3, the key in `Env`, is reached here); each context of `keys` with its pinned digest; the layer with
  no bit equal to the layer off; and the no-reuse check of §9.

Next: [`07-tc3-variables-and-captures.md`](07-tc3-variables-and-captures.md)
