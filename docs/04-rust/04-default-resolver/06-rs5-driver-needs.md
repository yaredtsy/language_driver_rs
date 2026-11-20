# 04.04.06 RS5: what the driver needs

```
RS5
├── 1. what comes from core, and what is Rust's
├── 2. the session
├── 3. edits: a file saved on disk
├── 4. cancel
├── 5. the memo, and one thing core's token must not assume
├── 6. reload
└── 7. steps, and the tag
```

The full design is TypeScript's ([`../../03-typescript/07-ts5-driver-needs.md`](../../03-typescript/07-ts5-driver-needs.md)); this file is
what differs.

## 1. What comes from core

| Need | Where | State |
|---|---|---|
| `Budget`, the frontier, a frontier node drawn `Mark::Cut` | `analyzer-core` | **built by TS5 steps 1 and 2a**: `Solver::graph_within`, `Status::Frontier`, `Graph.stats.frontier` |
| `more` tokens, `expand`, `Stale`, paging | `analyzer-core` | **not built** (TS5 step 2b, the owner's plan, D31). Until then a cut node is continued by a new request for its def |
| `Cancel` (a flag and a deadline) | `analyzer-core::solve` | built |
| stable ids, `environment_inputs` | RS3, RS4 | built by then |
| following the disk (`refresh`), reload | Rust | this phase. Text the editor has not saved is not read, for now (D42) |

## 2. The session

```rust
// rust-analysis (RS5). Built in step 2 (ac0c34b): open, graph, project. refresh, reload, Refreshed: steps 3 and 4.
pub struct Session { /* root, file, LoadOptions, Options; Option<Project>; lost; the stamps (§3) */ }

impl Session {
    /// `load` says what the load may run (build scripts, proc macros: RS3); `options` the limits and the marker style of every request.
    pub fn open(root: &Path, file: &Path, load: &LoadOptions, options: Options) -> Result<Session, OpenError>;
    /// One request: the graph under `entry`, walked no further than `budget`. `None`: no such def, or no project (`lost`).
    pub fn graph(&self, file: &str, entry: &Entry<'_>, budget: &Budget, cancel: &Cancel) -> Option<Graph>;
    pub fn project(&self) -> Option<&Project>;   // `&Project` in step 2; `Option` once a reload can fail (step 4)
    pub fn refresh(&mut self) -> Refreshed;      // before each request: follows the disk (§3)
    pub fn reload(&mut self) -> Refreshed;       // loads the workspace again, whatever the disk says (§6)
}
```

The same methods as TypeScript's session as built ([`../../03-typescript/07-ts5-driver-needs.md`](../../03-typescript/07-ts5-driver-needs.md)
§2), so a driver's main loop is written once. Two differences, both Rust's: `open` takes the `LoadOptions` (TypeScript's load runs nothing,
so it has none), and there is no `epoch` (TypeScript's is for tokens, which are not built). The answer is a `Graph` (rule 7); the driver
draws it with `render` and sends its wire. `call_graph` stays, unchanged, for the CLI and the tests; with `Budget::default()` a session's
graph is `call_graph`'s (pinned by a test). `Refreshed { reopened, lost }` is TypeScript's
([`../../03-typescript/11-step6-text-and-disk.md`](../../03-typescript/11-step6-text-and-disk.md) §3). The first version had
`set_text(file, Option<&str>)` here; text the editor has not saved is **not read, for now (owner, 2026-11-17, D42)**. `tree`, `expand`
and `Token` were here too, before TS5 was built: they are gone (rule 7; D31: tokens are TS5 step 2b, not built). A cut node is
continued by a new `graph` request for its def.

**There is no solver memo between requests** (answered 2026-11-19, the `[check]` of the first version: TypeScript kept none, and Rust
follows it). Each request builds its `Sem` and its `Solver` fresh, inside one `attach_db`, and drops them at the end. What stays warm is
the engine's database inside the `Project`: salsa keeps every parsed file, every name resolution and every inferred body from one request
to the next, which is where the time goes. A summary is one body's sites, cheap to walk again over warm types. **[measure]** in step 3:
the first and a second request for the same entry on a real workspace.

> **🦀 Rust note: a struct cannot borrow from itself.** `Rust<'a, 'p>` and its `Func<'p>` handles borrow the `Project`. A
> `Session { project: Project, solver: Solver<Rust<'?>> }` would need `'?` to mean "as long as the field next to me", and Rust has no such
> lifetime: moving the `Session` would move the `Project`, and the borrow would dangle. So the long-lived thing is the `Project`, and
> everything that borrows it lives for one request, as [`../02-architecture.md`](../02-architecture.md) §4 says. The first version of this
> note kept an owned memo (keyed by `StableDef` and key digest) to get around it; that costs a conversion each way for a gain nobody has
> measured, and TypeScript did without it.

> **🦀 Rust note: `&self` for a request, `&mut self` for a change.** `graph` takes `&self`: what a request fills lives in its own `Sem`, and
> the database is only read. `refresh` and `reload` write the database (`RootDatabase::apply_change` takes `&mut self`), so they take
> `&mut self`, and the compiler then forbids a change while a request still holds a borrow. A driver moves the session to a worker thread,
> so `Session` must be `Send`; pinned by a test in step 3.

## 3. Edits: a file saved on disk

**Only saved text is read, for now (owner, 2026-11-17, D42).** The session follows the disk by itself: the driver calls `refresh` before
each request, as Python's and TypeScript's drivers do (D41), because the contract sends no change notification.

What differs from TypeScript is the cost. There a changed file is a whole program built again. Here the file's new text goes into the
database and salsa recomputes only what depended on it.

| What changed on disk | How `refresh` sees it | What it does |
|---|---|---|
| a `.rs` file of the project saved | the stamp (length, modified time) of every `.rs` file the `Vfs` holds outside a library root (`SourceRootConfig::path_is_library`, checked at the pin below) | reads the bytes, `Vfs::set_file_contents`, then `RootDatabase::apply_change` with a `ChangeWithProcMacros` holding `change_file(id, Some(text))`. Incremental by construction |
| a `.rs` file created or deleted, **not** where cargo finds a target by itself | the stamp of each folder that holds a stamped file, up to the package root and never above the project root (TypeScript's fix 1b) (a folder's modified time moves when an entry is added or removed); a changed folder is listed again, with the load's excludes (`target/`) | the same `set_file_contents` (`None` for a deleted file), and the source roots partitioned again (`SourceRootConfig::partition(&vfs)` into `change.set_roots`) in the same `apply_change`. No reload: rust-analyzer's own main loop does exactly this |
| a `.rs` file created or deleted where cargo finds a target by itself (`build.rs`, `src/main.rs`, `src/lib.rs`, a file in `src/bin`, `examples`, `tests`, `benches`, or a `main.rs` one folder below them) | the folder stamps above | a reload (§6): the crate graph changed. The engine's own rule, copied: `should_refresh_for_change` in rust-analyzer's `crates/rust-analyzer/src/reload.rs`, which is not in a published `ra_ap_*` crate |
| a manifest, a lock, the toolchain and cargo config files, a `build.rs` when build scripts ran (`environment_inputs`) | their stamps | a reload (§6): seconds |
| a dependency's source (the registry, the sysroot) | not stamped | nothing. Cargo does not change the source of a published version, and a new version changes `Cargo.lock`, which is stamped. A registry file edited by hand needs `reload`: TypeScript's `node_modules` rule (D41 a) |
| text the editor has not saved | not read (D42) | when the contract adds the text: the same `apply_change` with the buffer's text, and `None` for "back to the disk". Cheap here, unlike TypeScript |
| the outline cache | per request (RS4 §5) | an edit cannot leave a stale one |

`refresh` takes `&mut self`, so the borrow checker guarantees no request is running on the old text (RS3 §5's note).

**Checked at the pin (`ra_ap_* 0.0.358`, 2026-11-19, by reading the source).** Everything the table needs is public:

| Item | Crate | What it does |
|---|---|---|
| `Vfs::set_file_contents(path, Option<Vec<u8>>) -> bool` | `ra_ap_vfs` | gives a new path a new `FileId`; `None` is a deletion; returns `false` when the bytes hash the same, so a file touched and not changed costs nothing |
| `Vfs::take_changes()` | `ra_ap_vfs` | the changes since the last call, each `Create`, `Modify` or `Delete`; drain it after each refresh, as the load does |
| `ChangeWithProcMacros::change_file(FileId, Option<String>)`, `set_roots(Vec<SourceRoot>)` | `ra_ap_hir_expand` (re-exported by `ra_ap_ide_db`) | one transaction; `FileChange` (`ra_ap_base_db`) is the part inside it |
| `RootDatabase::apply_change(&mut self, ChangeWithProcMacros)` | `ra_ap_ide_db` | applies it; salsa then recomputes only what read the changed inputs |
| `ProjectFolders::new(&[ProjectWorkspace], &[], None).source_root_config` | `ra_ap_load_cargo` | `load_workspace` builds this and **does not return it**, so `sem` builds its own copy from a borrow of the workspace (`slice::from_ref`, as `load_workspace_into_db` does) before the load takes it, and keeps it in `Loaded`. It keeps `ProjectFolders.load` (the `Vec<vfs::loader::Entry>`) too: the excludes a folder is listed again with (`target/`) are in the entries (`Entry::contains_dir`, `contains_file`), not in the config. (Corrected 2026-11-19 at the step 3 review: the first text said "a clone") |
| `SourceRootConfig::partition(&Vfs) -> Vec<SourceRoot>`, `path_is_library(&VfsPath)` | `ra_ap_load_cargo` | the source roots again after a file was created or deleted; whether a path is a dependency's or the sysroot's (a path in no file set is *not* a library: such a file is stamped) |

A file whose bytes are not UTF-8 gets `change_file(id, None)`, as it had no text after the load either (the load skips it).

**What the review of TypeScript's step 6 found holds here too** (2026-11-17,
[`../../03-typescript/11-step6-text-and-disk.md`](../../03-typescript/11-step6-text-and-disk.md) §4, §5). Each is a test in step 3 or 4:

1. **The stamps live in the `Session`, beside the project**, not inside it, so a session whose reload failed still knows what to watch.
2. **A failed reload leaves the session with no project (`lost`). Every `refresh` says so while it lasts, and the session comes back by
   itself** when a stamp changes. Here that is rare: by D37 a missing or broken manifest is a warning on a load that still succeeds, so
   only `NotUnderRoot` (the opened file is gone) and `Load` lose a session. Checked (step 4, a test): a `Cargo.toml` saved with a syntax
   error reloads with a warning (`cargo_failed`, by D37's reader), and fixing it reloads clean.
3. **Stamp first, read second.** `refresh` takes a file's stamp and then reads its text. A file saved between the two has a newer stamp
   than the one kept, so the next `refresh` reads it again. The other order would keep the new stamp with the old text, and nothing would
   notice. For a reload, a path already known keeps the stamp taken before it.

   **A path stamped for the first time after a load is read once more** (found at the step 3 review, 2026-11-19). The load reads the
   files before any stamp exists, so a file saved while `open` (or a reload) ran would get a stamp that describes the new text while the
   database holds the old, and nothing would notice until the next save: point 3's failure, at the one moment the order cannot be kept.
   So after a load, each newly stamped file is read again and handed to the `Vfs`, which compares the bytes: identical (the usual case)
   costs a read and a hash and applies nothing; different is one `apply_change`. Step 4, for `open` and for a reload alike.
   TypeScript's `Stamps::of_all` has the same gap (told to its reviewer; not ours to change).
4. **The old database is dropped before a reload builds the new one**, so the peak is one database and not two. Measured
   (step 4, `ENGINE.md`): at most one database alive through two reloads; 1.0 GB peak on `core/` + `languages/typescript/`.

**What makes `refresh` reload, as built in step 4 (63e7509) and fixed in 4b:**

| Watched | How | A change means |
|---|---|---|
| an environment input (`Project::environment_inputs()`) | a glob (`**/Cargo.toml`, ...) is resolved at the load to the name in the project root and in each package root under it, **missing ones kept** (so a `rust-toolchain.toml` created is seen), plus any file of that name the `Vfs` holds in the project; an exact absolute path (a manifest outside the root) as it is | a reload |
| the file the session was opened from | its own stamp, kept when missing | appearing or vanishing is a reload (a session is not loaded around a file that is not there: §3 point 2); a save is an ordinary `.rs` save |
| a `.rs` file created or deleted | deleted: its stamp goes from present to missing; created: found by listing a changed folder | at a target path (`should_refresh_for_change`, `sem/targets.rs`, copied with its plain `ends_with`; `additional_paths` empty, the input stamps cover those names) a reload, else the fast path |
| **a package created under the root (4b, found at the step 4 review; 4c)** | the folders from the project root down to each package root are stamped too; a new folder found in one is walked (with `target/` and `.git` left out) for an input file | a reload. Without it, a crate added under `members = ["crates/*"]` (no root manifest edit) was missed by `refresh` and found by `reload` (probed 2026-11-19). **4c (found at the 4b review):** only a sub-folder that is **new** is walked. 4b walked every sub-folder that was not stamped, so a folder that was there at the load and is no package's (a `vendor/` crate, a `fuzz/` crate the workspace excludes) was walked whenever the root changed, and its `Cargo.toml` made every file created at the root (`notes.md`) a reload (probed). A scout keeps the names of its sub-folders as they were at its last look; a name it did not have is new (built 1f0312e). **4d (found at the 4c review):** a scout's names must be taken at the moment of the stamp they are kept with. Before a reload the session restamps, but each scout kept its old names (`keeping(before)`), so a folder created there that the reload does not make a package (`fuzz/` with its own `[workspace]`, `cargo new scratch` outside `members`) stayed "new" for ever: every later change at the root reloaded (probed: `more.md`, `last.md`) |

A reload starts a cold database: the next request costs what the first did (3 s on `core/`, 6 s on the bigger workspace), and the one
after is warm again.

**Known limit, as TypeScript's:** a stamp is the length and the modified time, so a file rewritten with the same length within the file
system's clock tick is not seen (APFS and ext4 keep nanoseconds, so in practice a person cannot do it; FAT keeps two seconds). The tests
change a file's length.

**Answered (D46, owner, 2026-11-19):** point 2 is TypeScript's answer (D41 b) carried over: a reload that fails drops the old database.
A driver may offer a manual re-sync through the public `Session::reload()`.

## 4. Cancel

| Who checks | When |
|---|---|
| core's solver | before each body |
| `walk::body` | before each site |
| the engine | **only if needed.** salsa cancels running queries when another thread starts a write; with `&mut self` there is no other thread. A long single query (type inference of one huge function) cannot be interrupted from our flag. **[measure]** the longest single `callee` call on a real project in RS4; if it is under ~100 ms, the flag between sites is enough and nothing engine-side is built |

If it is not enough: the engine's own way is a second thread that requests cancellation on the database while the worker runs inside
`Cancelled::catch` (S-RS2 finds the public form). `sem` would own that; nothing outside it changes. Not built until the measurement asks.

A cancelled request returns what it has, marked `Incomplete`; cut summaries are not memoized (the solver already does this).

**Measured in step 5 (e25d4d5, reviewed 2026-11-19; `sem::cancel::measure_cancel`, numbers in `ENGINE.md`).** Warm, no single engine
call is long: the longest `sites(f)` after the first body is 35 ms and the longest `resolve` 5.5 ms, so the flag between sites is enough.
**Cold, two stretches have no look at the flag:** `Sem::entry` (2.3 s on `core/`, 7.2 s on `core/` + `languages/typescript/`: turning the
entry's syntax into an engine def builds the crate's name resolution, and std's with it) and the first body (0.67 s, 1.3 s: inference
with nothing warm). A cancel set 100 ms into a cold first request is honoured 2.0 s and 4.9 s later. Nothing checked before `Sem::entry`:
the doc was silent there.

**Decided (reviewer, 2026-11-19; the owner may say no): the cold cost moves into the load, and no engine-side cancel is built.** After
every load (`open` and each reload) `sem` primes the project's own crates (`ide_db::prime_caches::parallel_prime_caches` scoped to the
local crates, or our own walk of their def maps: step 5b measures both and keeps the cheaper one that brings a cold-database
`Sem::entry` under 100 ms), and the request looks at the flag before `Sem::entry` too (**withdrawn at the 5b review, 2026-11-20**: see below). The first tree costs the same in total; what
changes is the contract a driver gets, the same as TypeScript's: **`open` and `reload` are the slow calls, not cancellable; a request
is fast and is.** What the first body still costs after priming is written down, and engine-side cancel (a request on a clone of the
database in `Cancelled::catch`, the clone's owner calling `trigger_cancellation`) is built only if a real project leaves more than about
a second there.

**Built in 5b (16389db, reviewed 2026-11-20).** Our own walk of the def maps (`Crate::modules` of each local crate whose root file is
under the root), not the engine's `parallel_prime_caches`: both bring a cold `Sem::entry` to milliseconds, the walk costs less (on
`core/` open 2.4 s against 3.1 s, 440 MB against 490 MB after it; open plus first request 3.2 s against 3.4–3.7 s). On `core/` +
`languages/typescript/`: open 2.3 s → **6.4 s** (36 crates primed, test targets included: each integration-test file is a crate to the
engine), cold first request 9.0 s → **1.5 s**, `Sem::entry` 7.2 s → **12 ms**; a cancel set at 100 ms is honoured 0.99 s later (was 4.9 s);
peak footprint 0.99 GB, as before. The first body (1.16 s there) is what is left, under the "about a second" bar.

**The look before `Sem::entry` is withdrawn (fix 5c).** It answered `None` for a request cancelled before it started, which a driver
cannot tell from "no such def", and TypeScript has no such look: its cancelled request is a graph whose root context is `Incomplete`.
It was asked for when `Sem::entry` cost seconds; primed, it costs milliseconds, so the walk's and the solver's own looks are enough, and
a cancelled request is always `Some(graph)`, `Incomplete`, as §4's first rule says.

## 5. The memo

- **There is none between requests** (§2, as built). So a change has no memo to drop: the next request builds its solver over the
  changed database. What salsa keeps is invalidated by salsa.
- **One thing core's token must not assume, when tokens are built (TS5 step 2b, D31):** TS5 describes a token as "`ContextId` plus a
  child offset". A `ContextId` holds a *digest* of the key, and a digest cannot be turned back into a key. With `Key = ()` that does not
  matter. With layer 2 it does: `expand(token)` must find the `(function, key)` again. So a session resolves a token by **looking the id
  up** in the contexts it has listed, never by decoding it; an id it does not know is `Stale`. TS5's "Later: tokens as cursors" already
  keeps a table of what was listed, which is that lookup.

## 6. Reload

`reload()` loads the workspace again (`cargo metadata` and all), with the `LoadOptions` the session was opened with. The old `Project` is
dropped first (§3 point 4). `refresh` does the same by itself when the stamp of something in `environment_inputs()` changed, or when a
`.rs` file was created or deleted where cargo finds a target (§3's table); the driver calls `reload` only for what `refresh` cannot see
(§3: a dependency's source edited by hand). It is the slow path, seconds; a saved file applied by `refresh` is the fast one.

## 7. Steps

| # | Do | Proof |
|---|---|---|
| 1 | ~~core: `Budget`, the frontier, a frontier node `Cut`~~ | **done by TS5 steps 1 and 2a**; tokens and paging wait for TS5 2b (D31) |
| 2 | `Session::open / graph` (**done, ac0c34b, reviewed 2026-11-19**) | with `Budget::default()` a session's graph is `call_graph`'s, for every entry of the policy and context workspaces; a budget changes the graph (depth 0, `max_contexts`); at `max_depth` 1, 2 and 3, replacing each frontier node by the tree of a new request for its def gives the unbounded tree (48, 4 and 1 frontier nodes over all entries), with the ancestor rule (a def on the path above is a `Recursive` leaf), which a test shows is needed. The entries cover every scenario's shape (straight line, two callers, a method, a construction, an external call, direct and mutual recursion) |
| 3 | **done, 22e8412, reviewed 2026-11-19** (the numbers: `ENGINE.md`, "RS5 step 3"). `refresh`, the fast path: the stamps (in the `Session`, §3 point 1), a saved file applied to the database, a created or deleted file applied with the roots partitioned again (§3's table, the rows with no reload). No `set_text` (D42); no reload yet | TypeScript step 6's cases: the callee's file rewritten (the graph follows; without `refresh` it does not, pinned); a file a `mod` names created (and the `mod` line saved in the same refresh); a callee's file deleted; nothing changed (the graph byte-identical, and the `Vfs` sees no change); a file saved between the stamp and the read is read again by the next `refresh`; no folder above the root is stamped; `Session: Send`. **[measure]** on `core/` and on one bigger workspace: `refresh` with nothing changed (the files and folders stamped, the time), one saved file (the time, and the next request's), the first and a second request for the same entry |
| 4 | **done, 63e7509, reviewed 2026-11-19; 4b (new packages under the root) asked**. `reload`; `refresh` reloading when an environment input changed or a target file was created or deleted (§3's table); `lost` | add a dependency to a fixture's `Cargo.toml` between two requests: `refresh` reloads; `src/bin/new.rs` created: a reload, and its defs found; a `Cargo.toml` saved with a syntax error, then fixed (a warning, then clean: D37); the opened file deleted, then written back (`lost`, then back by itself); the old database dropped before the new one is built; a file saved while `open` or a reload ran is read by the re-read of §3 point 3 (a test hook between the load and the stamps). **[measure]** a reload's time and the database's footprint on a real workspace |
| 5 | **done, e25d4d5, reviewed 2026-11-19.** The cancel measurement of §4, written in `ENGINE.md`; engine-side cancel only if it asks | the number: warm under 40 ms, cold `Sem::entry` 2.3–7.2 s unchecked (§4) |
| 5b | **done, 16389db, reviewed 2026-11-20** (5c removes its look before `Sem::entry`). Prime the local crates after every load; a look at the flag before `Sem::entry` (§4's decision) | a cold-database first request's `Sem::entry` under 100 ms, the open's added time and footprint on `core/` (and the bigger workspace by the reviewer); a cancel set before a request is honoured before `Sem::entry`; every test green |
| 4d | **done, 83ed738, reviewed 2026-11-20.** A scout's names taken with the stamp they are kept with (§3's table) | the probe's shape: `fuzz/` (own `[workspace]`, excluded) created after the open is one reload, and later root changes none; `cargo new scratch` outside `members` the same |
| 5c | **done, 7e22a84, reviewed 2026-11-20.** No look before `Sem::entry`: a request cancelled before it starts is `Some(graph)`, its root context `Incomplete`, as TypeScript's | a cancel set before the request: `Some`, root `Incomplete`; `Cancel::after_checks(1)` lands at the first body |
| 6 | **done, 540ca48, reviewed 2026-11-20.** `rust-cli calls --depth --max-contexts --max-nodes`, as TypeScript's 520770f (`expand` waits for D31's 2b) | 20 tests; no flag byte-identical (tree, `--wire`, `--graph`) |
| 6b | **done, 2447928, reviewed 2026-11-20.** TypeScript's later wording (41dd2d3, 06fbab8): `Breakdown` gains `frontier` and says `cuts in the graph:` (the budget's frontier joined with the solver's limits); the CLI prints `cuts in the tree:` right after the breakdown's last line, as TypeScript's does (the renderer's cut belongs to the drawing, so `Breakdown` has no field for it) | the same one-line snapshot changes TypeScript made; no flag still identical apart from that line |
| | **tag `rust-v0.3.0`** | tier-3 conformance in the driver (`vnoc`) |

**Layer 1 is complete in this repo (2026-11-20, fe29721).** RS1–RS5 built and reviewed; `languages/rust/docs/policy.md` is a test
(`tests/policy.rs`, 43 row snapshots); swept on terminusdb and ruff (ENGINE.md, "Sweeps of real projects"). Left, none of it code here:
the tags `rust-v0.2.0` and `rust-v0.3.0` (the owner's; v0.3.0 after tier-3 conformance in the vnoc driver). Later, decided and written
down: the user's choice of the `cfg` set (D48, RS3 §3), build-time code (D39), `expand` tokens (D31).

Layer 1 ends here. Next: [`../05-context-layer/01-design.md`](../05-context-layer/01-design.md)
