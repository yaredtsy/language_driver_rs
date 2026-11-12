# 05.09 A def the engine found past something it could not see

```
a def past an unknown base
├── 1. what is wrong, with the line that shows it
├── 2. the decision (D28)
├── 3. the design
├── 4. the fixtures
├── 5. the steps, each with its failing tests and its proof
├── 6. what is not in this file
└── 7. to check while building
```

Written on 2026-11-12. §1 was **run** (the ignored helper `dump_graph_of_a_real_project`, on a Flask service of the owner, at `fd26c4a`).
§3 to §5 are from reading the code: **not built**. Every `[check]` is a claim to verify before depending on it.

## 1. What is wrong

```python
class SAModel(SA_MANAGER.Model, CRUDMixin): ...          # SA_MANAGER.Model is a class made at run time
class File(SAModel, OwnedResourceMixin): ...

def get_files(...):
    result = File.get_multiple_by_owner(owner_id=..., ...)   # the method is OwnedResourceMixin's
```

What the helper printed for the entry `get_files`: one context, and

```
app/api/file.py:162 File.get_multiple_by_owner( <- ty: a type that names no def
      [(bound method <class 'File'>.get_multiple_by_owner(owner_id=None, **kwargs) -> ModelQueryResult) & Unknown]
```

The tree is empty. The owner saw this.

**Why.** A method is looked up through the base classes in order. The engine passes `SA_MANAGER.Model`, which it cannot see into, and
then finds the method in the mixin. Its answer is an **intersection**: the method it found, `&`, `Unknown`. The `Unknown` says one thing:
if the class it could not see defines a method of that name, that one runs instead.

`Sem::shape` has no arm for an intersection, so the type is `Shape::Other`, `callee_definition` is `None`, and `call_values` marks the
site `Unresolved`. R3's `or_engine_callee` asks the engine again and gets the same type, which is not exact.

The same entry file showed two more forms on other entries (`FilesResource.post`, after R3):

| The engine says | Example | In this file |
|---|---|---|
| `(bound method <class 'File'>.aquery(…)) & Unknown` | `File.aquery(session)` | yes |
| `(bound method (AsyncQuery & Unknown).filter_by(…)) & Unknown` | `File.aquery(session).filter_by(…)`: the receiver is doubtful too | yes: the def is still the one the engine found |
| `Unknown \| (bound method File.to_dict(…))` | `file.to_dict()`, where `file` may be an untyped value | **no** (§6): a union says the value may be something else entirely |

This matters most where the engine sees few base classes: a class made at run time, and every library base when no virtual
environment is found.

## 2. The decision (D28)

Owner, 2026-11-12: **yes.**

**When the engine's type of a callee is one def with `Unknown` beside it in an intersection, the call is drawn to that def, with its own
basis, shown as a mark in the tree and on the wire, and counted.**

- It relaxes rule 8 ("never guess") in this one place, and says so on the edge. The def is the engine's, not invented.
- It can be wrong in one way: the class the engine could not see defines the same name. The mark is what a reader has against that.
- Plain `Unknown` stays `Unresolved`. A union with `Unknown` stays `Unresolved` (§6).
- When the base becomes visible (a virtual environment is found), the engine's answer is exact and the mark is gone, with no code change.

Rejected: strip `& Unknown` in `Sem::shape`, so everything after it takes the def as exact. The doubt is lost where it is read, nothing
can mark or count the edge, and every value changes, not only callees (values go into context keys).

## 3. The design

The doubt travels as data: the engine → `sem.rs` → the call site → the edge → the tree and the wire.

### 3.1 Core: `graph.rs`, `render.rs`, `wire.rs` (step R2a)

```rust
pub enum Basis {
    Path,
    Type,
    Name,
    /// The engine named this def, and said it could not see everything that may be in front of it (an intersection with an unknown
    /// type): a method found after a base class that is not resolvable. The def is the engine's; that it is the one called is likely,
    /// not certain.
    Likely,   // new, last, so the derived order of the three others does not move
}

pub enum Mark { /* … */ Likely }      // as_str: "Likely"
```

- **The tree.** A node made from an edge whose basis is `Basis::Likely` carries `Mark::Likely` (a call and a construction alike). The
  mark is on that node only, not on what is under it. The text form draws it as the other marks are drawn.
- **The wire.** `Frame::marks` already carries the marks as strings, **lowercased** (`head` writes `as_str().to_ascii_lowercase()`: `recursive`, `cut`), so `"likely"` appears there. No new field, and
  `WIRE_FORMAT_VERSION` stays 2 (D23: nothing changes what a node means; a reader that does not know the mark draws the call).
- **The merged wire** keys on it, as it keys on `passed` (D22): a likely call and a certain call of one def are two nodes, never one count. The wire has no
  field for it, so the key reads the mark (`is_likely`, a case-insensitive match on `Mark::Likely.as_str()`).
- **Edges of one group in the tree.** `children_of` groups edges by (site, def, kind, passed); `likely` is not in that key. If one edge of a group is
  `Likely`, the node is (errs on the side of doubt). With R2b's rule that the basis is the site's, a group never mixes the two.
- **An external with `Basis::Likely`** gets the mark too (§3.2 lets `External` be likely); it is only drawn with `show_external`.
- `Basis` and `Mark` are `#[non_exhaustive]`, so TypeScript's and Python's `match`es keep compiling. `[check]` that no code orders or
  compares `Basis` values in a way a fourth variant changes.

**Rust note.** `#[derive(PartialOrd, Ord)]` on an enum orders the variants by the order they are written in. That is why the new one
goes last: nothing that was `<` or `>` before changes.

### 3.2 Python (step R2b)

| Where | Change |
|---|---|
| `sem.rs` | `Sem::likely_def(ty, file) -> Option<Type<'db>>`: `ty` is `Type::Intersection`, and its positive parts are **exactly one** type whose shape is `Function`, `BoundMethod` or `Class`, plus one or more of shape `Unknown`. It returns that one part. Anything else is `None`. Negative parts are ignored: they say what a value is not, which does not change which def was found `[check]` |
| `interp/call.rs`, `or_engine_callee` | today: exact or not. New third answer: when the type (or every member of a union) is exact **or** `likely_def` gives a part, and at least one is likely, the values are `lift` of the exact parts and of the likely parts, and the site is **likely** |
| `interp/call.rs`, `call_values` · `interp/defer.rs`, `DeferredOp::Call` | the parameter and field `named: bool` becomes a small enum, `How { Named, Typed, Likely }`. `Likely` gives `Basis::Likely` for every target the site calls (`Call`, `Construct`, `External`); the two others are today's `true` and `false`. No other line of `call_values` changes |
| `interp/core_graph.rs` | a counter, `Stats.language["likely_callee"]`: the number of edges with `Basis::Likely` |

- The basis is the site's, not each value's. A site where the walk holds one callable value and the engine adds a likely one is all
  `Likely`: rare, and the mark errs on the side of doubt.
- `sem.rs` is the only place that knows how the engine writes the doubt (rule 4). `IntersectionType::iter_positive` is public; nothing is
  added to the vendored engine `[check]`.
- The counter is a new key in the `language` map, so the stats line of **every `.graph` snapshot gains one key** and nothing else.

## 4. The fixtures

`languages/python/crates/python-analysis/tests/fixtures/11-likely-callee/python/`. No `# expect:` blocks: read by `tests/api.rs`.

`base.py` (shared):

```python
from nowhere import Hidden  # no such module: the engine cannot see this class


def leaf():
    pass


class Mixin:
    @classmethod
    def find(cls):
        leaf()

    def save(self):
        leaf()


class Model(Hidden, Mixin):
    def own(self):
        leaf()
```

| File | The program | Entry | Expected |
|---|---|---|---|
| `classmethod.py` | `def run(): Model.find()` | `run` | an edge to `Mixin.find`, `Likely` |
| `instance.py` | `def run(m: Model): m.save()` | `run` | an edge to `Mixin.save`, `Likely` |
| `built_here.py` | `def run(): Model().save()` | `run` | an edge to `Mixin.save`; say in the report which basis the construction and the call have |
| `own.py` | `def run(m: Model): m.own()` | `run` | an edge to `Model.own`, **not** `Likely`: the method is found before the unknown base `[check]` |
| `either_unknown.py` | `def run(m: Model, flag): (m if flag else missing).save()`, `missing` defined nowhere | `run` | `Unresolved`: a union with `Unknown` |
| `untyped.py` | `def run(x): x.save()` | `run` | `Unresolved`: plain `Unknown` |

## 5. The steps

Each step is one commit. Tests first, seen red for the stated reason. R3 (`fd26c4a`) is before both.

### R2a. Core: the basis and the mark

| | |
|---|---|
| Change | §3.1. No language emits `Basis::Likely` yet |
| Failing tests | **core**, hand-written graphs like the existing ones. `render.rs`: (a) a `Context` edge with `Basis::Likely`: the child has `Mark::Likely`, its own child has not. (b) a `Construct` edge with `Basis::Likely`: the class node has the mark. (c) the text form shows it. `wire.rs`: (d) the frame's `marks` has `"likely"`. (e) one def called at two sites of one caller, one edge `Likely` and one `Type`: merged gives **two** children. (f) both `Likely`: one child, `call_count` 1, the mark kept |
| Proof | `cd core && cargo test`; then `cargo test` in `languages/python` and `languages/typescript` (rule 5), **no snapshot changed**; `cargo clippy -p analyzer-core --all-targets -- -D warnings` |

The TypeScript session works in `core/` too: the owner coordinates before this step starts.

### R2b. Python: the likely callee

| | |
|---|---|
| Change | §3.2 |
| Failing tests | `tests/api.rs`, project `11-likely-callee`, with `call_graph` and `call_wire`. **Red today, each because the site is `Unresolved`:** (a) `classmethod.py`: an edge to a context of `Mixin.find` with basis `Likely`; `stats.language["likely_callee"]` is 1; the wire's child has `"likely"` in `marks` and `leaf` under it without the mark. (b) `instance.py`: the same for `Mixin.save`. (c) `built_here.py`: an edge to `Mixin.save`. **Green before and after:** (d) `own.py`: an edge to `Model.own`, not `Likely`, counter 0. (e) `either_unknown.py` and (f) `untyped.py`: one `Unresolved` edge, counter 0. **Unit, `sem.rs`:** (g) `likely_def` on the type of `Model.find` gives the bound method; on the type of `Model.own` and on `Unknown` it gives `None` |
| Snapshots | every `.graph` snapshot gains the key `"likely_callee": 0` in its stats line. Nothing else may change: if an edge or a wire line moves, stop and report it |
| Docs | D28 in `docs/07-decisions-and-questions.md`. CLAUDE.md rule 8 gains "(one stated exception: D28)". [`07-the-root.md`](07-the-root.md) §6, first row: "R2, this file". `docs/README.md`. `languages/python/ENGINE.md`: the engine writes "found past an unknown base" as an intersection with `Unknown`, and which accessor reads it |
| Proof | `cargo test` in `languages/python`; `cargo clippy -p python-analysis --all-targets -- -D warnings`. By hand, the project of §1 (the owner gives the folder): entry `get_files` of `app/api/file.py` has more than one context, line 162 is not in the unresolved list, and its edge is `Likely`. Report the helper's tally for `FilesResource.post` before and after |

After R2b is pushed and CI is green, the V-NOC driver takes the new `rev`, and its agent is told that `marks` may hold `"likely"`.

## 6. What is not in this file

| Thing | Why not | Where it goes |
|---|---|---|
| A union with `Unknown` (`Unknown \| bound method File.to_dict`) | it says the value may be another thing entirely, not "this def unless something is in front of it". Seen on the owner's project (one site of `FilesResource.post`) | the owner decides, with a count from a real project |
| A def under `functools.wraps` (`_Wrapped[…]`) | another pattern | [`07-the-root.md`](07-the-root.md) §6 |
| An attribute read that is not a call (a property found past an unknown base) | accessors go through `call_accessor`, not `or_engine_callee` | when a real project shows one |
| A `Pending` callee that resolves to such a type in the fixpoint | as in [`08-the-engines-callee.md`](08-the-engines-callee.md) §6 | with that row |
| TypeScript and Rust emitting `Basis::Likely` | no case yet | their own plans |

## 7. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | Nothing in core, TypeScript or Python orders or compares `Basis` so that a fourth variant, written last, changes an answer | R2a. **Holds.** The only ordering is the derived `Ord` of `EdgeTo` and `Edge` (the sort of `Graph::edges`), which a new last variant moves only between two edges that differ in nothing but the basis. The only reads of a basis are `EdgeTo::basis()` (one test in `solve.rs`) and the renderer's new `== Basis::Likely`; no `match` on `Basis` outside core, and none in core had to change. Python's `summary_tree` matches `Mark` with a `_ => None` arm, so it drops `Likely`: nothing emits it yet, R2b decides |
| 2 | The text form and the wire take a new mark with no other change (`Mark::as_str`, the wire's `marks`) | R2a tests (c), (d). **Holds, with one correction.** `to_text` and `Frame::marks` took it with no change. The wire writes **`"likely"`**, lowercase, like every mark (the plan said `"Likely"`). The V-NOC driver's agent should be told that spelling (claim 8) |
| 3 | With an unresolved import as a base, the engine types `Model.find` as `(bound method …) & Unknown`, an intersection whose positive parts are the method and `Unknown` | R2b test (g); if the fixture gives another type, find a base that gives this one and report |
| 4 | `Type::Intersection` and `iter_positive` are reachable from `sem.rs` with no change to the vendored engine | R2b |
| 5 | A method defined in the class itself, before the unknown base, is typed exactly | R2b test (d) |
| 6 | The walk's own lookup (`eval_attribute` on a class or an object it holds) gives a value `or_engine_callee` sees as "cannot call" in all three red fixtures | R2b tests (a) to (c) |
| 7 | Ignoring negative parts is right: say whether any intersection with a negative part reaches `likely_def` in the test run | R2b, report only |
| 8 | The V-NOC driver tolerates a mark it does not know | `vnoc`, not here |

Next: [`../06-roadmap.md`](../06-roadmap.md)
