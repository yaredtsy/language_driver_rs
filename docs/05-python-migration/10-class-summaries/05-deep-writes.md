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
| `04-class-summaries/python/deep_write.py` (pylspt's, never run) | `main` shows `JsonHandler.process` only |
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
| 1 | Only `Attr` steps can go past `fp_depth` (receivers, free values and defaults stop at the key), so `Beyond { at, attr }` always has an attribute name | CS5b |
| 2 | Every caller of `OriginId::holds_state` can reach the walk's origin table, so it can become `Walk::holds_state` | CS5b |
| 3 | A `Beyond` object that is *read* (not written) changes nothing compared with today: its reads still go to the engine | CS5b |

Next: [`06-split-by-type.md`](06-split-by-type.md)
