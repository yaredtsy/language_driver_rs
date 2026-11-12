# 03.07 TS5: what the driver needs

```
TS5
├── 1. the list, from the V-NOC driver contract
├── 2. a session: the long-lived object
├── 3. bounded work, the frontier, expand, paging
├── 4. cancel
├── 5. the cache
├── 6. unsaved text and edits
├── 7. steps
└── 8. how drivers ship, and what that asks of the vnoc side
```

## 1. The list

TS4's `call_graph` computes everything reachable and returns. V-NOC's tier 3 asks for more (`09-adding-a-language/02` §4), and warns that for
Python these were added afterwards as one painful batch. Here they are designed in from TS4's shape; TS5 is where they are built.

| The UI does | The driver contract asks for | Where it lives |
|---|---|---|
| opens a function | one level, fast | **core** solver: `summary()` is one body |
| clicks ＋ on a node | continue from a cut node without redoing the tree above | **core**: the memo is kept between requests |
| clicks ＋ on a node with 300 callees | a page of children | **core** renderer: `children` + `next` |
| clicks away | really stop | **core** `Cancel` + `sem` passing it to the engine |
| opens the same function after a restart | instant | the V-NOC core's cache; the analyzer supplies stable ids and `environment_inputs` |
| types in the file, then opens the tree | the unsaved text is used | **TypeScript**: `tsrs_project::Session` |

Four of six are in `analyzer-core`, so **Rust gets them for free** and Python can adopt the same request shape.

## 2. A session

```rust
// typescript-analysis/src/lib.rs (added in TS5)
pub struct Session { /* a Project + a Solver that outlives one request */ }

impl Session {
    pub fn open(root: &Path, file: &Path) -> Result<Session, OpenError>;

    /// One request: the tree under `entry`, limited by `budget`. Cut nodes carry a token.
    pub fn tree(&mut self, file: &str, entry: &Entry, budget: &Budget, cancel: &Cancel) -> Option<Tree>;
    /// Continue under a cut node.
    pub fn expand(&mut self, token: &Token, budget: &Budget, cancel: &Cancel) -> Result<Tree, Stale>;

    /// A file changed on disk, or has unsaved text (None = back to the disk content).
    pub fn set_text(&mut self, file: &str, text: Option<&str>);
    /// Something in `environment_inputs()` changed: start again.
    pub fn reload(&mut self) -> Result<(), OpenError>;
}
```

`call_graph` from TS4 stays, as the simple entry for the CLI and the tests. `Session` is the same machinery kept alive.

> **🦀 Rust note: `&mut self` as a design statement.** `tree` and `expand` take `&mut self`: they fill the solver's memo. That makes the
> compiler enforce "one request at a time on one session", which is exactly what the single-threaded, deterministic walk needs (rule 11). The
> driver that wants two requests at once puts the session behind a `Mutex`, or has one session per project and a queue. Do not reach for
> `RefCell` or atomics inside `Session` to make the methods `&self`; that hides the question instead of answering it.

## 3. Bounded work, the frontier, expand, paging

```rust
// analyzer-core::solve / render
pub struct Budget { pub max_depth: u32, pub max_nodes: u32, pub max_children: u32, pub deadline: Option<Duration> }

pub struct Tree { pub root: Node, pub stats: Stats }
// Node gains:  pub more: Option<Token>      // set on a node whose children were not (all) computed

pub struct Token(String);     // opaque outside the analyzer
```

How it works with one-body summaries:

```
tree(entry, budget):
    render lazily: for each node within depth and node budget → solver.summary(context)   ← walks that ONE body if not memoized
    a node beyond the budget gets  more = token(context, child offset)                    ← no body below it was walked
expand(token):
    decode (context, offset) → render from that context with a fresh budget
```

- **Bounded work, not bounded output.** A body below the frontier is never walked. (pylspt's `render_depth` only shortens the output; the V-NOC
  plan lists that as an open ❌ for Python.)
- **The token** is `ContextId` (the stable def and the digest) plus a child offset for paging, encoded as text. Because the id is stable, a
  token stays valid across requests as long as the function's file has not changed; after an edit it is answered with `Stale`, and the driver
  falls back to asking by `{ file, qualname, occurrence }`, as the contract describes. `occurrence` is 0-based (D18); the Python driver's is 1-based, so the driver converts and the contract with the extension states the base.
- **Paging** is the renderer's `max_children`: a node with more children than that gets `more` with the offset of the next child.
- **A cut by budget is not `Incomplete`.** It is a frontier (`more`), and what is above it is complete and cacheable. `Incomplete` and
  `Mark::Cut` remain for the cost limits of rule 10 (deadline, stack).

This lives in the core. TS5 step 1 is to add it there, with tests on a fake `Lang` (a hand-written graph), before any TypeScript code uses it.

## 4. Cancel

```rust
// analyzer-core::solve
#[derive(Clone)]
pub struct Cancel(Arc<AtomicBool>);
impl Cancel { pub fn new() -> Self; pub fn cancel(&self); pub fn is_cancelled(&self) -> bool; }
```

| Who checks | When |
|---|---|
| the core solver | before each body |
| `walk.rs` | before each site (a resolver call can take milliseconds on a cold checker) |
| the engine | if tsrs's `Context` can be cancelled from outside, `sem` connects it to `Cancel`, so that a long type computation stops too **[check in TS0/S-TS3: `Checker::was_canceled` exists; find how a `Context` is cancelled]** |

A cancelled request returns what it has, marked `Incomplete`, and the partial summaries are **dropped from the memo** (rule 10: a cut result is
never reused).

> **🦀 Rust note: `Arc<AtomicBool>`.** The driver's JSON-RPC reader thread calls `cancel()` while the worker thread is deep in a walk. Two
> threads, one flag: `AtomicBool` can be written and read from different threads without a lock, and `Arc` lets both threads own it. `Clone` on
> `Cancel` clones the `Arc` (a counter bump), not the flag, so every clone sees the same value. Use `Ordering::Relaxed`: a cancel only needs to
> be noticed soon, not at an exact instant.

## 5. The cache

Two levels, with different owners:

| Level | Lives | Keyed by | Dropped when |
|---|---|---|---|
| the solver's memo: one summary per context | in the `Session`, in memory | `(function, key)` | that function's **file** changes (`set_text`), for a default resolver. A summary depends on its own body and on what its callee expressions resolve to, so a change in another file can also change it: see below |
| rendered trees | in the V-NOC **core**, on disk | entry + `environment_inputs` hash + file hashes | the core's business |

The honest difficulty: `a.ts` calls `f` imported from `b.ts`; an edit in `b.ts` renames `f`. The summary of the function in `a.ts` is now wrong,
though `a.ts` did not change. Two ways:

1. **simple (TS5):** any `set_text` drops the whole memo. Summaries are cheap to recompute (one body each, on demand), and the expensive thing,
   the loaded program, is kept by the engine's own incremental layer (§6). **[measure]** before doing anything smarter;
2. **precise (later):** each summary records the files of its targets; an edit drops the summaries that mention the edited file.

Start with 1.

## 6. Unsaved text and edits

TS3's `Project` builds a program from disk. For a session that follows the editor, tsrs has the layer its own language server uses:
`tsrs_project::Session` (`did_open_file`, `did_change_file`, `did_change_watched_files`, `snapshot()`, `get_language_service`). It keeps an
overlay of unsaved files over the disk, rebuilds the program **incrementally** (unchanged files keep their parsed trees), and owns the memory
question of TS3 §4 with reference-counted snapshots.

So TS5 changes what is *inside* `sem::Sem`, behind the same functions:

| | TS3–TS4 | TS5 |
|---|---|---|
| the program comes from | `tsrs_compiler::new_program`, once | `tsrs_project::Session::snapshot()` → that snapshot's program |
| `set_text(file, Some(text))` | not supported | `did_open_file` / `did_change_file` on the tsrs session |
| a file changed on disk | reload everything | `did_change_watched_files` |
| handles (`Func<'p>`) valid for | the `Project` | one snapshot; the solver's memo is dropped when the snapshot changes (§5, way 1) |

This is the reason `sem.rs` is a wall: nothing in `walk.rs`, `calls.rs` or the core knows which of the two columns it is running on.

**[check]** when TS5 starts: how much of `tsrs_project` needs the LSP types (`lsproto`), and whether a session can be driven without a client
connection. `tsrs_api` (the Node API's backend) is a second example of driving sessions from outside the language server.

If the tsrs session layer turns out to be too entangled with its server, the fallback is modest: keep TS3's `Project`, implement `set_text` by
rebuilding the program with an overlay file system (`tsrs_vfs` has the file-system trait), and accept a slower first request after an edit.

## 7. Steps

| # | Do | Where | Proof |
|---|---|---|---|
| 1 | `Budget`, lazy render, `more` tokens, paging | `core/` | tests on a hand-written `Lang`: a 3-level tree with budget 2 walks exactly the bodies above the frontier (count the `walk` calls) |
| 2 | `Cancel` in solver and walk; cut results not memoized | `core/`, `walk.rs` | a `Lang` whose `walk` cancels after N bodies |
| 3 | `Session::open / tree / expand` on TS3's `Project` | `typescript-analysis` | expand under every cut node of a scenario gives the same tree as one unbounded request |
| 4 | `Stale` tokens | | edit a fixture between `tree` and `expand` |
| 5 | `set_text` with "drop the whole memo" | | change a callee's name in an unsaved buffer; the tree follows |
| 6 | move `Sem` onto `tsrs_project::Session` (or the fallback) | `sem.rs` only | the same tests; the time of the first request after an edit **[measure, before and after]** |
| 7 | `typescript-cli` gets `--depth`, `--max-nodes`, and an interactive `expand` for trying it by hand | | — |
| 8 | **tag `typescript-v0.3.0`**; the driver implements `vnoc/callTree`, `vnoc/expand`, `$/cancelRequest`, `vnoc/environmentInputs` | `vnoc` | the tier-3 conformance checks |

Steps 1 and 2 can be done right after B0, before any TypeScript exists; they need only the core. Doing them early means TS4's `call_graph` is
built on the final solver.

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
