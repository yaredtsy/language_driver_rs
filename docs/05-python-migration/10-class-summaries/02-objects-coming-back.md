# 05.10.02 Objects coming back (CS2)

```
objects coming back
├── 1. what is wrong
├── 2. pylspt's decisions, kept
├── 3. what this repo changes in them
├── 4. the design here
├── 5. the fixtures
├── 6. the commits
└── 7. to check while building
```

From pylspt's `04-class-summaries/02-objects-coming-back.md`, with the two-phase seam of its step 4 built here (§4.3; README §5 rule
2). **Not built.** Read against the code at `e55ab37`.

## 1. What is wrong

```python
def make(h):
    return Pipeline(h)

def via_factory():
    make(JsonHandler()).run(1)     # Python: make > Pipeline, then Pipeline.run > JsonHandler.process
```

Summary mode today (`unverified/03-classes/python/state.py.wire`): `make` and `Pipeline` under it, then **nothing**: no
`Pipeline.run`. `make`'s walk returns an object, and `finish_round` turns every object into `Unknown` (`summary.rs`, `plain`: the arm
`_ => PlainVal::Unknown`). The caller's `.run` is then a read on `Unknown`.

The same gap takes out `special.py main`: `Registry.create` is a classmethod that returns `cls(…)`, so `r.active` and `r.get_h` have no
receiver (the first two lines of the gap test).

## 2. pylspt's decisions, kept

| # | Decision | Why |
|---|---|---|
| 1 | A returned object is **yours at this path** (`Ref(Path)`), **new** (`Obj(j)`, described in `objects`), or **without identity** (`Plain(Opaque(class))`) | identity: `ident(b)` must give back `b` itself, or a write through the result lands on a copy. The path is the handshake: the callee knows where among its inputs the object was, the caller knows which object that was |
| 2 | A new object carries its final state, and only if it escapes: every `New` object reachable from the returns (and, step 3, from effect values) | an object built and dropped inside the call is nobody's business. Numbered in the order `finish` reaches them, so equal walks give equal summaries |
| 3 | The caller resolves every `SVal` into its own walk: `Ref` by following the path from the call's own arguments, `Obj(j)` as a new origin keyed by the call site and `j` | two calls of `make_box` from two sites give two objects, so their handlers stay apart |
| 4 | Every path is read from the env **as it was before the call** (`before`) | a `Ref` means "the object at this path when the call started". Step 4 is why |

## 3. What this repo changes in them

| pylspt | Here | Why |
|---|---|---|
| `OriginKey::New { site, act, made }` in step 2, `t` and `combo` in step 6 | **`t` (the callee's index at the site) in this step**; `combo` stays in step 6 | a site with two callees is already possible today (`h.process()` on two classes). Without `t`, both callees' `Obj(0)` share one origin and their attributes mix: a wrong edge (README §5 rule 4) |
| apply a summary in one place | **two phases from the start**: `resolve(&Env) -> Plan`, then `apply(Plan, &mut Env)` (§4.3) | the seam is cheaper to build before there are effects. Step 3 adds effects to the plan; step 4 is then a fixture, not a fix |
| the walk's `Unknown` is the end | where the walk holds `Unknown` and the engine's type is exact, the engine answers (D27, `or_engine_callee`) | this step makes the walk hold the object instead. A site D27 answered (basis `Type`) may now be answered by the walk (basis `Path`): every such move in a snapshot is reviewed, not accepted |
| `Plain(Opaque(class))` becomes `Instance { class, OPAQUE }` | the same: an object of a project class with no state, as `Walk::lift` makes it from the engine | one meaning of `OPAQUE` |

## 4. The design here

### 4.1 Summary values

```rust
// interp/summary.rs
pub(crate) enum SVal<'db> {
    Plain(PlainVal<'db>),
    Ref(Path),                                            // the caller's object at this path, when the call started
    Obj(u16),                                             // objects[j], built in this call
    Bound { func: Definition<'db>, recv: Box<SVal<'db>> },
    Closure { func, free, defaults },                     // as today
    Back(u8),                                             // as today
}
pub(crate) enum PlainVal<'db> { /* as today */ Opaque(Definition<'db>) }

pub(crate) struct NewObject<'db> {
    pub class: Definition<'db>,
    pub site: SiteId,                                     // where it was built, inside this function
    pub attrs: Vec<(Name, SVals<'db>)>,                   // its final state, sorted by name
}

pub(crate) struct FunctionResult<'db> {
    pub calls, pub returns,
    pub objects: Vec<NewObject<'db>>,                     // ← new
    pub effects, pub effects_dropped,                     // the spike's, until step 3
    pub status,
}
```

**Rust note: rule 2 as a type.** `SVal` gains no variant that can hold an `OriginId`, an `ActivationId` or a `ValueId`. `Ref` holds a
`Path` (a root and steps: data, not an id), `Obj` an index into the summary's own list. A summary that leaked a walk-local id would not
compile, which is the strongest test there is.

### 4.2 Finishing: the conversion table

`to_svals` (`summary.rs`) gains the object arms. The end state is the joined exit env, which `walk_context` stores at
`activations[frame.act]` (the spike's `init_effects` reads it there too).

| Value in the walk | In the summary |
|---|---|
| `Instance { origin }`, `OriginKey::Entry { path }` | `Ref(path)` |
| `Instance { origin }`, `OriginKey::New { .. }` | `Obj(j)`; the object is numbered and described |
| `Instance { origin: OPAQUE }` | `Plain(Opaque(class))` |
| `BoundMethod { func, receiver }` | `Bound { func, recv: <the receiver, converted> }` |
| anything else | as today |

Numbering: a small `ObjectNumbering` (origin → `j`, first reached first). `describe_objects` walks it as a queue: describing object `j`'s
attributes may reach object `j + 1`. Returns are converted first, so `j` depends only on the walk.

### 4.3 Applying: resolve, then apply

```rust
// interp/call.rs (new module-private items)
struct CallSite<'a, 'db> {                       // what resolving needs from the call
    site: TextRange,
    t: u8,                                       // the callee's index at the site
    bound: &'a Bound<'db>,                       // the slots: Param(i) is bound.slots[i]
    enclosing: Option<ActivationId>,             // a closure's run: Free(i) and Default(i)
}

struct Plan<'db> {
    returns: Values<'db>,
    objects: Vec<(OriginId, Vec<(Name, Values<'db>)>)>,
    // step 3 adds: effects
}

fn resolve(&mut self, res: &FunctionResult<'db>, call: &CallSite<'_, 'db>, before: &Env<'db>) -> Plan<'db>;
fn apply(&mut self, plan: Plan<'db>, env: &mut Env<'db>) -> Values<'db>;   // writes, then gives the returns
```

- `Ref(path)`: start at the root (`Param(i)` is `call.bound.slots[i]`; `Free(i)` and `Default(i)` read the enclosing run), then each step
  in `before`: `Attr` reads the object's **state only** (`before.attrs`), never a property or the class side (phase 1 runs nothing);
  `Recv` takes a bound method's receiver; `Free` and `Default` read a closure's run. A path that does not resolve gives `Unknown` (what `plain` gave
  before a summary could name an object), counted `ref_unresolved` (§7 claim 3 says it should not happen in this step). Never an empty
  `Values`: an empty value means "Python never makes this call" (`EvaluatedArgs::has_empty`), and would drop the edges D27 finds.
- `Obj(j)`: a new origin `New { site, act, t, made: Made::Obj(j) }`, class `objects[j].class`; its attributes resolved against `before`
  and put in the plan.
- `apply` writes each new object's attributes (strong: a new object has nothing to keep), then returns the values.

`resolve(&res.returns)` today has two callers, `call_project` and the replay of `DeferredOp::Apply` (`defer.rs`). Both go through
`resolve` + `apply`. **`DeferredOp::Apply { ctx }` gains what `CallSite` needs** (the site, `t`, the bound slots, the enclosing run):
without them a replay cannot follow a `Ref`. pylspt adds these fields in its step 8; here they are needed now.

`call_project` takes `env: &Env` today. It becomes `&mut Env` for `apply`: the write is the caller's.

Built as `CallSite`, `Plan`, `resolve` and `apply` in `summary.rs`, next to `resolve_in`. Three things the text above did not say:

- **A replay has no env to write into.** The env of a deferred operation is dropped after it, so `apply` also records a new object's attributes in `Walk.replay_attrs` while replaying; `replay_env` puts them back in the env of every later operation, and `describe_objects` reads them for the walk's own summary. A new object has one origin per call site, callee and `j`, so each round writes the same keys again.
- **`t` counts the project callees of a site** (functions, closures, then methods, each in definition order), so `h.make()` on `A | B` gives `A.make`'s `Box` and `B.make`'s `Box` two origins.
- **`set_current` compares `objects` as well as `returns`** (`same_objects`), so a cycle whose returned object's attributes change between rounds does not stop early. No fixture's walk or `reuse_hits` count moved because of it (measured: the blessed snapshots are identical with and without it); no fixture needs it yet.

**Rust note: the borrow checker enforces phase 1.** `resolve` takes `before: &Env`, a shared borrow. It cannot write to the env even by
mistake; only `apply`, which takes `&mut Env`, can. `before` is a clone taken once per call expression, after the arguments are evaluated:
the env is built on `rpds` persistent maps, so the clone copies a pointer, not the maps.

## 5. The fixtures

| Fixture | Entry | Expected | Today in summary mode |
|---|---|---|---|
| `04-class-summaries/python/coming_back.py` (pylspt's; inline passes since CS0) | `identity` | `c = ident(b)` is `b`: `b.run()` shows `JsonHandler.process` only | `Box.run > CsvHandler.process`: the write through `c` is lost, the handler is the one `b` had |
| | `built_inside` | `make_json().run()` shows `JsonHandler.process` | no `Box.run`: the returned object is lost |
| | `built_with_arg` | `make_box(CsvHandler()).run()` shows `CsvHandler.process` | no `Box.run` |
| | `two_from_one_site` | `a.run()` Json only, `b.run()` Csv only | no `Box.run` at all |
| `03-classes/python/state.py` | `via_factory` | `Pipeline.run > JsonHandler.process` | `make > Pipeline`, no `run` |
| `03-classes/python/special.py` | `main` | `r.active`, `r.get_h` (getters) under `main` | missing (gap test, lines 1–2) |
| **new**, `04-class-summaries/python/two_callees.py` | `main` | `h = A() if c else B()`; `h.make().run()` where `A.make` and `B.make` each return their own `Box` with their own handler: each `run` sees only its own handler | without `t`, a mix |

`state.py` and `special.py` still need effects (step 3) for their other entries: they stay inline-only `check` until step 3's gate.

After CS2b `via_factory` has `Pipeline.run` and not yet `JsonHandler.process` under it: that needs `__init__`'s `self.handler = h`, an effect (CS3). The same holds for the `run` and `go` children of `isolation.py one_site` and `split.py main`, and for `special.py main`'s `r.active.handle()`.

`two_callees.py` is checked by its two blocks and by a graph test. The tree cannot tell the mix from the answer: core's renderer joins the siblings of one def at a site, so summary mode draws both handlers under one `Box.run` (`# expect summary:`), where inline mode draws two (`# expect inline:`). `p04_02_two_callees_each_run_sees_only_its_own_handler` reads the graph: two `Box.run` contexts, each calling only its own handler.

## 6. The commits

### CS2a. Objects in summaries

| | |
|---|---|
| Change | §4.1, §4.2. `resolve_in` handles the new values only as far as `Plain(Opaque)` and `Bound`; `Ref` and `Obj` still give `Unknown`, as every object did (counted `ref_unresolved`, so the gap is visible; not an empty `Values`, which means "never called" and would drop D27's edges). The counter is in `Stats.language`, so every graph snapshot with it above 0 moves its `stats:` line; the fixtures that have it are CS2b's list |
| Failing tests | unit tests in `summary.rs`, hand-built from small programs through the harness: `ident`'s returns are `[Ref(Param(0))]`; `make_json`'s are `[Obj(0)]` with `objects[0]` a `Box` whose `handler` is `[Obj(1)]`; two runs of one walk give `==` summaries |
| Proof | `cargo test -p python-analysis`; **no snapshot changes** (nothing resolves the new values yet) |

### CS2b. Resolve, then apply

| | |
|---|---|
| Change | §4.3, `t` in `OriginKey::New`, `DeferredOp::Apply`'s new fields |
| Failing tests | `p04_02_coming_back`: `check_both("04-class-summaries/python/coming_back.py")`, red because `Ref` and `Obj` resolve to nothing. `two_callees.py` with `check_both` |
| Snapshots | the moves expected: `unverified/03-classes/python/state.py.wire` gains `Pipeline.run` under `via_factory` (not its `JsonHandler.process` child: that is CS3); `special.py.wire` gains the getters, `fire` and the second `make`; `isolation.py` and `split.py` gain `Pipeline.run` and `Job.go`. `coming_back.py`'s wire moves to the verified snapshots. Every `.graph` snapshot loses its `ref_unresolved` line, and a context id changes where a key held `Unknown` for an object it now holds. **Any verified snapshot that moves is read and explained in the report** (the likely cause: a D27 `Type` edge becoming a `Path` edge) |
| Gap test | the two `special.py` lines go (if their getters now run); nothing is added |
| Proof | `cargo test` in `languages/python`; clippy on `python-analysis`; the three real projects (README §5 rule 10): the golden either still holds, or every difference is listed and explained before it is regenerated |

## 7. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | `walk_context` leaves the joined exit env at `activations[frame.act]` for every walk, so `finish` can read final attributes there | CS2a. **Holds**, with one case that does not matter. `walk_context` joins `frame.exits` (every `return` and the fall off the end) into `activations[frame.act]`; nothing after it writes that slot (a replay runs operations, not statements). Test `the_state_a_summary_reads_is_every_exit_joined`: two `return`s that left a `JsonHandler` and a `CsvHandler` on one object give both on its attribute. When every path raises there is no exit, the join is `None`, and the slot keeps the env at the start of the last statement walked; such a walk returns nothing, so `finish` reads no attribute from it |
| 2 | Objects reached only through a closure's free value (returned closure that captured a new object) are numbered and described too | CS2a, a unit test. **Holds.** `an_object_only_a_returned_closure_holds_is_numbered_and_described` (`closure_holds_object.py`): `make_getter` returns `Closure { get, free: [b: [Obj(0)]] }`, and `objects` holds the `Box` (`handler: [Obj(1)]`) and the `Handler`. The closure's free value reaches `to_svals` through `lookup_in`, which reads the same exit env |
| 3 | In this step every `Ref` path is at most `fp_depth` steps and resolves in the caller's `before` state: the key took those attributes from it. `ref_unresolved` stays 0 on every fixture | CS2b. **Holds.** The `ref_unresolved` entry CS2a added to eight `.graph` snapshots is gone from all eight, and `tests/api.rs` asserts it absent on `coming_back.py`. On the three real projects it was 3 (Flask `post`) and 14 (FastAPI `include_router`) before and is 0 after; Jarvis never had one |
| 4 | The basis of a call on an `Obj(j)` object is `Path` (`is_built_here` is true for its `New` origin), and on a `Ref` object whatever the caller's object gives | CS2b, a test in `tests/api.rs` on `coming_back.py`. **Holds.** `a_method_on_an_object_a_callee_built_rests_on_the_path`: `built_inside` and `built_with_arg` call `Box.run` with `Path`. A `Ref` object is the caller's own, so its edge has the basis of the caller's object (`identity`: `b` is built there, `Path`) |
| 5 | No `debug_assert` fires in `fp_one`'s pending arm on the whole suite | CS2b. **Holds.** The suite runs with debug assertions on; none fired, and `reaches_pending` still covers every operation `fp_one` reads |

Next: [`03-effects.md`](03-effects.md)
