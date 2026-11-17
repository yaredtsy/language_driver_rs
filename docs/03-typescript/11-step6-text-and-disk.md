# TS5 step 6: unsaved text, a reload, and the disk

Status: **design, not built** (reviewer, 2026-11-17). It answers row 6 of [`07-ts5-driver-needs.md`](07-ts5-driver-needs.md) §7, §6 of that
file (unsaved text), and D41 (an open `Session` follows the disk by itself, before each request; [`10-review-fixes.md`](10-review-fixes.md) §5).
The `[check]` and `[measure]` marked **[ran]** were run on the pinned tsrs (`13a31a4`) and the V-NOC frontend, with a throwaway probe in a
scratch worktree (deleted). The three choices in §9 are the owner's.

```
step 6
├── 1. what it answers
├── 2. what the engine gives            trackingvfs, the watcher, cachedvfs; the numbers
├── 3. the API                          Session::set_text, refresh, reload
├── 4. how it is built                  all in the sem module
├── 5. rules
├── 6. known limits
├── 7. tests
├── 8. steps
└── 9. for the owner
```

## 1. What it answers

A driver keeps one `Session` open for many requests (07 §2). Three things can change under it:

| What changes | Who knows | Step 6's answer |
|---|---|---|
| text the editor has not saved | the driver (the editor sends it) | `Session::set_text(file, Some(text))`; `None` goes back to the disk |
| a source file saved, created or deleted on disk | nobody tells the analyzer (the contract has no change notification) | `Session::refresh()`, called by the driver before each request, finds it itself (D41) |
| a config, a `package.json`, a lock file (`environment_inputs`) | the same: nobody tells | `refresh` finds it too; `Session::reload()` forces a rebuild for anything it cannot see (§6) |

Every change costs the same thing: the program is built again (step 7 makes that incremental, only if step 6's number is too slow).
Nothing outside the `sem` module and `session.rs` changes: `walk.rs`, `calls.rs` and `core/` do not know how the program was made.

## 2. What the engine gives

**[ran] The `[check]` of `10-review-fixes.md` §5, completed.** The program keeps no failed lookups (`Program::missing_files` is only root
and referenced files it could not open, behind a "testing only" accessor, `tsrs_compiler/src/program.rs:3134`). **The file system does:**
`tsrs_vfs::trackingvfs` is a port of TypeScript-Go's watch-mode wrapper and "records every path read during a compilation, so watch mode
knows which files and directories the program depended on, including paths that did not exist (failed module resolution lookups)"
(`trackingvfs.rs:1`). It records the path of every `file_exists`, `read_file`, `directory_exists`, `get_accessible_entries`, `stat` and
`realpath`.

**The engine's own watch mode** (`tsrs_execute/src/watcher.rs:778`) builds each program on a **fresh** `cachedvfs` wrapped in a
`trackingvfs`, adds the config files and the config's wildcard directories as seen, re-runs the config's file list, and afterwards watches
every seen path. Step 6 does the same, with one difference: there is no OS watcher, so the seen paths are stamped and compared before each
request.

**[ran] The numbers** (V-NOC frontend, `TaskDetailPanel`'s program, dev profile; a `stat` is a system call, so the profile hardly matters):

| | |
|---|---|
| paths seen after open | **9143**: 2616 files, 852 directories, 5675 that do not exist (failed lookups) |
| after a body is walked | the same 9143: the checker reads nothing new |
| under `node_modules` | 8057 |
| `stat` of all 9143 | 14 to 18 ms |
| `stat` of the 1086 outside `node_modules` | **1.5 ms** |
| `Sem::open` (the cost of a rebuild) | 241 ms (dev; about 150 ms release, 07 §0) |

The config host reads through the same file system as the compiler host (`Sem::open`: `ConfigHost { fs: Arc::clone(&fs) }`), so the
directories the config's `include` lists are tracked as well. Bundled libraries are not: `bundled::wrap_fs` sits outside the tracker.

**No overlay in tsrs_vfs fits.** `wrapvfs::wrap` takes a `&'static dyn FS`, and ours is an `Arc`. `tsrs_project`'s `snapshotfs` is the
language server's and would bring its whole crate. The overlay is a small `FS` of our own, in `sem` (§4).

## 3. The API

```rust
// typescript-analysis, session.rs
impl Session {
    /// Unsaved text for `file` (relative to the root): `Some(text)` is what the editor holds, `None` goes back to the disk. Recorded only:
    /// the next `refresh` builds the program with it. A file outside the root is not recorded (`false`).
    pub fn set_text(&mut self, file: &str, text: Option<String>) -> bool;

    /// Brings the session up to date; call it before each request. Builds the program again when unsaved text was set since the last
    /// build, or when a path the program depended on changed on disk (a source file saved, created or deleted, a config, a lock file).
    /// Otherwise it only compares (milliseconds). Never fails.
    pub fn refresh(&mut self) -> Refreshed;

    /// Builds the program again whatever the stamps say: for a change `refresh` does not see (§6).
    pub fn reload(&mut self) -> Refreshed;

    /// `None` once a rebuild failed (§5): the session then has no program.
    pub fn project(&self) -> Option<&Project>;   // was `&Project`: an API change, before 1.0 and before any driver uses it
}

#[non_exhaustive]
pub struct Refreshed {
    /// The program was built again: the next request is as slow as the first.
    pub reopened: bool,
    /// The rebuild failed (the file the session was opened from is gone, or no config holds it any more): every request now answers `None`.
    pub lost: Option<OpenError>,
}
```

`graph` keeps its signature and answers `None` when the session has no program. `Refreshed.reopened` has Python's name and meaning
(`python-analysis` `Refreshed`); `lost` is new, because a TypeScript rebuild can fail where Python's reopen keeps its old database (§9 b).

> **🦀 Rust note: `&mut self` is the lock.** `set_text`, `refresh` and `reload` take `&mut Session`; `graph` takes `&Session`. The compiler
> lets a value have one `&mut` or any number of `&` at a time, never both, so a rebuild can never run while a request still reads the old
> program, and no mutex is needed. The old `Sem` is dropped (and its program freed) when the new one replaces it.

## 4. How it is built

All of it is in the `sem` module (rule 4): `trackingvfs`, the overlay and the stamps are engine types and never leave it.

**The file system of one build**, inside out:

```
osvfs::fs()                      the disk
  cachedvfs::from(…)             fresh for each build: a cache kept across builds would answer the old text
    Overlay { inner, texts }     ours: read_file and file_exists answer the unsaved texts first
      trackingvfs::FS::new(…)    records every path asked, the unsaved ones too
        bundled::wrap_fs(…)      the bundled libraries, not tracked
```

**The overlay** implements `tsrs_vfs::FS` and forwards every method to `inner` except two: `read_file(path)` returns the unsaved text when
there is one, and `file_exists(path)` is true for it. `texts` is an `Arc<BTreeMap<String, String>>` keyed by the engine's spelling of the
absolute path (`to_engine_path`), owned by the `Session` and handed to each build. **[check]** on a case-insensitive file system (macOS,
Windows) whether the engine asks with the spelling of the import (`./foo` for `Foo.ts`): if it does, the key is compared as the engine
compares paths (its `to_path` with `use_case_sensitive_file_names`), not as a plain string. A file with unsaved text that is not on disk is
readable and importable, but it is in no directory listing (§6).

**`Sem::open_with(root, file, texts)`** is `Sem::open` with the overlay in the stack; `Sem::open` becomes `open_with` with no texts. After
`bind_source_files` it takes `seen_files()`, sorts it (rule 9), drops the paths under a `node_modules` folder (a path with a
`/node_modules/` segment; the `node_modules` folder itself stays), and adds the stamp paths of §5. For each path it keeps a **stamp**:

```rust
enum Stamp { Missing, File { len: u64, modified: Option<SystemTime> }, Dir { modified: Option<SystemTime> } }
```

from `std::fs::metadata` (it follows symlinks, as the engine's `realpath` does). `Sem::disk_changed(&self) -> bool` stamps the same paths
again and compares. A folder's modified time changes when an entry is created, deleted or renamed in it (POSIX, APFS and NTFS), so a source
file created under a folder the `include` listed is seen, and so is a file created at a path a failed lookup asked for.

**`Session`** keeps `root`, `file` (what it was opened from), `options`, the texts, a `dirty` flag that `set_text` sets, and
`project: Option<Project>`. `refresh` is `if dirty || project.disk_changed() { rebuild }`; `reload` always rebuilds. A rebuild is
`Project::open_with(root, file, texts)`; on success it replaces the project, on failure the session keeps no project and `lost` says why.

## 5. Rules

| Rule | Why |
|---|---|
| A rebuild opens from the same `root` and `file` as `Session::open`, never from a file found again | the config is found from that file; a different file could pick a different config (a solution's child) |
| The stamp skips paths under `node_modules` and stamps, instead, each `node_modules` folder the program looked into **and the lock files beside it** (`package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock`, `pnpm-lock.yaml`, `bun.lock`, `bun.lockb`, the names `ENVIRONMENT_GLOBS` has) | 8057 of V-NOC's 9143 paths are there and cost 12 of the 14 ms. An install rewrites a lock file and adds or removes folders in `node_modules`; a package is not edited by hand (§6, §9 a) |
| Every config file of `config_files()` is stamped, even one outside the root | an `extends` target outside the root changes the options; the tracker sees it read, this makes it explicit |
| A failed rebuild leaves the session with no program (`lost`) | an old program answers from text that is no longer there; `None` is what rule 8 allows, a stale tree is not (§9 b) |
| `refresh` with nothing changed does not rebuild and does not touch the caches | the answer must be byte-identical to the one before it (rule 9), and the cost is only the stamps |
| `set_text` with the text the disk already has still marks the session dirty | comparing would read the file; one extra rebuild is cheaper than a wrong "nothing changed" |
| `set_text` on a path outside the root is refused (`false`) | project code is under the root (D33); a file outside is `External` and its text does not change a tree's shape |
| Neither `refresh` nor `reload` takes a `Cancel` | a program build is one engine call that cannot be stopped (07 §4); the driver calls `refresh` before the request, outside its cancel window |

## 6. Known limits

Each is written in `ENGINE.md` with the test that pins it.

- **A package edited by hand** (a file under `node_modules` changed, no lock file touched, no folder added) is not seen by `refresh`.
  `reload` sees it.
- **A modified time that does not move.** Two writes of the same length within the clock's resolution (1 s on HFS+ and FAT, finer on APFS
  and NTFS) read as no change. Python has the same limit.
- **A spurious rebuild** when an editor writes a temporary file beside a source file: the folder's modified time moves. It costs one rebuild
  and gives the same answers.
- **An unsaved file that is not on disk** is in no directory listing, so the config's `include` does not pick it up; it is in the program
  only when something imports it.
- **The file the session was opened from is deleted:** the rebuild fails and the session is lost (§5). The driver opens a new `Session` from
  another file if it wants one.

## 7. Tests

`tests/session_refresh.rs`, a project written to a temporary folder by the test (`CARGO_TARGET_TMPDIR`), so the test can change the disk.
Each case first asks, then changes, then asks again; the expected tree is written before the run.

| # | Case | Expected |
|---|---|---|
| 1 | the callee's file is rewritten to call another function | without `refresh` the old tree (pinned: the rule has a reason); after it the new one, `reopened` true |
| 2 | an import names a file that is created after the first request | `Unresolved` first; after `refresh` a `Call` into the new def |
| 3 | the callee's file is deleted | after `refresh` the call is `Unresolved`; nothing panics |
| 4 | a file outside the root that the program read is rewritten (D33: an `External` named by its path) | the external's name follows |
| 5 | a new source file under an `include` folder, imported by nobody | it is in `Project::files()` after `refresh` |
| 6 | nothing changed | `reopened` false, and the graph is byte-identical to the one before |
| 7 | `set_text(main, Some(…))` renames the callee in the buffer | after `refresh` the tree follows; `set_text(main, None)` and `refresh` give the disk's tree back |
| 8 | `set_text` for a file not on disk that `main` imports | the import resolves to it after `refresh` |
| 9 | the `tsconfig.json` changes a `paths` mapping | after `refresh` the import resolves through the new mapping |
| 10 | a file under `node_modules` is edited by hand (a known limit) | `refresh`: the old answer, `reopened` false (pinned); `reload`: the new one |
| 11 | the file the session was opened from is deleted | `refresh`: `lost` is `Some`, `graph` answers `None`, nothing panics |
| 12 | after any rebuild | the graph is byte-identical to a fresh `Session::open` on the same disk and texts |
| 13 | `set_text` outside the root | `false`, and the next `refresh` does not rebuild |

Also: the memory test gains a loop of `set_text` + `refresh` + `graph` (50 rounds) whose resident memory stays bounded, as TS3's open/drop
loop does (the old program is freed). A unit test in `sem` checks that the seen paths after a call graph are the seen paths after open
(the probe's answer, kept as a test).

**[measure]** on V-NOC, written in `ENGINE.md`: `refresh` with nothing changed (the stamps; expected near 1.5 ms plus the configs and lock
files), and the first request after a `set_text` (the rebuild and a cold graph; 07 §6 asks for it). The owner decides from the second whether
step 7 runs.

## 8. Steps

One commit each, a stop for review after each.

1. **The stamps and `refresh`** (cases 1–6, 9–12, the seen-paths unit test). `Sem::open_with` with an empty overlay, `trackingvfs`, a fresh
   `cachedvfs` per build, `Stamp`, `disk_changed`; `Session::refresh` and `reload`, `Refreshed`, `project()` as an `Option`.
2. **`set_text` and the overlay** (cases 7, 8, 13, the case-spelling `[check]`), the memory loop.
3. **The measurement** and the CLI, if the owner wants one (`typescript-cli calls` is one request and needs none).

## 9. For the owner

a. **`node_modules` by its lock files (recommended)** or every seen path. Every path is the engine's rule and catches a package edited by
   hand, at 14 ms a request on V-NOC (about the request's own time) and more on a bigger tree. The lock files cost nothing measurable, and the
   miss is covered by `reload`.

b. **A failed rebuild loses the program (recommended)** or keeps the old one. Python keeps its old database, but its requests then find no
   file and answer `None`. A kept TypeScript program would answer from text that is gone, so the honest answer is the same `None`, said
   once in `Refreshed.lost`.

c. **`refresh` called by the driver (recommended)** or inside `graph`. Inside, `graph` would take `&mut self` and pay the stamps on every
   call, and the driver would lose the choice of when a rebuild happens (once before a batch of requests, not in the middle of one). As in
   Python, the driver calls it, and the vnoc side's rule is one line: refresh before each request.
