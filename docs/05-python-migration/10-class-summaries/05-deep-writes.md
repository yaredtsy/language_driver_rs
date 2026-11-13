# 05.10.05 Deep writes (CS5)

```
deep writes
├── 1. what is wrong
├── 2. pylspt's decisions, kept
├── 3. what this repo changes in them, and the core question
├── 4. the fixtures
├── 5. the commits
└── 6. to check while building
```

From pylspt's `04-class-summaries/05-deep-writes.md`. **Not built.** Needs CS3. Needs the owner's answer to README §7 question 3 first.

## 1. What is wrong

```python
class Sink:    def __init__(self, h): self.h = h
class Config:  def __init__(self, sink: Sink): self.sink = sink
class App:     def __init__(self, config: Config): self.config = config
class Service: def __init__(self, app: App): self.app = app

def rewire(svc: Service):
    svc.app.config.sink.h = JsonHandler()

def main():
    svc = Service(App(Config(Sink(CsvHandler()))))
    rewire(svc)
    svc.app.config.sink.h.process()     # Python: JsonHandler.process
```

`rewire`'s key holds `svc.app.config` as `Opaque(Config)` (`fp_depth` 2). In the walk, `.sink` misses the state, goes to the class side,
and the engine's declared type `Sink` lifts to `Instance { Sink, OPAQUE }`: an object with no identity. The write to its `h` is dropped.
`main` keeps `CsvHandler.process`, **a call Python never makes**, and misses `JsonHandler.process`. Nothing is marked.

## 2. pylspt's decisions, kept

| # | Decision | Why |
|---|---|---|
| 1 | An input object's attribute that the key did not hold is still named by its path: when `member`'s class side gives an `OPAQUE` instance and the receiver's origin is `Entry { path }`, the result's origin is `Entry { path.attr(name) }` (`track_entry_attr`, a third step in `member`'s `Instance` arm) | the class comes from the engine, the same for every caller; the path is a location, not a value. So rule 1 (a summary is a function of its key) holds |
| 2 | Paths are capped at `max_path` (6, a new `Limits` field, at least `fp_depth`). Past it, the object's origin is `Beyond { at, attr }` (`at`: the deepest path that fits). A `Beyond` object holds no state, and every object under it shares its origin | a summary stays small and finite, like a key |
| 3 | A write onto a `Beyond` object becomes the fallback effect: weak `[Unknown]` at `at.attr`, and the summary is marked (`Gaps::DEEP_WRITE`) and counted | dropping it gives the same tree with no mark: silently wrong. Weak, because the summary does not know which object below `at.attr` changed |
| 4 | `DEEP_WRITE` depends only on the code, so the summary is still reusable | it is a precision limit, not a cost one |
| 5 | Reads are **not** fixed: a read at that depth still gets the engine's class, not the caller's object | writes reach the caller by path; reads need the key (step 1, §5) |

## 3. What this repo changes in them, and the core question

| pylspt | Here | Why |
|---|---|---|
| `Stats.deep_write_fallbacks` | `Stats.language["deep_write"]`, only when above 0 | README §4 |
| the summary is `[incomplete]` | **the question.** `core_graph.rs` `status` maps any non-empty `Gaps` to `Status::Incomplete(Cut::MaxRounds)`. A deep write is not a round limit, and core's `Status::Incomplete` says "the edges are a prefix of the real ones", which is not true here either: the edges are complete, a caller's state is not | see below |
| `holds_state` is a check on the id (`OriginId::OPAQUE`) | a `Beyond` origin holds no state either, so the check needs the origin table: `Walk::holds_state(origin)` | `OriginId::holds_state` cannot see what kind an origin is (§6 claim 2) |
| `follow` reads the state | `resolve`'s `follow` (CS2) gains the same rule: an attribute absent on an `Entry` object of the caller is the caller's `Entry(path.attr)` (interning an origin, not writing the env, so phase 1 stays pure) | so a deep effect travels up through several callers, each naming it by its own path |

**The core question** (README §7 question 3). Two ways:

| | A: a new `Cut` variant in core, `MaxPath` ("max_path") | B: a counter only |
|---|---|---|
| what the tree shows | the context is marked `incomplete`, as `MaxRounds` is | nothing; the count is in `Stats` |
| core change | one `#[non_exhaustive]` variant and its `as_str`, as R2a added `Basis::Likely`; the doc comment of `Cut::MaxRounds` already allows "a precision limit, not a cost one" | none |
| TypeScript | nothing emits it; the TypeScript session is told first (core is shared) | — |
| rule 8 | holds: marked and counted | strains: counted, not marked |

**Recommendation: A**, with the core change as its own commit (CS5a), coordinated like R2a.

## 4. The fixtures

| Fixture | Expected |
|---|---|
| `04-class-summaries/python/deep_write.py` (CS0: inline passes; summary fails `main`, `JsonHandler.process` missing) | `main` shows `JsonHandler.process` only |
| `04-class-summaries/python/deep_write_capped.py` (`# limits: max_path=2`; today `fixture_limits` panics on the key) | `main` keeps `CsvHandler.process`, gains nothing, `rewire` is marked; `deep_write` is 1 |
| **new**, `deep_write_two_levels.py` | `def outer(svc): rewire(svc)`: `main` calls `outer`, and the effect reaches it through two summaries (§3, row 4) |

## 5. The commits

### CS5a. Core: the mark (if the answer is A)

| | |
|---|---|
| Change | `Cut::MaxPath` in `core/analyzer-core/src/graph.rs`, `as_str` "max_path" |
| Tests | core: a hand-built graph with a `MaxPath` context renders the incomplete mark; the wire writes it |
| Proof | `cd core && cargo test`; `cargo test` in `languages/python` and `languages/typescript`, **no snapshot changed** |

### CS5b. Python: paths past the key

| | |
|---|---|
| Change | §2 decisions 1–4, §3. `max_path` in `Limits` and in `fixture_limits`; `Gaps::DEEP_WRITE`; `status` maps it to `Cut::MaxPath` (or, with B, to `Finished` plus the counter) |
| Failing tests | `p04_05_deep_writes`: `check_both` on both fixtures, and `deep_write_two_levels.py` |
| Proof | `cargo test` in `languages/python`; the three projects: how many `deep_write` there are on real code (if 0 everywhere, say so: the limit is then theory) |

## 6. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | Only `Attr` steps can go past `fp_depth` (receivers, free values and defaults stop at the key), so `Beyond { at, attr }` always has an attribute name | CS5b: **true.** `Beyond` is made only in `attr_origin`, from a member name or a `Step::Attr`. Every `Entry` made from a key has at most `fp_depth` steps (`from_fp` asserts it, and every test runs the assertion), and `max_path` is never less (`Limits::path_cap`, `p04_05_max_path_is_at_least_fp_depth`), so the `at` of a `Beyond` always fits |
| 2 | Every caller of `OriginId::holds_state` can reach the walk's origin table, so it can become `Walk::holds_state` | CS5b: **true.** The callers were `key.rs` (`fp_one`, which holds the walk), `call.rs` (`reaches_pending`), `member.rs` (two) and `summary.rs` (`resolve`, `follow`): every one has a `Walk`. `OriginId::holds_state` is deleted and `Walk::holds_state` replaces it |
| 3 | A `Beyond` object that is *read* (not written) changes nothing compared with today: its reads still go to the engine | CS5b: **true, as far as a test can say.** A `Beyond` object holds no state, so a read of it goes to the class side exactly as an `OPAQUE` object's does, and the result shares its origin. `p04_05_a_read_below_the_cap_goes_to_the_engine`: `deep_read.py` (`max_path=2`) gives the same graph, digests included, as `max_path` 6. What did change against before CS5b is decision 1 (an object past the key has a *name*, not `OPAQUE`); one nobody wrote onto still keys as `Opaque`, so no digest moves (Jarvis golden `equal`): see `languages/python/ENGINE.md` "CS5b". An object past `max_path` that leaves a summary with no identity is counted too, as `deep_return` |
| 4 | **Why `max_path` exists (reviewer, 2026-11-13, from reading the code; not run).** Today a path cannot outgrow the key: `Entry` origins are made only from the key (`key.rs` `from_fp`, ≤ `fp_depth`), and `follow` reads state only (`summary.rs`). Decision 1 and §3 row 4 lift both limits. A recursive group is iterated until its summaries stop changing (`solver.rs`, the `'rounds` loop, `set_current`; `defer.rs` `replay`), so `def last(n): return last(n.next) if n.next else n` returns `Ref(Param(0))`, then `Ref(Param(0).next)`, then `.next.next`: one step longer every round, never equal, until `max_rounds` (32) cuts it as `Incomplete(MaxRounds)`. With `max_path` the path stops at `Beyond`, the summaries become equal and the group settles. Effects grow the same way once CS8 carries a cycle's effects (until then they are dropped, 03 §3 row 5). **The test:** a fixture `deep_recursion.py` with `last` (and a writing variant for CS8): with `max_path` it settles, `rounds_capped` 0; with `max_path` set very large (a scratch run) it is capped | CS5b: **true, run.** `deep_recursion.py`: with `max_path` 1000 `last` returns one `.next` longer every round (2 paths in round 1, 34 in round 33), `rounds_capped` is 1 and the group is `Incomplete(MaxRounds)`. With the default 6 the paths stop at six steps, round 7 adds `Opaque(Node)`, round 8 equals it, `rounds_capped` 0. `p04_05_a_recursion_that_grows_a_path_settles_at_max_path` and `p04_05_the_same_recursion_without_a_cap_is_cut_by_max_rounds` |

Next: [`06-split-by-type.md`](06-split-by-type.md)
