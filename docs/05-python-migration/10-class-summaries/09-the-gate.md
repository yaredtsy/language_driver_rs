# 05.10.09 The gate (CS9): every fixture, both modes, every difference named

```
the gate
├── 1. the gate
├── 2. every difference, named
├── 3. the known limits, both modes
├── 4. what inline mode becomes
├── 5. the commit
└── 6. to check while building
```

From pylspt's `04-class-summaries/09-retiring-inline-mode.md`. **Not built.** It comes last, after whichever of CS5–CS8 the owner chose
(README §7 question 5). If the owner stops after CS4, this gate runs then, with the differences of the steps not built listed as known
limits.

## 1. The gate

Every fixture file with an `# expect` block, parts `00` to `11`, runs with `check_both`: inline, summary, summary without reuse. All three
pass. A fixture that fails now is a bug from an earlier step that a later one exposed: find the first step whose fixture fails, and fix it
there. `tests/snapshots/unverified/` is empty and deleted, and the gap test `what_summary_mode_misses_…` is replaced by the list of §2 and
§3, which is the specification of what each mode may not find.

## 2. Every difference, named

**Summary mode finds more:**

| Fixture | Why |
|---|---|
| `02/pending.py` `walk`, `02/groups.py` `ping_entry`, `merge_entry` | the fixpoint finds a recursive call's value; inline stops at the guard |
| `04/in_keys.py` `again`, `again_closure` | a pending value inside an object's attribute or a closure's free value: deferred and keyed with the round's value |
| `04/split.py` (CS6) | each handler keeps its own key |
| `04/recursion_objects.py` `walk` (CS8) | a pending object's method, called in the fixpoint |

**Inline mode finds more**, because a key or a summary is bounded:

| Fixture | The bound |
|---|---|
| `04/in_keys.py` `deep` | `fp_depth`: a read three steps deep (step 1 §6: measure 2 against 3) |
| `04/deep_write_capped.py` (CS5) | `max_path`: a write past it is a weak `Unknown`, marked |
| `04/aliases.py` `nested` (CS7) | aliasing through an attribute is not in the key |

**Summary mode marks what it cut:** `02/cuts.py` (`max_stack`).

**Summary joins what inline lists** (rule 7, no information lost): `03/state.py` `either`, unless CS6 splits it (step 6 §6 claim 3).

Everything else is the same tree in both modes. That is the claim of the whole folder: **summaries change the cost, not the answer,
except where a bound says so, and every bound is named, counted and pinned.**

## 3. The known limits, both modes

Pinned, so a fix shows as a changed expected tree:

| Fixture | Limit |
|---|---|
| `01/branches.py` `loop` | a loop body is walked once |
| `03/isolation.py` `global_write` | writes to module globals are dropped (README §8) |
| `04/recursion_objects.py` `identity_limit` | an object does not keep its identity around a cycle (CS8 decision 3) |
| `04/recursion_objects.py` `tree_fill` | effects inside a cycle reach only the exit state (CS8 decision 2) |
| `03/entries.py` `Job.run -> B.go` (an extra edge) | the entry rule: only `__init__` is part of a method entry's start |

## 4. What inline mode becomes

pylspt left it open: keep inline mode as a test oracle, or delete it. **This repo already keeps it test-only**: `call_tree` and
`inline_tree` are `#[allow(dead_code)] // inline mode is the fixture suite's cross-check and is not reachable from the public API`, and
`lib.rs` always asks for `Mode::Summary`. Keep it (README §7 question 6):

| Keep, test-only | Delete |
|---|---|
| a second, independent answer on every fixture: a difference not in §2 is a bug in one of the two. No-reuse mode cannot do that, it compares summaries with summaries | one way to follow a call |
| every change to the walker must keep inline mode working; it is exponential, so small fixtures only | the cross-check is gone |

The step writes the rule where the code is: on `Mode::Inline`, a doc comment saying "test oracle only: exponential, no limits, never on
the request path; its trees equal summary mode's except the differences in `docs/05-python-migration/10-class-summaries/09-the-gate.md`
§2". Whether to put it behind `#[cfg(test)]` instead of `allow(dead_code)`: §6 claim 1.

## 5. The commit

### CS9. The gate

| | |
|---|---|
| Change | every remaining `check` → `check_both`; `unverified/` deleted; the gap test replaced by §2–§3 as assertions; the doc comment of §4 |
| Proof | `cargo test` in `languages/python`; `scripts/check-all.sh python`; the three projects' final numbers in `languages/python/ENGINE.md`; Q-PY1 marked answered in [`../../07-decisions-and-questions.md`](../../07-decisions-and-questions.md) |

## 6. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | Inline mode can move behind `#[cfg(test)]`: no non-test code path reaches `Mode::Inline` (the `dump_graph` helpers included) | CS9 |
| 2 | Every fixture in `05-output` to `11-likely-callee` passes with `check_both`, not only the class programs | CS9 |

Back to the start: [`README.md`](README.md)
