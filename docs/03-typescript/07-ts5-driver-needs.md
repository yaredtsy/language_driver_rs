# 03.07 TS5: what the driver needs

```
TS5
├── 0. what the code already has (checked at e46646d, 2026-11-12)
├── 1. the list, from the V-NOC driver contract
├── 2. a session: the long-lived object
├── 3. bounded work: the frontier, `more`, paging
├── 4. cancel
├── 5. the cache
├── 6. edits: saved text now, unsaved text later
├── 7. steps
└── 8. how drivers ship, and what that asks of the vnoc side
```

This page was first written before any code existed. It was rewritten after TS4 (2026-11-12) against the code at `e46646d`: §0 is what was
found, and the design below rests on it. Every `[check]` of the first version is answered in §0 or is still marked.

## 0. What the code already has

| The first plan asked for | In the code | Where |
|---|---|---|
| `Cancel` | **built** (with the first Python commit): a shared flag **and an optional deadline**; `reason()` says `Cancelled` or `Deadline` | `core/analyzer-core/src/solve.rs`, `Cancel` |
| the solver checks cancel before each body | **built** | `Solver::graph`, before `ensure` |
| the walk checks cancel before each site | **built** | `typescript-analysis/src/walk.rs`, `body` |
| a cut result is never reused | **built**: a `Summary` with `complete: false` is kept but walked again on the next ask | `Solver::ensure` |
| one body on demand | **built** | `Solver::summary(f, key, cancel)` |
| the memo kept between requests | **built in the solver** (`Solver::clear` drops it), **not usable by TypeScript**: see §2 | `Solver.memo` |
| a depth / node limit | **built twice, neither is a frontier.** The solver's `Limits { max_contexts, max_depth }` are cost limits: a context past them is `Status::Incomplete` and counted in `Stats.cuts`. The renderer's `RenderOptions { max_nodes, max_depth }` only shorten the output (`Mark::Cut`): every body was walked anyway | `solve.rs` `Limits`; `render.rs` `RenderOptions` |
| a construction as an entry (D25) | `Root::Construct` exists in the graph, the renderer and the wire, and Python makes it. **The flat solver cannot**: `Solver::graph` names every root `Root::Context` | `graph.rs` `Root`; `solve.rs` `graph` |
| `Budget`, `more` tokens, paging (`max_children`), `Session`, `set_text`, `Stale` | **not built** | |

**Measured: a second request on an open project is already warm** (V-NOC frontend, dev profile with optimization, one process, 2026-11-12;
the throwaway test is not in the repo). Open: 151 ms. Then `call_graph` with a **fresh solver** each time:

| Entry | 1st | 2nd | 3rd |
|---|---|---|---|
| `TaskDetailPanel` (90 contexts) | 39.9 ms | 1.2 ms | 1.0 ms |
| `CoverageSummary` (4 contexts) | 1.3 ms | 0.03 ms | |
| `ConfirmDialog` (12 contexts) | 4.4 ms | 0.09 ms | |

What makes the first request slow lives **inside `Sem`**, not in the solver: the per-body sites (`Sem.bodies`), the outlines (`Sem.file_defs`)
and the checker's own type caches (one checker per program, `checkers: Some(1)`). All of it survives between requests without the solver's
memo.

**Answered: the engine cannot be told to stop** (the `[check]` of the first §4). tsrs's checker reads its `Context` only while it checks a
whole file for diagnostics (`check_source_file`, `checker_02.rs`), which the analyzer never asks for; symbol and type queries do not look at it.
And a checker that saw a cancel is poisoned: `check_not_canceled` (`utilities.rs`) asserts on the next file check. With one checker per
program, linking our `Cancel` to the engine could only break the next request. So cancel stays at body and site granularity (§4).

**Still open:** `tsrs_project` depends on `tsrs_lsproto` (its `Cargo.toml`); whether a `tsrs_project::Session` can be driven without a client
connection is the `[check]` of step 7.

## 1. The list

TS4's `call_graph` computes everything reachable and returns. V-NOC's tier 3 asks for more (`09-adding-a-language/02` §4), and warns that for
Python these were added afterwards as one painful batch.

| The UI does | The driver contract asks for | Where it lives |
|---|---|---|
| opens a function | a few levels, fast | **core** solver: a `Budget` makes a frontier; bodies past it are not walked (§3) |
| clicks ＋ on a node | continue under that node | **now (D31):** the node is `Cut`, the driver asks for that def's tree as a new entry. **Later:** the node carries `more`, a cursor; **TypeScript** `Session::expand` answers it with a new bounded graph rooted there (§2, §3 "Later") |
| clicks ＋ on a node with 300 callees | a page of children | **later (D31):** core renderer: `max_children`, `more` with the next offset. Now the node's children all come in one answer |
| clicks away | really stop | **core** `Cancel` (built); the engine is not involved (§0) |
| opens the same function after a restart | instant | the V-NOC core's cache; the analyzer supplies stable ids and `environment_inputs` (built in TS3). An entry is a glob or a path relative to the root, **or an absolute path** (with `/`) for a config outside the root that feeds the program (an `extends: "../base.json"`): `relative_to` keeps the engine's absolute path when the root is not its prefix. The doc comment of `Project::environment_inputs` says "relative" and no fixture has it; step 0b fixes both. Python's list has the same case (an environment outside the root). **Every entry of both drivers is written with `/`, the absolute one too** (owner, 2026-11-13; Python since `1e45633`, TypeScript asserted in its tests since `3043f52`): the V-NOC core turns a Windows watcher path into `/` once and compares strings, and never rewrites an entry. On Unix a `\` is a character of a name, so the core normalizes only on Windows; the vnoc text says so |
| saves the file, then opens the tree | the saved text is used | **TypeScript**: `Session::refresh` before each request (§6, D41) |
| types in the file, then opens the tree | the unsaved text is used | **not built, for now (D42):** the tree follows a file when it is saved. The contract's request has no text in it, and here each changed text is a program rebuild (§6) |

Bounding, `more`, paging and cancel are in `analyzer-core`, so Rust gets them for free and Python can adopt the same shape (M18).

## 2. A session

```rust
// typescript-analysis (TS5)
pub struct Session { /* a Project, the caller's epoch, the stamps of the paths it depends on */ }

impl Session {
    /// `epoch` is the caller's number for this session (the driver's start time, say): a token from another session is `Stale`.
    pub fn open(root: &Path, file: &Path, epoch: u64, marker: MarkerStyle<'static>) -> Result<Session, OpenError>;

    /// One request: the graph under `entry`, walked no further than `budget`.
    pub fn graph(&self, file: &str, entry: &Entry<'_>, budget: &Budget, cancel: &Cancel) -> Option<Graph>;
    /// Continue under a node's `more`: a new bounded graph rooted at the def the token names, and where its children start.
    pub fn expand(&self, token: &str, budget: &Budget, cancel: &Cancel) -> Result<(Graph, Page), Stale>;
    /// The render options that put `more` tokens of this session on the nodes.
    pub fn render_options(&self, page: &Page) -> RenderOptions;

    /// Brings the session up to date with the disk; the driver calls it before each request (step 6, D41). Every token made before a
    /// rebuild is `Stale` after it.
    pub fn refresh(&mut self) -> Refreshed;
    /// Builds the program again whatever the disk says.
    pub fn reload(&mut self) -> Refreshed;
}
```

The first version of this page had `set_text(file, Option<&str>)` here, for text the editor has not saved. It is **not built, for now
(D42)**: a tree is made from saved text only. `refresh`, `reload` and `Refreshed` are step 6's
([`11-step6-text-and-disk.md`](11-step6-text-and-disk.md) §3).

The answer is still a graph (rule 7): the driver draws it with `render(&graph, &graph.roots[0], &session.render_options(&page))` and sends the
wire of that tree, as it does today. `call_graph` from TS4 stays, unchanged, for the CLI and the tests.

**There is no solver memo between requests.** `graph` and `expand` build a fresh `Solver` each time, which §0 measured at about 1 ms for 90
contexts once the project is warm. This is a choice, not a shortcut: keeping the memo would mean a `Session` that owns the `Project` and also a
`Solver<TypeScript<'p>>` that borrows it.

> **🦀 Rust note: a struct cannot borrow from itself.** `TypeScript<'p>` holds `&'p Sem`, and its `Func<'p>` handles point into the program.
> A `Session { project: Project, solver: Solver<TypeScript<'?>> }` would need `'?` to mean "as long as the field next to me", and Rust has no
> such lifetime: moving the `Session` would move the `Project`, and the borrow would dangle. The ways around it (`ouroboros`, raw pointers,
> keying the memo on owned ids and finding each `Func` again) all cost more than the 1 ms they would save. So the long-lived thing is the
> `Project`, and everything that borrows it lives for one request.

> **🦀 Rust note: `&self` here, `&mut self` for edits.** `graph` and `expand` take `&self` because what they fill (`Sem`'s caches) is behind
> `RefCell`s inside `Sem`. That is safe because `Sem` is not `Sync` (pinned by a proof since TS3): two threads can never hold `&Session` at
> once, so the `RefCell`s are only ever touched by one thread. `refresh` and `reload` replace the program, so they take `&mut self`: the
> compiler then forbids an edit while a request still holds a borrow.

## 3. Bounded work: the frontier, `more`, paging

**What is built now and what waits (D31, owner, 2026-11-13).** The budget and the frontier are built (step 1). Step 2a is small: the renderer
gives a node whose context is `Status::Frontier` the mark `Cut`, and nothing else changes. `Token`, `more`, `max_children` / `start`,
`Frame.more`, wire v3, `Session::expand` and `Stale` are step 2b, built when the owner asks for them. When they are built, the token is the cursor of
"Later" at the end of this section, not the decoded text sketched below. The sketch stays as the shape of the API.

```rust
// analyzer-core::solve
pub struct Budget { pub max_depth: Option<u32>, pub max_contexts: Option<u32> }   // a frontier, not a cut
pub enum Start<L: Lang> { Call(L::Func, L::Key), Construct { class: L::Func, init: Option<(L::Func, L::Key)> } }
impl<L: Lang> Solver<L> {
    pub fn graph_within(&mut self, starts: &[Start<L>], budget: &Budget, cancel: &Cancel) -> Graph;   // `graph` stays as it is
}
// analyzer-core::graph
Status::Frontier       // new: listed, not walked, because the request's budget ended here
Stats.frontier: u64    // new: how many; not a cut, so not in `Stats.cuts`

// analyzer-core::render
RenderOptions { …, tokens: Option<Epoch>, max_children: Option<u32>, start: u32 }   // all off by default
Node { …, more: Option<Token> }
// analyzer-core::wire
Frame { …, more: Option<String> }    // absent when None; WIRE_FORMAT_VERSION 2 -> 3
```

How it works:

```
graph_within(start, budget):
    breadth-first as today; a context at depth max_depth, or past max_contexts walks, is listed with Status::Frontier and not walked
render(graph, root, opts with tokens):
    a node whose children are not all drawn gets more = Token { epoch, edits, file, qualname, occurrence, offset }
expand(token):
    check epoch and edits (else Stale) → find the def by (file, qualname, occurrence) (else Stale) → graph_within(that def, budget)
    → render with start = offset
```

- **Bounded work, not bounded output.** A body past the frontier is never walked. Breadth-first already gives the property the proof needs: a
  node drawn at tree depth `d` stands for a context at graph distance `≤ d`, so with `max_depth = d` every node above the frontier was walked
  and the ones on it were not.
- **Which nodes get `more`:** a `Frontier` context; a node the renderer's `max_nodes` / `max_depth` stopped above; a node with more than
  `max_children` children (the offset of the next one); a node of an `Incomplete` context (cancel, deadline: asking again may succeed). Not
  a `Recursive` node (expanding it gives the same tree), not an `External` or `Unresolved` leaf. `Mark::Cut` stays as it is; `more` is next to
  it.
- **The token names the def the way the contract does**, `{ file, qualname, occurrence }` (D18, D20; from `DefInfo`), plus the child offset,
  plus the session's `epoch` and edit count. So `expand` uses `Sem::entry`, which exists, and a token from before an edit, or from another
  session, is `Stale`; the driver then asks again by `{ file, qualname, occurrence }`. A construction's token names the class, and the class
  entry is a `Root::Construct` (D25). The text form is core's, has a format version, and is never parsed by the driver.
- **A frontier is not `Incomplete`.** What is above it is complete. `Incomplete` and `Stats.cuts` stay for the cost limits of rule 10
  (`Limits`, deadline, cancel).
- **Off by default, so nothing changes for anyone who does not ask.** `graph` (no budget), `RenderOptions::default()` (no tokens, no paging)
  and the wire of a tree without `more` are byte for byte what they are today. Python, which uses core's renderer and wire but not the flat
  solver, keeps its output (rule 5 is checked by its tests, not assumed).
- **(Step 2b, waits for the call-table decision.) The wire version goes to 3.** A reader that does not know `more` would draw a cut node as a leaf: that changes what a node means, which is
  the rule for a bump (§8, item 2).

### Now: a frontier node is `Cut` (step 2a)

```
render(graph, root, opts):
    a node with a Frontier context gets Mark::Cut   (no children: its body was not walked)
the driver, to go further:
    a new graph_within request with the cut node's def as the entry, found by { file, qualname, occurrence }
```

- `Cut` already means "the children here are not all drawn": the renderer's `max_depth` sets it today, and the wire carries it as the mark
  `"Cut"`. A frontier is the same thing, seen by the reader. So the wire stays at version 2.
- **What a new request at the cut node loses: nothing in TypeScript today.** There is one context per function (`Key = ()`), so the subtree
  under a def is the same however it was reached. The new request does not know the path above it, though: a call back to an ancestor of the
  cut is drawn again, where the unbounded tree has `Recursive` (why 2a's proof is for an acyclic program). V-NOC knows the path the user
  opened, so **the vnoc side must apply `render.rs`'s rule when it grafts an answer under a cut node**: a node whose def (by def, not by
  context: rule 8) is on the path from the root is drawn as a leaf, `Recursive` plus the edge's marks, its site, `passed` and contexts kept,
  no children. Without it a cycle through a cut node grows by one request per click, for ever (step 4: `ping` at depth 1). The reference is
  the test stitcher, `Stitcher::complete` in `typescript-analysis/tests/session.rs`; it goes in the vnoc text.
- The merge (`call_count`) unions marks, so a merged node with one frontier context is `Cut`, and its other children are drawn.

### Later: tokens as cursors (step 2b; not built, the owner's plan, D31)

Build this when the owner asks: trees too big for V-NOC to fetch whole, nodes with hundreds of children, or real context keys (Rust's RC layer,
where a fresh request at the cut node would lose what the caller passed).

```rust
// in the session: owned data only, nothing that borrows the Project
struct Cursor {
    def: DefRef,        // file, qualname, occurrence of the node to continue
    key: Key,           // the context: () in TypeScript; a real key once there is one
    path: Vec<DefRef>,  // the ancestors, so a call back up is still Recursive
    offset: u32,        // the first child not drawn yet
}
struct Session { project: Project, epoch: u64, edits: u64, cursors: Vec<Cursor> /* the id is the index */ }
```

- **The token is looked up, never decoded.** On the wire it is `{epoch}:{index}` (text form and version in core). `expand` finds
  `cursors[index]`, builds a fresh solver (about 1 ms warm, §0), runs `graph_within` from `def` with `key`, and returns the next page with new
  cursors for the nodes still cut. A real key cannot be rebuilt from text (a digest is not the key, RS5 §5), so a decoded token stops working
  the day Rust has keys. A looked-up one does not.
- **It lives in memory and dies with the process.** A new process has a new `epoch`, so an old token is `Stale`. A rebuild (`refresh`,
  `reload`) clears the table, so a token from before it is `Stale` too. On `Stale` the driver asks again by `{ file, qualname, occurrence }`,
  the same request as step 2a. The table is plain data, so it does not need the self-borrow of §2. It grows only until the next edit; a cap
  (drop the oldest, which then answer `Stale`) is enough.
- **Page after the merge.** The cursor pages the merged children (`call_count`), so one target is on one page with its full count. V-NOC must
  then use that merge and not merge the pages again (its own `calls.rs`): two merges across a page boundary give one target on two pages with
  wrong counts. That is a `vnoc` decision, made with this step.
- **On disk is V-NOC's.** A library never writes a file (rule 2). If a cut point must survive a restart, V-NOC stores the cut node's
  `{ file, qualname, occurrence }` (and, later, the key as owned data) next to its file hashes, and asks again. If the analyzer ever needs a
  disk cache of its own, the shape is a `Store` trait (get / put bytes) that the CLI or driver implements: the library still writes no file.
  Not worth it now: what the engine holds (the parsed program, the checker's types) is arena pointers and cannot be saved, so a restart pays
  the open (§0: 151 ms) whatever is stored, and the rest is the 40 ms first request, whose cache key would need every file that feeds a type.
- **The call table** (a view beside the wire that sends each context once, as rows of its call sites and their targets, so the reader opens one level from the table and nothing is unfolded; D31, still open) would replace most of this: a `Frontier` row is the `more`. Decide it
  before building the cursor.

## 4. Cancel

Built (§0); TS5 adds only proofs. Who checks, and when:

| Who checks | When |
|---|---|
| the core solver | before each context, **after the budget**: a context past the budget is `Frontier` even when a cancel would have stopped it too (built). After a cut walk it asks `reason()` once more, to name the cut |
| `walk.rs` | before each site (built). A body cut at a site keeps the sites before it and **loses its hand-overs and its counters**, which are noted after the loop, so a cancelled request's `Stats.language` can be below the uncancelled one's |
| the engine | **never** (§0: it cannot be told to stop, and a cancelled checker breaks the next request) |

**Two steps cannot be stopped (measured at TS5 step 5, release, cold, load 5–6, so read to a factor of 2):**

| Step | V-NOC (1084 defs) | postcss a / b |
|---|---|---|
| listing one body's sites, before the walk's first check: max (median) | 12–13 ms, `CodeLinePopoverLayer`, 18 sites (3 µs) | 0.8 / 0.4 ms (5 / 5.6 µs) |
| one site's resolution, between two checks: max (median) | 9.6–9.8 ms, `CanvasView` site 92 (1.8 µs) | 79–92 µs / 0.63 ms |

So a cancel is seen within about 12 ms on V-NOC, not "one site's work" as this page first said. The slowest listing is not the first def of its
file, so it is not the outline; most likely it is the checker's first questions for that body (not isolated). If a project shows a much larger
number, a check inside the listing is the place to add one. A
cancelled request returns what it has, its cut contexts `Incomplete` and the expanded node of each marked `Incomplete` (a `Recursive` leaf
standing for one is not: the node above it is), carrying `more` once tokens are built, D31. No cut summary outlives the request,
because the solver itself does not (§2); `Sem`'s caches hold only the syntax of a body and the checker's answers, which a cancel cannot leave
half-made.

> **🦀 Rust note: which thread holds what.** The driver's reader thread receives `$/cancelRequest` while the worker thread is inside `graph`.
> The worker owns the `Session` (`Sem` is `Send`, so it can be moved to the worker, but not `Sync`, so it cannot be shared). The reader holds
> only a clone of the request's `Cancel`: an `Arc<AtomicBool>`, which is `Send + Sync`, so both threads can hold it. `cancel()` stores `true`
> with `Ordering::Relaxed`: the worker only has to notice soon, not at an exact instant.

## 5. The cache

| Level | Lives | Keyed by | Dropped when |
|---|---|---|---|
| a body's sites, the outlines, the checker's types | inside `Sem`, in memory | the program's nodes, files | the program is replaced (`refresh`, `reload`) |
| the solver's memo | one request | `(function, key)` | the request ends |
| rendered trees | in the V-NOC **core**, on disk | entry + `environment_inputs` hash + file hashes | the core's business |

The first version of this page worried about an edit in `b.ts` that renames `f`, called from an unchanged `a.ts`: a memo kept across the
edit would hold a wrong summary for `a.ts`. With no memo across requests, and every cache inside `Sem` belonging to one program, an edit
gives a new program and nothing from before it is read again. The question comes back only if a later step keeps something across an edit
(an incremental program, step 7).

## 6. Edits: saved text now, unsaved text later

TS3's `Project` builds a program from disk, and a `Session` keeps it for many requests.

**Now (step 6): the session follows the disk, and only the disk** (owner, 2026-11-17, D41 and D42). The driver calls `Session::refresh`
before each request; it compares the stamps of the paths the last build depended on and builds the program again when one changed: a source
file saved, created or deleted, a config, a lock file. `Session::reload` forces it. The contract sends no change notification, so nothing
else could tell the analyzer (review, 2026-11-16; the cases and the two ways that were weighed are in
[`10-review-fixes.md`](10-review-fixes.md) §5). The cost of the first request after a save is about a fresh open plus a cold graph: on V-NOC
about 150 ms + 40 ms (§0), **to be measured** on the built thing. **The design is
[`11-step6-text-and-disk.md`](11-step6-text-and-disk.md).**

**Text the editor has not saved is not read, for now (D42).** A tree that followed the buffer would pay that rebuild for each request made
while the user types, and the contract's request has no text in it. When the contract asks, there are two ways, the simple one first:

1. **An overlay file system.** `set_text` records the text, and the program is built again through a `tsrs_vfs` file system that
   answers the unsaved files from memory and the rest from disk. Designed, not built: [`11-step6-text-and-disk.md`](11-step6-text-and-disk.md)
   §10.
2. **`tsrs_project::Session` (step 7, only if a rebuild is too slow).** The layer tsrs's own language server uses (`did_open_file`,
   `did_change_file`, `did_change_watched_files`, `snapshot()`): it rebuilds the program incrementally, so unchanged files keep their parsed
   trees. **[check]** whether it can be driven without a client connection (it depends on `tsrs_lsproto`); `tsrs_api` is a second example of
   driving sessions from outside the server. The owner sets what "too slow" means from step 6's number, which is the number for a **saved**
   file too.

Either way the change is inside the `sem` module: nothing in `walk.rs`, `calls.rs` or the core knows how the program was made.

## 7. Steps

Steps 1 and 2 change `core/`. The Python session works in the same repo and uses core's renderer and wire: start a core step only when
`git status core/` is clean, and run Python's tests before committing it (rule 5).

| # | Do | Where | Proof |
|---|---|---|---|
| 0b | `Project::environment_inputs`: the doc comment says an entry outside the root is absolute (engine form, `/`); a fixture whose `tsconfig.json` extends `../base.json` from a root one folder down | `typescript-analysis` | the fixture's list ends with the absolute path of `base.json`; the existing three `environment_inputs` tests unchanged |
| 0c | `matches_a_glob` drops an entry by its last segment only, so a config **outside** the root named like a glob (`../tsconfig.base.json`, a common name) is dropped, though a glob relative to the root cannot cover it. A glob covers only an entry under the root | `typescript-analysis` | the `extends-outside` fixture with `../tsconfig.base.json`: the list ends with its absolute path; a config under the root named `tsconfig.*.json` is still covered by the glob and not listed twice |
| 1 | `Budget`, `Start` (a call or a construction, D25), `Solver::graph_within`, `Status::Frontier`, `Stats.frontier`. `graph` becomes `graph_within` with no budget and `Start::Call` | `core/` | **done, `bfb118d`.** A toy `Lang` (the `Toy` in `solve.rs`'s tests, which records its walks; the testkit's does not, and the testkit depends on core): a 3-level program with `max_depth = 2` walks exactly the bodies above the frontier (count the `walk` calls); a frontier is not in `Stats.cuts`; `Start::Construct` gives a `Root::Construct` with the class's row and the `init` context; every existing core test unchanged; TypeScript's golden output byte-identical (trees and wires were; each `.graph` file, a `{:#?}` dump, gains the line `frontier: 0,` in `Stats`, accepted: the Debug dump is not a contract); Python's tests pass |
| 2a | **A frontier node is drawn `Mark::Cut`** (owner, 2026-11-13). A node standing for a `Status::Frontier` context gets `Cut`, as a node the renderer's budget stopped above does: the mark already says "the budget ran out here, the children are partial", and the wire's `cut` is already known to the reader, so **no new field and no wire version bump**. Expanding it is a new request for that callee by `(file, qualname, occurrence)`, which the reader already makes for a cut node | `core/` (render only) | on the toy: at `max_depth = 1, 2, 3` exactly the frontier nodes and the renderer-stopped nodes are `Cut`, no other node; for an acyclic program, replacing each `Cut` frontier node by the tree of a new request rooted at its callee gives the unbounded tree; `Incomplete` is unchanged and a frontier node is never `Incomplete`; default options and no budget give output byte-identical to today (core tests, TypeScript golden trees and wires, Python's tests) |
| 2b | **Not yet (owner, 2026-11-13, D31); the design is §3 "Later": cursors kept in memory, gone with the process.** Waits for the owner's decision on a call table (§3 "Later", D31; not in the plan yet): `Token`, `RenderOptions.tokens / max_children / start`, `Node.more`, `Frame.more`, `WIRE_FORMAT_VERSION = 3`, the merge's rule for `more`. If the answer is a call table, a frontier row in the table is the `more` and most of this is not built. If not, it is built with the four answers of the step-2 review (2026-11-13): the expansion knows its ancestors, so recursion across a seam is a `Recursive` leaf as in one request; a token is **looked up** in what the session listed, never decoded (a key digest cannot be turned back into a key, RS5 §5); paging happens **after** merging, or the reader's merge sees part of a target's calls on each page; a node's `more.offset` is the index of its first child not drawn | `core/` | as before: every node of the four kinds in §3 has `more` and no other; stitching gives one unbounded request; paging with `max_children = 1` visits every child once; default output byte-identical |
| 3 | TypeScript class entry = `Root::Construct` (D25 (1)); the counter for the definition-time sites of a top-level class (D25 (3)). **Answered at the step-3 review (2026-11-13):** (1) the entry and a `new C()` share `walk::construction`, and the counters are noted there, so an entry's tree **and its counters** are the subtree a construction draws; (2) one new counter, `uncovered_class_definition_sites`, the name of [`02-tree-rules/07`](../02-tree-rules/07-what-a-construction-runs.md) Q1: the sites of a **top-level** class's decorators (its own, its members', its parameters'), its `extends` expression, its static field initializers and its static blocks. Noted once per construction drawn, for the constructed class only, **not** its base chain: these sites run when a class is defined, so the instance-initializer rule (a construction runs the chain's initializers) does not carry over. The two existing counters keep their names and meaning (D25 (3) lists them as built; `typescript-v0.2.0`'s `policy.md` names them): `uncovered_computed_member_name_sites` moves into `walk::construction` with the rest, `uncovered_local_class_decorator_sites` is unchanged (nested classes); (3) the two tests that used a class entry to reach a body keep their fixtures and assert the counters, with nothing drawn. Real projects read 0 (V-NOC 0 of 5 classes, postcss 0 of 19 and 0 of 2: a throwaway probe) | `typescript-analysis` | **done, `f98b5bb`.** A fixture of its own (`class-entry`, 12 classes, each with a `newX` caller) rather than `construct`; policy rows `class-entry` and `counter-class-definition`; for each class, the entry's tree, edges, defs and `Stats.language` equal what its `new C()` caller draws below the site; V-NOC and postcss golden trees, wires, merged wires and graphs byte-identical for function entries. **Known gap, left open:** a class declared in a function or a namespace has its static initializers, static blocks and `extends` in no tree and no counter, and a namespace class reads 0 for its computed names too (`parent.is_some()`; older than step 3, pinned by `class_entry_a_class_declared_in_a_function_…`, written in `policy.md` and `ENGINE.md`) |
| 4 | `Session::open / graph` (`expand`, `render_options` and `Stale` come with 2b) | `typescript-analysis` | for every scenario and every policy row, at `max_depth` 1, 2 and 3: asking for every `Cut` node's def as a new entry and stitching gives the tree of one unbounded request (the recursion exception of step 2 aside); `TaskDetailPanel` the same | **done, `c989c0f`.** `Session { project, options }`, `open(root, file, Options)`, `graph(&self, file, entry, &Budget, &Cancel) -> Option<Graph>`, `project()`; `call_graph` and `Session::graph` share the private `graph_within`; `Budget` and `Session` re-exported; `Send` / `!Sync` proof doctests (8 and 8). `Budget::default()` gives `call_graph`'s graph, tree and `frontier == 0` on every case. The stitcher (`tests/session.rs`) carries the defs above each node and turns one already on that path into the leaf `render.rs` draws (`Recursive` + the edge's marks, site, `passed`, contexts, no children). **The recursion exception fires in no scenario, no policy row and not in `TaskDetailPanel`**: no cycle there passes through a cut node, and the golden's one `Recursive` (`flattenAll` calling itself) is drawn by the request rooted at it. A fixture of its own, `recursion` (a pair, a 3-ring, a construction on its own path, a def handing itself over), is where it fires (14 runs); with the rule off, `ping` at depth 1 never ends. `TaskDetailPanel` at depth 1/2/3: `Stats.frontier` 30/42/9 (frontier nodes in the tree 31/73/47: a context is counted once, a node once per place it is called), 284/163/49 further requests, about 48/3/2 ms in release. The `Likely` branch of the stitcher is never reached: TypeScript has no `Likely` edge (D28 is Python's) |
| 5 | the cancel proofs (the code is built). **Changed at the step-4 review (owner, 2026-11-13):** a cancel fired at a random time cannot promise that every check point is hit, so core gets **`Cancel::after_checks(n)`**, a cancel that fires on its n-th check (a shared countdown in `Cancel`; `reason()` counts down). Test infrastructure, but public: Python's cancel proofs can use it too (rule of two) | `core/` (`Cancel` only), `typescript-analysis` (tests) | (1) on every scenario, policy row and the `recursion` fixture: count the checks N of a full request, then cancel at every check 1..N, each on a **fresh** session: every graph is part of the full one (a `Finished` context has exactly its edges, an `Incomplete` one the non-passed edges of its first k sites), every cut is `Incomplete(Cancelled)` and counted in `Stats.cuts`, its node is `Incomplete`, and a fresh `Cancel` on the same session then gives the graph of a session that never saw a cancel (`Sem`'s caches are not left half-made); (2) one cancel fired from another thread (the flag is seen across threads); (3) a passed deadline cuts with `Deadline`; (4) cancel + budget: the root is `Incomplete`, never `Frontier`; (5) a class entry with no constructor loses nothing to a cancel (its counters are noted before the solver runs): identical to the uncancelled request; (6) **[measure]** the step that cannot be stopped, listing one body's sites, timed for every def of V-NOC and both postcss inputs: the maximum is the bound §4 states; (7) V-NOC `TaskDetailPanel`, `#[ignore]`: a smoke test that prints numbers and asserts nothing about timing; Python's tests stay green (rule 5) | **done, `a1ffe8f` (core) + `2edf717` (tests).** `after_checks(n)`: `n == 0` is already fired; a call that finds the flag set does not count; a deadline still speaks while the countdown runs. 93 cases, N from 0 (a class entry with no constructor: no context, no check) to 46, 467 checks, 110 cuts inside a body, every one part of the full graph and recovered; V-NOC `TaskDetailPanel` N = 554 for 90 contexts, 9 cancels spread over it all recover to the golden tree. **Precision to (1):** every `Incomplete` context has an expanded node marked `Incomplete`, and no other expanded node is; a `Recursive` leaf for it is not marked (scenario `recursion-direct`). (5) holds: `Rich`, which has a constructor, is cut and its `Stats.language` still equals the uncancelled one's. (6) is the table in §4. core 153, TypeScript 226, Python 238 |
| 3c | **(after 5) a coverage test for rule 8** (owner, 2026-11-13): every call-like node of a fixture's syntax is drawn in some tree or counted, so a site that vanishes is found by a test, not by a review. An **independent** scan (not the walker's `Collector`, or it agrees by construction), compared **by span** (a count hides one missing behind one extra), one way only (accessor sites come from the checker, a syntax scan cannot see them). Starts with a list of known gaps, each with its reason; the owner decides each one | `typescript-analysis`: the scan is a `#[cfg(test)]` module inside `sem` (it may read tsrs, rule 4; no new public API); first a refactor, the four count-only counters (`initializer_sites`, `computed_name_sites`, `definition_sites`, `decorator_sites`) return their sites' spans, values unchanged (golden) | the test passes with the list; the list holds the nested-class gaps of 3b and whatever else it finds (expected: calls at the top level of a file, `const x = f();`; likely enum member initializers). The list is by **category** (the miss's syntactic position): a miss in an unlisted category fails, and so does a listed category no fixture hits. A fixture per category (`tests/fixtures/coverage/`, numbers per file written first); every fixture project and scenario scanned; V-NOC and postcss as an `#[ignore]` [measure] of misses by category, for the owner's decisions **done, `0334771` (the four counters return spans; golden byte-identical) + `02aeb03` (`src/sem/coverage.rs`, `tests/fixtures/coverage/`).** The anchors line up exactly. The scan shares only `own_span`, `NodeFlags::Ambient` and `is_string_literal_like` with the walker; `body` is read only for the counted side and a printed diagnostic. The test caught one fault of the oracle itself: a property arrow is a def only in a literal a `const` holds (D21). Misses on real projects (drawn / missed of the call-like nodes): V-NOC 5728 / **786 (10.3%)**, postcss a 832 / 49 (5.1%), b 277 / 23 (7.4%), and **every miss is `top-level` or `top-level-anonymous`** (V-NOC 325 + 461). `enum-member`, `namespace-level` (new, found by the test), and 3b's two categories are 0 on all three: they are in the fixtures only. No `Uncovered` kind is a vanished call node. 230 tests. **Open for the owner: the top of a file.** (a) It is not only setup code: an arrow passed to a call at the top is anonymous, so its whole body is in no tree. Examples: a store's actions (`create((set) => ({ … }))`), a schema's `.superRefine(…)` callback, and React components written `const TreeView = React.forwardRef<…>((props, ref) => { … })` (V-NOC `components/ui/tree-view.tsx`: `TreeView`, `TreeItem`, `TreeLeaf`… are not defs at all, `calls --entry TreeView` says "no def"). (b) **The engine has the answer:** its call hierarchy takes the source file itself as a caller (`tsrs_ls/src/callhierarchy.rs:115`, `is_valid_call_hierarchy_declaration`: `is_source_file`, a namespace with an identifier name, a class static block). Under "follow the engine" that is a **module def**, a new kind of def: rule 10 allows adding one, and the defs at the top keep their names (the module is not a qualname prefix). It needs its own naming rule (frozen once tagged), an outline entry, the wire's kind, and V-NOC's answer to "open a file's own code". The calls of `namespace-level` would follow the same way (the engine's namespace caller). A `forwardRef` component would still not be openable by its name, because the engine does not name it either: its calls would show under the module. Recommended: decide this before `typescript-v0.3.0`, as its own row; 3b is small and independent of it |
| R | **(next; before M step 2) the root bounds project code** (owner, 2026-11-16, D33): a file outside the root is `External`, named by its path from the root with `..` and counted (`outside_root`); no def's file, id or external's name is an absolute path; an entry is a def of project code; `open` with a file outside the root is `OpenError::OutsideRoot`. Steps R1–R3 in [`10-review-fixes.md`](10-review-fixes.md) §3 | `typescript-analysis` (`sem`; one variant and two doc comments in `lib.rs`) | the `outside-root` fixture; **the same fixture in a second folder gives a byte-identical wire**; the real-project goldens byte-identical, `outside_root` reported for each |
| D | **(D1 after R; D2 with M step 1) two defs with one marker** (owner, 2026-11-16, D34): in one file the later def is named by where it is and counted (`duplicate_marker`); across files core counts the rows `Graph::new` drops that are another def. [`10-review-fixes.md`](10-review-fixes.md) §4 | D1 `typescript-analysis` (`sem`); D2 `core/` | the `duplicate-marker` fixture: `main > a{x}, b{y}`, and `b` asked for alone has the id it has under `main`; core's hand-written graphs; Python's tests pass |
| M | **done (2026-11-17, steps 1 to 5: `133c6c2`, `7e4a870`, `8a997dc`, `449dfb4` + `c6dbfd7`, `5dc6ba8`; D2 `cf0eb8b`).** **(after 3c, before 3b) the module def** (owner, 2026-11-13, D32): a file's own code is an entry, the **file itself** (core `Entry::File`, by its path; no reserved qualname, no new prefix: the root's `target_id` is empty and the reader uses the file id it asked with); it owns the top-level statements and the anonymous functions there, flat. Design and the choices still open in [`09-module-def.md`](09-module-def.md): the outline unchanged; `DefKind::Module` on the root frame; no wire change, no wire bump; a namespace an entry; a class's or enum's definition code drawn in the body that declares it, which retires three counters and replaces 3b | `core/` (`Entry` only), `typescript-analysis` | row 3c's coverage test is the acceptance: the known-gap list empty, `measure_coverage` missed 0 on V-NOC and both postcss inputs; a fixture per pattern of the 461 (store, `forwardRef` component, test callback, schema callback, property-assigned function); golden function entries byte-identical, new file-entry golden entries read by hand; outline snapshots byte-identical (rule 10); **[measure]** the largest module tree |
| 3b | **Replaced by M's step 4: the owner accepted 09 §3.3 (2026-11-17), so this row is not built.** Kept for the record: **(after M)** count the definition-time sites of a nested class** (owner, 2026-11-13): a class in a function or a namespace has its static initializers, static blocks and `extends` counted in `uncovered_class_definition_sites`, and a namespace class its computed names in `uncovered_computed_member_name_sites`. No rename, no tree change. The two `parent.is_some()` checks (`sem/uncovered.rs`) are **two rules**, not one: the def around a class in a function draws its computed names, nobody draws its definition sites; one small function per rule, named for what it decides | `typescript-analysis` | 3c's list loses the nested-class entries; `class_entry_a_class_declared_in_a_function_…` asserts the count instead of 0; policy rows reworded; golden unchanged (real projects read 0 for top-level classes) |
| H | **done (2026-11-17, H1–H10, `506f945` .. `dc1bc9a`; H11 found, not built).** **(after M, before 6) the hardening batch** of the 2026-11-16 review: one checker handle at a time, checked in debug builds; the memory tests one at a time; `found.sort()`; `Sem::region`; `matches_a_glob` from `ENVIRONMENT_GLOBS`; a request with `occurrence` above 0; the deep-nesting test on a thread of the driver's stack size. [`10-review-fixes.md`](10-review-fixes.md) §6, §7 | `typescript-analysis` | no answer changes: golden and snapshots byte-identical; **[measure]** the smallest stack that passes the deep fixture in a release build, written in `ENGINE.md` |
| 6 | **Designed and decided** in [`11-step6-text-and-disk.md`](11-step6-text-and-disk.md) (2026-11-17; the owner's answers are its §9). **Built** (`d3b394e`, `716d33a`, `2b0ba62`; reviewed 2026-11-19). On V-NOC: `refresh` with nothing changed 1.25 ms; the first answer after a saved file about 185 ms (dev profile). Whether that asks for row 7 is the owner's call. **A file changed on disk**: `Session::refresh` before each request and `Session::reload`, by stamps of the paths the engine's `trackingvfs` saw (D41, [`10-review-fixes.md`](10-review-fixes.md) §5). **No `set_text`: only saved text is read, for now (D42)** | the `sem` module, `session.rs` | a callee's file rewritten on disk, a file an import names created, a callee's file deleted, a file outside the root rewritten: the tree follows each; the opened file deleted and written back, a config saved broken and fixed: the session is lost and comes back by itself; a file saved during a rebuild is caught by the next `refresh`; **[measure]** `refresh` with nothing changed, and the first request after a saved file, on V-NOC |
| 7 | only if the owner finds step 6's number too slow: the program from `tsrs_project::Session`. **First, the free contract**: `Sem::drop` calls `free_unshared_program`, whose safety comment says the program shares nothing with another (no `update_program`); an incremental program shares its unchanged files, so the drop is redesigned before anything else in this step (review, 2026-11-16) | the `sem` module | the same tests; the memory tests; the number before and after |
| 8 | `typescript-cli calls … --depth N --max-contexts N` (`--max-children N` and `typescript-cli expand <token>` come with 2b) | `typescript-cli` | — |
| 9 | **tag `typescript-v0.3.0`**; **the tag notes list the policy rows changed since v0.2.0** (`git diff typescript-v0.2.0 -- languages/typescript/docs/policy.md`; as of M step 4): `anonymous` (a file is an entry, D32); `class-body`, `decorators`, `counter-computed-names`, `counter-local-decorators`, `counter-initializers` (M step 4: a class's definition code is drawn by the body that declares it; `uncovered_computed_member_name_sites` and `uncovered_local_class_decorator_sites`, which v0.2.0's `policy.md` names, are retired, so a reader of `Stats.language` stops seeing those keys); new rows `class-entry`, `counter-class-definition` (TS5 step 3, D25) and `duplicate-marker` (D34); `project-code` (D33); `any` (its reason named, wording only); the driver implements `vnoc/callTree`, `vnoc/expand` (until 2b: a `callTree` on the cut node's def), `$/cancelRequest`, `vnoc/environmentInputs`. **Every thread that calls the analyzer gets a large stack** (`std::thread::Builder::stack_size`, **32 MiB**, the number from row H, `10-review-fixes.md` §7): a stack overflow aborts the process, it is not a panic, and 2 MiB (a spawned thread's default) was seen to abort on a 10 000-term `+` chain, in `outline` too ([`10-review-fixes.md`](10-review-fixes.md) §7) **Added at the review of 2026-11-19:** `interface` (D43: a call through an interface's or abstract method is `Unresolved(function_typed_value)`, no longer a leaf), `function-typed` (D43: a method or accessor signature of a type literal; the four `function_typed_value_from_*` keys), `passed` (a signature handed over draws no edge, `signature_passed`), `external-call` (a computed member is spelled as written, `Array.[Symbol.iterator]`; a string key that is not an identifier keeps its quotes), `anonymous` (H15: an anonymous class or function written directly after `new`), and the counter `engine_kind_unhandled`. | `vnoc` | the tier-3 conformance checks |

Until 2b is decided: step 4 builds `Session::open` and `Session::graph` (a budget, frontier nodes `Cut`) and not `expand`, `render_options` or `Stale`. Its API (reviewer, 2026-11-13): `Session::open(root, file, options: Options)`, with **no `epoch`** (only a token reads it; 2b adds it), `graph(&self, file, entry, &Budget, &Cancel) -> Option<Graph>`, and `project(&self) -> &Project` for `files` / `environment_inputs`. `call_graph` and `Session::graph` share one private function, so `Budget::default()` gives `call_graph`'s graph exactly. `Budget` is re-exported; step 5's cancelled request gives `Incomplete` nodes, with no `more`; step 8 has no `expand` subcommand. Step 3 does not depend on 2b.

## 8. How drivers ship, and what that asks of the vnoc side

Context for `vnoc`, decided by the owner (recorded 2026-11-12). Nothing here is built in this repo.

- **One VS Code extension holds the core.**
- **Each language driver is a separate executable.** The analyzer from this repo is compiled into its driver, pinned by git tag
  ([`01-monorepo/06`](../01-monorepo/06-versions-and-vnoc.md) §4), so an analyzer and its driver always ship together.
- **v1 bundles the Python driver** in the extension and copies it to a global drivers folder on first run. Later drivers are downloaded by the
  core on demand.
- **So the core and a driver can be at different versions.** Example: V-NOC installed in both VS Code and Cursor, updated at different times,
  sharing one global drivers folder.

A reader that is older than the driver ignores a field it does not know. For a field that changes what a node means that is a wrong tree, not
an error: a core that predates `passed` (D22, [`02-tree-rules/06`](../02-tree-rules/06-passed-edges.md)) would draw a def that is handed over
as a call. Two `vnoc`-side items follow:

| # | Item | When |
|---|---|---|
| 1 | **Version the drivers folder path** (`drivers/typescript/0.2.0/`), so each core runs the driver version it was built for | before a second core can share the folder |
| 2 | **The driver reports the wire format version in `initialize`, and the backend checks it.** The number is the constant in `analyzer_core::wire`, bumped whenever a field is added that changes what a node means (added in step 2 of the passed-edges note, bumped for `passed`). What the backend does with a version it does not know (refuse the tree, ask for an update) is `vnoc`'s choice | **in the same `vnoc` change as the first TypeScript tree view** |

Nothing changes in this repo beyond that constant: there is no handshake here (drivers stay in `vnoc`, Q-M7), and the constant is a fact about
the wire, with no product word in it (rule 1).

Next: [`08-later-context-sensitive.md`](08-later-context-sensitive.md)
