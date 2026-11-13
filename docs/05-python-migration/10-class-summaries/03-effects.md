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

From pylspt's `04-class-summaries/03-effects.md`. **A spike of it was in the tree, off** (`54b513e`); CS3b replaced it. Read against
the code at `e55ab37`, and again after CS2 (`b2bc150`, 2026-11-13): the rows marked **after CS2** say what CS2 already built or changed.
Needs CS2.

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
| 3 | the summary-mode prologue is `inline_tree`'s `prologue` | the same: `Walk::prologue` (`entry.rs`) is called by `solve` too, and the spike's method-only prologue in `mod.rs` goes. The **entry rule** stays as it is: only `__init__` is part of a method entry's start, with its defaults. **After CS2:** `solve` calls the prologue only when `Options::effects` is on; off, a summary-mode entry starts as it does today (a classmethod's `cls`, nothing else), so the request path does not move until the owner's yes (row 6). Without effects a prologue's object holds nothing `__init__` wrote, so it would only add walks | `outer.inner` and `Svc.build.step` need the parents run, and a closure's free values read the parent's run. `Job.run -> entries.B.go` stays excluded by the entry rule (the gap test says why) |
| 4 | the fixpoint compares effects and objects in step 8 | **in this step**: `Slot::set_current` (`solver.rs`) compares `returns`, `effects` and `objects` (all sorted, so `==` after `same_svals` on the values). **After CS2:** `returns` and `objects` are compared already (`same_objects`, CS2b); this step adds `effects` | before CS2 it compared `returns` only. A recursive group whose writes change while its returns do not would stop one round early, with no mark |
| 5 | a recursive call's effects are applied in the fixpoint, weakly (step 8) | **until CS8, dropped and counted**: a replayed `Apply` and a construct whose `__init__` is `Pending` apply no effect, and add to `Stats.language["effects_in_cycle"]` | rule 8: not carried, so counted. CS8 carries them |
| 6 | there is no flag | the spike's internal flag becomes `interp::Options::effects` (renamed): **on in every fixture run, off on the request path** (`lib.rs`, `Options::solver`) until the owner's yes after CS4 (README §7 question 2). **After CS2:** "every fixture run" is the harness's `summary()` (`fixture.rs`); the snapshots then show effects on, while the request path is proved unchanged by the API tests and the golden | the wire changes when it goes on; the public `Options` has no such field, so turning it on is one line in `lib.rs` |
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

`Plan` (step 2; **after CS2** it is in `summary.rs`, beside `resolve`, `apply` and `CallSite`) gains
`effects: Vec<(SmallVec<[OriginId; 2]>, Name, Values<'db>, bool)>`: targets resolved by following the path in `before` (the same `follow`
a `Ref` uses), values resolved against `before`, the effect's `strong`. `apply` writes new objects' attributes first, then the effects.
In a replay (`DeferredOp::Apply`) the effects are **not** applied, and are counted (§3 row 5); CS2b's `replay_attrs` keeps only new
objects' attributes and does not grow here:

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
| **new**, `04-class-summaries/python/forget.py` | `main` | `class P: def __init__(self, h: Handler): self.h = h`; `def wipe(p): p.h = untyped()`; `p = P(Json()); wipe(p); p.h.process()`: the edge goes to `Handler.process` with basis `Type` (the engine's declared type), **not** to `Json.process` (§3 row 2). Entry `main2`: the same through `def wipe_twice(p): wipe(p)`, one level further up: the same answer (§4, the forgotten attribute travels as `[Unknown]`). Built: `p04_03_forget` checks the edge's basis in the harness (`Type`, effects on); `tests/api.rs` has the same check on the request path, where effects are off and the edge is `Type` for a lesser reason (the constructor's write is not applied, so nothing is stale), so it guards the request path and does not prove the rule. The rule is proved by `p04_03_forget`: with the all-`Unknown` branch of `apply` left out (the spike's rule) it draws `Json.process` |
| **new**, `04-class-summaries/python/closure_effect.py` | `main` | claim 3 of §7: `swap` writes `b.h`, `b` is `main`'s own object `swap` captured; `Json.process` only |
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
| Proof | `cargo test` in `languages/python`. The request path is unchanged (flag off): **no API test changes, and the golden is equal**. **After CS2:** the harness now runs with the flag on, so a verified snapshot *can* move; none is expected (the verified programs either build no object a callee writes, or are `coming_back.py` / `two_callees.py`, whose callees write only new objects); any move is read and explained before it is blessed. The unverified `03-classes/` and `04-class-summaries/` wires are expected to gain the handler children (`state.py via_factory` gains `JsonHandler.process` under `Pipeline.run`) |

### CS3c. One prologue

| | |
|---|---|
| Change | §3 row 3 |
| Failing tests | `p03_08_entries` with `check_both`, red for `outer.inner`, `Svc.build.step`, `Svc.run`, `Child.go`, `Job.run`, `fire`. `p03_08_prologue_runs_the_parents` run in summary mode too: the same counts |
| Proof | as CS3b |

### CS3d. The fixpoint compares what a caller reads

| | |
|---|---|
| Change | §3 row 4: `effects` joins the comparison (`returns` and `objects` are compared since CS2b) |
| Failing tests | a recursive group whose returns and objects are equal in rounds 0 and 1 and whose effects differ: today it stops after round 1 with the round-0 effects. pylspt's `recursion_objects.py` `visitor` is the larger case (step 8). **Built as** a unit test of `Slot::set_current` (`solver.rs`, `a_round_that_changes_only_the_effects_is_a_change`): two summaries with equal `returns` and `objects` and other effects. No program was found where only the effects move between rounds and the end differs, because an effect's value comes from the same pending returns, and a recursive call's own effects are not carried until CS8; the fixpoint test is the guard for that step |
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
| 1 | The spike's comment "the same site builds the object again in a loop" (why it joins): the walker walks a loop body once (`walk.rs`, `Stmt::For` / `Stmt::While`), so one walk evaluates a site once. Find the case the comment meant, or show there is none | CS3b. **No loop case.** `For`, `While` and `Try` each walk their body once, so a site is evaluated once per run; the one repeat is a deferred replay, which writes the same `(origin, attr)` keys again and now applies no effect. The case that does need a join is **one site, two classes** (`K = A if c else B; K()`): both constructs are `OriginKey::New { site, t: 0, Made::Here }`, one origin, and a strong replace by the second would drop the first's attributes. `strong_ok` is false there, so both are joined: a scratch program (not committed) draws `Json.process` and `Csv.process` |
| 2 | `call_values` can tell `strong_ok` (one project callee at the site) before it applies the first callee's effects | CS3b. **Holds, one addition.** `targets`, `methods` and `classes` are collected and sorted before the first `call_project` or `construct_summary`, so `strong_ok = targets.len() + methods.len() + classes.len() == 1 && !ask_ty` is known there and passed down (`CallSite.strong_ok`, a parameter of `call_project` and `construct_summary`). **`!ask_ty` is added:** a site with an engine callee next to the project one (`f = install if c else print`) is two alternatives too |
| 3 | A write onto a closure's free object (`Root::Free`) is reported and applied through the enclosing run | CS3b. **Holds.** Unit test `a_write_onto_a_closure_s_free_object_is_reported_on_its_free_root` (`closure_effect.py`: `main.swap` has the one effect `Free(0).h = [Obj(0)]`, strong), and `p04_03_closure_effect` (`check_both`): `main` applies it to its own `b`, so `b.h.process()` is `Json.process` only |
| 4 | Effects sort with a derived `Ord` on `Path` (README §9 claim 4) | CS3b. **Holds, committed.** `#[derive(PartialOrd, Ord)]` on `Path`, `Root` and `Step`; `effects_sort_by_target_then_attribute` (`summary.rs`). `finish_round` sorts the written set by (target, attr) **before** it converts the values, because converting numbers the objects it reaches and a hash map's order is no order (rule 9) |
| 8 | **Found.** A site with a project callee and an engine callee (`ask_ty`) keeps the project callee's writes | CS3b. **Did not hold; fixed in CS3b.1** (`mixed_callee.py`, `p04_03_mixed_callee`: both handlers drawn; no other snapshot moved, the golden is equal). In summary mode `call_values` applies each callee's writes straight into `env`, and `after` stays empty unless `ask_ty`, when it holds only `before.clone()`: `Env::join(after)` then sets `env = before`, which throws the project callee's applied writes away (CS2b's new objects as much as CS3b's effects). Repro (scratch, not committed): `f = install if c else print; f(p, Json()); p.run()` draws `Csv.process` only, and nothing marks the missing `Json.process`. The fix is the one that was made: push `env.clone()` into `after` in summary mode before the `ask_ty` push (the join then weakens `written` to `May`) |
| 9 | **Found.** Every new edge on the unverified wires is a call Python makes | CS3b. **Does not hold for three programs.** Each is the stated target of a later step, and each had a marked gap (`? unknown_callee`, or a missing node) that is now an unmarked wrong edge: `aliases.py same` draws `CsvHandler.process` (Python: `JsonHandler.process`; the key does not say `a` is `b`: CS7); `deep_write.py main` draws `CsvHandler.process` (Python: `JsonHandler.process`; the write is three levels down: CS5); `split.py main` draws `JsonHandler.process -> CsvKey.encode` and `CsvHandler.process -> JsonKey.encode` (Python never pairs them; inline mode draws the same mix: CS6). Effects make the attributes known, and the key does not yet tell these cases apart. Off the request path until the owner's yes (README §7 question 2) |
| 10 | **Found.** What `effects_in_cycle` counts | CS3b. A replayed `Apply` (or a replayed call that reaches `apply`) counts **once per site and callee, and only when the callee has an effect to drop** (`Walk.effects_in_cycle`, a set: every round replays it); a construct whose `__init__` is `Pending` counts **whether or not** `__init__` writes anything, because a running summary cannot be read. So `construct.py` (`Tree.__init__` builds a `Tree`) shows `effects_in_cycle: 1` with no effect lost: the one verified snapshot that moved, stats line only. An effect whose target path leads to nothing in the caller (`follow` is empty) adds to `ref_unresolved` and is dropped |
| 5 | The FastAPI numbers of the spike (436 → 970, 0 extra) still hold with general effects, and `extra` stays 0 | CS3e |
| 6 | A *local* strong write of all-`Unknown` (`self.h = untyped()` in the same walk) writes `[Unknown]` today, which hides the engine's declared type from the next read. Measure what treating it like an effect (§3 row 2) changes on the fixtures and the three projects; if it is better, its own commit | after CS3e |
| 7 | `Env::join` marking a one-armed attribute write as `Unknown` (§3 row 7): how many `Unresolved` sites it adds on the fixtures and projects now that effects fill most attributes | after CS3e |
| 11 | **Found (CS3c).** The one prologue makes every `entries.py` entry right in summary mode | CS3c. **Holds for all but one.** `Child.go` and `outer.inner` pass with `Walk::prologue` in `solve`; `Svc.run`, `Job.run` and `fire` already passed with CS3b's spike prologue (they were not red). **`Svc.build.step` does not:** the prologue is right (`Svc.__init__`, then `build`), but the entry is a closure whose free value `self` sits at depth 1 in the key, so with `fp_depth` 2 `self.p` is cut to its class and `handler` is unknown. With `fp_depth: 3` the whole file passes in both modes (tried, not kept). Its block is split: `# expect inline:` has `JsonHandler.process`, `# expect summary:` has the limit. The fix is a decision about the key (count an entry closure's free values from depth 0, or raise `fp_depth`), not a prologue change |
| 12 | **Found (CS3c).** An entry's start is the same object as an `__init__` effect gives | CS3c. **Not quite.** The prologue runs `__init__` inline, so a parameter nobody passed (`Service.__init__(self, handler: Handler)`) is written as a local `[Unknown]`, where the effect route removed the attribute (§3 row 2). The tree is the same (`Handler.handle`, `Type`), but the entry's key says `handler=Unknown` instead of nothing, so `chains.py` `Service.call`'s digest moved. It is claim 6 (a local strong write of `Unknown`), now seen at entries; decide it with claim 6 |
| 13 | **Found (CS3e, step 1).** Why `chains.py` `inherited` and `via_super` failed in summary mode | CS3e. Four conversions, in order: the **key** was the first wrong one: `Child.__init__` and `Special.make` were keyed with `self` as `Unknown`, because `relevant_params` saw no use of it (their only use is the implicit one in a zero-argument `super()`). With `self` `Unknown` nothing was written onto it (`Base.__init__`'s summary had no effect) and `super().build()` had no `cls`. **Fixed:** a zero-argument `super()` marks the first parameter relevant (`relevant.rs`, using `member.rs`'s `is_zero_arg_super`, so the two cannot disagree); test `zero_arg_super` / `named_super` in `p02_04_relevance_cases`. Request path too (keys change): the golden is equal |

Next: [`04-resolve-then-write.md`](04-resolve-then-write.md)
