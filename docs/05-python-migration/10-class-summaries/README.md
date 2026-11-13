# 05.10 Class summaries: objects that cross a call

```
class summaries
├── 1. what is wrong, with the line that shows it
├── 2. what a summary must say, and the four conversions
├── 3. what is built, what is a spike, what is not built
├── 4. pylspt's decisions, checked against this repo's
├── 5. the design rules every step keeps
├── 6. the steps, in the order to build them
├── 7. what the owner decides
├── 8. what is not in this folder
└── 9. to check while building
```

Migrated on 2026-11-12 from pylspt's `experience/12-call-tree/04-class-summaries/` (README and nine step files, identical at the pinned
commit `670ae79a15` and at pylspt's HEAD: checked with `git diff`, D14), with `plan/03-call-tree/09-summaries.md`, `10-one-call.md` and
`15-checks-and-build-order.md`. Each pylspt step has one file here, with the same number. Every pylspt decision was checked against the
code at `e55ab37` and against D1–D28; where this repo decided otherwise, the file says so and which one wins.

§1 and §3 were **run** (the built test binary of `python-analysis`, `constructor_effects_spike`, and `dump_graph` on the Jarvis project,
at `e55ab37`). Everything from step 2 on is **not built**. Every `[check]` is a claim to verify before depending on it.

This folder answers **Q-PY1** ([`../../07-decisions-and-questions.md`](../../07-decisions-and-questions.md) §4).

## 1. What is wrong

```python
class Pipeline:
    def __init__(self, handler):
        self.handler = handler
    def run(self, data):
        self.handler.process(data)

def read_json():
    Pipeline(JsonHandler()).run(1)        # Python: Pipeline.run > JsonHandler.process
```

What the wire says today (`tests/snapshots/unverified/03-classes/python/state.py.wire`):

```
state.read_json
  state.Pipeline
  state.Pipeline.run                      ← no child: JsonHandler.process is missing
  state.JsonHandler
```

The wire is made in **summary mode**: each function is walked once per key, and every caller with an equal key reuses the result. Today
a result can carry the calls and a return value **without identity** (a function, a class, a closure). It cannot say:

1. "I return the object you gave me" or "I return an object I built" (`make(h): return Pipeline(h)`: the caller's `.run()` is lost);
2. "I wrote this onto your object" (`__init__`'s `self.handler = handler`; `install(p, h)`'s `p.handler = h`).

Inline mode, the test-only cross-check, follows every call into its body and gets both right. pylspt's part 04 is the plan to make
summaries say both. It was never finished.

**Measured, 2026-11-12** (`constructor_effects_spike`, which compares summary mode with inline mode on the class programs):

| | failing entries | missing nodes | extra nodes |
|---|---|---|---|
| today | 21 | 44 | 2 |
| with the spike's constructor effects (§3) | 15 | 27 | 4 |

On real projects (from the spike's commit `54b513e` and a run at `e55ab37`):

| Project, entry | effects off | effects on | inline |
|---|---|---|---|
| FastAPI, `include_router` | 436 nodes | 970 | 1019 |
| Jarvis, `CallGraphGenerator.do_pass` | 207 nodes, 55 contexts, 30 unresolved | 207, 68 contexts, 30 unresolved | 207 |

So the gap is large on code built from objects that hold other objects (FastAPI), and absent on Jarvis, whose 30 unresolved sites are
untyped values (list elements, library objects), not lost object state.

## 2. What a summary must say, and the four conversions

An object's identity lives in one walk (`OriginId`). A summary is reused by callers it has never seen, so it can never name "object 7".
It names an object by **where it sits among the inputs** ("parameter 0's `.handler`"), or as "a new object I built". Every feature of
this folder is one arm in each of four conversions:

```
caller's walk               key                   callee's walk                  summary
Instance{origin 3} ─fingerprint─► Instance{class, attrs} ─materialize─► Instance{Entry(Param(0))} ─finish─► Ref(Param(0))
Instance{origin 3} ◄────────────────────────────── resolve ─────────────────────────────────────── Ref(Param(0))
```

| Conversion | Code | Built for objects |
|---|---|---|
| fingerprint: the caller's values → the key | `Solver::fingerprint`, `fp_one` (`interp/key.rs`) | yes (step 1) |
| materialize: the key → the callee's starting env | `Walk::materialize`, `from_fp` (`interp/key.rs`) | yes (step 1) |
| finish: the callee's end state → the summary | `Walk::finish_round`, `to_svals` (`interp/summary.rs`) | **no**: an object becomes `Unknown` (`plain`) |
| resolve: the summary → the caller's values | `Walk::resolve`, `resolve_in` (`interp/summary.rs`), called from `call_project` and the replay (`defer.rs`) | **no** |

**Debugging rule** (pylspt's, kept): when a fixture fails in summary mode, print the four in order (the key, the starting env, the
summary, what the caller made of it). The bug is at the first one that looks wrong.

## 3. What is built, what is a spike, what is not built

| pylspt step | Here | State at `e55ab37` |
|---|---|---|
| 1. objects in keys | [`01-objects-in-keys.md`](01-objects-in-keys.md) | **built**: `Fp::Instance/Opaque/Bound`, `fp_depth`, `Path`, `OriginKey::Entry`, method calls and constructors in summary mode, `reaches_pending`, `Walk.now`; `in_keys.py` passes in both modes (`p04_01_in_keys`) |
| 2. objects coming back | [`02-objects-coming-back.md`](02-objects-coming-back.md) | not built (`SVal` says `// Ref, Obj, Bound: part 04`). Baseline pinned (CS0): `coming_back.py` passes inline, all 4 summary entries fail |
| 3. effects | [`03-effects.md`](03-effects.md) | **a spike, off** (`interp::Options::constructor_effects`, `54b513e`): only `__init__`, only onto `self`, found by comparing start and end state, always joined. Not reachable from the public `Options`. Baseline pinned (CS0): `effects.py` passes inline, all 5 summary entries fail |
| 4. resolve, then write | [`04-resolve-then-write.md`](04-resolve-then-write.md) | not built. Baseline pinned (CS0): `resolve_first.py` passes inline, both summary entries fail |
| 5. deep writes | [`05-deep-writes.md`](05-deep-writes.md) | not built (`max_path` does not exist: `deep_write_capped.py`'s `# limits:` line would panic in `fixture_limits`, so it is still never run). Baseline pinned (CS0): `deep_write.py` passes inline, its summary entry fails |
| 6. split by type | [`06-split-by-type.md`](06-split-by-type.md) | not built. Baseline pinned (CS0): `split.py` passes its inline block, its summary block fails |
| 7. aliases | [`07-aliases.md`](07-aliases.md) | not built. Baseline pinned (CS0): `aliases.py` passes inline; summary fails `same` and `different`, passes `nested` (its block is the known limit) |
| 8. recursion with objects | [`08-recursion-with-objects.md`](08-recursion-with-objects.md) | not built (`DeferredOp` is `Apply` and `Call`). Baseline pinned (CS0): `recursion_objects.py` passes inline; summary fails `use_walk` only (`Handle.close` missing), passes `identity_limit`, `tree_fill` (known limits) and `visitor` (`Report.emit` from ty's type of `v.found`, a `Type` edge, not from the walk) |
| 9. retiring inline mode | [`09-the-gate.md`](09-the-gate.md) | half decided: summary is the only request mode, inline is test-only |

Of the nine fixture programs of `04-class-summaries/`, eight are run since CS0: `in_keys.py` in both modes (`p04_01_in_keys`), the
other seven in inline mode only (`check`, `p04_02_coming_back` to `p04_08_recursion_objects`), where all seven pass their blocks. Their
summary-mode wire is snapshotted under `tests/snapshots/unverified/04-class-summaries/`, and
`what_summary_mode_gives_on_the_part_04_programs_against_their_blocks` pins, against their own blocks, the 16 of their 20 summary-mode
blocks that fail. One, `deep_write_capped.py`, is never run: its `# limits: max_path=2` asks for a limit that does not exist until CS5.
The six class programs of `03-classes/` that summary mode cannot answer are checked in
inline mode only (`check`), and their summary-mode wire is snapshotted under `tests/snapshots/unverified/` as a change detector. The test
`what_summary_mode_misses_on_the_class_programs_and_what_the_graph_says_there` (`interp/fixture.rs`) pins today's gaps line by line.

## 4. pylspt's decisions, checked against this repo's

The rows where this repo already decided something else. Each step file has its own, longer list.

| pylspt says | This repo | Which wins, and why |
|---|---|---|
| render draws a construct as its class (step 1, decision 7) | the tree is core's `render` over `Graph` (PY6); a construct is `EdgeTo::Construct` | **this repo**: nothing to build in render |
| `Target::Construct { class, init }` | `Target::Construct { class, init, basis }` and `Target::Call(ctx, basis)` (`Basis`, D28) | this repo. Every new target carries a basis (§5 rule 6) |
| counters are `Stats` fields (`cpa_collapses`, `deep_write_fallbacks`) | `Stats.language`, string keys, written only when above 0 (R2b) | this repo: a new counter moves no snapshot |
| `[incomplete]` for a deep write (step 5) | `Status::Incomplete(Cut)` in core; Python maps any non-empty `Gaps` to `Cut::MaxRounds` (`core_graph.rs`, `status`) | **open**: a deep write is not a round limit. Needs a core decision (§7, question 3) |
| a weak write is old ∪ new | a weak write onto an attribute the path holds nothing for also adds `Unknown` (`570ab69`) | this repo: the alternative is marked, not dropped |
| materialize writes a key's attributes weakly (step 1, decision 4) | they are **joined**, adding no `Unknown` (`Env::join_attr`, `11e20bb`) | this repo: rebuilding is not a write |
| the summary-mode prologue runs `prologue` as inline mode does (step 3) | the spike's prologue builds the object and runs `__init__` only, for a method entry (`mod.rs`); closures' parents are not run | **pylspt**: one prologue, `entry.rs`, in both modes (step 3) |
| the fixpoint compares returns, effects and objects (step 8, decision 4) | `set_current` compares `returns` only (`solver.rs`) | **pylspt, moved earlier**: needed as soon as effects exist (step 3) |
| inline mode: keep as a test oracle, or delete (step 9) | not reachable from the public API, `#[allow(dead_code)]`, summary is the only request mode | this repo already chose "keep, test-only"; step 9 writes it down |
| the wire format is part 05's business | the wire is D17's, its version stays 2 (D23) | this repo: nothing in this folder adds a field to the wire |
| an object from a summary is `Path`-based (`Ref`) | `Basis::Path` may rest only on an object built in this walk (`OriginKey::is_built_here`) | both: an `Obj(j)` object is built in the caller's walk (a `New` origin); a `Ref` object is the caller's own, with its own origin. [check] in step 2 |
| where the walk holds `Unknown`, nothing | the engine's exact type answers (D27, `or_engine_callee`) | this repo. Steps 2 and 3 make the walk hold the object, so D27 answers less often: a snapshot of `10-engine-callee` may move, and each move is reviewed |

## 5. The design rules every step keeps

1. **Rule 2 as a type.** No summary value can hold a walk-local id (`OriginId`, `ActivationId`, `ValueId`, `DeferId`). `SVal::Ref` holds a
   `Path`, `SVal::Obj` a `u16` index, `Effect.target` a `Path`. A leak then does not compile.
2. **Two phases, enforced by the borrow checker.** Applying a summary is `resolve(&Env) -> Plan`, then `apply(Plan, &mut Env)`. Phase 1
   takes the caller's env by shared reference, so it *cannot* write; phase 2 writes only what phase 1 resolved. The seam is built in step 2,
   before there are effects to get wrong (step 4 is then a proof, not a fix).
3. **Canonical order.** Effects sorted by (target, attribute), objects numbered in the order `finish` reaches them (returns first, then
   effect values). Equal walks give `==` summaries: the fixpoint needs it, and rule 9 needs it.
4. **Wrong is worse than missing.** A step may leave a call missing (it is then counted or pinned). It may not add an edge Python never
   takes. The gate of every step counts `extra` separately and expects 0, or each one named in the gap test.
5. **One mechanism.** The spike is replaced, not grown: `init_effects`, `EffectValue`, `apply_effects`, `Walk.inputs` and `start_attrs` go
   when step 3 lands. There is one prologue, one way to apply a summary.
6. **Every new edge has a basis.** A call on an object that came back as `Obj(j)` rests on `Path` (it was built in this walk, by a callee);
   on a `Ref` object, on whatever the caller's own object rested on.
7. **A change to the key adds nothing when it is empty.** A new key field (step 7's alias vector) is left out of `CanonKey` when it has its
   trivial value, so the stable context ids of code that does not use it do not move, and the golden does not move with them.
8. **Every limit that cuts is marked and counted** (rule 8): `max_path` (step 5), the split cap (step 6), effects that cannot be carried
   yet (step 3 until step 8). A counter is written only when above 0.
9. **The ratchet.** `what_summary_mode_misses_…` and `tests/snapshots/unverified/` only shrink. A step that fixes a program moves its
   snapshot to the verified ones, switches its test from `check` to `check_both`, and deletes its lines from the gap test. A line added is
   a regression.
10. **Measure at every step** on three real projects (Jarvis: the golden; the Flask service; a FastAPI project): nodes, contexts, walks,
    unresolved, time. The numbers go in `languages/python/ENGINE.md`.

## 6. The steps, in the order to build them

```
CS0  baseline: seven programs run once, pinned (33d65ca) ──────────────── DONE
CS1  objects in keys ─────────────────────────────────────────────────── DONE
CS2  objects coming back, and the resolve → plan → apply seam ────────── DONE (99c60b3, b2bc150)
CS3  effects: every write onto an input, the one prologue ────────────── DONE (253783e … 5b907ff; the flag is off on requests)
CS4  resolve, then write: the proof ──────────────────────────────────── DONE (a28baf1; CS4.1 0eb5fa6: each callee an arm)
CS5  deep writes ─────────────────────────────────────────────────────── DONE with B (6ec20e7); A (core `Cut::MaxPath`) is §7 question 3
CS7  aliases ─────────────────────────────────────────────────────────── DONE (8de962a); a free value aliasing a parameter is a known limit
 ──  owner: effects on by default (the wire changes) ─────────────────── §7 question 2: on. CS4b DONE (04 §5): the flag is gone
CS6  split by type (CPA) ─────────────────────────────────────────────── 06   measure the cost first
CS8  recursion with objects ──────────────────────────────────────────── 08
CS9  the gate: every fixture, both modes, every difference named ─────── 09
```

**CS0, before anything.** Run the seven programs of `04-class-summaries/` that were never run and can run (all but `in_keys.py`, which
is run, and `deep_write_capped.py`, whose `max_path` does not exist yet), through the harness the six class programs use: an inline-only
`check` where inline passes, and an unverified summary snapshot. Their summary-mode result is pinned **against their own `# expect`
blocks**, not against inline mode: several of them say on purpose that summary mode differs from inline (`split.py`, the `again`
entries), so the existing gap test, which takes inline mode as the answer, stays restricted to `03-classes/`. No solver change. It turns
"never run" into a list, and it is the baseline every later step is measured against. One commit.

**Why this order and not pylspt's.** pylspt's 2 → 3 → 4 is kept, but the two-phase seam of step 4 is built in step 2 (§5 rule 2), so
step 3 only adds effects to the plan, and step 4 is the fixture that proves it. pylspt calls steps 1 to 4 "the core": together they make
every `03-classes/` program pass in summary mode. **Stop there and measure** before steps 5 to 8. Each of those fixes a narrower pattern,
and step 6 multiplies contexts.

Each step is one or more commits: tests first, seen red for the stated reason, one commit per sub-step, explicit paths. Each step file
lists its commits, failing tests and proof in the form of [`../09-a-def-past-an-unknown-base.md`](../09-a-def-past-an-unknown-base.md) §5.

## 7. What the owner decides

| # | Question | Recommendation |
|---|---|---|
| 1 | Q-PY1: tag `python-v0.2.0` before this work, saying the limit in the README, or after | **before**: nothing gets worse, the work is several steps |
| 2 | When effects go on by default. It changes the wire: the snapshots, the FastAPI tree (458 → 1019), maybe the golden (Jarvis did not move in the spike) | ~~at the end of CS4, if `extra` is 0 on the fixtures and the three projects~~ **Changed at CS3b (2026-11-13): not before CS5 §2 row 1 and CS7.** With effects on, two summary-only wrong edges appear, unmarked, where before there was a marked gap: `deep_write.py main` (a write past the key's depth lands on an `OPAQUE` object and is dropped, so the caller keeps the old `CsvHandler`) and `aliases.py same` (the key does not say the two arguments are one object, so the callee reads the old handler). Both are the stated targets of CS5 (`track_entry_attr`, which needs no core decision; only the mark past `max_path` does) and CS7. `split.py main`'s mix is drawn by inline mode too (CS6; a known kind, like §8's narrowing) **ANSWERED (owner, 2026-11-13): on.** After CS7 the summary-only wrong edges left are known limits that real code did not hit (a deep write past `max_path`: `deep_write` 0 on the three projects; a parameter aliasing a closure's free value, `alias_free.py`). CS4b (04 §5) turns it on |
| 3 | A deep write past `max_path` (CS5): its mark. Core's `Cut` has no variant for it; `MaxRounds` would be a lie | a new `Cut` variant in core, as R2a added `Basis::Likely` (coordinate with the TypeScript session); or only a counter, unmarked, if core must not move |
| 4 | Above the split cap (CS6) the combinations collapse into a union, which is today's behaviour and can pair values that never met | count it (`cpa_collapses`), no mark: it is what every call does today |
| 5 | Steps 5 to 8: build them, or stop after CS4 | decide on CS4's numbers. **After CS3b:** CS5 §2 row 1 and CS7 are needed for question 2 either way; CS6 and CS8 stay this question |
| 6 | After the gate: keep inline mode as the test oracle, or delete it | keep (step 9) |

## 8. What is not in this folder

| Thing | Why not | Where it goes |
|---|---|---|
| Writes to module globals (`REGISTRY.add(h)`) | a `Root::Global` in keys and effects: a growth item of pylspt's plan (v1.2) | its own plan, if a real project shows it |
| Reads past `fp_depth` (`self.app.config.handler`) | writes reach the caller by path (step 5), reads need the key. pylspt: measure `fp_depth` 2 against 3 | a measurement in step 1's file, §6 |
| Aliasing through attributes (`self.peer is other`) | pairs of paths in the key | step 7 names it as a known limit |
| Containers (`self.items.append(h)`), `nonlocal` cells, a loop fixpoint | pylspt's growth path (v1.2) | not planned |
| Calls on untyped values (Jarvis's 30, most of the Flask service's 79) | not object state: the engine has no type | a virtual environment for the engine, not analyzer work |
| A union with `Unknown`, `functools.wraps` | D28 §6, R3 §6 | the owner's list |
| Narrowing on a branch (`isinstance(x, C)`, `if PYDANTIC_V2:`): the walk takes every arm with every value, in both modes | not object state, and inline mode does the same, so `extra` (summary against inline) cannot see it. Found at CS2b: 20 of FastAPI's 22 new nodes are edges Python never takes (a v2 `ModelField` down the v1 `isinstance` arm; `languages/python/ENGINE.md`, CS2). Effects (CS3) carry more objects into more arms, so it can grow | its own plan, if CS3e's FastAPI numbers show it; the owner's list until then |

## 9. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | The gap test's count (48 entries, 21 failing, against inline mode: `what_summary_mode_misses_…`) and Q-PY1's (22 of 48, against the `# expect:` blocks) count different things; say which entry makes the difference | CS0. **Does not hold.** The two comparisons are different, but today they give the same answer: the new test's comparison (summary mode against the blocks, `sorted_siblings` on both), run once by hand on `03-classes/` and not committed, fails **21 of 48**, and they are the same 21 entries the gap test counts against inline mode. The entry that makes 22 is `entries.py Reg.create`, in Q-PY1's list and passing both ways: `c8b98bb` bound a classmethod entry's `cls` (its message says "22 -> 21 entries failing"), and Q-PY1's row was not updated |
| 2 | No verified snapshot changes in CS0 | CS0. **Holds.** After `ANALYZER_BLESS=1 cargo test -p python-analysis --lib the_wire_of_every_fixture_equals_its_snapshot`, and the same for `the_graph_of_every_fixture_equals_its_snapshot`, `git status tests/snapshots` shows only new files (seven `.wire` under `unverified/04-class-summaries/python/`, seven `.graph` under `04-class-summaries/python/`, where `in_keys.py.graph` and the inline-only `03-classes/` graphs already are) and `git diff --stat tests/snapshots` is empty; the counts are `(29, 13)` programs for the wire and `(42, 149)` programs and entries for the graph |
| 3 | A summary-mode run of each of the seven programs finishes (no panic, no `debug_assert` from `fp_one`'s pending arm), and inline mode passes each one's `# expect` blocks | CS0. **Holds.** `p04_02_coming_back` to `p04_08_recursion_objects` (`check`, inline) pass; the summary-mode runs of the wire snapshot test, the core-invariants test and the new pinned test finish on all 20 summary blocks, in the debug test profile (`debug_assert` on) |
| 4 | `Path`, `Step` and `Name` can derive `Ord`, so effects sort (§5 rule 3) | CS2. **Holds.** The derive compiles on `Path`, `Root` and `Step` (`Root` is needed too: `Path` holds it); `Name` is `Ord` (`Env::attrs_of` sorts by it). CS3b. **Committed:** the derive is on `Path`, `Root` and `Step` (`value.rs`), and `effects_sort_by_target_then_attribute` uses it |

Next: [`01-objects-in-keys.md`](01-objects-in-keys.md)
