# 05.10.04 Resolve, then write (CS4), and effects on by default

```
resolve, then write
├── 1. what goes wrong without it
├── 2. pylspt's decisions, kept
├── 3. where it is built here
├── 4. the fixture and the commit
├── 5. after CS4: effects on by default (the owner's yes)
└── 6. to check while building
```

From pylspt's `04-class-summaries/04-resolve-then-write.md`. Here the rule is **built in CS2 and CS3 by the shape of the code** (README §5
rule 2), so this step is the fixture that proves it, plus the switch that turns effects on for requests.

## 1. What goes wrong without it

```python
class Box:
    def __init__(self, h): self.h = h
    def replace(self, h):
        old = self.h
        self.h = h
        return old

def replacing():
    b = Box(JsonHandler())
    old = b.replace(CsvHandler())
    old.process()          # JsonHandler.process only
    b.h.process()          # CsvHandler.process only
```

`replace`'s summary: returns `[Ref(Param(0).h)]`, effect `Param(0).h = [Ref(Param(1))]`, strong. If the caller writes the effect first and
then follows `Param(0).h` for the return, it reads the **new** handler: `old.process()` shows `CsvHandler.process` and loses
`JsonHandler.process`. A wrong edge and a missing one, unmarked. `Pair.swap` (`t = self.x; self.x = self.y; self.y = t`) has two effects
that read each other's attribute: applied one by one, both end up `CsvHandler`.

**Why inline mode never needed a rule.** It runs the statements in order, in one walk. A summary keeps only the start and the end; phase
1 puts the order back: everything the summary says about the start is read at the start.

## 2. pylspt's decisions, kept

| # | Decision |
|---|---|
| 1 | Phase 1 resolves every value the summary mentions (returns, new objects' attributes, effect targets, effect values) against `before`, the env when the call started. Phase 2 writes. Never interleaved |
| 2 | In phase 2, new objects' attributes are written before effects: an effect's value can be a new object, which should hold its state when it lands. (An effect's target is never a new object: step 3, §2 decision 2) |
| 3 | One `before` per call expression, taken after the arguments are evaluated, shared by every callee of the site (and, step 6, every combination): they are alternatives, not a sequence |

## 3. Where it is built here

| Decision | Where |
|---|---|
| 1 | `resolve(…, before: &Env) -> Plan` and `apply(Plan, &mut Env)` (CS2b). `resolve` holds a shared borrow, so it cannot write: the compiler checks decision 1 |
| 2 | the order inside `apply` (CS2b objects, CS3b effects) |
| 3 | `call_values` takes `before` once, after the arguments, and passes `&before` to every callee's `resolve` |

## 4. The fixture and the commit

### CS4. The proof

| | |
|---|---|
| Change | none expected. If the fixture is red, the seam leaks: find where a callee's `resolve` reads `env` instead of `before` |
| Tests | `p04_04_resolve_first`: `check` → `check_both("04-class-summaries/python/resolve_first.py")` (CS0: inline passes; summary fails `replacing` and `swapping`, both `.process` calls missing): `replacing` and `swapping` as in §1. A new entry in the same file, `two_callees_write` (`h = A() if c else B(); h.reset(box)` where `A.reset` and `B.reset` write `box.h` differently and each returns the old `box.h`): both returns are the old handler (decision 3) |
| Proof | `cargo test` in `languages/python`. pylspt's demonstration, kept as a review step and not as code: swap the two phases in a scratch edit, see `replacing` fail with `CsvHandler.process` under `old`, swap back |

## 5. After CS4: effects on by default (the owner's yes)

README §7 question 2. When CS3e's gate holds (`extra` 0 on the fixtures and the three projects):

### CS4b. Effects on

| | |
|---|---|
| Change | `lib.rs`, `Options::solver`: `effects: true`. Then delete the option itself: one mechanism (README §5 rule 5). The harness's `summary()` stops setting it |
| Tests | `tests/api.rs` pins move where a class program is an entry: each move read. `tests/snapshots/` moves only where CS3e already showed the new tree |
| Golden | run the golden (`GOLDEN=out/golden scripts/run-on-project.sh python …`). The spike did not move Jarvis (207 / 207). If it moves now, **stop and report** the difference before regenerating it |
| Proof | `cargo test` in `languages/python`; `scripts/check-all.sh python`; the three projects' numbers in `languages/python/ENGINE.md` |
| After | the V-NOC driver's agent is told: the wire format is unchanged (version 2, D23); trees on class-heavy code are larger (FastAPI 436 → 970 nodes) |

This is a natural point for a tag (`python-v0.3.0`), and for stopping to measure before CS5–CS8 (README §7 question 5).

## 6. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | `call_values` evaluates every argument before it takes `before`, so a call inside an argument (`f(g(x))`) has its effects in `before` | CS4 |
| 2 | The `Plan` of one callee is applied before the next callee's is resolved, and that is harmless because each resolves against the same `before` | CS4 |

Next: [`05-deep-writes.md`](05-deep-writes.md)
