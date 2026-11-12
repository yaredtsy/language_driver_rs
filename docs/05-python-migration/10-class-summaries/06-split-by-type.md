# 05.10.06 Split by type (CS6)

```
split by type
├── 1. what is wrong
├── 2. pylspt's decisions, kept
├── 3. what this repo changes in them
├── 4. the cost, measured before building
├── 5. the fixtures and the commits
└── 6. to check while building
```

From pylspt's `04-class-summaries/06-split-by-type.md`. **Not built.** Needs CS2 and CS3. Build it only after §4's measurement and the
owner's answer to README §7 question 5.

## 1. What is wrong

```python
class JsonHandler:
    def key(self): return JsonKey()
    def process(self, k): k.encode()
class CsvHandler:
    def key(self): return CsvKey()
    def process(self, k): k.encode()
class Job:
    def __init__(self, h, k): self.h = h; self.k = k
    def go(self): self.h.process(self.k)

def build(h):
    return Job(h, h.key())

def main(c):
    h = JsonHandler() if c else CsvHandler()
    build(h).go()      # Python: Json.process > JsonKey.encode, or Csv.process > CsvKey.encode. Never Json with CsvKey
```

One walk of `build` with `h = JsonHandler | CsvHandler` computes `h.key()` as `JsonKey | CsvKey`, and "which key goes with which handler"
is gone: a union is a set of alternatives, not of pairs. The tree gets all four `encode` edges, two of them calls Python never makes.
Inline mode has the same mix. Summary mode keys `build` with `Fp::Union` today, the same problem.

## 2. pylspt's decisions, kept

| # | Decision | Why |
|---|---|---|
| 1 | One context per **combination** of the relevant slots' values (the Cartesian product, CPA). Each is walked as its own context; inside each, a slot holds one value | the alternatives' whole walks stay apart |
| 2 | Above `split_cap` (4) combinations, the relevant slots collapse into **one** combination whose slots hold unions (`Fp::Union`): today's behaviour. Counted | the product explodes (four slots of three values: 81) |
| 3 | Irrelevant slots never split (`relevant_params`, `relevant.rs`) | they are `Unknown` in the key anyway |
| 4 | Objects returned by different combinations get different origins: `OriginKey::New { site, act, t, combo, made }` | two `Job`s from one site with two combinations are two objects, or the mix comes back one level later |
| 5 | A strong write needs one callee **and** one combination: `strong_ok` | a combination is an alternative, like a callee |
| 6 | Every combination's phase 1 reads the same `before` (step 4, decision 3) | alternatives |
| 7 | One node per site and function in the tree, with the union of the contexts' children | the renderer already does this for several contexts at one site |
| 8 | Inline mode does not split: the fixture names the difference (summary finds less mixing, inline all four) | inline mode is the oracle, not the product |

## 3. What this repo changes in them

| pylspt | Here | Why |
|---|---|---|
| `Stats.cpa_collapses` | `Stats.language["cpa_collapses"]`, only when above 0 | README §4 |
| a collapse is counted, not marked | the same, **pending the owner** (README §7 question 4). A collapse is today's behaviour for every call, so marking it would mark what is unmarked today | rule 8 asks for a mark on a limit that cuts; this one only fails to improve |
| `t` and `combo` added to `New` here | `t` came in CS2; `combo` here | CS2 §3 |
| `key` fingerprints a `Combo` instead of a `Bound` | `Bound { slots }` (`bind.rs`) is split into several `Bound`s, one per combination, so `key` keeps its signature | the smallest change |
| the stats of `keys.py` change | `p02_03_solver` (`interp/fixture.rs`): `keys.py`'s `(walks, reuse_hits)` goes from `(11, 6)` to `(10, 6)` (the `run(a \| b)` walk is gone; its two inner reuses become the split calls'), a comment says why, in the same commit. `p02_01_keys`'s `contexts` stays 11: it measures inline mode | pylspt's own prediction: a `[check]` |
| — | context ids are digests of the key (`canon.rs`); a context that was keyed by a union becomes two keyed by one value each. **The wire's `contexts` change wherever a call had a union argument**, and the golden with it | each move is reviewed; the golden is regenerated only after the owner sees the difference |

## 4. The cost, measured before building

CPA trades contexts for precision. Before CS6 is built, measure with a throwaway counter (not committed): on the three real projects,
at how many project call sites a relevant slot holds more than one value, and how many combinations they would make (capped at 4). If
the number of contexts would grow by more than about half, say so before building: `contexts_per_func` (64) and `max_contexts` (50 000)
would start to cut, and a cut is a mark in the tree.

## 5. The fixtures and the commits

| Fixture | Expected |
|---|---|
| `04-class-summaries/python/split.py` (pylspt's, never run) | summary: `Json.process > JsonKey.encode` and `Csv.process > CsvKey.encode`, nothing else; inline: all four, a mode-specific block |
| `03-classes/python/state.py` `either` | each `Pipeline.run` with its own handler (the join rule 7 made, now split) [check] |
| **new**, `split_cap.py` | a call with five relevant combinations: one context with unions, `cpa_collapses` 1 |

### CS6. Split by type

| | |
|---|---|
| Change | §2, §3 |
| Failing tests | `p04_06_split`: `check_both("04-class-summaries/python/split.py")`; `split_cap.py`; `p02_03_solver`'s new numbers |
| Proof | `cargo test` in `languages/python`; `groups.py` (part 02, the parser) unchanged as a tree; the three projects: contexts, walks, time, and whether any `contexts_per_func` cut appears |

## 6. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | The order of combinations is fixed by the slot order and each slot's value order (rule 9) | CS6 |
| 2 | Splitting never makes a summary-mode tree on an existing fixture worse; a change is a hidden correlation the fixture did not name (then a mode-specific block, with why) or a bug in the combo index | CS6 |
| 3 | The `either` entry's join (rule 7) becomes two nodes or stays one; say which and why | CS6 |

Next: [`07-aliases.md`](07-aliases.md)
