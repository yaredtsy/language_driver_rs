# 03.07 TS5: what the driver needs

```
TS5
├── 0. what the code already has (checked at e46646d, 2026-11-12)
├── 1. the list, from the V-NOC driver contract
├── 2. a session: the long-lived object
├── 3. bounded work: the frontier, `more`, paging
├── 4. cancel
├── 5. the cache
├── 6. unsaved text and edits
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
| opens the same function after a restart | instant | the V-NOC core's cache; the analyzer supplies stable ids and `environment_inputs` (built in TS3). An entry is a glob or a path relative to the root, **or an absolute path** (with `/`) for a config outside the root that feeds the program (an `extends: "../base.json"`): `relative_to` keeps the engine's absolute path when the root is not its prefix. The doc comment of `Project::environment_inputs` says "relative" and no fixture has it; step 0b fixes both. Python's list has the same case (an environment outside the root), so the vnoc text says the core's hashing takes both |
| types in the file, then opens the tree | the unsaved text is used | **TypeScript**: `Session::set_text` (§6) |

Bounding, `more`, paging and cancel are in `analyzer-core`, so Rust gets them for free and Python can adopt the same shape (M18).

## 2. A session

```rust
// typescript-analysis (TS5)
pub struct Session { /* a Project, the caller's epoch, an edit count, the unsaved texts */ }

impl Session {
    /// `epoch` is the caller's number for this session (the driver's start time, say): a token from another session is `Stale`.
    pub fn open(root: &Path, file: &Path, epoch: u64, marker: MarkerStyle<'static>) -> Result<Session, OpenError>;

    /// One request: the graph under `entry`, walked no further than `budget`.
    pub fn graph(&self, file: &str, entry: &Entry<'_>, budget: &Budget, cancel: &Cancel) -> Option<Graph>;
    /// Continue under a node's `more`: a new bounded graph rooted at the def the token names, and where its children start.
    pub fn expand(&self, token: &str, budget: &Budget, cancel: &Cancel) -> Result<(Graph, Page), Stale>;
    /// The render options that put `more` tokens of this session on the nodes.
    pub fn render_options(&self, page: &Page) -> RenderOptions;

    /// A file has unsaved text (`None`: back to the disk). Every token made before is `Stale` after it.
    pub fn set_text(&mut self, file: &str, text: Option<&str>) -> Result<(), OpenError>;
    /// Something in `environment_inputs()` changed: load again.
    pub fn reload(&mut self) -> Result<(), OpenError>;
}
```

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
> once, so the `RefCell`s are only ever touched by one thread. `set_text` and `reload` replace the program, so they take `&mut self`: the
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
  opened and can mark it.
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
- **It lives in memory and dies with the process.** A new process has a new `epoch`, so an old token is `Stale`. An edit (`set_text`,
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
- **The call table** (`report/02-graph-or-tree-to-the-frontend.md` §5) would replace most of this: a `Frontier` row is the `more`. Decide it
  before building the cursor.

## 4. Cancel

Built (§0); TS5 adds only proofs. Who checks, and when:

| Who checks | When |
|---|---|
| the core solver | before each body (built) |
| `walk.rs` | before each site (built) |
| the engine | **never** (§0: it cannot be told to stop, and a cancelled checker breaks the next request) |

A single site's resolution is the largest step that cannot be stopped: microseconds warm (§0: 90 contexts in about 1 ms), well under a
millisecond on average cold (S-TS3: 437 sites in 57 ms). A
cancelled request returns what it has, its cut contexts `Incomplete` and their nodes marked `Incomplete` (carrying `more` once tokens are
built, D31). No cut summary outlives the request,
because the solver itself does not (§2); `Sem`'s caches hold only the syntax of a body and the checker's answers, which a cancel cannot leave
half-made.

> **🦀 Rust note: which thread holds what.** The driver's reader thread receives `$/cancelRequest` while the worker thread is inside `graph`.
> The worker owns the `Session` (`Sem` is `Send`, so it can be moved to the worker, but not `Sync`, so it cannot be shared). The reader holds
> only a clone of the request's `Cancel`: an `Arc<AtomicBool>`, which is `Send + Sync`, so both threads can hold it. `cancel()` stores `true`
> with `Ordering::Relaxed`: the worker only has to notice soon, not at an exact instant.

## 5. The cache

| Level | Lives | Keyed by | Dropped when |
|---|---|---|---|
| a body's sites, the outlines, the checker's types | inside `Sem`, in memory | the program's nodes, files | the program is replaced (`set_text`, `reload`) |
| the solver's memo | one request | `(function, key)` | the request ends |
| rendered trees | in the V-NOC **core**, on disk | entry + `environment_inputs` hash + file hashes | the core's business |

The first version of this page worried about an edit in `b.ts` that renames `f`, called from an unchanged `a.ts`: a memo kept across the
edit would hold a wrong summary for `a.ts`. With no memo across requests, and every cache inside `Sem` belonging to one program, an edit
gives a new program and nothing from before it is read again. The question comes back only if a later step keeps something across an edit
(an incremental program, step 7).

## 6. Unsaved text and edits

TS3's `Project` builds a program from disk. Two ways to follow the editor, the simple one first:

1. **An overlay file system (step 6).** `set_text` records the text, and the program is built again through a `tsrs_vfs` file system that
   answers the unsaved files from memory and the rest from disk (`sem` already builds its file system from `osvfs` and `cachedvfs`). The cost
   of the first request after an edit is about a fresh open plus a cold graph: on V-NOC about 150 ms + 40 ms (§0), **to be measured** on the
   built thing.
2. **`tsrs_project::Session` (step 7, only if 1 is too slow).** The layer tsrs's own language server uses (`did_open_file`,
   `did_change_file`, `did_change_watched_files`, `snapshot()`): it rebuilds the program incrementally, so unchanged files keep their parsed
   trees. **[check]** whether it can be driven without a client connection (it depends on `tsrs_lsproto`); `tsrs_api` is a second example of
   driving sessions from outside the server. The owner sets what "too slow" means from step 6's number.

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
| 2b | **Not yet (owner, 2026-11-13, D31); the design is §3 "Later": cursors kept in memory, gone with the process.** Waits for the owner's decision on a call table (`report/02-graph-or-tree-to-the-frontend.md`, not in the plan yet): `Token`, `RenderOptions.tokens / max_children / start`, `Node.more`, `Frame.more`, `WIRE_FORMAT_VERSION = 3`, the merge's rule for `more`. If the answer is a call table, a frontier row in the table is the `more` and most of this is not built. If not, it is built with the four answers of the step-2 review (2026-11-13): the expansion knows its ancestors, so recursion across a seam is a `Recursive` leaf as in one request; a token is **looked up** in what the session listed, never decoded (a key digest cannot be turned back into a key, RS5 §5); paging happens **after** merging, or the reader's merge sees part of a target's calls on each page; a node's `more.offset` is the index of its first child not drawn | `core/` | as before: every node of the four kinds in §3 has `more` and no other; stitching gives one unbounded request; paging with `max_children = 1` visits every child once; default output byte-identical |
| 3 | TypeScript class entry = `Root::Construct` (D25 (1)); the counter for the definition-time sites of a top-level class (D25 (3)) | `typescript-analysis` | the `construct` fixture and a policy row; V-NOC and postcss numbers unchanged for function entries; a class entry's tree is the class callee's subtree |
| 4 | `Session::open / graph` (`expand`, `render_options` and `Stale` come with 2b) | `typescript-analysis` | for every scenario and every policy row, at budgets 1, 2 and 3: asking for every `Cut` node's def as a new entry and stitching gives the tree of one unbounded request (the recursion exception of step 2 aside); `TaskDetailPanel` the same |
| 5 | the cancel proofs (the code is built) | `typescript-analysis` | a `Cancel` fired from another thread during a V-NOC request gives `Incomplete` nodes, and the next request with a fresh `Cancel` is the full tree |
| 6 | `set_text` and `reload` through an overlay file system | the `sem` module | rename a callee in an unsaved buffer: the tree follows, `set_text(file, None)` goes back (with 2b: the old tokens are `Stale`); **[measure]** the first request after an edit on V-NOC |
| 7 | only if the owner finds step 6's number too slow: the program from `tsrs_project::Session` | the `sem` module | the same tests; the number before and after |
| 8 | `typescript-cli calls … --depth N --max-contexts N` (`--max-children N` and `typescript-cli expand <token>` come with 2b) | `typescript-cli` | — |
| 9 | **tag `typescript-v0.3.0`**; the driver implements `vnoc/callTree`, `vnoc/expand` (until 2b: a `callTree` on the cut node's def), `$/cancelRequest`, `vnoc/environmentInputs` | `vnoc` | the tier-3 conformance checks |

Until 2b is decided: step 4 builds `Session::open` and `Session::graph` (a budget, frontier nodes `Cut`) and not `expand`, `render_options` or `Stale`; step 5's cancelled request gives `Incomplete` nodes, with no `more`; step 8 has no `expand` subcommand. Step 3 does not depend on 2b.

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
