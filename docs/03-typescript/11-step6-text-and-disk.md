# TS5 step 6: a reload, and the disk

Status: **built** (`d3b394e` step 1, `716d33a` step 1b, `2b0ba62` step 2; reviewed 2026-11-19, accepted; the numbers are in
`languages/typescript/ENGINE.md`, "TS5 step 6 as measured"). Written by the reviewer, 2026-11-17, and revised the same day after its review. It answers row 6 of
[`07-ts5-driver-needs.md`](07-ts5-driver-needs.md) §7 and D41 (an open `Session` follows the disk by itself, before each request;
[`10-review-fixes.md`](10-review-fixes.md) §5). **Text the editor has not saved is not read, for now (D42):** `set_text` and the overlay are
not built, and what was designed for them is kept in §10. The `[check]` and `[measure]` marked **[ran]** were run on the pinned tsrs
(`13a31a4`) and the V-NOC frontend, with a throwaway probe in a scratch worktree (deleted). The owner's answers are in §9.

```
step 6
├── 1. what it answers
├── 2. what the engine gives            trackingvfs, the watcher, cachedvfs; the numbers
├── 3. the API                          Session::refresh, reload
├── 4. how it is built                  the tracker in sem, the stamps beside the program
├── 5. rules
├── 6. known limits
├── 7. tests
├── 8. steps
├── 9. decided
└── 10. later: unsaved text             set_text and the overlay, not built (D42)
```

## 1. What it answers

A driver keeps one `Session` open for many requests (07 §2). Three things can change under it:

| What changes | Who knows | Step 6's answer |
|---|---|---|
| a source file saved, created or deleted on disk | nobody tells the analyzer (the contract has no change notification) | `Session::refresh()`, called by the driver before each request, finds it itself (D41) |
| a config, a `package.json`, a lock file (`environment_inputs`) | the same: nobody tells | `refresh` finds it too; `Session::reload()` forces a rebuild for anything it cannot see (§6) |
| text the editor has not saved | the editor | **not read, for now (D42).** The tree follows a file when it is saved. Reading a buffer would cost a program rebuild for each changed text, and the contract's request has no text in it (§10) |

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
Module resolution asks for each `node_modules` folder itself before it looks inside (`tsrs_module/src/resolver.rs:1744`,
`directory_exists`), so the folders are among the seen paths.

## 3. The API

```rust
// typescript-analysis, session.rs
impl Session {
    /// Brings the session up to date with the disk; call it before each request. Builds the program again when a path the last build
    /// depended on changed (a source file saved, created or deleted, a config, a lock file). Otherwise it only compares (milliseconds).
    /// Never fails: a rebuild that cannot be done leaves the session with no program, and `lost` says why on every call until one works.
    pub fn refresh(&mut self) -> Refreshed;

    /// Builds the program again whatever the stamps say: for a change `refresh` does not see (§6).
    pub fn reload(&mut self) -> Refreshed;

    /// `None` while the session has no program (§5).
    pub fn project(&self) -> Option<&Project>;   // was `&Project`: an API change, before 1.0 and before any driver uses it
}

#[non_exhaustive]
pub struct Refreshed {
    /// The program was built again by this call: the next request is as slow as the first.
    pub reopened: bool,
    /// The session has no program, and why (the file it was opened from is gone, its config does not parse, no config holds the file
    /// any more). Every request answers `None`. Reported by **every** `refresh` and `reload` while it lasts, not only the first.
    pub lost: Option<OpenError>,
}
```

`graph` keeps its signature and answers `None` when the session has no program. `Refreshed.reopened` has Python's name and meaning
(`python-analysis` `Refreshed`); `lost` is new, because a TypeScript rebuild can fail where Python's reopen keeps its old database (§9 b).

**A lost session comes back by itself.** The stamps are kept beside the program, not inside it (§4), so a session with no program still
knows which paths to watch. When one of them changes, `refresh` tries the rebuild again. A `tsconfig.json` saved with a syntax error
(`OpenError::ConfigError`) or a file that is gone for a moment during a `git checkout` costs some `None` answers, and the trees are back
with the next `refresh` after the disk is right again. The driver does nothing special: it keeps calling `refresh` before each request.

> **🦀 Rust note: `&mut self` is the lock.** `refresh` and `reload` take `&mut Session`; `graph` takes `&Session`. The compiler lets a
> value have one `&mut` or any number of `&` at a time, never both, so a rebuild can never run while a request still reads the old
> program, and no mutex is needed.

## 4. How it is built

The tracker is an engine type and stays in the `sem` module (rule 4). What leaves `sem` is a list of paths and their stamps: plain text and
`std` types.

**The file system of one build**, inside out:

```
osvfs::fs()                      the disk
  cachedvfs::from(…)             fresh for each build: a cache kept across builds would answer the old text
    trackingvfs::FS::new(…)      records every path asked, missing ones too
      bundled::wrap_fs(…)        the bundled libraries, not tracked
```

**`Sem::open_tracked(root, file) -> (Result<Sem, OpenError>, Vec<String>)`** is `Sem::open` with the tracker in the stack, and it returns
the paths to stamp **even when the build fails**. `Sem::open` becomes `open_tracked` with the list dropped. The list is:

1. `seen_files()`, sorted (rule 9), without two kinds of path:
   - the paths under a `node_modules` folder (a path with a `/node_modules/` segment). The folder itself stays, and when a path is
     dropped its outermost `node_modules` folder is put in the list, so the list does not depend on the engine having asked for the
     folder by itself first;
   - **the directories strictly above the root** (§5). A file the engine asked for in one of them stays (`package.json`), and so does
     a `node_modules` folder there.
2. the stamp paths of §5: beside each `node_modules` folder of the list **that exists**, the file names `ENVIRONMENT_GLOBS` spells in
   full (the lock files, and `package.json` and `jsconfig.json` with them: one list of names, not two); and every config file the
   owner search read (the absolute paths behind `config_files()`), a broken one too;
3. always, the root and the file the session was opened from. A build that fails before it reads anything (`NoSuchFile`, `OutsideRoot`)
   still gives these two, so the session sees the file come back. They are canonical when they can be made so, else as they were given.

**A stamp** is what `std::fs::metadata` says of a path (it follows symlinks, as the engine's `realpath` does):

```rust
enum Stamp { Missing, File { len: u64, modified: Option<SystemTime> }, Dir { modified: Option<SystemTime> } }

/// The paths one build depended on, each with its stamp. Sorted by path. Lives in the `Session`, beside the program.
pub(crate) struct Stamps(Vec<(String, Stamp)>);
```

A folder's modified time changes when an entry is created, deleted or renamed in it (POSIX, APFS and NTFS), so a source file created
under a folder the `include` listed is seen, and so is a file created at a path a failed lookup asked for (its own stamp goes from
`Missing` to `File`).

**`Session`** keeps `root` and `file` (as `open` was given them), `options`, `project: Option<Project>`, `lost: Option<OpenError>` and
`stamps: Stamps`. `Session::open` keeps its signature, `Result<Session, OpenError>`: a first open that fails gives no session, as today, and
the driver opens again when it is asked again. Only a session that was open once can be lost and come back.

```
refresh:
    before = the session's paths, stamped again            (the 1.5 ms)
    if before == stamps:  return { reopened: false, lost }  (nothing changed; a lost session stays lost and says so)
    rebuild(before)

reload:
    rebuild(the session's paths, stamped again)

rebuild(before):
    project = None                                          (1) the old program is freed first
    (result, paths) = open_tracked(root, file)
    stamps = for each path: before's stamp if before has the path, else a stamp taken now      (2)
    result is Ok:   project = Some(…), lost = None,  return { reopened: true,  lost: None }
    result is Err:  lost = Some(error),              return { reopened: false, lost }
```

**(1) The old program is dropped before the new one is built.** A failed rebuild keeps nothing anyway (§9 b), so holding the old program
during the build would only make the peak memory two programs instead of one.

> **🦀 Rust note: assigning `None` frees the program.** `self.project = None;` drops the `Project` that was in the `Option` at that line:
> Rust runs a value's `Drop` as soon as nothing owns it, and `Sem`'s `Drop` gives the program's memory back to the engine. No call to a
> "free" function is needed, and the order (free first, then build) is simply the order of the two lines.

**(2) A path the session already knew keeps the stamp taken before the build.** The build reads a file at some moment *after* `before`
was taken. If the file is saved during the build, a stamp taken *after* the build would describe the new text while the program may hold
the old one, and nothing would ever notice. With `before`'s stamp kept, the next `refresh` sees a difference and builds again: at worst
one rebuild too many, never a stale program. Only a path seen for the first time by this build has its stamp taken after it (§6).
`Session::open` has no `before`, so all its stamps are taken after the build.

## 5. Rules

| Rule | Why |
|---|---|
| A rebuild opens from the same `root` and `file` as `Session::open`, never from a file found again | the config is found from that file; a different file could pick a different config (a solution's child) |
| The stamp skips paths under `node_modules` and stamps, instead, each `node_modules` folder the program looked into **and the lock files beside it** (`package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock`, `pnpm-lock.yaml`, `bun.lock`, `bun.lockb`, the names `ENVIRONMENT_GLOBS` has) | 8057 of V-NOC's 9143 paths are there and cost 12 of the 14 ms. An install rewrites a lock file and adds or removes folders in `node_modules` (§6, §9 a) |
| A directory strictly above the root is not stamped; a file or a `node_modules` folder the engine asked for there is | The engine asks about every folder up to `/` (a `package.json` in each), so they are all seen. A folder's modified time moves whenever anything is created in it, and a home folder moves all day: on the reviewer's machine twice in 30 seconds (a shell's cache file, a tool's settings file), each one a rebuild of a project that did not change. What matters above the root is asked for by its own path (`package.json`, `node_modules`) and keeps its own stamp |
| The lock-file names are stamped only beside a `node_modules` folder that exists | the engine asks for a `node_modules` in every folder it resolves a package from, and most are not there; a folder that appears later changes its own stamp, and the rebuild then lists the names beside it |
| Every config file of `config_files()` is stamped, even one outside the root | an `extends` target outside the root changes the options; the tracker sees it read, this makes it explicit |
| A failed rebuild leaves the session with no program (`lost`) | an old program answers from text that is no longer there; `None` is what rule 8 allows, a stale tree is not (§9 b) |
| The stamps live in the `Session`, and a failed build gives its paths too | a lost session must see the disk come right again (§3); stamps inside the program would be gone with it |
| `lost` is reported by every `refresh` and `reload` while the session has no program | a driver that missed the first report would only see `None` and could not say why |
| A lost session rebuilds only when a stamp changed | a failure that stays (the file moved away for good) must not cost a build before every request |
| The old program is dropped before the new one is built | peak memory stays one program (§4 (1)) |
| A known path keeps the stamp taken before the build | a file saved during a build is caught by the next `refresh` (§4 (2)) |
| `refresh` with nothing changed does not rebuild and does not touch the caches | the answer must be byte-identical to the one before it (rule 9), and the cost is only the stamps |
| Neither `refresh` nor `reload` takes a `Cancel` | a program build is one engine call that cannot be stopped (07 §4); the driver calls `refresh` before the request, outside its cancel window |

## 6. Known limits

Each is written in `ENGINE.md` with the test that pins it.

- **A file changed inside `node_modules` with no lock file touched and no folder added** is not seen by `refresh`; `reload` sees it. Two
  ways it happens: a package edited by hand, and a **generator that writes into `node_modules`** (Prisma's `prisma generate` rewrites
  `node_modules/.prisma/client` after a schema change). So the vnoc side needs a "reload" the user can reach. If users meet this often,
  the middle way is to stamp the files under `node_modules` that the program read and that exist, and still skip the failed lookups
  there: §7's measurement prices it.
- **A file saved during the build that first reads it.** Its stamp is taken after the build (§4 (2)), so the program may hold the text
  from before the save. This is every path of `Session::open`, and on a rebuild only the paths that are new. A later save of the file,
  or `reload`, puts it right.
- **A modified time that does not move.** Two writes of the same length within the clock's resolution (1 s on HFS+ and FAT, finer on APFS
  and NTFS) read as no change. Python has the same limit.
- **A spurious rebuild** when an editor writes a temporary file beside a source file: the folder's modified time moves. It costs one rebuild
  and gives the same answers.
- **A file created above the root that only the config's `include` would find** (`"include": ["../*"]`, and nothing imports the file) is
  not seen by `refresh`, because the directories above the root are not stamped (§5); `reload` sees it. Project code is under the root
  (D33), so such a file could only add global declarations.
- **A file rewritten in place inside a package** does not move the package folder's modified time, which is why stamping the
  `node_modules` folders alone cannot see the first limit above.
- **The file the session was opened from is deleted:** the session is lost until the file is back (§3). The driver opens a new `Session`
  from another file if it wants one sooner.

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
| 7 | the `tsconfig.json` changes a `paths` mapping | after `refresh` the import resolves through the new mapping |
| 8 | a file under `node_modules` is edited by hand (a known limit) | `refresh`: the old answer, `reopened` false (pinned); `reload`: the new one |
| 9 | the file the session was opened from is deleted | `refresh`: `lost` is `NoSuchFile`, `project()` and `graph` answer `None`, nothing panics; a second `refresh`: `lost` again |
| 10 | case 9, then the file is written back | `refresh`: `reopened` true, `lost` `None`, and the graph is the one of a fresh `Session::open` |
| 11 | the `tsconfig.json` is saved with a syntax error, then fixed | broken: `lost` is `ConfigError` on each `refresh`; fixed: `reopened` true and the tree is back |
| 12 | after any rebuild | the graph is byte-identical to a fresh `Session::open` on the same disk |
| 13 | the callee's file is rewritten **during** a rebuild | that `refresh` may give either tree; the next `refresh` has `reopened` true and the new tree |
| 14 | case 8's edit, and the lock file beside that `node_modules` is rewritten (what an install does) | `refresh`: `reopened` true and the new answer |
| 15 | a folder is created beside the project, in the root's parent | `refresh`: `reopened` false (a directory above the root is not stamped) |
| 16 | `main` imports a package that is nowhere; then it is installed in the **parent's** `node_modules` (a monorepo's hoisted install) | `Unresolved` first; after `refresh`, `reopened` true and an `External` of the package |

Case 13 needs the change to land between the build and the stamps, so it is an in-crate test with a test-only hook in `sem` that
`open_tracked` calls once the program is bound (as the `def_without_node` test's hook, `sem/mod.rs`). With rule (2) of §4 taken out, the
second `refresh` says "nothing changed": that is the mutant the test must kill.

**As built** (`d3b394e`): cases 1–12 and 14 are `tests/session_refresh.rs`, with two tests of `reload` (it rebuilds when no stamp moved;
a lost session whose file is back). Case 13 is in `session.rs`, beside a test that counts builds (a second test-only hook, `BUILDS`): a
session lost for good builds once, then nothing on three more `refresh`, and `reload` builds. The in-crate tests write their projects
through one test-only file, `src/scratch.rs`, which is the only file `scripts/check-all.sh` leaves out of rule 2 in a library's `src`.
A rewrite in a test changes the file's length, so no test rests on the clock's resolution; case 5, told by a folder's time alone, waits
50 ms first. An `Unresolved` edge is asserted without its reason: the reasons are the policy table's.

**The mutants, run by the coding session and undone:** stamps taken after the build → case 13 fails; stamps lost with the program →
cases 10 and 11 and the count-of-builds test fail; no lock names beside `node_modules` → case 14 fails.

Also: the memory test gains a loop of "rewrite a file, `refresh`, `graph`" (50 rounds) whose resident memory stays bounded, as TS3's
open/drop loop does. A unit test in `sem` checks that the seen paths after a call graph are the seen paths after open (the probe's answer,
kept as a test).

**[measure]** on V-NOC, written in `ENGINE.md` (the coding session writes the `#[ignore]` test, the reviewer runs it: the coder's sandbox
cannot see V-NOC):

- `refresh` with nothing changed (the stamps; expected near 1.5 ms plus the configs and lock files);
- the first request after a saved file (the rebuild and a cold graph). **The owner decides from this number whether step 7 runs;**
- the seen paths after **every def of the frontend** is walked, against the seen paths after open (the probe walked one entry);
- how many of the seen paths under `node_modules` are files that exist: the price of §6's middle way.

## 8. Steps

One commit each, a stop for review after each.

1. **The stamps, `refresh` and `reload`** (cases 1–14, the seen-paths unit test). `Sem::open_tracked` with `trackingvfs` and a fresh
   `cachedvfs` per build, `Stamp`, `Stamps`; `Session::refresh` and `reload`, `Refreshed`, `project()` as an `Option`.
   **Built, `d3b394e`, accepted** (328 pass, 5 ignored; V-NOC `TaskDetailPanel` unchanged).

   1b. **The list of stamped paths, after the review** (cases 15 and 16): no directory strictly above the root; the outermost
   `node_modules` folder of a dropped path; the lock-file names only beside a `node_modules` that exists. As built, step 1 stamps every
   folder up to `/`, so a file created in the home folder rebuilds the project.
2. **The memory loop and the measurements.** No CLI change: `typescript-cli calls` is one request and needs no `refresh`.

## 9. Decided

The owner's answers (2026-11-17), each as recommended, and what the review of this design added.

a. **`node_modules` by its lock files**, not every seen path. Every path is the engine's rule and catches a file changed inside a package,
   at 14 ms a request on V-NOC (about the request's own time) and more on a bigger tree. The lock files cost nothing measurable, and the
   miss is covered by `reload` (§6 names the generator case).

b. **A failed rebuild loses the program**, it does not keep the old one. Python keeps its old database, but its requests then find no
   file and answer `None`. A kept TypeScript program would answer from text that is gone, so the honest answer is the same `None`.
   **Added by the review:** the session comes back by itself (§3), `lost` is said on every call, and the old program is dropped before the
   new one is built.

c. **`refresh` is called by the driver**, not inside `graph`. Inside, `graph` would take `&mut self` and pay the stamps on every
   call, and the driver would lose the choice of when a rebuild happens (once before a batch of requests, not in the middle of one). As in
   Python, the driver calls it, and the vnoc side's rule is one line: refresh before each request.

d. **Only saved text (D42).** `set_text` is not built (§10).

## 10. Later: unsaved text

**Not built (owner, 2026-11-17, D42): for now a tree is made from saved text only.** In this engine every changed text is a program
rebuild (241 ms on V-NOC in the dev profile), so a tree that follows the buffer would pay one for each request made while the user types.
The contract's request carries no text either (Python's [`11-driver-needs.md`](../05-python-migration/11-driver-needs.md) §1). What was
designed is kept here for the day the contract asks; none of it is in §8's steps.

- **The API:** `Session::set_text(&mut self, file: &str, text: Option<String>) -> bool`. `Some(text)` is what the editor holds, `None` goes
  back to the disk. It only records; the next `refresh` builds with it (a `dirty` flag beside the stamps). A file outside the root is
  refused (`false`): project code is under the root (D33).
- **Text the session already holds is no change.** `set_text` with the same text as the one held for that file, or `None` for a file with
  no held text, does not mark the session dirty. Comparing with the held text reads no file, and a driver that sends the buffer before
  every request would otherwise pay a rebuild each time. Text equal to the **disk's** still marks it dirty: comparing would read the file.
- **The overlay:** a small `FS` of our own in `sem`, between `cachedvfs` and the tracker. It forwards every method except two:
  `read_file` answers the unsaved text when there is one, and `file_exists` is true for it. The texts are an
  `Arc<BTreeMap<String, String>>` keyed by the engine's spelling of the absolute path. No overlay in `tsrs_vfs` fits: `wrapvfs::wrap`
  takes a `&'static dyn FS` and ours is an `Arc`, and `tsrs_project`'s `snapshotfs` is the language server's and would bring its whole
  crate.
- **[check]** on a case-insensitive file system (macOS, Windows) whether the engine asks with the spelling of the import (`./foo` for
  `Foo.ts`): if it does, the key is compared as the engine compares paths (its `to_path` with `use_case_sensitive_file_names`).
- **A known limit:** an unsaved file that is not on disk is in no directory listing, so the config's `include` does not pick it up; it is
  in the program only when something imports it.
- **Tests:** `set_text` renames the callee in the buffer and the tree follows, `None` gives the disk's tree back; `set_text` for a file
  not on disk that `main` imports resolves the import; `set_text` outside the root is `false` and the next `refresh` does not rebuild;
  the same text twice rebuilds once; the memory loop with `set_text`.
- **The cost to measure then:** the first request after a `set_text`. If it is too slow, step 7 (an incremental program) is the answer,
  not a faster overlay.
