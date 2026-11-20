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

**`Expr::Param(i)`.** An identifier whose symbol's `value_declaration` is a `Parameter` node, a direct child of **this def's** node, at
position `i` of its parameter list, declared by a plain name. Not yet: a parameter of an enclosing def (TC3), a name bound by a
destructuring pattern (TC4), a parameter of an anonymous function inside the def (it is that function's, filled by whoever calls it:
unknown). The table also says, per parameter:

| `Param` field | Meaning | From |
|---|---|---|
| `rest` | `...args` | the syntax |
| `default` | the default's `Expr`, if any | the syntax |
| `assigned` | the body assigns to the parameter (`cb = cb \|\| noop`). Until TC3 reads assignments, such a parameter is **open**: its key value is joined with unknown | `ast::is_write_access` on its references |
| `capable` | its declared type can hold a function, an object or a class instance. `false` for `string`, `number`, `boolean`, `bigint`, `symbol`, `null`, `undefined`, literal types, enums, and unions, arrays and tuples of only those. `true` for everything else, `any` and an untyped JavaScript parameter included | `get_type_of_symbol`, `TypeFlags` **[check]** in S-TC1 |

`capable` is the first filter of [03](03-design.md) §12. It is computed from the declaration alone, so it is the same for every caller.

## 3. Values

| `One` | Made by | Called |
|---|---|---|
| `Fn(func)` | `Expr::Def` naming a def with a body | `Target::Call(func, key_of(func, site))` |
| `Anon(def, n)` | `Expr::Anon(n)` | `Target::Unresolved(anonymous_function_called)`: known, and drawn where it is written (Q-TC1) |
| `Undefined` | a parameter with no argument | nothing: a site whose value is only `Undefined` has no edge, counted `callee_undefined` |

`Expr::Def` naming a **class** is still unknown here (TC4), and so is one naming a signature without a body (D43: nothing to enter).

`join(a, b)`: the union of the known members, sorted by a stable order (a def by file and offset, an anonymous function by its def and
number), open if either is. More than `max_set` members: unknown, counted `value_widened_set`.

A parameter with a default: where its value holds `Undefined`, that member is replaced by the default's value, evaluated in the
callee's own env (a default may name another parameter). Where its value is open, the default's value is joined in: the caller may
have passed nothing.

## 4. The key

`Key.params[i] = admit(g, i, value)`:

```
admit(g, i, v)
    flow(g).params[i] is not capable, or is a rest parameter   → unknown
    v has more than max_set members                            → unknown   (counted)
    otherwise                                                   → v
then: unknown slots at the end are dropped
```

**The canonical form and the digest** (S-TC3's function, used for real for the first time):

```rust
#[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
enum Canon { Unknown, Fn(StableDef), Anon(StableDef, u32), Undefined, Set { known: Vec<Canon>, open: bool } }
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

**Binding** is by position, against `flow(g).params`. `new C(a, b)` binds to the constructor that runs (`init`). A tagged template, a
decorator and an accessor pass nothing in TC2 (every slot unknown). A JSX element passes nothing until TC4.

## 6. What a real key forces

Both are in [03](03-design.md) §10; both are built here because TC2 is the first phase that needs them.

**Counters per def.** `TypeScript::note*` are called from `walk::body`, once per walk. They move to a map keyed by the def's id, filled
the first time a def is walked in a request, and summed by `finish`. A test: a function with one `uncovered_bind`, reached under two
keys, counts 1.

**A request that starts from a context** (Q-TC3). `Session` keeps `HashMap<ContextId, CanonKey>` for the contexts of the graphs it
answered, cleared by `reload`. `Session::graph_from(&ContextId, budget, cancel)` turns the canonical key back into a `Key<'p>` (each
`StableDef` is looked up as an entry is) and starts the solver there. A digest it does not have, or a def that is gone: the open key,
and `context_not_found` in `Stats.language`. The test is the one that motivates it: a tree cut at depth 1 and continued from each cut
node equals the tree asked for whole.

Until you answer Q-TC3 the method exists and the driver does not call it; a tree continued the old way is correct and less precise.

## 7. Core: two small changes

Each gets a short note in `docs/02-tree-rules/` first, as passed edges did, and keeps every language green.

| Change | Why now | Shape |
|---|---|---|
| **the scenario level is per scenario** ([`04-rust/02-architecture.md`](../../04-rust/02-architecture.md) §9, row 2) | TC2 makes one scenario precise, not all | `analyzer_testkit::scenarios::run` takes a function from the scenario's name to its `Level`; a language lists the scenarios it is precise for |
| **a cut for "too many contexts of one function"** ([03](03-design.md) §12; Rust's design §7 leaves the same choice open) | a real key can multiply contexts | `Limits::max_contexts_per_function: Option<u32>` (`None` by default, so flat languages do not change), a `Cut` value for it, counted like the others |

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
is N6's answer.

## 9. Fixtures and tests

`tests/fixtures/context/`, one folder each, a row of the precise policy table each:

`callback-by-name` · `callback-two-callers` · `callback-passed-through` · `callback-either` · `callback-default` ·
`callback-not-passed` (`cb?.()` with no argument: no edge) · `callback-method` · `callback-anonymous` · `callback-returned-arrow`
(line h) · `callback-recursive` (R1) · `callback-mutual` (R2) · `callback-spread` (`run(...args)`: unknown) · `callback-rest` ·
`callback-reassigned` (open: the handler **and** the engine's answer) · `callback-not-capable` (a `string` parameter: one context) ·
`callback-entry` (asked at the callee: today's tree) · `callback-javascript` (the same in a `.js` file, untyped).

| Test | |
|---|---|
| the harness's three properties on every fixture | [03](03-design.md) rules 1, 4, 5, 7 |
| the policy row `callback-param` (the default table) | **unchanged**: it is the default level's answer |
| a new shared scenario, `scenarios/callback-argument/typescript`, with `expected.default.tree` and `expected.precise.tree` | TypeScript is precise for it; the other languages' folders are theirs to add |
| digests: the same key from two entries, in two processes | equal |
| `contexts_per_function` set to 2 on `callback-two-callers` with three callers | the third is `Incomplete`, counted, and the first two are whole |
| a tree cut and continued (§6) | equals the whole tree |
| the four projects | the per-site measure before and after; contexts, walks, time |

## 10. Steps

| # | Do | Proof |
|---|---|---|
| 1 | the two core notes; core: the level per scenario, the per-function cut | core green; every language's tests green; flat graphs byte-identical |
| 2 | `sem/flow.rs`: `Param`, the `Param` table | the tour's table snapshot changes as expected |
| 3 | `value.rs`: `Fn`, `Anon`, `Undefined`, `join`; `eval`: `Param`, `Def`, `Anon`, `OneOf`, defaults | unit tests on hand-written tables |
| 4 | `key.rs`: `params`, `admit`, the canonical form; `key_of` at every project target | `callback-two-callers`: two contexts, two digests, still no new edge |
| 5 | `resolve_from_env` answers at open sites | the fixtures flip; lines a, b, h, i of the tour |
| 6 | counters per def | the two-keys counter test |
| 7 | `Session::graph_from` | the cut-and-continue test |
| 8 | relevance | equal edges, fewer contexts |
| 9 | the four projects | `ENGINE.md` |
| 10 | (mine) `policy.md` rows, `ENGINE.md`, this folder's forecast corrected | reviewed |

After step 9 the layer can be tagged for the first time, off by default.

Next: [`07-tc3-variables-and-captures.md`](07-tc3-variables-and-captures.md)
