# 05.05 Entries and targets: a fix to the Python analyzer and to core

```
entries and targets
├── 1. what is wrong, with the line that shows it
├── 2. the decision (D20)
├── 3. the design
├── 4. the fixtures
├── 5. the steps, each with its failing tests and its proof
├── 6. what is not in this file
└── 7. to check while building
```

Written on 2026-11-12 from reading the code. **Nothing here was built or run when it was written.** Every `[check]` is a claim to verify before depending on it.

**Status (2026-11-12):** E1 is done (`8fafd0f`), E2 (`acc1b2a`), E3 (`9120198`), E4 (`3236d54`) and E5 (`58748f3`). EW (`a5625af`) is **proven by CI run #11 on `4aae76a`** (the `analysis (python, windows-latest)` job, green). E6 is done (`6676087`). Decisions of the owner while building: the kind on the wire is the outline's (`Method` for a Python method), made in E2; the TypeScript files are edited only after the TypeScript session has committed; EW stayed *unproven* until CI had run it, and is now proven (see above).

Where it comes from: a review of the V-NOC Python driver found three bugs whose cause is in the analyzer, not in the driver. pylspt fixed part
of them in `c395dde` ("wire names each target by file, qualname and occurrence; call_wire_at"). This repo was ported from `670ae79`, one
commit earlier (D14), so it has none of that. This file does not port `c395dde`: it puts the same information where this repo's design says
it belongs (the graph's table of definitions, then the wire as a view of it).

## 1. What is wrong

| # | Bug | Where | What happens |
|---|---|---|---|
| B1 | A frame does not say which file its call site is in | `core/analyzer-core/src/wire.rs`, `struct Frame` | The line and column are computed in the right file (the constructor's, `wire.rs` `open`: "for a construction it is the constructor, not the class"), but a reader of `Frame` only has the parent's name. For `class Child(Base): pass` with `Base.__init__` in another file, the reader takes the class's file and opens the wrong place. No fixture has a constructor in another file |
| B2 | A class cannot be the entry | `python-analysis/src/sem.rs` `Sem::entry`, `interp/mod.rs` `solve` | `Sem::entry` matches only `Stmt::FunctionDef` for the last name part, and `solve` then asks for a function node. A class gives `None`. The driver worked around it by picking `__init__` from the outline, which misses an inherited one |
| B3 | With two defs of one name, the second cannot be asked for | `Sem::entry`, `lib.rs` `call_graph` / `call_wire` | The entry is a dotted `&str` with no occurrence. For twin functions the first is returned; for a twin *container* class the last one is kept (`next` is overwritten). The two rules disagree |
| B4 | The outline's name for a nested function finds nothing | `Sem::entry`: `dotted.split('.')` | The outline says `top.<locals>.inner` (frozen, rule 10). Split on `.` it becomes `top`, `<locals>`, `inner`, and `<locals>` matches no statement. Only the unofficial `top.inner` works, which is what the fixtures use |
| B5 | A def inside a block is never found | `Sem::entry`: `for stmt in body` | Only the direct statements of a body are read. A def under a module-level `if`, `try`, `with`, `for`, `while` or `match` is not an entry. This is the usual place for twins (`if WIN: class Impl … else: class Impl …`), so B3 is not reachable at all there |
| B6 | Merging joins two different defs into one node | `core/analyzer-core/src/wire.rs` `group` | The key is `target_qname` alone. Two defs with one qualname in one module, called side by side, become one node with `call_count` 1 and their children mixed |
| B7 | A frame does not say what its target is | `struct Frame` | `target_qname` is a display name (with the module in Python). A driver cannot turn it back into (file, outline qualname, occurrence), which is what its protocol speaks. `Graph::defs` has the file and the kind, but not the outline's qualname and not the occurrence |

B5 and B6 were not in the review; they were found while checking it.

## 2. The decision (D20)

1. **A def is named the same way going in and coming out: (file, outline qualname, occurrence).** The outline of `<lang>-syntax` is the
   one naming rule. A request names its entry that way; every row of `Graph::defs` and every frame of the wire names its target that way.
   `occurrence` is 0-based (D18).
2. **The information lives in the graph; the wire only shows it.** `DefInfo` gains the outline's qualname and the occurrence. `Frame`
   reads them from `DefInfo`. Rule 7 and D17 hold: nothing is on the wire that is not in the graph.
3. **A graph's entry may be a construction.** `Graph::entries` holds a `Root`: a context, or a class with its constructor's context. The
   renderer draws a class root exactly as it draws a class callee, so the two cannot differ.
4. **`Entry` moves to core.** TypeScript has it; Python is its second user (rule of two). Both languages take
   `&analyzer_core::graph::Entry`.
5. **Additive on the wire.** No field of `Frame` is removed or renamed (D17). The merge key changes from the display name to the
   target's identity, which changes the merged wire only where B6 made it wrong.

Rejected: `call_graph_at` / `call_wire_at` next to the old functions (pylspt's way). It keeps two entry lookups with two naming rules, and the
old one is the buggy one.

## 3. The design

### 3.1 Core: `graph.rs`

```rust
/// A def of one file, as the outline names it: its qualname and which of the same-named defs it is (0-based, in source order).
/// Moved from typescript-analysis, unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    pub qualname: &'a str,
    pub occurrence: u16,
}

/// What a tree starts from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum Root {
    /// A function: the tree is its calls.
    Context(ContextId),
    /// A class: the tree is its constructor's calls. `init` is `None` when the class has no constructor function in project code.
    Construct { class: StableDef, init: Option<ContextId> },
}

pub struct DefInfo {
    pub id: StableDef,
    pub qname: String,       // unchanged: the display name
    /// The outline's name of this def in `file`. With `file` and `occurrence` it names the def in a request.
    pub qualname: String,    // new
    /// The number of defs with the same `qualname` before this one in `file`, in source order (D18).
    pub occurrence: u16,     // new
    pub kind: DefKind,
    pub file: String,
    pub range: Span,
}

pub struct Graph {
    pub entries: Vec<Root>,  // was Vec<ContextId>
    // the rest unchanged
}
```

- `DefInfo::new` takes the two new fields as required parameters (seven in all). They are not optional: a row without them is the bug.
- `Root::Construct` has the same two fields as `EdgeTo::Construct` minus `basis`, on purpose: it is the same thing, without a caller.
- **Rust note for the implementer.** `#[non_exhaustive]` on an enum means another crate's `match` must have a `_ =>` arm, so core can add
  a variant later without breaking the languages. Inside core the `match` is still checked for every variant.

### 3.2 Core: `render.rs`

`render(graph, root: &Root, opts) -> Node`.

| Root | The root node |
|---|---|
| `Root::Context(id)` | as today: `func = id.func`, `contexts = [id]` |
| `Root::Construct { class, init }` | what `Renderer::class` builds for a callee, with `site: None`: `func = class`, `qname` from the class's row, `contexts = init` (zero or one) |

The path set for rule 8 starts with the root's `func` in both cases. Nothing else in the renderer changes.

### 3.3 Core: `wire.rs`

`wire(graph, root: &Root, sources, opts) -> Frame`.

```rust
pub struct Frame {
    pub target_qname: String,              // unchanged
    pub target_id: String,                 // unchanged
    /// The target's file, relative to the project root, with `/`. `None` when the graph has no row for it (external, unresolved).
    pub target_file: Option<String>,       // new
    /// The outline's qualname of the target in `target_file`. Empty when `target_file` is `None`.
    pub target_qualname: String,           // new
    /// 0-based (D18). pylspt's `target_nth` was 1-based.
    pub target_occurrence: u16,            // new
    /// `None` when the graph has no row for the target.
    pub target_kind: Option<DefKind>,      // new
    pub call_count: u32,                   // unchanged
    pub call_site: Option<LineSpan>,       // unchanged
    /// The file `call_site` is in. `None` for the root. For a call made in an inherited constructor it is the constructor's file.
    pub call_site_file: Option<String>,    // new
    pub contexts: Vec<ContextId>,          // unchanged
    pub marks: Vec<String>,                // unchanged
    pub children: Vec<Frame>,              // unchanged
}
```

- All four `target_*` fields come from `graph.def(&node.func)` in `head`. `call_site_file` is `caller.file`, the same row `head` already
  reads for the position, so the file and the position cannot disagree.
- **The merge key** in `group` becomes `(target_file, target_qualname, target_occurrence, target_qname)`. For project code the first
  three decide; for externals and unresolved leaves (no row) `target_qname` decides, as today.
- `target_file` is relative (pylspt's was absolute): rule 6, and `DefInfo::file` already is. The driver joins it with the root it opened.

### 3.4 Python: the entry lookup

`Sem::entry(file, &Entry) -> Option<Definition>` replaces the dotted lookup. One way to find a def:

1. `python_syntax::outline(text, marker)` of the file (python-analysis already depends on python-syntax).
2. The `occurrence`th def whose `qualname` equals `entry.qualname`, in the outline's order (source order). `None` if there are fewer.
3. The AST statement whose **name** starts at that def's `name_range.start`: a `Visitor` over every statement, blocks included, the way
   `nth_of` walks. A `FunctionDef` or a `ClassDef`; its `definition(&model)` is the answer.

The same per-file table gives `DefInfo::qualname` and `DefInfo::occurrence` for `core_graph::def_info`: look the def up by the start of its
name. Cache it per file in `Sem`, next to `nth` (`Rc<…>` in a `RefCell`, the same pattern).

- The dotted form (`top.inner`) is no longer accepted. The fixture harness uses it in its `# expect:` blocks (19 entries, for example
  `outer.inner`, `Host.meth.Local.m`). The harness, not `Sem`, translates: the outline def whose qualname with every `.<locals>` removed
  equals the block's name, occurrence 0. Test-only code in `interp/fixture.rs`; the fixtures and the snapshot headers do not change.
- `StableDef::Path::nth` is **not** touched. It is hashed into every context digest (`stable::hashed_def`), and a digest is an identity
  outside the process. `nth_of` counts by a name without `<locals>`, the outline counts with it, so they can differ in one case: a
  function `top` with a nested `inner` and a class `top` with a method `inner` in one file (`nth` 0 and 1, `occurrence` 0 and 0). D18
  says they are the same number; E1 pinned the difference with a test (`08-entries/nth.py`) and D18 is corrected.

### 3.5 Python: a class as the entry

In `interp::solve`, when `Sem::entry` gives a class:

1. Run the code a callee runs: `Walk::construct_summary(sol, class, site = def_range(class), &EvaluatedArgs::default(), Basis::Type, &mut targets, &mut env)` (`Basis::Type`: the root draws no edge; with `Options::constructor_effects` it also replays the constructor's writes, as a `Child()` call does).
2. Take the `Target::Construct { class, init, .. }` it pushed. That is the root: `Root::Construct { class, init }`.
3. `core_graph` starts reaching from `init` (if any) and always pushes the class's `def_info`.

It is the same function a `Child()` call site goes through, so `init_of` (the MRO), the "project code only" filter and the binding of
`self` are shared, not copied. A class with no project `__init__` gives `Some(graph)` with a root that has no children, **not** `None`:
`None` means "no such def".

`solve` returns the root as a small enum instead of a bare `ContextId`; `call_graph`, `call_frame` and `summary_tree` pass it on.
`inline_tree` (the fixture cross-check, not public) returns `None` for a class entry: no fixture asks for one in inline mode.

### 3.6 The public API

```rust
// python-analysis/src/lib.rs
pub use analyzer_core::graph::{Entry, Root, /* … as today */};
pub fn call_graph(project: &Project, file: &str, entry: &Entry<'_>, opts: &Options) -> Option<Graph>;
pub fn call_wire(project: &Project, file: &str, entry: &Entry<'_>, opts: &Options, merged: bool) -> Option<Frame>;
```

TypeScript: `pub use analyzer_core::graph::Entry;` replaces its own struct, so its callers do not change. Its `call_graph` wraps the
entry as `Root::Context`. A TypeScript class as entry is not built here (§6).

## 4. The fixtures

A new project directory, `languages/python/crates/python-analysis/tests/fixtures/08-entries/python/`. No `# expect:` blocks: these are
read by `tests/api.rs` through the public API, so `every_entry` skips them and its `(29, 6)` count does not move `[check]`.

`twins.py` (the condition is not one ty can decide, so neither branch is unreachable):

```python
import os


def a():
    pass


def b():
    pass


if os.environ.get("X"):

    def f():
        a()

    class Impl:
        def run(self):
            a()

else:

    def f():
        b()

    class Impl:
        def run(self):
            b()
```

`nested.py`:

```python
def leaf():
    pass


def top():
    def inner():
        leaf()

    inner()
```

`blocks.py`:

```python
def leaf():
    pass


try:

    def in_try():
        leaf()

except ImportError:
    pass

with open(__file__):

    def in_with():
        leaf()
```

`inherit_b.py` (the call to `helper` is on line 7):

```python
def helper():
    pass


class Base:
    def __init__(self):
        helper()
```

`inherit_a.py` (the call to `Child` is on line 9):

```python
from inherit_b import Base


class Child(Base):
    pass


def make():
    Child()
```

`no_init.py`:

```python
class Plain:
    pass
```

## 5. The steps

Each step is one commit (the owner asks for the commit). In each: write the tests first and see them fail for the stated reason, then fix.

**Rust note.** A test against a signature that does not exist yet does not fail, it does not compile, and then no test in the crate runs.
So where a step changes a signature, do it in this order: (1) change the signature and adapt the callers, keeping the old behaviour, so
everything compiles and is green; (2) add the tests, see them red; (3) change the behaviour, see them green.

Core changes (E1–E3) must keep both languages green (rule 5). After each: `cd core && cargo test`, then `cargo test` in
`languages/python` and `languages/typescript`.

### E1. `DefInfo` names its def

| | |
|---|---|
| Change | `DefInfo` gains `qualname` and `occurrence` (§3.1). Python fills them from the outline (§3.4, the per-file table only; `Sem::entry` is not touched yet). TypeScript fills them from the outline def it already holds (`Func { def }`; the occurrence is computed in `sem.rs::describe`). Python's `graph_text` printed no def rows, so E1 adds them: `def <qname> = <qualname>#<occurrence> <kind> <file>` |
| Failing tests | **python** (`interp/fixture.rs`): for every fixture entry, every row of `graph.defs` whose file is a project file has a `(qualname, occurrence)` that the outline of that file also has, at the same name position. **python**: a unit test with `def top(): def inner()` and `class top: def inner()` in one file: both `inner` rows have `occurrence` 0 while their `StableDef::Path::nth` are 0 and 1. **typescript**: the same row check on three call fixtures (`tests/def_names.rs`). Python's row test compares the kind as "a class or not": the outline says `Method` where `DefInfo::kind` says `Function` (E2 settles it) |
| Snapshots | Python: every `.graph` file gains its def rows (35 files, lines added, none removed). TypeScript has no snapshot of def rows. Review that the diff is only that |
| Docs | D18: replace "which is what `StableDef::Path::nth` holds" with what the unit test showed |
| Proof | the three `cargo test` runs; `git diff --stat` on snapshots shows `.graph` files only |

### E2. `Frame` names its target and its call-site file; the merge key

| | |
|---|---|
| Change | §3.3. Extend `wire_text` in `interp/fixture.rs` to print `target_file`, `target_qualname`, `target_occurrence` and `call_site_file`, or the snapshots do not cover the new fields. **Python's `core_graph::def_info` takes the outline's kind** (`Method` for a method, not `Function`): the def rows of the `.graph` snapshots change in that one word, and `target_kind` says what the outline says (owner's decision). The row test of E1 then compares the kind exactly |
| Failing tests | **core** (`wire.rs` tests, hand-written graphs like the existing ones): (a) two defs with one `qname`, different `occurrence`, called at two sites of one caller: merged gives **two** children, each `call_count` 0 (fails today: one child, `call_count` 1). (b) the same def called twice: still one child, `call_count` 1. (c) a `Construct` edge whose class is in `a.py` and whose `init` is in `b.py`, with a call inside `init`: that call's frame has `call_site_file == Some("b.py")`, and the class frame has `target_file == Some("a.py")`. (d) an external leaf (`show_external` on): `target_file` and `target_kind` are `None`. (e) the root: `call_site_file` is `None`, `target_*` are filled. (f) the JSON of one frame, pinned as text, so the names and how `DefKind` is written cannot drift `[check]` |
| Snapshots | every `.wire` snapshot changes, in both languages: new fields on every line. Merged sections must change **only** where two twins were joined; list any such place in the commit message |
| Docs | `04-the-wire.md` §4: the field table gains the five fields and the new merge rule |
| Proof | the three `cargo test` runs; the Jarvis golden comparison (`out/golden/`) still content-equal on the old fields (pylspt's wire has not the new ones, so the comparison leaves them out), then regenerated |

### E3. `Root`, and `Entry` in core

| | |
|---|---|
| Change | §3.1, §3.2, the `wire` signature of §3.3, `Entry` moved (§3.6). Every caller wraps its context as `Root::Context`; behaviour is unchanged for functions |
| Failing tests | **core** (`render.rs` tests): (a) `Root::Construct` with an `init` that has two edges: the root is named by the class, has `site: None`, `contexts == [init]`, and two children. (b) `Root::Construct { init: None }`: a root with no children and no marks. (c) a `Construct` root whose `init` constructs the same class again: the inner node is `Recursive` (rule 8). **core** (`wire.rs`): (d) under a `Construct` root, a child's `call_site_file` is the `init`'s file |
| Proof | the three `cargo test` runs; no snapshot changes (`graph_text` prints a context root as it does today and a class root as `entries: new <Class>`); `grep -rn "struct Entry<" languages core` finds only core's (`typescript-syntax` has an unrelated private `struct Entry` in its outline builder) |

### E4. Python: the entry by (qualname, occurrence)

| | |
|---|---|
| Change | §3.4 and §3.6 for Python. The old `Sem::entry` body is deleted, not kept beside the new one. The fixture harness translates its dotted names |
| Failing tests | `tests/api.rs`, project `08-entries`: (a) `twins.py`, `f` occurrence 0: one child, `a`; occurrence 1: one child, `b`; the root's `target_occurrence` is the one asked for. (b) `Impl.run` occurrence 1: one child, `b`. (c) `f` occurrence 2: `None`. (d) `nested.py`, `top.<locals>.inner`: one child, `leaf`; `top.inner`: `None`. (e) `blocks.py`: `in_try` and `in_with` each have one child, `leaf`. (f) round trip on every file of `08-entries`: for every def of `python_syntax::outline`, asking for its `(qualname, occurrence)` gives `Some` for functions, and the root's `target_qualname` and `target_occurrence` are the ones asked for (classes join in E5) |
| Proof | `cargo test` in `languages/python`: the 129 fixture entries still pass with unchanged snapshots; `cargo test -p python-syntax` still does not compile the type checker |

### E5. Python: a class as the entry

| | |
|---|---|
| Change | §3.5 |
| Failing tests | `tests/api.rs`: (a) `inherit_a.py`, entry `Child`: `Some`; the root has `target_kind` class, `target_file` `inherit_a.py`, and one child `helper` with `target_file` `inherit_b.py`, `call_site_file` `inherit_b.py`, call-site line 7. (b) entry `make`: its child `Child` has `call_site_file` `inherit_a.py`, line 9, and the subtree under it equals the subtree under the root of (a), compared on `target_file`, `target_qualname`, `target_occurrence`, `call_site`, `call_site_file` and `marks` (not `contexts`). (c) `no_init.py`, entry `Plain`: `Some`, no children, no marks. (d) `twins.py`, `Impl` occurrence 1: `Some`, no children. (e) the round trip of E4 (f) now covers classes |
| Proof | `cargo test` in `languages/python`; one reviewed snapshot of the wire of `inherit_a.py` entries `Child` and `make`, side by side |

### EW. Windows: the root has no `\\?\` prefix

Added 2026-11-12. It does not depend on E1–E5 and can be done at any point before the push; it is listed here because the V-NOC driver
waits for it (`vnoc/docs/15-leave-pylspt.md` §3). Not run on Windows when written: from reading the code.

**What is wrong.** `Project::open` and `Project::file` (`python-analysis/src/sem.rs`) call `std::fs::canonicalize`. On Windows that returns
a *verbatim* path, `\\?\C:\proj` (and `\\?\UNC\server\share\proj` for a network share). The paths the engine hands back for files have no
such prefix `[check]`. Then `relative_file` cannot strip the root, so `DefInfo::file` and the wire's `target_file` / `call_site_file` come
out absolute, and a file given by an absolute path may not be found. The V-NOC driver hid this with its own `plain` function, which
its migration deletes. Its `plain` also had a bug: it turned `\\?\UNC\server\share` into the relative path `UNC\server\share`.

| | |
|---|---|
| Change | One private function in `sem.rs`, used by `Project::open` and `Project::file` right after `canonicalize`: `\\?\UNC\server\share\…` → `\\server\share\…`; `\\?\C:\…` → `C:\…`; anything else unchanged. It works on the path as text, so it compiles and is tested on every OS |
| Failing tests | **unit** (`sem.rs`, runs on every OS): the three rows above, as strings. **`tests/api.rs`** (runs on every OS, fails only on Windows before the fix): for the `08-entries` project opened by a relative directory, every `DefInfo::file` of a graph is relative and has no `\`, and after E2 every frame's `target_file` and `call_site_file` too; opening the same project by its canonical absolute path gives the same graph |
| Proof | this repo's CI, the `analysis` job for `python` on `windows-latest`, green on the pushed commit. That run is the only Windows proof: do not call the step done from a local run on another OS. **Status: proven by CI run #11 on `4aae76a`** (the `analysis (python, windows-latest)` job was green; until then it was unproven on Windows, by the owner's decision). Built: `plain` in `sem.rs` (four unit tests, one of them Unix-only for the non-UTF-8 path), called in `Project::open` and `Project::file`; `files_are_relative_and_the_root_does_not_change_the_answer` in `tests/api.rs`, green on macOS and able to fail only on Windows |

**Rust note.** A path on Windows is not guaranteed to be valid UTF-8, so `Path::to_str()` returns an `Option`. When it is `None`, return
the path unchanged: never fail and never guess (rule 8).

### E6. The rest of the docs

- `07-decisions-and-questions.md`: D20 is there; add to §5 any `[check]` of this file that turned out wrong.
- `02-what-moves-where.md` §4: the public surface (`Entry`, `Root`).
- `languages/typescript` docs: one line in `06-ts4-calls.md` saying `Entry` is core's.
- Run `scripts/check-all.sh`; `cargo fmt` and `cargo clippy -p <crate> -- -D warnings` on `analyzer-core`, `python-analysis`,
  `typescript-analysis` and the two CLIs.

## 6. What is not in this file

| Thing | Why not | Where it goes |
|---|---|---|
| A cancel flag for Python (`Cut::Cancelled` is never set from outside) | a different seam (row M18: core's cancel, "later Python") | its own step after TS5 |
| The solver recursing to `max_stack` = 256 | not a bug in the answer; the driver must give its request thread a big enough stack until the solver is iterative | driver, then a Python step |
| A TypeScript class as the entry | no user yet; `Root::Construct` makes it possible without another core change | TS5 |
| `python-cli calls` | **built** (PY7). It takes `--occurrence <n>`, with **no default**: a name that matches several defs is listed and the exit code is 2, the first is never picked | [`06-python-cli.md`](06-python-cli.md) §3.1 |
| The driver-host findings (timeout on write, cancel before request, lost exit reason) and the driver-only ones (`catch_unwind`, UNC paths) | they are in `vnoc`, not here | `vnoc` `docs/14-call-trees.md` |

After E5 the driver's part is small: send `(file, qualname, occurrence)` as the outline gave them (converting its 1-based occurrence, D18),
stop picking `__init__` itself, and read `call_site_file` instead of the parent's file.

## 7. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | `python_syntax::outline`'s `name_range.start` equals the AST name's start for every `def` and `class`, decorated ones included | E1's row test: **yes**, for every row of the 129 entries (decorated ones included) |
| 2 | TypeScript has the outline qualname and can compute the occurrence where it calls `DefInfo::new` | E1: **yes** (`sem.rs::describe`) |
| 3 | `DefKind` serializes as a plain string for `Function` and `Class` | E2 test (f) |
| 4 | `every_entry` skips a `.py` file with no `# expect:` block, so `08-entries` does not move the `(29, 6)` count | E4: **yes**. The six new files have no `# expect:` block, and both counts held: `(29, 6)` programs and `(35, 129)` programs and entries |
| 5 | `construct_summary` can be called on a `Walk` made for a class entry (today `Walk::new` is given a function def) | E5: **yes**. `Walk::new` and `frame_for` take any def (`def_position` handles a class), so `solve_class` makes the walk from the class and calls `construct_summary` as it is; the fallback was not needed |
| 6 | ty resolves `from inherit_b import Base` inside the fixture project, and `Child()` reaches `Base.__init__` through `init_of` | E5: **yes**. Test (b) sees `helper` under `Child` from `make`, with its call site in `inherit_b.py` |
| 7 | A class entry and a no-argument `Child()` callee give the same `init` context (same key), not only the same subtree | E5: **the same context**, function and digest. `the_constructor_context_of_a_class_entry_is_the_one_a_construction_has` compares `Root::Construct::init` of `Child` with the `init` of `make`'s `Construct` edge, and they are equal; the two objects are built at different sites and the digests are still equal, so for a constructor with no arguments the object's origin does not reach the key (shown for this case only) |

Next: [`06-python-cli.md`](06-python-cli.md)
