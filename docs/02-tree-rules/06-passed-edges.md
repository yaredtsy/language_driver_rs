# Note: an edge that passes a def instead of calling it (D22)

Status: **the shape was approved by the owner on 2026-11-12; step 2 (core) is built** (the sections below are the shape as approved, with the four additions of step 2: the wire format version, the pointer
to the `vnoc` side, classes, and text dumps). Steps 3 (TypeScript, by name), 4 (written inside an expression) and 5 (the counted remainder) are built; step 6 is not. Written on 2026-11-12 from reading `core/analyzer-core/src/{graph,solve,render,wire}.rs`, `core/analyzer-testkit`,
`languages/python/.../interp/core_graph.rs` and the TypeScript counter (`languages/typescript/ENGINE.md`, "Defs handed over and not called"). Every `[check]` is a claim to verify while building.

## Why

A def can be **handed over** instead of called: `onClick={handleClose}`, `xs.map(parse)`, `register(api.load)`, `{ f }`, `run({ onSuccess() {} })`, a D21 arrow in a local literal that is passed on.
The def is known, nothing calls it through a name the checker follows, so no edge reaches it and everything it calls is in no tree. The TypeScript counter measured it: `TaskDetailPanel` 0 written and
7 by name; the whole V-NOC frontend 3 written and 89 by name. One of the three losses the review found was a real call (`toggleNodeExpansion(nodeId)` inside an `onToggle` arrow).

**The owner decided (Q-TS3, option 2, recorded as D22):** the def that hands it over gets an edge, at the hand-over site, to the handed-over def's context, so the solver walks it and its calls are in the
tree. The edge is marked **passed, not called**, so no reader takes it for a call. This note is the shape of that mark in `core/`.

Rules that hold through all six steps (the owner's):

1. No def is renamed or removed. A passed edge is added; nothing else moves.
2. A def that is both called and passed in one body has **both edges, each at its own site**. They are two nodes, in the tree and on the wire, merged or not.
3. A def that nothing names stays uncounted and unreached (`const unused = () => {}`).
4. What is still not covered (an element access, `f.bind(…)`, `f.call(…)`) stays a number (step 5).

## The shape

### 1. The graph: one field on the edge

```rust
// graph.rs
 #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
+#[non_exhaustive]
 pub struct Edge {
     pub from: ContextId,
     pub site: Site,
     pub to: EdgeTo,
+    /// `true`: `from` hands the target over at `site` and does not call it there (`onClick={handleClose}`). The target is walked like any other,
+    /// so what it calls is in the graph, but this edge is not a call and is never counted as one. Only an edge to a `Context` can be passed.
+    pub passed: bool,
 }
+impl Edge {
+    /// An edge for a call. What every language built before D22.
+    pub fn new(from: ContextId, site: Site, to: EdgeTo) -> Self { Edge { from, site, to, passed: false } }
+    /// An edge for a def handed over.
+    pub fn passed(from: ContextId, site: Site, to: EdgeTo) -> Self { Edge { from, site, to, passed: true } }
+}
```

`passed` is last, so the derived order of `Edge` (and so the sorted `Graph::edges`, rule 11) is what it was for every graph without a passed edge.

**Why a field on `Edge`, and not the three places the owner ruled out first:**

| Place | Why not |
|---|---|
| a new `EdgeTo` / `Target` kind (`Passed`) | The thing handed over *is* a context, walked by the solver and drawn like a call's target. A new kind needs an arm in the solver, the renderer, the wire, the invariant checker, Python's `core_graph.rs` and every language's tests, to say the same thing as `Context` |
| a `Basis` value | `Basis` says *how sure*: a name fixes `onClick={handleClose}` as surely as it fixes `handleClose()`, and it is still not a call. "How sure" and "what the code does" are two questions; one enum for both would make `Name` mean two things |
| a third field of `EdgeTo::Context(id, basis, passed)` | It is the type-safe place (no passed `External`). But it changes every pattern `EdgeTo::Context(id, _)` in four crates and the renderer reads it at `child(edge)`, where the edge is already in hand. The price of the field on `Edge` is one invariant (below) |

A `bool`, not an enum: there are two states. If a third ever exists ("returned", "stored"), that is a decision of its own and an enum then.

**Invariant** (`analyzer_testkit::graph::check_invariants`): an edge with `passed` has `to` = `EdgeTo::Context`. A passed `Construct`, `External` or `Unresolved` is a language bug and the checker names it.

**`Edge` becomes `#[non_exhaustive]`** (rule 3 says returned structs are), so a language cannot write `Edge { … }` and silently drop the field: it calls `Edge::new` or `Edge::passed`. That is a mechanical change at
the ~20 places that build an edge today (the core's own tests, the testkit, `solve.rs`, and **one** in Python, `core_graph.rs:174`).

### 2. The solver: the language says it per site

```rust
// solve.rs
 pub struct SiteCalls<L: Lang + ?Sized> {
     pub site: Site,
     pub targets: Vec<Target<L>>,
+    /// The targets are handed over at this site, not called (D22). Only `Target::Call` can be.
+    pub passed: bool,
 }
+impl SiteCalls { pub fn new(..) -> Self /* passed: false, unchanged */; pub fn passed(site: Site, targets: Vec<Target<L>>) -> Self }
```

`SiteCalls` becomes `#[non_exhaustive]`. The solver stamps `Edge.passed = calls.passed && matches!(target, Target::Call(..))`, with a `debug_assert!` that a language did not pass anything else, and
`discover`s the target exactly as for a call, **so a passed def is walked and its context is in the graph**. Nothing in the walk, the memo, the limits or the key changes. `Stats` gets no core counter: a language
counts its own passed edges (TypeScript's `Breakdown` splits them out of "call").

What a language owes the solver: the **hand-over site** (a span inside the handing def's body, in source order, the span of the name: `handleClose` in `onClick={handleClose}`, `api.load` in `register(api.load)`) and a
`Target::Call(f, key)`. What counts as a hand-over is the language's (TypeScript: steps 3 and 4).

### 3. The tree: `Node.passed`, drawn `[Passed]`

```rust
// render.rs
 pub struct Node {
     ...
     pub site: Option<Site>,
+    /// The parent hands this def over here and does not call it. Its children are what the def calls when it is walked.
+    pub passed: bool,
```

- **A field, not a `Mark`.** Marks are sorted, deduplicated and unioned, and describe a node's condition (`Recursive`, `Cut`, `Incomplete`, `External`, `Unresolved`). A passed node can also be `Recursive`
  (`xs.map(f)` inside `f`): both are true at once. And the merge key needs it as a value, not as a string in a list.
- **Rule 7 is amended: one node per site, callee function *and whether it is passed*.** `children_of` groups on `(Site, StableDef, u8)`; it becomes `(Site, StableDef, u8, bool)`. Two edges at one site to one
  def, one called and one passed, cannot happen in a walk of source (a name and a callee are different spans), but the key must not be the thing that depends on it.
- Rule 8 is unchanged: a passed def that is already on the path above is a leaf marked `Recursive` with `passed: true`, not entered.
- Rule 9 is unchanged: `only_marked` hides a passed node and everything under it, like any other.
- Order of siblings is unchanged: by site, so a passed node stands where its name is written.
- **Text** (`to_text`, the scenario files): ` [Passed]` right after the name, before the marks. `qname [Passed]`, `qname [Passed] [Recursive]`. A tree with no passed node prints exactly what it printed.

```
TaskDetailPanel
  useTasks
  handleBack [Passed]
    handleClose
  handleClose [Passed]
```

(`handleClose` appears twice: called inside `handleBack`, which the panel hands over, and handed over itself by `onClick={handleClose}`. Two nodes, as rule 2 says.)

### 4. The wire: `Frame.passed`

```rust
// wire.rs   (E2's fields are in the same struct; see "Order of work")
 pub struct Frame {
     ...
+    /// `true`: the parent hands this def over at `call_site` and does not call it. `call_site` is where it is handed over, `call_count`
+    /// counts the hand-overs joined into this node (in a merged wire), and `children` are what the def calls when walked. Absent when
+    /// `false`, so a wire without a passed node is byte for byte what it was.
+    #[serde(skip_serializing_if = "std::ops::Not::not")]
+    pub passed: bool,
```

- Additive (D17): no field removed or renamed; **absent when false**, so every wire a language built before D22 serializes the same bytes (Python's wire tests and snapshots stay as they are).
- `marks` is untouched and gets **no** `"passed"` string: a UI that keys on marks sees nothing new, and a UI that wants to show it differently reads one boolean.
- **The merged wire does not fold a passed node and a call node of one def.** `group` merges siblings that have one key; the key gains `passed`. After E2 the key is
  `(target_file, target_qualname, target_occurrence, target_qname)`; it becomes `(…, passed)`. Two passed nodes of one def merge as two calls do (`call_count` + 1, children concatenated); a passed node
  and a call node stay two siblings. A test pins it: `a` called at one site and passed at another is two frames merged or not.
- `[check]` whether V-NOC's reader of the wire (`vnoc/drivers/*`) tolerates a field it does not know. The field is absent unless a passed node exists, so a driver that rejects unknown fields fails only on
  a graph that has one; the TypeScript driver is the first that will.

#### The wire format version

`analyzer_core::wire::WIRE_FORMAT_VERSION: u32`, a plain number, documented as **bumped when a field is added that changes what a node means**: a reader that predates such a field draws a wrong tree
and does not fail, which ignoring an unknown field cannot catch. This repo has no handshake (drivers stay in `vnoc`, Q-M7); the driver reports the constant in `initialize` and the backend checks it
([`03-typescript/07-ts5-driver-needs.md`](../03-typescript/07-ts5-driver-needs.md) §8 says how, and is not repeated here).

- **1** is the wire through E3: D17's fields, and E2's `target_*` and `call_site_file` and the merge key by the target's identity. By the owner's rule E2 does **not** bump it: those fields say more about a
  node, they do not change what it is. The Python session did not plan a number and left this count to the owner; **the owner confirms it** (the alternative is to count E2's merge-key change as a
  change of meaning, which makes `passed` 3).
- **2** is `passed`. A test (`d22_the_wire_format_version_is_a_number_changed_on_purpose`) holds the value, so changing it is a decision with a line in the doc.

### 5. Python

Nothing yet, and its tests stay green: Python builds its graph by its own solver and writes `Edge::new` (the one line in `core_graph.rs`); its wire is the same bytes. Python's context-sensitive solver could
one day pass a callback to `map`; that is a decision for its session and needs nothing from this note beyond the field.

## Where a graph is printed as text

A passed edge is **shown as passed and is not counted as a call** wherever a graph becomes text. Each place says it only when it is true, so a graph with no passed edge prints what it always printed and no
snapshot changes:

| Where | What |
|---|---|
| the tree (`to_text`, the scenario files) | ` [Passed]` after the name |
| the TypeScript breakdown (`typescript-cli calls`, `Breakdown`) | `calls` no longer includes a passed edge; the edges line ends with `, N passed` when N > 0 (`Breakdown.passed`) |
| the TypeScript edge listing used by the tests (`edges_as_text`) | `pass X` where a call is `call X` |
| Python's graph snapshot printer (`graph_text`) | ` passed` at the end of an edge line, only when the flag is true |
| the graph's `Debug` dump (`typescript-cli calls --graph`) | `passed: true` is a field of `Edge`, so it is there as any field is |

## What this does not do

A passed edge says what the handing def **gives**, not what the receiver **calls**. `register(handler)`, where `register` calls its parameter, still has an unresolved `cb()` inside `register`
(`function_typed_value`, D16): the tree shows `handler` under whoever passes it, and `register`'s own call stays unresolved. Linking the two is what context sensitivity (C3, unscheduled) would do.

## Risks

- **The graph gets bigger.** Every handed-over body is now walked: on `TaskDetailPanel` the 7 handlers and what they reach. Contexts, tree nodes and graph time are measured **before and after at step 3**
  (graph time was 16–18 ms for 41 contexts).
- **A reader older than `passed` draws a handed-over def as a call.** It ignores the field it does not know, and the node looks like a call at the site where the def is only named. That is the
  case `WIRE_FORMAT_VERSION` exists for, and the `vnoc`-side items that make use of it (a versioned drivers folder, the driver reporting the version in `initialize` and the backend checking it) are written
  down in [`03-typescript/07-ts5-driver-needs.md`](../03-typescript/07-ts5-driver-needs.md) §8, not here. Nothing in this repo checks it.
- **A hook's dependency array is over-reported.** `useEffect(fn, [a, b])` hands `a` and `b` over by the rule (an element of an array that is an argument), and they are passed edges when they are project functions;
  the array only lists what the hook watches. The rule stays as it is (the owner's decision); the answer is pinned in the `handed-over` fixture (`inDependencyArray`) and written in `languages/typescript/ENGINE.md`.
- **A tree can show a def twice** (passed and called). Intended, and why `passed` is part of the key.
- **An unseen caller of a passed def is not an edge.** If the receiver calls it, the tree shows it under the handing def anyway: the tree says "this def gives X to something", not "this def runs X".
  That is the one sentence a reader needs, and what the `[Passed]` mark is for.

## Order of work

E2 (the Python session's entries and targets: `Root`, `DefInfo` names, `Frame`'s `target_*` fields, the merge key) is **in the working tree, not committed** (`wire.rs` has its `Frame` fields). Both changes edit
`Frame`, `head`, `group`, `Renderer::child` and `children_of`. **Step 2 starts after E2's commit**, on top of it, and adds `passed` to the key E2 sets; this note and the key are written to agree. The
TypeScript callers (`typescript-analysis`'s wire re-exports and its tests, `typescript-cli`) are E2's to fix, as agreed; step 2 touches only `Edge` literals there if any exist (none today: `tests/calls.rs`
reads `Edge { to, .. }`, which `#[non_exhaustive]` still allows inside a pattern).

| # | Do | Proof |
|---|---|---|
| 1 | this note, and D22 in `docs/07` | the owner's OK on the shape |
| 2 | **Done.** core: `Edge.passed` with `new`/`passed`, `SiteCalls::passed`, the solver stamp, `Node.passed`, `[Passed]`, `Frame.passed`, the merge key, the invariant; rule 7 in `02-the-rules.md` is amended in the same commit | a toy language in `core/` (below); every language's tests green; the wire of a graph with no passed edge is byte-identical |
| 3 | **Done** (TypeScript, by name; numbers in `languages/typescript/ENGINE.md`, "Passed edges, by name"). TypeScript, by name: every reference the counter finds is a passed edge at its name | the `handed-over` fixture rows change on purpose; `TaskDetailPanel`'s 7 are edges and `handleClose`'s call inside `handleBack` is in the tree; `def_named_not_called` is 0 for the shapes covered; graph time before and after |
| 4 | **Done** (see `languages/typescript/ENGINE.md`, "written inside an expression"). TypeScript, written inside an expression (literal methods, D21 arrows in a passed-on literal) | the Q-TS2 fixture changes its answer on purpose; `traverse` gets `toggleNodeExpansion` back; the 3 written on the frontend are edges. **Q-TS2 is closed** |
| 5 | **Done** (`languages/typescript/ENGINE.md`, "What no edge covers"; the keys are `uncovered_element_access`, `uncovered_bind`, `uncovered_call_apply`, `uncovered_class`). Keep the counters for what is still not covered: an element access, `f.bind(…)` and `obj.m.bind(obj)` handed over, `f.call(…)`, **and a class handed over by name** (`register(MyClass)`, `{ Icon: MyClass }`). A passed edge points at a context, and a class has none (a class is a `Construct` target; its constructor is `init`), so it cannot be passed; the counter of the handed-over defs filters function-like defs and does not see a class at all. Step 5 adds a count for it | the remainder is a number, a class included |
| 6 | TS step 6: JSX, tagged templates, decorators, accessors | its own fixtures |

### The test of step 2 (a toy language in `core/`)

The toy `Lang` of `solve.rs` takes `SiteCalls::passed` for a site written `>b` in its program table (`"main: a >a >b"`: `main` calls `a`, hands `a` over, hands `b` over; `"b: c"`). Written from the rules
before the run:

| Check | Expected |
|---|---|
| tree of `main` | `main`, `a`, `a [Passed]`, `b [Passed]` with `c` under it |
| `Recursive` | `f: >f` renders `f`, `f [Passed] [Recursive]` |
| `only_marked` | a passed node with no marker is hidden with everything under it |
| wire, unmerged | the passed frames have `passed: true`, the call frame has `false`, and `call_site` is each one's own site |
| wire, **merged** | `a` called and passed stays two frames; two passed frames of one def become one with `call_count` 1 |
| serialization | a graph with no passed edge has no `passed` key in its JSON (byte-identical to the golden of today) |
| invariant | a passed `External` edge is reported by `check_invariants`; a passed edge to a context with no row is reported as a call's is |
| determinism | the same graph twice is equal (rule 11); `passed` does not change the order of edges that have none |
