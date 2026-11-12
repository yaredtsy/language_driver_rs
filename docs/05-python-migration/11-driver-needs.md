# 05.11 PY9: what the driver needs

```
PY9
├── 0. what the code has today (read and run at e46646d, 2026-11-12)
├── 1. what the contract asks, and what it does not
├── 2. what is not copied from TypeScript's TS5, and why
├── 3. cancel
├── 4. keeping the project open
├── 5. environmentInputs
├── 6. the stack
├── 7. the steps
├── 8. what the vnoc side does
├── 9. what is not in this file
└── 10. to check while building
```

Written on 2026-11-12 after reading TypeScript's [`../03-typescript/07-ts5-driver-needs.md`](../03-typescript/07-ts5-driver-needs.md), the V-NOC
driver contract (`vnoc/docs/14-call-trees.md` §5–6, `vnoc/docs/06-drivers.md` §c, the protocol crate `vnoc-driver-protocol`), the V-NOC
Python driver (`vnoc/drivers/python/src/`), `python-analysis`, and the vendored engine (`ty_project`, `ruff_db`). §0's numbers were **run**
(the built `python-cli`, `--timing`); the rest is **not built**. Every `[check]` is a claim to verify before depending on it.

## 0. What the code has today

| The driver needs | Today | Where |
|---|---|---|
| a project kept open between requests | **no**: the driver calls `Project::open` inside every `vnoc/callTree` | `vnoc/drivers/python/src/call_tree.rs`, `call_tree` |
| cancel that stops the work | **no**: the driver keeps a flag per request and answers `-32800`, but `call_wire` never sees it; the analysis runs on until it ends or the 20 s deadline (`Limits::deadline`) cuts it. The driver says so: "the analyzer has no cancel flag, VS-D69" | `vnoc/drivers/python/src/main.rs`, `respond` |
| a deadline | **yes**: `Limits::deadline`, checked before each walk; a cut context is `Incomplete(Deadline)` | `interp/solver.rs`, `cost_limit` |
| `depth` | **output only** (`Limits::render_depth`): every body is walked, the tree is shortened | the contract says so too: "`depth` … shortens the output only" |
| `vnoc/expand` | not implemented in the driver; the contract makes it `callTree` with the callee as the entry | `14-call-trees.md` §5 |
| `vnoc/environmentInputs` | not implemented; `python-analysis` has no such function (TypeScript has `Project::environment_inputs`) | |
| a big enough stack for the solver's recursion (`max_stack` = 256) | **no**: each request runs on `std::thread::spawn`, which gives 2 MB; [`05-entries-and-targets.md`](05-entries-and-targets.md) §6 already says the driver must give more | `main.rs` |
| the mark `likely` (D28) | the driver keeps `recursive`, `cut`, `incomplete` and **drops any other mark** (`call_tree.rs`, the `marks` filter); the protocol's `CallMark` has no `Likely` | the vnoc side (§8) |

**Measured** (the built `python-cli calls --wire --timing`, debug build, one process per run, 2026-11-12):

| Project, entry | open | solve |
|---|---|---|
| Jarvis, `CallGraphGenerator.do_pass`, first run | 35 ms | 123 ms |
| Jarvis, the same, second run (the OS file cache warm) | 1 ms | 90 ms |
| the Flask service, `FilesResource.post` | 2 ms | 201 ms |

`open` is cheap: the engine is lazy. The cost is in `solve`, and most of it is the engine's own work (parsing, resolving imports, inferring
types) done **again on every request**, because every request starts a new database. A database kept open keeps those answers (salsa
memoizes them). TypeScript's second request went from 40 ms to 1 ms
([`../03-typescript/07-ts5-driver-needs.md`](../03-typescript/07-ts5-driver-needs.md) §0). Step PY9.0 measured Python's: **a request on a kept
project costs 2.5–10 ms against 85–175 ms cold** (15× to 37× less; debug build, the same three entries;
[`languages/python/ENGINE.md`](../../languages/python/ENGINE.md), "Keeping the project open (PY9.0)").

## 1. What the contract asks, and what it does not

The **shipped 0.2 contract** (`14-call-trees.md` §5) is narrower than the tier-3 wish list (`vnoc_playground/fable_plan/09-adding-a-language/02`
§4):

| Need | Shipped 0.2 contract | This file |
|---|---|---|
| a call tree | `vnoc/callTree { root, file, entry, depth? }`, read from disk | unchanged |
| more under a node | `vnoc/expand` **is** `callTree` with the callee as the entry | nothing new to build: a kept project makes it fast (§4) |
| cancel | "the driver drops the answer of a cancelled request. That is enough for 0.2" | **stop the work too** (§3): a dropped answer still holds a thread and a CPU until the deadline |
| environment inputs | tier 3 names `vnoc/environmentInputs { root }` → globs | §5 |
| bounded work, a frontier, `more` tokens, paging | not in 0.2 | not here (§2) |
| unsaved text for a call tree | `CallTreeParams` has no `text` | not here (§9) |

The core's host sends requests **in parallel** and times `callTree` out at 30 s (`14-call-trees.md` §6.1). Today each parallel request opens
its own database, so two trees at once are two databases in memory.

## 2. What is not copied from TypeScript's TS5, and why

**No frontier.** TS5 bounds the *work*: bodies past a depth are not walked. That is safe there because TypeScript's flat solver carries no
value from a callee back to its caller: skipping a body removes its subtree and nothing else. Python's summaries carry **values** back (a
return value today; after class summaries also the objects and the writes: [`10-class-summaries/`](10-class-summaries/README.md)). An
unwalked callee would return `Unknown`, so calls **above** the frontier, on what it returned, would become `Unresolved`. A bounded Python tree
would be a different tree, not a shorter one. Python keeps walking what its answer needs, and bounds the request by **time** (the deadline,
and cancel) and the **output** (`render_depth`, and later paging).

**No session type of its own, no token.** TS5 adds a `Session` because a TypeScript program must be rebuilt for edits and its `more` tokens
need an epoch. Python's `Project` already is the long-lived handle (it owns the engine's database and borrows nothing), and the contract's
`expand` names a def by `(file, qualname, occurrence)`, not by a token. So Python adds two methods to `Project` (§4, §5) and no new type.

**What comes from TS5 anyway.** TS5 steps 1–2 change core: `Status::Frontier`, `Node.more`, `Frame.more`, `RenderOptions` paging, and
`WIRE_FORMAT_VERSION` 2 → 3 (D23's rule allows it: `more` changes what a node means). Python uses core's renderer and wire, so:

- with default options none of it appears in Python's output (TS5 §3, "off by default"), and Python's tests must show that (rule 5);
- **the wire version Python's driver reports becomes 3** with the next `rev` after TS5 step 2, though Python's wire has no `more`: the vnoc
  agent is told (§8). The TypeScript session coordinates before that step (TS5 §7 says so).

> **🦀 Rust note: why Python can keep the project and TypeScript has to work around it.** TypeScript's solver borrows the program (`&'p Sem`),
> so a struct that holds both the program and a solver would borrow from itself, which Rust does not allow (TS5 §2). Python's `Project` is
> `struct Project { db: ProjectDatabase }`: an owned value with no lifetime. The solver is built per request from `&Project` and dropped at the
> end, so keeping the `Project` in the driver is just keeping a value.

## 3. Cancel

| | |
|---|---|
| The type | core's `analyzer_core::solve::Cancel` (an `Arc<AtomicBool>` and an optional deadline; `Debug`, `Clone`, `Default`; the default never fires). Python already depends on `analyzer-core` |
| How it reaches the solver | a new field `Options::cancel: Cancel` (public `Options`, `lib.rs`), passed into `interp::Options`. Additive: a caller that builds `Options { .., ..Options::default() }` compiles unchanged. `lib.rs` re-exports `Cancel` (`pub use analyzer_core::solve::Cancel`), as TypeScript's does, so the driver names it from the analyzer |
| Where it is checked | `Solver::cost_limit` (`interp/solver.rs`), after the deadline, before every walk, with core's `Cancel::reason()`: a cancel gives `Cut::Cancelled`, the `Cancel`'s own deadline (`with_deadline`) gives `Cut::Deadline`. **Not `is_cancelled()`**: it is true for the deadline too, and the cut would then say `cancelled` for a deadline |
| What it gives | a new `interp::summary::Cut::Cancelled`, counted in `Stats.cuts` (`Cuts.cancelled`), mapped to core's existing `Cut::Cancelled` in `core_graph.rs` `cut`. The cut contexts are `Status::Incomplete(Cancelled)`: the wire marks them `incomplete`, as the deadline does |
| What it never does | interrupt the engine. The check is between walks; one walk's engine queries finish. A cancelled request leaves nothing half-made behind: the solver lives for one request, and the engine's memoized answers are whole answers. After the last check the tree is still rendered and written as the wire (PY9.0: up to 36 ms cold, 12 ms warm); a cancel cannot stop that part, and the driver drops the answer anyway |

`Limits::deadline` stays as it is (§10 claim 4): both deadlines cut the same way and are counted as `Deadline`; the driver may use either.

**A fix this step needs (found while building PY9.1, 2026-11-13).** A context cut by `cost_limit` before its walk was never given the cut:
`Solver::get` returns `FunctionResult::cut` without storing anything, so `core_graph.rs` `status` fell back to `Incomplete(Cancelled)` ("the
limit is unknown") whatever the limit was: a stack, a context or a deadline cut all read `cancelled` on that context. So `get` records the
cut on the slot (`slots[ctx].cut.get_or_insert(cut)`) **only when the slot has no result** (a context walked before keeps its own status),
and `status` reads it. A later walk of the context clears it as today (`get`, `slots[ctx].cut = None`). The counters already told the cause;
now the context does too. The wire does not change: core's renderer marks any `Incomplete(_)` as `incomplete`, whatever the cut. No
snapshot shows a never-walked cut context today (none contains `Incomplete(Cancelled)`), so none moves.

## 4. Keeping the project open

The driver keeps one `Project` per root and asks it many times. Two things then need care: **files change on disk** while it is open, and
**the configuration or the environment changes**.

The contract sends no notification when a file changes (`06-drivers.md`: an unknown notification is ignored, and no change notification is
defined). So the project checks for itself, **before each request**:

```rust
// python-analysis, lib.rs / the sem module
impl Project {
    /// Bring the project up to date with the disk before a request: every file the engine has read is checked again (its content and
    /// whether it exists), and only what changed is recomputed. If an environment input changed (§5), the project is opened again from the
    /// same root. Never fails: a file that cannot be read is what the engine sees, as on open.
    pub fn refresh(&mut self) -> Refreshed;
}

#[non_exhaustive]
pub struct Refreshed { pub reopened: bool }
```

| Rule | Why |
|---|---|
| Files: the engine's `Files::sync_all` (`ruff_db/src/files.rs`), called in the sem module | it re-reads the metadata of every path the engine has seen and bumps the revision of the ones that changed; salsa then recomputes only what read them. This is the engine's own incremental model |
| **Never `apply_changes(&[ChangeEvent::Rescan])`** | `Rescan` also reloads the project, and the reload starts discovery again from the closest config: it could take a **parent** folder's `pyproject.toml` as the root, which D26 forbids (`open_project` exists to prevent exactly that) |
| An environment input changed: open again with `open_project(root)` | a config or a virtual environment changes the search paths and the Python version: everything may differ. Rare, so a full open is fine. The project keeps a stamp (path, size, modified time) of each input file that exists |
| `&mut self` | a refresh changes the database: the compiler then forbids it while a request still borrows the project. It also keeps salsa happy: setting an input while another handle of the database is alive blocks until that handle drops, and nothing in the crate clones the database (checked 2026-11-13: no `db.clone()`) |
| The stamps (PY9.2) | a private list of §5's inputs in `sem/project.rs`, the same list PY9.3 makes public. A stamp is, for each input that exists, `(path relative to the root, size, modified time)`, sorted by path (rule 9); the glob `requirements*.txt` is the root folder's entries that start with `requirements` and end with `.txt` (no glob crate: no new dependency). An input that appears or disappears changes the stamp |
| A reopen that fails | the root is gone or no longer a folder: keep the synced database and answer `reopened: false`. Every request then finds no file and answers `None`, which is what a missing project should give (rule 8: never fail) |

> **🦀 Rust note: `&mut self` is the lock.** `refresh` takes `&mut Project`, every request takes `&Project`. Rust lets you have either one
> `&mut` or any number of `&` at a time, never both, so a refresh can never run in the middle of a request. No mutex is needed inside the
> library; the driver decides how requests reach the project (§8).

**Threads.** `ProjectDatabase` is `Clone` (a handle to shared storage) and its file system is `Send + Sync`. Whether `Project` is `Send` and
whether it is `Sync` is §10 claim 1: the plan assumes `Send` and **not** `Sync` (a salsa database keeps per-thread state). So the driver
gives the project to **one worker thread** and sends it the call-tree requests (§8). Outline, `planIds` and `planFileId` do not use a project
and keep running on their own threads.

## 5. environmentInputs

`Project::environment_inputs(&self) -> Vec<String>`: paths and globs, relative to the root, whose change means "resolution may differ now"
(the contract's `vnoc/environmentInputs`). The core hashes them and drops cached trees when the hash changes; `refresh` uses the same list
for its stamps.

| Input | Why | [check] |
|---|---|---|
| `pyproject.toml`, `ty.toml` in the root only | the configuration; a parent's is never read (D26) | |
| `.python-version` | the Python version | does the vendored engine read it |
| `.venv/pyvenv.cfg` | the project's virtual environment (`ty_site_packages`: `<root>/.venv` when `VIRTUAL_ENV` is not set) | a venv elsewhere, set by `[tool.ty.environment] python`: add that path when the config names one |
| `uv.lock`, `poetry.lock`, `Pipfile.lock`, `requirements*.txt` | a stand-in for "the installed packages changed", as TypeScript lists lock files | whether a `pip install` into `.venv` without a lock change is seen (the `site-packages` folder itself is not listed: thousands of files) |

`VIRTUAL_ENV` and `CONDA_PREFIX` are environment variables of the driver process: fixed for its life, so not inputs.

## 6. The stack

The solver recurses: a walk that calls a callee walks it before returning, to `max_stack` (256) deep. The driver's request threads have
Rust's default 2 MB. Step PY9.0 measured it (debug build, `languages/python/ENGINE.md`, "Keeping the project open (PY9.0)"): one level takes
about 7.5 KB, a chain that reaches `max_stack` needs **2052 KB** (it overflows at 2048 KB, so it would abort the driver on a default thread),
and the real entries need at most 288 KB. **The driver asks for 8 MiB** (`std::thread::Builder::stack_size(8 << 20)`): the 2 MiB needed,
rounded up to the next power of two (4 MiB), and doubled for a margin, because a level of real code can be larger than the chain's one-line
body. Making the solver iterative is not in this file.

## 7. The steps

Each step is one commit with explicit paths, tests first and seen red for the stated reason. They touch only `languages/python` and docs;
nothing in `core/` (rule 5 is still checked: core's tests run).

### PY9.0. Measure (a spike, deleted after)

| | |
|---|---|
| What | an `#[ignore]` test in `python-analysis` (not a spike workspace: a second workspace would build the vendored engine again, several GB, and the disk does not have them; CLAUDE.md's spike rule bends here, and the test is deleted when the numbers are written) |
| Measures | on Jarvis and the Flask service: (a) three requests, each on a fresh `Project`; (b) three requests on one `Project`; (c) the cost of `Files::sync_all` on a warm project with nothing changed, and after touching one file; (d) the stack: the debug CLI under `ulimit -s` (a subshell), on a generated chain that reaches `max_stack` and on the real entries; (e) the resident memory of a warm project (`/usr/bin/time -l`, and `ps` from the test). **Done** (`f4c8885`): the numbers are in `languages/python/ENGINE.md`, "Keeping the project open (PY9.0)" |
| Proof | the numbers in `languages/python/ENGINE.md`, "Keeping the project open"; the test removed in the same commit |

### PY9.1. Cancel

| | |
|---|---|
| Change | §3 |
| Failing tests | `tests/api.rs`: (a) a `Cancel` fired before the request: the root context is `Incomplete(Cancelled)`, `Stats.cuts` has `Cancelled` 1, the wire's root carries `incomplete`. (a′) `Cancel::new().with_deadline(Instant::now())`: the cut is `Deadline`, not `Cancelled`. (c) the next request with a fresh `Cancel` on the same `Project` gives the full tree, equal to a request on a fresh project. **Unit** (`interp/fixture.rs`): (b) a cancel fired after walk *n*, through a test-only `interp::Options::cancel_at_walk: Option<u32>` (`#[cfg(test)]`; `cost_limit` fires the `Cancel` when `stats.walks` reaches it, then checks as usual): on `02-function-summaries/python/cuts.py` `main` with `n` = 2, exactly two walks, every cut is `Cancelled`, the root is `Incomplete(Cancelled)`. A second thread is not used: nothing in the public API tells a test that a walk has happened, so a thread could only wait on time; the flag crossing threads is core's `Arc<AtomicBool>`. (e) the fix of §3: `cuts.py` `main` with `max_stack` = 2: `b` and `mid` are cut before their walk and walked nowhere else, and read `Incomplete(MaxDepth)`; red before the fix (`Incomplete(Cancelled)`). (d) `Options::default().cancel` never fires: every existing test unchanged |
| Snapshots | none moves |
| Proof | `cargo test` in `languages/python`; clippy on `python-analysis`; `cd core && cargo test` |

### PY9.2. Refresh

| | |
|---|---|
| Change | §4: `Project::refresh`, `Refreshed`, the stamps (§5's list, private until PY9.3) |
| Failing tests | each on a small project the test writes into `std::env::temp_dir()` (a unique folder per test, removed at the start and at the end; no new dependency; a test may write files, rule 2 is about the library): (a) open, ask, rewrite the callee's file so it calls another function, `refresh`, ask: the tree follows; without `refresh` it does not (pinned, so the rule "refresh before each request" has a reason in the tests). (b) a new file that an import names, created after the first request: after `refresh` the import resolves. (c) a deleted callee file: after `refresh` the call is `Unresolved`, nothing panics. (d) a `ty.toml` added to the root: `reopened` is true. (e) **D26**: a `pyproject.toml` added to the **parent** folder after open: after `refresh` the root and every answer are unchanged. (f) a compile-time check that `Project: Send` (`fn send<T: Send>() {}`), and a comment saying it is not `Sync` if §10 claim 1 finds so. (g) the root folder removed after the first request: `refresh` does not panic, `reopened` is false, the request answers `None` |
| Proof | `cargo test` in `languages/python`; the numbers of PY9.0 (c) say a refresh is cheap enough to run before every request |

### PY9.3. environmentInputs

| | |
|---|---|
| Change | §5: `Project::environment_inputs`, the list PY9.2's stamps already use, made public; §10 claim 6 checked, and the list fixed if the engine reads more |
| Failing tests | on `tests/fixtures/09-root/…` and a fixture with a `.venv/pyvenv.cfg`: the list, in a fixed order (rule 9); a config named by the project (`[tool.ty.environment] python`) adds its path |
| Proof | as PY9.1 |

### PY9.4. The CLI shows it

| | |
|---|---|
| Change | `python-cli calls … --repeat <n>`: the same request `n` times on one `Project`, with `refresh` before each; with `--timing`, one `open` line and `n` `refresh` / `solve` lines. The output is printed once (the answers are equal: the test checks it) |
| Failing tests | `tests/cli.rs`: `--repeat 3 --timing` prints three `solve` lines; stdout equals a run without `--repeat`; `--repeat 0` is exit 2 |
| Proof | as PY9.1; by hand on Jarvis: the second and third `solve` numbers, next to PY9.0's |

After PY9.4: the vnoc side (§8), then a tag when the owner says (`python-v0.3.0`, or with class summaries' CS4b: [`10-class-summaries/04-resolve-then-write.md`](10-class-summaries/04-resolve-then-write.md) §5).

## 8. What the vnoc side does

Text for the vnoc agent, after PY9.4 is pushed and CI is green. Nothing here is built in this repo.

1. **One worker thread owns the projects.** `vnoc/callTree` (and `vnoc/expand`, the same request) is sent to it; it keeps one `Project` per
   root (open on first use), calls `refresh()` and then `call_wire` for each request. Built with `std::thread::Builder::new().stack_size(…)`,
   the size from `ENGINE.md` (§6). Outline, `planIds`, `planFileId` stay as they are.
2. **Cancel stops the work.** The per-request flag becomes an `analyzer_core::solve::Cancel`, stored in the in-flight map; `$/cancelRequest`
   calls `cancel()`; the request passes it in `Options::cancel` (`python_analysis::Cancel`). The answer of a cancelled request is still
   dropped (`-32800`). A request still queued when it is cancelled is answered `-32800` without being run. To count the time a request
   waits in the queue against its deadline, build the `Cancel` with `with_deadline(received + 20 s)` when the request arrives (§10 claim 4).
3. **`vnoc/environmentInputs { root }`** answers `Project::environment_inputs()`; capability `environmentInputs: true`.
4. **The mark `likely`** (D28, from the last `rev`): add `CallMark::Likely` to the protocol (additive) and map `"likely"` to it, instead of
   dropping it. Until then the UI shows a likely call as a certain one.
5. **After TypeScript's TS5 step 2**, the wire version the driver reports is 3; Python's wire has no `more` field with default options.

## 9. What is not in this file

| Thing | Why not | Where it goes |
|---|---|---|
| a frontier, bounded work, `more` tokens for Python | §2: values flow, so a frontier changes answers above it | if the contract asks for it, a plan that marks every node whose value depends on an unwalked body |
| paging (`max_children`) | core's renderer gets it in TS5 step 2; the 0.2 contract does not ask for it | when the contract does |
| unsaved text for a call tree | `CallTreeParams` has no `text` | when the contract adds it: a file system in the sem module that answers open buffers from memory (the `System` trait, `ruff_db/src/system.rs`), and `ChangeEvent::Changed` for the path |
| an iterative solver (no deep stack) | larger work; the stack size is enough for now (§6) | its own plan |
| a cache of trees across driver restarts | the V-NOC core's job (`14-call-trees.md`); the analyzer supplies `environment_inputs` and stable ids | — |
| letting the engine itself be cancelled (salsa's cancellation) | not needed: the check between walks is fine-grained enough, and a cancelled engine query unwinds as a panic, which would need catching | if PY9.0 shows one walk can take long |

## 10. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | `Project` is `Send` and not `Sync` (salsa's database keeps per-thread state) | PY9.2 (f), `a_project_can_be_moved_to_another_thread`. **Holds.** `send::<Project>()` compiles; `sync::<Project>()` fails with E0277: `RefCell<QueryStack>` and an `UnsafeCell` map inside salsa's `ZalsaLocal` (in the database's storage) cannot be shared between threads. So the driver may move a project to the thread that serves it, and must not share `&Project` across threads: one owner, or a mutex around it |
| 2 | `Files::sync_all` also sees a file that did not exist when it was first looked up (the engine records a path it could not find, so a later creation bumps it) | PY9.2 (b), `a_refresh_finds_a_module_that_did_not_exist_at_the_first_request`. **Holds.** `main.py` imports `extra`, which is not there at the first request (the call is `Unresolved`); after `extra.py` is created and `refresh` runs, the call reaches `extra.run`. Nothing but `Files::sync_all` was needed (no `sync_path`, no directory sync, no `ChangeEvent`). The test moves the file's modified time forward, because the engine sees a change only through it. Next to it, PY9.0 (c′) saw a file **changed** after the first request seen after `sync_all` and not without it, and PY9.2 (a) pins both |
| 3 | A warm request costs a small part of a cold one, so keeping the project is worth the memory it holds | PY9.0. **Holds.** Warm 5.7 / 9.5 / 2.5 ms against cold 86 / 175 / 94 ms (Jarvis, post, get_files: 15×, 18×, 37×), debug build. A kept project holds at most 60 / 78 / 62 MiB resident (the process starts at 6 MiB), and `sync_all` with nothing changed costs 0.05–2.3 ms and keeps it warm |
| 4 | The deadline could move into `Options::cancel` (`Cancel::with_deadline`) and `Limits::deadline` go: one mechanism. Say whether anything is lost (the deadline is measured from the solver's start today, from the caller's clock with `Cancel`) | **Answered in the plan (2026-11-13), not yet built: keep both.** Nothing in the analysis is lost either way: `cost_limit` reads `Cancel::reason()`, so a `Cancel` deadline is a `Deadline` cut, as `Limits::deadline` is. They differ in the clock: `Limits::deadline` counts from the solver's start (not the open, the refresh or the queue), while a `Cancel` deadline is an `Instant` the caller picks and can include them, which suits the host's 30 s timeout. Removing `Limits::deadline` would break every caller that sets it (the V-NOC driver does), for no gain. The vnoc text (§8) says the driver may pass `with_deadline` |
| 5 | One walk never runs long enough that a cancel between walks feels late (the longest single walk on the three projects) | PY9.0. **Holds.** Measured as the longest time between two `cost_limit` calls in one request (a walk recurses into its callees, so its own time includes theirs): cold 35 / 15 / 5 ms (Jarvis, post, get_files; one outlier each, 96 and 63 ms), warm 1.8 ms at most, debug build on a busy machine. After the last check, the render and the wire take up to 36 ms cold, 12 ms warm, and no check covers them |
| 6 | The engine reads `.python-version` and a venv named in the config; the list of §5 is complete for the vendored engine | PY9.3 |

Next: [`../06-roadmap.md`](../06-roadmap.md)
