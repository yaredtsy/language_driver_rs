# 05.10.03 Effects (CS3)

```
effects
├── 1. what is wrong, and what the spike showed
├── 2. pylspt's decisions, kept
├── 3. what this repo changes in them
├── 4. the design here
├── 5. the fixtures
├── 6. the commits
└── 7. to check while building
```

From pylspt's `04-class-summaries/03-effects.md`. **A spike of it is in the tree, off** (`54b513e`); this step replaces it. Read against
the code at `e55ab37`. Needs CS2.

## 1. What is wrong, and what the spike showed

```python
class Pipeline:
    def __init__(self, handler): self.handler = handler
    def run(self): self.handler.process()
def install(p, h): p.handler = h

def through_a_function():
    p = Pipeline(CsvHandler())
    install(p, JsonHandler())
    p.run()                       # Python: JsonHandler.process only
```

A write a callee makes on an object its caller can see never reaches the caller. Today `Pipeline.run` has no child.

**The spike** (`interp::Options::constructor_effects`, off; `init_effects` in `summary.rs`, `apply_effects` in `call.rs`, a method-entry
prologue in `mod.rs`) carries **one kind** of write: an `__init__` onto `self`. With it on, the class programs go from 21 failing entries
to 15 and FastAPI's `include_router` from 436 nodes to 970. It also shows why it cannot go on alone. In `through_a_function` it carries
`__init__`'s `handler = CsvHandler` and not `install`'s `handler = JsonHandler`, so the tree shows **`CsvHandler.process`: a call Python
never makes, unmarked** (rule 8). The spike's own commit message names this as "the gate before the option goes on by default".

What the spike does differently from pylspt's design, and what this step does with it:

| Spike | pylspt | This step |
|---|---|---|
| only `__init__`, only onto `self` | every write onto an input object | pylspt's |
| a write is found by comparing the end state with the start (`start_attrs`) | a `written` set, `Must` or `May` | pylspt's: the diff misses `p.h = p.h` and cannot say "on every path" |
| `EffectValue::Param(k) \| Plain` | `SVals` (step 2 makes them say objects and closures) | pylspt's: `Param(k)` is `Ref(Param(k))` |
| always joined into the object | replaced when strong, one target, one callee | pylspt's (§2 decision 4) |
| a write whose values are all `Unknown` is left out, so a read asks the engine | the write is applied | **a new rule** (§3, row 2) |
| a method entry starts on an object `__init__` ran on, nothing else | the one `prologue` of inline mode, in both modes | pylspt's (§3, row 3) |
| counters `effects_applied`, `effects_dropped`, `prologue_walks` | `Stats` fields | `effects_applied` and `prologue_walks` kept; `effects_dropped` goes (everything has a value after CS2) |

## 2. pylspt's decisions, kept

| # | Decision | Why |
|---|---|---|
| 1 | A walk records every attribute it writes: `Must` when written strongly on every path that reaches here, `May` otherwise. `Env::join` keeps `Must` only where every arm has it | the record is of the write, not of a changed value |
| 2 | The summary reports every write onto an **input** object (`OriginKey::Entry { path }`) as an `Effect { target: Path, attr, value: SVals, strong }`, sorted by (target, attr). A write onto a `New` object is not an effect: the object is described whole in `objects` (step 2) | relative to the inputs, so one summary serves every caller |
| 3 | `__init__` is not special: a constructor keys `__init__` with `self` as parameter 0, so `self.handler = handler` is the effect `Param(0).handler = [Ref(Param(1))]`, strong | one rule. A method that writes `self` (`b.set(h)`) is the same effect |
| 4 | The caller applies each effect to its own objects. It **replaces** the old value only if the effect is strong, the target resolved to exactly one object, and the site had one callee (`strong_ok`; step 6 adds "one combination"). Otherwise it is joined. Each write records `Must`/`May` in the caller's own `written`, so `outer_install` reports `install`'s effect as its own | a site with two callees is two alternatives: Python ran one of them |
| 5 | Targets that resolve to an object with no state (`OPAQUE`), `Unknown` or nothing are dropped | as a local write onto them is today (`isolation.py` `global_write`, a known limit) |

## 3. What this repo changes in them

| # | pylspt | Here | Why |
|---|---|---|---|
| 1 | effects are a new thing | they **replace the spike**: `init_effects`, `EffectValue`, `apply_effects`, `Walk.inputs`, `Walk.start_attrs` and the `mod.rs` prologue are deleted in this step | one mechanism (README §5 rule 5) |
| 2 | a strong write whose values are all `Unknown` replaces the old value with `[Unknown]` | a strong **effect** whose values are all `Unknown` **removes** the attribute from the caller's object; a weak one is joined as any weak write | a read of an attribute that holds `[Unknown]` returns it (`member`, step 1 of three); a read of an absent one asks the engine's declared type (step 3), as D27 does for a callee. Leaving the write out, as the spike does, keeps the **old** value: a stale handler, the wrong edge of §1 again. Removing it is what the spike's rule meant, said safely. Whether a *local* strong write of `Unknown` should do the same is §7 claim 6 |
| 3 | the summary-mode prologue is `inline_tree`'s `prologue` | the same: `Walk::prologue` (`entry.rs`) is called by `solve` too, and the spike's method-only prologue in `mod.rs` goes. The **entry rule** stays as it is: only `__init__` is part of a method entry's start, with its defaults | `outer.inner` and `Svc.build.step` need the parents run, and a closure's free values read the parent's run. `Job.run -> entries.B.go` stays excluded by the entry rule (the gap test says why) |
| 4 | the fixpoint compares effects and objects in step 8 | **in this step**: `Slot::set_current` (`solver.rs`) compares `returns`, `effects` and `objects` (all sorted, so `==` after `same_svals` on the values) | today it compares `returns` only. A recursive group whose writes change while its returns do not would stop one round early, with no mark |
| 5 | a recursive call's effects are applied in the fixpoint, weakly (step 8) | **until CS8, dropped and counted**: a replayed `Apply` and a construct whose `__init__` is `Pending` apply no effect, and add to `Stats.language["effects_in_cycle"]` | rule 8: not carried, so counted. CS8 carries them |
| 6 | there is no flag | the spike's internal flag becomes `interp::Options::effects` (renamed): **on in every fixture run, off on the request path** (`lib.rs`, `Options::solver`) until the owner's yes after CS4 (README §7 question 2) | the wire changes when it goes on; the public `Options` has no such field, so turning it on is one line in `lib.rs` |
| 7 | — | `Env::join` treats a name one arm never wrote as nothing, not `Unknown` (the known limit the gap test describes, "left until constructor effects make an absent attribute the exception") | revisited here, at the gate: measure what marking it changes (§7 claim 7). Its own commit if it is done |

## 4. The design here

```rust
// interp/env.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Write { Must, May }

pub(crate) struct Env<'db> {
    /* locals, attrs, scope, enclosing, flat: as today */
    pub written: rpds::HashTrieMap<(OriginId, Name), Write>,   // ← new
}
```

- `bind_attr(…, strong)` records `Must` when strong, else `May`; a `May` on top of a `Must` stays `Must`.
- `join_attr` (rebuilding from a key) records nothing: it is not a write.
- `Env::join`: `Must` only where every arm reaching the join has `Must`; anything else `May`.

```rust
// interp/summary.rs
pub(crate) struct Effect<'db> {
    pub target: Path,          // the input object written to, as it was when the call started
    pub attr: Name,
    pub value: SVals<'db>,
    pub strong: bool,          // written on every path that exits normally
}
```

`finish_round` reads the end env's `written`: one `Effect` per `(origin, attr)` whose origin is `Entry { path }`, its value the end value
converted by step 2's table, `strong` from `Must`. An effect with an empty value is not emitted, **except** an attribute that is written
and absent at the end (a `forget_attr` of §3 row 2): it is the effect `[Plain(Unknown)]`, strong, so the next caller up forgets it too
and no caller keeps a stale value. New objects reachable from effect values join `objects` (numbered after the returns' objects).

`Plan` (step 2) gains `effects: Vec<(SmallVec<[OriginId; 2]>, Name, Values<'db>, bool)>`: targets resolved by following the path in
`before`, values resolved against `before`, the effect's `strong`. `apply` writes new objects' attributes first, then the effects:

```rust
for (targets, attr, vals, strong) in plan.effects {
    let targets: SmallVec<[OriginId; 2]> = targets.into_iter().filter(|o| o.holds_state()).collect();
    let strong = strong && targets.len() == 1 && strong_ok;
    for t in targets {
        if strong && vals.iter().all(|v| matches!(v, AbstractValue::Unknown)) {
            env.forget_attr(t, &attr);            // new: removes the value, records `Must` (it is a write); §3 row 2
        } else {
            env.bind_attr(t, &attr, vals.clone(), strong);
        }
        sol.stats.effects_applied += 1;
    }
}
```

`strong_ok` is "this site has one project callee". `call_values` knows it: the number of project targets pushed for the site.

## 5. The fixtures

| Fixture | Entry | Expected |
|---|---|---|
| `04-class-summaries/python/effects.py` (CS0: inline passes; summary fails all five: `Pipeline.run` with no child) | `ctor` | `Pipeline.run > JsonHandler.process` |
| | `installed` | `JsonHandler.process` only (the strong effect replaces `CsvHandler`) |
| | `maybe_installed` | both (weak) |
| | `fresh_installed` | `JsonHandler.process` only (`Obj(0)` as an effect value) |
| | `through_two` | `JsonHandler.process` only (`outer_install` reports `install`'s effect as its own) |
| `03-classes/python/*.py`, the six inline-only programs | every entry | the inline tree, through `check_both` |
| **new**, `04-class-summaries/python/forget.py` | `main` | `class P: def __init__(self, h: Handler): self.h = h`; `def wipe(p): p.h = untyped()`; `p = P(Json()); wipe(p); p.h.process()`: the edge goes to `Handler.process` with basis `Type` (the engine's declared type), **not** to `Json.process` (§3 row 2). Entry `main2`: the same through `def wipe_twice(p): wipe(p)`, one level
further up: the same answer (§4, the forgotten attribute travels as `[Unknown]`) |
| **new**, `04-class-summaries/python/cycle_effect.py` | `main` | a recursive method that writes `self.x` only in its recursive branch: until CS8, `effects_in_cycle` is 1 and nothing wrong is drawn |

## 6. The commits

### CS3a. The written set

| | |
|---|---|
| Change | `Write`, `Env.written`, recording in `bind_attr`, the join rule. Nothing reads it yet |
| Failing tests | unit tests in `env.rs`: a strong write is `Must`; a weak one `May`; `May` over `Must` stays `Must`; a join of `Must` and nothing is `May`; a join of two `Must` is `Must`; `join_attr` records nothing |
| Proof | `cargo test -p python-analysis`; **no snapshot changes** |

### CS3b. Effects, replacing the spike

| | |
|---|---|
| Change | §4; §3 rows 1, 2, 5, 6. The spike's pieces deleted; the flag renamed and on in the harness |
| Failing tests | `p04_03_effects`: `check_both("04-class-summaries/python/effects.py")`. `forget.py`, `cycle_effect.py`. A unit test: `install`'s summary has exactly one effect, `Param(0).handler = [Ref(Param(1))]`, strong; `maybe`'s is weak and its value holds `Ref(Param(0).handler)` |
| Proof | `cargo test` in `languages/python`. The request path is unchanged (flag off): **no API test and no verified snapshot changes** |

### CS3c. One prologue

| | |
|---|---|
| Change | §3 row 3 |
| Failing tests | `p03_08_entries` with `check_both`, red for `outer.inner`, `Svc.build.step`, `Svc.run`, `Child.go`, `Job.run`, `fire`. `p03_08_prologue_runs_the_parents` run in summary mode too: the same counts |
| Proof | as CS3b |

### CS3d. The fixpoint compares what a caller reads

| | |
|---|---|
| Change | §3 row 4 |
| Failing tests | a recursive group whose returns are equal in rounds 0 and 1 and whose effects differ: today it stops after round 1 with the round-0 effects. pylspt's `recursion_objects.py` `visitor` is the larger case (step 8) |
| Proof | as CS3b; `02-function-summaries` snapshots unchanged |

### CS3e. The gate

| | |
|---|---|
| Change | `state.py`, `isolation.py`, `writes.py`, `special.py`, `chains.py`, `entries.py`: `check` → `check_both`. Their `unverified/` snapshots move to the verified folder, read line by line. `constructor_effects_spike` is replaced by the gate it was a sketch of. The gap test loses every line this step fixed |
| Failing tests | the six `check_both` calls, red until CS3b–d are in |
| Expected | every `03-classes/` entry equal in both modes, except rule 7's join (`state.py either`: summary draws one `Pipeline.run` for two objects, inline two) and the excluded `Job.run -> B.go`. `extra` 0 otherwise |
| Proof | `cargo test` in `languages/python`, then the three real projects **with the flag on** (a test-only run): nodes, contexts, walks, unresolved, time, in `languages/python/ENGINE.md` |

## 7. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | The spike's comment "the same site builds the object again in a loop" (why it joins): the walker walks a loop body once (`walk.rs`, `Stmt::For` / `Stmt::While`), so one walk evaluates a site once. Find the case the comment meant, or show there is none | CS3b |
| 2 | `call_values` can tell `strong_ok` (one project callee at the site) before it applies the first callee's effects | CS3b |
| 3 | A write onto a closure's free object (`Root::Free`) is reported and applied through the enclosing run | CS3b, a unit test |
| 4 | Effects sort with a derived `Ord` on `Path` (README §9 claim 4) | CS3b |
| 5 | The FastAPI numbers of the spike (436 → 970, 0 extra) still hold with general effects, and `extra` stays 0 | CS3e |
| 6 | A *local* strong write of all-`Unknown` (`self.h = untyped()` in the same walk) writes `[Unknown]` today, which hides the engine's declared type from the next read. Measure what treating it like an effect (§3 row 2) changes on the fixtures and the three projects; if it is better, its own commit | after CS3e |
| 7 | `Env::join` marking a one-armed attribute write as `Unknown` (§3 row 7): how many `Unresolved` sites it adds on the fixtures and projects now that effects fill most attributes | after CS3e |

Next: [`04-resolve-then-write.md`](04-resolve-then-write.md)
