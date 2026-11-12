# 05.10.08 Recursion with objects (CS8)

```
recursion with objects
├── 1. what is wrong
├── 2. pylspt's decisions, kept
├── 3. what this repo changes in them
├── 4. the fixture and the commits
└── 5. to check while building
```

From pylspt's `04-class-summaries/08-recursion-with-objects.md`. **Not built.** Needs CS2 and CS3. Its decision 4 (the fixpoint compares
effects and objects) is already moved to CS3d.

## 1. What is wrong

Part 02's fixpoint (`defer.rs`, `DeferredOp::Apply` and `Call`) handles a recursive call's **value**. With objects, three more things
can wait on a recursive call:

```python
def walk(node):
    r = walk(node)        # pending
    r.close()             # an attribute of a pending value, then a call on it
    return Handle()

class Visitor:            # a cycle of four methods: accept > visit > visit_node > visit_children > accept
    def visit_children(self, node: Node):
        node.left.accept(self)
        self.found = Report()      # a write inside the cycle
def visitor(root: Node):
    v = Visitor(); root.accept(v)
    v.found.emit()                 # Python: Report.emit
```

Today `r.close` on a pending `r` goes to the engine's type of the expression (`eval_attribute`, `member.rs`: any receiver that is not an
object, class or module), and the write inside the cycle is dropped and counted `effects_in_cycle` (CS3 §3 row 5).

## 2. pylspt's decisions, kept

| # | Decision | Why |
|---|---|---|
| 1 | Two more deferred operations: `Member { base, name, site }` (`p.name`, `p` pending; also the bound method of `p.m()`, then a `Call` on it) and `Write { base, name, value }` (`p.name = v`) | the same answer part 02 gave for values: record the operation, replay it in the fixpoint |
| 2 | Writes and effects replayed in the fixpoint are **never strong**: joined into the walk's exit state | the statements after the operation were walked once, without its result. The only place left for its write is the end, beside what is there (`Tree.fill`, a known limit in both modes) |
| 3 | **An object does not keep its identity around a cycle**: in a group member's own summary, an object that came from a member's summary is exported as `Opaque(class)`, a closure as `Function(func)` | without it, `build(n) = Branch(build(n - 1))` adds a level each round and never stops. Inside the walk the object keeps its identity; only the summary forgets. It costs `identity_limit`, pinned |
| 4 | The fixpoint compares returns, effects and objects | **moved to CS3d** |

**Why it terminates.** Values only grow, and every one is bounded: fingerprints by `fp_depth`, paths by `max_path`, unions by the fan-out
caps, objects by decision 3. `max_rounds` stays as the safety net, and hitting it is marked (`Cut::MaxRounds`).

## 3. What this repo changes in them

| pylspt | Here | Why |
|---|---|---|
| `Apply` gains the site, callee index and combination, and `strong_ok` | it gained the site, `t` and the bound slots in CS2b, the combination in CS6; here `strong_ok` (for decision 2 it is always false in a replay, so it may need no field: §5 claim 1) | CS2 needed them first |
| `Write`'s replay pushes onto `exit_writes`, joined in `finish_round` | the same; and the effects of a replayed `Apply`/`Call` join the same list | one path for "late writes" |
| — | `effects_in_cycle` (CS3) **goes**: those effects are now carried. A new counter is not needed: an object exported as `Opaque` by decision 3 is not a cut, it is the rule | rule 8: the counter existed only while the writes were dropped |
| the identity flag is set on origins made while resolving an `Apply` or `Call` to a group member | the same, as a field of `OriginKey::New` (`from_cycle: bool`) or a set on the walk; the second keeps `New` small | [check] which is simpler with `canon` and `is_built_here` |

## 4. The fixture and the commits

| Fixture | Expected |
|---|---|
| `04-class-summaries/python/recursion_objects.py` (pylspt's, never run) | `walk`: `Handle.close` (summary finds it, inline does not: a mode-specific block); `visitor`: `Report.emit`; `identity_limit`: nothing (pinned); `tree_fill`: nothing (pinned, both modes) |
| `02-function-summaries/python/groups.py` | unchanged as a tree (after CS6 the parser's `combine` splits per type) |
| CS3's `cycle_effect.py` | the write is carried, weakly; `effects_in_cycle` is gone |

### CS8a. Member and Write

| | |
|---|---|
| Change | §2 decision 1; the deep pending check before keying already looks as far as the key (`reaches_pending`, step 1) |
| Failing tests | `recursion_objects.py` `walk` with `check_both` |

### CS8b. Late writes, and identity around a cycle

| | |
|---|---|
| Change | §2 decisions 2 and 3; `effects_in_cycle` removed |
| Failing tests | `p04_08_recursion_objects`: the whole file with `check_both`; `cycle_effect.py`'s new expectation |
| Proof | `cargo test` in `languages/python`; `max_rounds` is never hit on the fixtures (it would be marked) |

## 5. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | A replayed `Apply` never needs `strong_ok`, since everything it writes is weak (decision 2) | CS8b |
| 2 | Decision 3 does not move any part 02 snapshot (closures exported as `Function(func)` only inside a cycle) | CS8b |
| 3 | The `visitor` cycle has one key per member (the inner `accept` equals the outer one), so each method is walked once | CS8b, the stats |

Next: [`09-the-gate.md`](09-the-gate.md)
