# 05.10.07 Aliases (CS7)

```
aliases
├── 1. what is wrong
├── 2. pylspt's decisions, kept
├── 3. what this repo changes in them: the stable id
├── 4. the fixture and the commit
└── 5. to check while building
```

From pylspt's `04-class-summaries/07-aliases.md`. **Not built.** Needs CS3 (effects: an alias only matters when something is written).

## 1. What is wrong

```python
def set_and_read(a, b):
    a.h = JsonHandler()
    b.h.process()

def same():
    x = Box(CsvHandler())
    set_and_read(x, x)        # Python: JsonHandler.process
def different():
    x = Box(CsvHandler()); y = Box(CsvHandler())
    set_and_read(x, y)        # Python: CsvHandler.process
```

`x` and `y` hold the same things, so their fingerprints are equal, and both calls share one key. In the walk `a` and `b` are two origins,
so the write to `a.h` is not seen through `b`. `same` gets `CsvHandler.process`: a call Python never makes there.

## 2. pylspt's decisions, kept

| # | Decision | Why |
|---|---|---|
| 1 | The key records which parameters are the same object: `alias[i]` = the first parameter holding the same object as `i`. `same` is `[0, 0]`, `different` `[0, 1]`: two keys | a fingerprint says what an object holds, never which object it is. Identity needs its own place in the key |
| 2 | Only a slot holding exactly one object with an identity can alias. A slot with several values aliases nothing | after CPA (step 6) one value per slot is the usual case |
| 3 | `materialize` gives parameter `i` the origin of parameter `alias[i]`, so a write through one is a read through the other. The summary names the object by its first parameter | one write, landing once |
| 4 | **Only top-level parameters**: `a.peer is b` is not seen (`nested`, a known limit pinned in both modes). The fix is pairs of paths in the key, listed for later | top-level aliasing (`f(x, x)`, `self.merge(self)`) is the common case and cheap; pairs of paths cost a walk over every pair of reachable objects at every call |

"First parameter" is enough because "same object" is an equivalence: pointing each member of a group at its first member describes it
completely, in one byte per parameter, and equal patterns compare equal with `==`.

## 3. What this repo changes in them: the stable id

pylspt: "the stable encoding of a key (part 05) will include `alias`; nothing to do for that now". Here part 05 exists: every context's
id is a digest of its key (`CanonKey`, `canon.rs`), and the wire carries those ids (`Frame::contexts`). **Adding a field to `CanonKey`
changes every digest**: every context id in every wire, and the golden, would move for code that has no aliasing at all.

So `alias` goes into `CanonKey` only when it is not the identity (README §5 rule 7):

```rust
#[derive(serde::Serialize)]
struct CanonKey {
    /* func, args, free, defaults: as today */
    #[serde(skip_serializing_if = "is_identity")]
    alias: Vec<u8>,          // [0, 1, 2, …] is the identity: left out
}
fn is_identity(a: &Vec<u8>) -> bool { a.iter().enumerate().all(|(i, &j)| i == j as usize) }
```

**Rust note.** `#[serde(skip_serializing_if = "path")]` makes serde call that function on the field and leave the field out of the
output when it returns `true`. The digest of a key without aliasing is then byte-for-byte what it was before the field existed.

`Context` (`key.rs`) gains `alias: SmallVec<[u8; 4]>`, and `key` fills it from the bound slots (per combination, after CS6).

## 4. The fixture and the commit

| Fixture | Expected |
|---|---|
| `04-class-summaries/python/aliases.py` (CS0: inline passes; summary fails `same` and `different`, the `.process` under `set_and_read` missing; `nested` passes, its block is the known limit) | `same`: `JsonHandler.process`; `different`: `CsvHandler.process`; `nested`: no edge in summary mode (a mode-specific block: inline finds it) |
| **new**, `self_merge.py` | `def merge(self, other): other.x = J(); self.x.run()`, called `a.merge(a)`: `J.run` |

### CS7. The alias vector

| | |
|---|---|
| Change | §2, §3 |
| Failing tests | `p04_07_aliases`: `check_both("04-class-summaries/python/aliases.py")`; `self_merge.py`; a unit test in `canon.rs`: a key with the identity alias has the same digest as before the change (pin one digest from today) |
| Proof | `cargo test` in `languages/python`; **the golden does not move** (no aliasing on Jarvis is expected: [check]) |

## 5. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | The receiver is slot 0, so `a.merge(a)` aliases slots 0 and 1 with no special case | CS7 |
| 2 | An alias between a parameter and a closure's free value (`Root::Free`) is not covered; say whether any fixture needs it | CS7 |

Next: [`08-recursion-with-objects.md`](08-recursion-with-objects.md)
