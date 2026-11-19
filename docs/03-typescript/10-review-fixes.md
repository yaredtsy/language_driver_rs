# 03.10 The fixes from the 2026-11-16 review

```
the review's fixes
├── 1. what the review found, and what was checked
├── 2. the order
├── 3. step R: the root bounds project code (D33)        ← the next task
├── 4. step D: two defs with one marker (D34)
├── 5. step 6 gains: a file changed on disk
├── 6. step H: the hardening batch
├── 7. the stack
└── 8. not scheduled
```

Written on 2026-11-16, after a review of the whole TypeScript analyzer (`languages/typescript/crates/*`). §1 was **run**: the debug
`typescript-cli` on throwaway projects, outside the repo. §3 to §7 are from reading the code: **not built**. Every `[check]` is a claim to
verify before depending on it.

## 1. What the review found, and what was checked

| # | Finding | Checked how | Result | Goes to |
|---|---|---|---|---|
| 1 | a file outside the root is walked as project code | run | **true.** `app/src/main.ts` imports `../../shared/lib`: `target_file`, `call_site_file` and `StableDef::Path.module` are the machine's absolute path; `files()` does not list the file. A `.d.ts` outside the root is already `External`, and its `package` is the absolute path too | §3 |
| 2 | two defs with one marker are one def | run | **true.** `main(){a();b()}`, both `ID: SAME`: the tree is `main > a{x,y}, a{x,y}`. The merged wire has one `a` with `call_count: 1`, no `b`, and `x`, `y` with `call_count: 1` each though each is called once. Nothing is counted. Python does the same (`sem/defs.rs`, `stable_def`) | §4 |
| 3 | nothing survives an edit | read | by design (07 §5): every cache belongs to one program. One part is real: an incremental program (step 7) breaks the contract of `free_unshared_program` | row 7 of 07 §7 |
| 4 | a created or deleted file is not noticed | read | **true, and wider**: `Session` has `open` and `graph` only, and the planned `reload` is for `environment_inputs`. Nothing says who notices a source file saved on disk. Python's plan has the rule (`refresh`, PY9.2); it was not carried over | §5 |
| 5 | a second checker handle on one thread never returns | read | **true as a risk.** The handle holds a `std::sync::MutexGuard`; `body()` takes the checker itself on a cache miss (`accessor_sites`). Today no path holds one and asks for another | §6 |
| 6 | the two memory tests measure the whole process while the other runs | read | true | §6 |
| 7 | `Options.marker` is `MarkerStyle<'static>` | read | true, **and Python has the same field and the same `Box::leak` in its CLI**. Both languages or neither | §8 |
| 8 | two lists kept by hand | read | the `Display` label table of `Breakdown` drops a key it does not know; `matches_a_glob` repeats `ENVIRONMENT_GLOBS` | M step 4; §6 |
| 9 | `handed_not_called` sorts by `Debug` text | read | true; `StableDef` is `Ord` | §6 |
| 10 | quadratic work per file | run | **true and harmless**: one file, `main` calling every def: 8 000 defs 104 ms, 32 000 defs 1 143 ms of graph time (debug build) | §8 |
| — | deep nesting on a small stack | run | **true.** With a 2 MiB stack (`ulimit -s 2048`, debug build) a 10 000-term `+` chain, a 10 000-call method chain and 10 000 nested calls each **abort the process**, in `outline` alone too. All pass at 8 MiB; 5 000 nested calls pass at 2 MiB | §7 |

Not defects: the real-project goldens are `#[ignore]` because they need a folder of the owner's machine (rule 6). A package of the same
repo reached through a `node_modules` symlink is `External` whatever the root is (run: named `packages/b/src/index.ts`), which
[`05-ts3-project.md`](05-ts3-project.md) §2 accepts for phase 1.

## 2. The order

| When | What | Why there |
|---|---|---|
| **next** | **step R** (§3) | only the `sem` module and fixtures, no `core/`; M step 2 names a file by its path, so "a def's file is under the root" should hold first. It can run while M waits for the answers to 09 §6 |
| after R | **step D1** (§4): the second def of a file with a marker already used | `sem` only |
| with M step 1 | **step D2** (§4): core counts the defs it drops | both change `core/`: one clean `git status core/`, one run of Python's tests |
| in M step 4 | the label table of `Breakdown` comes from `Uncovered` | that step retires three counters and edits the table |
| after M, before step 6 | **step H** (§6) | small, tests and `sem` |
| step 6 | §5 | it is a part of that design |
| step 7, if it is built | the free contract (row 7) | only then |

## 3. Step R: the root bounds project code (D33)

### 3.1 What is wrong

`Sem::is_project_code` asks five things of a file (TypeScript or JavaScript, not a bundled library, not a declaration file, not in
`node_modules`, not found by searching `node_modules`). It never asks **where the file is**. So a file the program pulled in from outside
the root is walked, and three names leave the analyzer as the engine's absolute path:

| Name | Made in | For |
|---|---|---|
| `DefInfo::file` → the wire's `target_file`, `call_site_file` | `describe` (`sem/resolve.rs`): `relative_path(..).unwrap_or(file_name)` | a def outside the root |
| `StableDef::Path.module` → every `ContextId` | the same line | the same def, when it has no marker |
| `External.package` | `resolve.rs`, "the repo's own `.d.ts`, named by where it is": the same fallback | a `.d.ts` outside the root |

What that breaks: the same project in another folder or on another machine gives another answer (rule 9); no snapshot of it can be in git
(rule 6); a tree keyed by these ids is never found again; and [`02-tree-rules/04`](../02-tree-rules/04-the-defs-table.md) says "every
row of `defs` has a file inside the project". Python decided this as D26 and pinned it in R1's test (d) (no row of `defs` outside the
root). TypeScript took D26's first half (the config search stops at the root, TS3 step 2) and not this one.

Also seen: **the entry itself may be outside the root.** `Project::open(app, "../shared/lib.ts")` loads an inferred project, and
`calls … --entry helper` answers with a tree of absolute paths. `Sem::entry` does not ask whether the file is project code, so a file in
`node_modules` can be an entry too.

### 3.2 The rule (owner, 2026-11-16)

**Project code is under the root. A file outside it is `External`, for now.** Its calls are a leaf, as a package's are. Monorepo layouts
(a sibling package by a relative import or a `paths` mapping, with a sub-folder opened) get a design of their own later, from the
owner's monorepo work or from what users report.

1. A def's file, a def's id and an external's name are **never an absolute path**, with one stated exception (§3.3, the name).
2. A call into a source file outside the root is an `External` edge named by the file's path **relative to the root, with `..`**:
   `External { package: "../shared/lib.ts", name: "helper" }` (the root is `app`, the file is `shared/lib.ts` beside it). The same for a `.d.ts` outside the root.
3. Each such edge into **source** code is counted in `Stats.language["outside_root"]`: it is a body that was there to walk and was not,
   and the number is what tells the owner how often real projects reach out (rule 8). A `.d.ts` has no body, so it is not counted.
4. An entry is a def of project code. A file outside the root, or in `node_modules`, has no entry: `None`.
5. `Project::open` with a file outside the root is an error of its own, `OpenError::OutsideRoot`.

What is given up: the callee tree of a sibling folder's code, and the doc link of a marked def out there (an `External` has no
`target_id`).

### 3.3 The design

All of it is in the `sem` module (rule 4), plus one variant and two doc comments in `lib.rs`.

| What | Change |
|---|---|
| `Sem::is_project_code` | one more condition: `self.relative_path(file).is_some()`. Every caller follows: `resolve` (calls, constructions, `init_chain`), `handed.rs` (hand-overs), `project_defs`, the coverage scan, `Project::files` |
| the name of an external with no package | one function, `Sem::place_of(file) -> String`: `relative_path`, else the path from the root to the file with `..`. The two `unwrap_or_else(.. file_name ..)` fallbacks become calls to it; `describe`'s goes away, because a `Func` is now always under the root (`expect`, with the reason, like the one above it) |
| `outside_root` | `SiteFacts` gains the number of targets of the site that are source files outside the root; `walk::resolve` notes it, as it notes `getter_result`. `Breakdown` gains `outside_root: u64` and a line in `Display` **said only when it is above 0**, so every existing breakdown is byte-identical |
| `Sem::entry` | `None` unless `is_project_code(file)` |
| `Sem::open` | after the two `canonicalize` calls: the file is not under the root → `OpenError::OutsideRoot`, before any config search |

- **The path with `..`.** `tsrs_core::tspath::get_relative_path_from_directory(from, to, …)` is TypeScript's own function for this
  (`path.rs:979` in the clone). `[check]` that it is at the pin and reachable through the `tspath` path `sem` already imports. If not, it
  is a few lines over the path's parts: drop the common leading parts, one `..` per part left of the root, then the rest of the file.
- **The stated exception.** On Windows a file on another drive (`D:\shared\lib.ts`, root on `C:`) has no relative path, and TypeScript's
  function answers with the absolute one. That is kept, spelled with `/`: it is the same choice `environment_inputs` makes for a config
  outside the root (07 §1). It cannot happen on Unix. A unit test on strings pins it.
- **`relative_path` compares spellings.** It is `strip_prefix` of the root's engine spelling, then of `/`, so `/a/root` is not a prefix
  of `/a/rootx/f.ts`. `[check]` on macOS and Windows that a file under the root never fails it because of letter case (the root is
  `canonicalize`d, the engine's names come from its own file system): if it can, **every** file of such a project becomes external,
  which the existing fixtures would show at once on that OS. CI's three systems are the check.
- **Nothing stored moves** (rule 10). A def outside the root was named by an absolute path, which no other checkout could match.

**Rust note.** `OpenError` is `#[non_exhaustive]`: code outside the crate cannot `match` it without a `_ =>` arm, so adding a variant
does not break a caller's build. That attribute is what rule 3 buys.

### 3.4 The fixture

`languages/typescript/crates/typescript-analysis/tests/fixtures/outside-root/`. The tests open **`app`**, as `extends-outside` does.

```
outside-root/
├── shared/
│   ├── lib.ts            export function helper() { inner(); }      function inner() {}
│   │                     export class Tool { constructor() { setup(); }  run() {} }
│   │                     export class Base { constructor() { setup(); } }       function setup() {}
│   └── api.d.ts          export declare function typed(): void;
└── app/                  ← the root
    ├── tsconfig.json     { "compilerOptions": { "strict": true }, "include": ["src"] }
    └── src/
        ├── main.ts       import … from "../../shared/lib";  import { typed } from "../../shared/api";  import { local } from "./local";
        │                 export function main()  { helper(); typed(); new Tool().run(); local(); }
        │                 class Mine extends Base {}
        │                 export function build() { new Mine(); }
        │                 export function hands() { take(helper); }      function take(f: () => void) { f(); }
        └── local.ts      export function local() {}
```

### 3.5 The steps

One commit each, tests first and seen red for the stated reason, stop after each for review.

| # | Change | Failing tests | Docs, in the same commit |
|---|---|---|---|
| R1 | `place_of`, used for an external's name | **unit**, on strings: a sibling folder (`../shared/lib.ts`), two levels up, a folder whose name starts with the root's (`/a/root`, `/a/rootx/f.ts` → `../rootx/f.ts`), another Windows drive (kept absolute, with `/`). **`tests/calls.rs`**: `main`'s edge for `typed()` is `External { package: "../shared/api.d.ts", name: "typed" }` (**red today: the absolute path**) | `ENGINE.md` "Which files are the project's": how a file with no package is named |
| R2 | `is_project_code` asks for the root; `outside_root`; the `Breakdown` line | on `outside-root`, entry `main`, `build`, `hands`: (a) no row of `defs` has a file that is absolute or starts with `..`, and no `StableDef::Path.module` either (**red today**); (b) `helper()` is `External { "../shared/lib.ts", "helper" }` and `inner` is nowhere in the graph; (c) `new Tool()` and `.run()` are externals `Tool` and `Tool.run`; (d) `new Mine()` is a `Construct` with no `init`, and `inherited_constructor_external` is 1; (e) `hands` has no passed edge for `helper`; (f) `outside_root` is 3 for `main` (`helper`, `Tool`, `Tool.run`; `typed` is not counted) and the breakdown says so; a graph with none has no such line; (g) **the proof of the point**: the fixture copied to a second folder (`std::env::temp_dir()`, a folder of the test's own, removed at the start and the end; a test may write files, rule 2 is about the library) gives a wire that is **byte-identical** to the first folder's; (h) `files()` is `src/local.ts`, `src/main.ts` | `policy.md`: the `project-code` row says "under the root" and names the fixture; `ENGINE.md` the same paragraph; the doc comment of `Project::files` (there is no "project code outside the root" any more) |
| R3 | `Sem::entry` and `Sem::open` | `call_graph(&project, "../shared/lib.ts", helper)` is `None` (**red since R2: a panic**, the `expect` in `describe`; before R2 it was a graph of absolute paths. Found at R2's review, 2026-11-17: `typescript-cli calls outside-root/app ../shared/lib.ts --entry helper` panics at `b9a0dee`); an entry in a file of `with-node-modules/node_modules` is `None`; `Project::open(app, "../shared/lib.ts")` is `Err(OpenError::OutsideRoot)` (**red today: an inferred project**); an absolute path **under** the root still opens (`an_absolute_file_path_is_accepted` unchanged) | the doc comments of `Project::open` and `OpenError`; D33 is already in `docs/07-decisions-and-questions.md`. **For M step 2** (`Entry::File`): a file entry goes through the same check, so a file outside the root or in `node_modules` has no file entry either; written into `09-module-def.md` step 2 |

**Built** (2026-11-17): R1 `65fb828`, R2 `b9a0dee`, R3 `09ad1cd`. One rule for "under the root", `project::under_root`, serves
`relative_path`, `relative_to` and `Sem::open`. **R4, found at R3's review:** `Project::defs_named` still lists the defs of a file that is
not project code (in `node_modules`, a `.d.ts`), so `typescript-cli calls node-modules-source node_modules/lib/index.ts --entry inside`
says ``no def `inside` `` though the def is there, and a driver's ambiguity list would offer defs that have no entry. R4: `defs_named`
answers `None` unless the file is project code, as `entry` does, and the CLI's message for `None` names the reason (not project code: in
`node_modules`, a declaration file, outside the root, or not code). A test in `tests/outside_root.rs` and one in the CLI's.

**Proof, after each step.** `cargo test` in `languages/typescript`; `cargo clippy -p typescript-analysis --all-targets -- -D warnings`;
every existing snapshot unchanged. After R2, by hand: the real-project goldens (V-NOC and both postcss inputs) are **byte-identical**,
and `outside_root` is reported for each. If a golden moves, the diff is the list of that project's imports from outside its root: read
it, do not accept it. `core/` and Python are not touched.

### 3.6 Not in this step

- **Who is told when a file outside the root changes.** It is not in `files()`, `declaration_files()` or `environment_inputs()`, so a
  cache does not know the tree depends on it. That is one case of §5 (what the program read), not a list of its own.
- **`node_modules` above the root.** With a sub-folder opened, the packages and the lock file are often one level up; the globs of
  `environment_inputs` are relative to the root and do not cover them. Found while reading, not run. Monorepo work.
- **A `paths` mapping to a sibling's source.** Not run. It should behave as the relative import does.

## 4. Step D: two defs with one marker (D34)

### 4.1 What happens

A marker is how a def keeps its identity when it is renamed or moved. A person copies a function, and the copy carries the original's
marker. Two defs now say "I am `SAME`":

```ts
/** ID: SAME */ function a() { x(); }
/** ID: SAME */ function b() { y(); }      // pasted from a, then edited
export function main() { a(); b(); }
```

A def's id is its marker (`StableDef::Marked`), so the graph has **one** def and one context for both: `Graph::new` sorts the rows and
keeps the first of each id. `b` is drawn as `a`, each shows the other's calls, the merged wire counts them twice, and no number says so.
Copying a whole file does this to every def in it.

### 4.2 The rule (owner, 2026-11-16: change the second, or warn)

1. **In one file, the first def in source order owns a marker text.** A later def of that file with the same text is identified by where
   it is (`StableDef::Path`), exactly like a def with no marker, and is counted in `Stats.language["duplicate_marker"]`, once per such
   def in the graph. The tree is then right: `main > a{x}, b{y}`.
2. **Across files nothing is renamed; it is counted.** `Graph::new` counts each row it drops that is **another def** (same id, another
   file or another start) in a new field of core's `Stats`. The tree is still wrong there, and the number says how often.
3. **The outline does not change.** Both defs still report `marker: Some("SAME")`: text in, data out. So a driver can warn about a
   repeat in one file from the outline alone, with no call graph.

Why the split stops at the file: **an id must not depend on what the request reaches.** If `b` were renamed only when `a` is in the same
graph, `b` asked for alone would be `Marked("SAME")` and `b` under `main` would be a `Path`: two ids for one def, and a stored tree would
not find it. Inside one file the outline has every def, so the answer is the same in every request and costs nothing. Across files it
needs the markers of **every** file of the project before the first request: an outline of each (a second parse of each, since the
outline is made from text). `[measure]` that on V-NOC before deciding whether the split goes project-wide. V-NOC's core already stores
every marker with its file, so it can find a cross-file repeat more cheaply than an analyzer can: a note for `vnoc`, not built here.

Known, and accepted: if the copy is pasted **above** the original, the copy owns the marker and the original is the one named by where
it is, until a person removes the copied marker.

### 4.3 The steps

| # | Change | Where | Failing tests |
|---|---|---|---|
| D1 | rule 1. `FileDefs` learns, once per file, which defs own their marker; `describe` reads it | `typescript-analysis` (`sem`) | a fixture `duplicate-marker` and a `policy.md` row: (a) the tree of `main` is `a{x}`, `b{y}` (**red today: `a{x,y}` twice**); (b) merged: two nodes, every `call_count` 0; (c) `a`'s `target_id` is the prefix and `SAME`, `b`'s is empty and its `target_qualname` is `b`; (d) the counter is 1 with `b` called twice; (e) three carriers: 2; (f) a class and a function with one marker; (g) **`b` asked for as the entry has the id it has under `main`**; (h) the copy above the original: the copy owns it (pinned as known); (i) `typescript-cli calls --id SAME` with two defs of one qualname carrying it lists both and exits 2 (`[check]`: it takes the first today); (j) the outline snapshot of the fixture shows the marker on both |
| D2 | rule 2. `Graph::new` counts the dropped rows that are another def; `Stats` gains the field | `core/` | hand-written graphs, as the existing tests: two rows with one id in two files → 1; in one file at two starts → 1; **the same row given twice → 0**; none → 0. TypeScript: the marker of `duplicate-marker` in a second file → one node (as today, pinned as the known limit) and the count is 1. Python's tests pass (rule 5); a `{:#?}` dump of `Stats` gains one line, as it did for `frontier` |

**Built** (2026-11-17): D1 `c30e969` (`FileDefs::owns_marker`, the first by start position owns a text; the count is taken once the graph is
built, from the ids `Lang::describe` saw demoted, and every row of `graph.defs` comes through it, `solve.rs:450`). **(i) answered:** with two
defs of one qualname carrying `SAME`, `typescript-cli calls … --id SAME` takes the first and exits 0; that is the owner only when the owner
has that qualname, so `--entry b --id SAME` opens `b`, whose id is not `SAME`. Not a wrong tree, a person not told: H9.

**D2 built** (2026-11-17, `cf0eb8b`): `Stats.merged_defs`; a row given twice and a def given twice count once; `Breakdown` prints it only above 0. Its TypeScript fixture sits in `tests/limits/duplicate-marker-across/`, because the coverage sweep (row 3c) lists the merged def's calls as a miss in `in-named-function`, a category the test declares a bug that no known-gap entry may name. M step 2 brings it back (09 §5 step 2).

Python gets D2's number for free. Rule 1 for Python is a row of Python's plan, after `python-v0.3.0`.

## 5. Step 6 gains: a file changed on disk

07 §6 covers text the editor has not saved (`set_text`). It does not say what happens when a file **on disk** changes while a `Session`
is open, and the driver contract has no change notification (Python's [`11-driver-needs.md`](../05-python-migration/11-driver-needs.md)
§4 found the same and answers it with `Project::refresh`, called before each request). Until step 6 a `Session` cannot follow the disk
at all: a driver that keeps one must open it again.

What step 6's design must answer, with a test for each (Python's PY9.2 (a) to (c), carried over):

| Case | Test |
|---|---|
| a file of the program is rewritten on disk | open, ask, rewrite the callee's file so that it calls another function, bring the session up to date, ask: the tree follows. Without that call it does not (pinned, so the rule has a reason in the tests) |
| a file an import names is created after the first request | the call is `Unresolved` first, and reaches the new def after |
| a callee's file is deleted | the call is `Unresolved`, nothing panics |
| a file outside the root that the program read is rewritten (§3.6) | the external's name follows |

Two ways, for that design to choose from:

1. **The session checks for itself**, as Python does: remember the modified time and size of every file the program read, compare
   before a request, rebuild when one differs. It cannot see a **created** file that way. `[check]` whether the tsrs program keeps the
   paths it looked for and did not find (TypeScript has them as the failed lookups of module resolution); if it does, they are checked
   too.
2. **The driver says so**: V-NOC's watcher already compares paths with `environment_inputs`. Then the rule is written for `vnoc`: any
   file of `files()` or `declaration_files()` changed, or any source file under the root created or deleted, means `reload`.

Either costs a program rebuild, which is what `set_text` already costs in step 6.

**Decided (owner, 2026-11-17, D41): way 1**, the session checks for itself before each request. **And only saved text is read, for now (D42):** `set_text`, which this section speaks of as part of step 6, is not built, so step 6 is `refresh` and `reload` alone. **`[check]` answered (reviewer,
2026-11-17), corrected the same day:** the **program** keeps no failed lookups, but the engine's **file system** wrapper
`tsrs_vfs::trackingvfs` records every path asked, missing ones too, and the engine's watch mode uses it (`tsrs_execute/src/watcher.rs:778`).
Step 6 uses it the same way: [`11-step6-text-and-disk.md`](11-step6-text-and-disk.md) §2, which also has the numbers (9143 paths on V-NOC,
1.5 ms to stamp the 1086 outside `node_modules`). What follows was the first answer, kept for the record, and its known limit does not hold:
tsrs keeps no failed lookups of module resolution. `Program::missing_files` holds only root and referenced files it could not
open, behind a "Testing only" accessor (`tsrs_compiler/src/program.rs:3134`), and `get_unresolved_imports` gives module specifiers, not paths.
So the stamp covers the program's files and the config's file list, re-run each time: a source file created or deleted under the root
changes that list. A file created outside it and only imported is not seen; that is a known limit, with its test pinning it. Python can
see such a file because ruff's `Files` keeps an entry for each path it looked up and did not find. The step 6 design is written before the
code.

## 6. Step H: the hardening batch

**Built** (2026-11-17, one commit each, in the order H1–H6, H8, H10, H9, H7; reviewed, accepted). TypeScript 302 pass; V-NOC `TaskDetailPanel`
unchanged; the golden trees, wires, merged wires and breakdowns of the five entries byte-identical (`.graph` less the `frontier` and
`merged_defs` lines). What was built and measured: `ENGINE.md`, "TS5 step H". H11 was found while building H10 and is not built.

Small, each with its test, one commit each or one for all as the coding session finds natural. None changes an answer (H10 changes a reason
only).

| # | What | Proof |
|---|---|---|
| H1 | **built, `506f945`.** one checker handle at a time, checked: `Sem` holds a `Cell<bool>` that a small guard sets when a handle is taken and clears on drop; taking a second is a `debug_assert!` | a `#[should_panic]` unit test that takes two (debug builds only) |
| H2 | **built, `23eb02c`.** the two memory tests do not run at once: one `static Mutex<()>` both lock | read; the numbers in the file's header still hold |
| H3 | **built, `7f6cfb4`.** `handed_not_called`: `found.sort()` | golden unchanged |
| H4 | **built, `4d7be1e`.** `Sem::_region` → `region` (it is used; the underscore says it is not); the header of `tests/threads.rs` says `Send` and not `Sync` | builds |
| H5 | **built, `bbee118`.** `matches_a_glob` is derived from `ENVIRONMENT_GLOBS` instead of repeating its names | the four `environment_inputs` tests unchanged |
| H6 | **built, `e61afcc`.** a request test with `occurrence` above 0 (D15: a getter and its setter, by `Entry { occurrence: 1 }`) | the test |
| H7 | **built, `dc1bc9a`.** the deep-nesting test of §7 | the test |
| H8 | **built, `f4d00af`.** a root that is a drive root (`D:\` on Windows, `/`) ends in `/` in the engine's spelling, so `under_root` (`project.rs`) finds nothing under it, and since R3 every file is `OutsideRoot`. Older than R (TS3's `relative_path` had it); found at R3's review, not run. Take the root's trailing `/` off before the compare | a unit test on strings (`under_root("C:/", "C:/a.ts")` is `Some("a.ts")`, `under_root("/", "/a.ts")` too) |
| H10 | **built, `2dbc5c8`.** **an IIFE's own call is `Unresolved(no_declaration)`** (found at M step 5's review, run): `(() => { h(); })()` draws `h` in the enclosing def, as the phase-1 rule says, and adds one `Unresolved` edge for the call of the arrow itself, though nothing about it is unknown. Older than M (a function's IIFE does the same); it inflates `unresolved` on any file with IIFEs. Give the call of a function expression written in place its own reason (`anonymous_function_called`), still `Unresolved` and counted, so the number says what it is | the `iife.ts` fixtures: the reason is the new one; `no_declaration` is 0 there |
| H9 | **built, `d727c8e`.** **the warning of D34, where a person sees it.** `DefMatch` gains `owns_marker: bool` (additive: it is `#[non_exhaustive]`, `lib.rs:373`, checked), so a driver can say "this def carries `ID: SAME`, but `a` at line N owns it" without parsing the file again. `typescript-cli calls … --id X` picks the def that **owns** `X` (its id is `X`); a carrier that does not own it is not picked by `--id`, and the CLI prints which def owns it. The tree does not change | `tests/duplicate_marker.rs`: `defs_named` reports `owns_marker` for both; a CLI test of `--entry b --id SAME` (the message names `a` and its line) |
| H11 | **built, `6c1dfc4` (D40; reviewed, accepted).** **(found at H10, 2026-11-17; decided by the owner the same day as recommended, D40) a named function expression called in place is not entered.** `(function named() { h(); })()`: the outline makes `named` a def (the engine's rule: a function expression with a name), so the enclosing body stops at it, and the call is `Unresolved` (since H10 `anonymous_function_called`, a reason that is wrong for it: the function has a name and a def). So `h` is drawn in no tree but `named`'s own, which only a request for `named` reaches; the coverage sweep cannot see it because it enters every def. The engine gives no answer either: its call hierarchy takes the callee, a `ParenthesizedExpression`, which is no declaration and has no symbol, and drops the call (`tsrs_ls/src/callhierarchy.rs:936`). **Recommended:** when the callee, under parentheses, assertions and `!`/`void`, is a function expression that `def_of` holds, the edge is `Call` to that def: the callee is the def's own node, so it is not a guess (rule 8), and it is what the row `nested` says for a named function (it appears where it is called). An anonymous one stays `anonymous_function_called`. Where it is common: UMD and bundler wrappers (`(function webpackUniversalModuleDefinition(root, factory) {…})(this, …)`) | `file-entry/src/iife-forms.ts`: the named case is a `Call` to `named` and the module's tree shows `named ▸ h`; the four anonymous ones unchanged; V-NOC and postcss goldens unchanged unless they hold a named IIFE (say which) |
| H12 | **built, `c6835fe` (reviewed, accepted; (a) decided by the owner, D40 extended).** **(found at H11's review, 2026-11-17) two more defs written in place and reached without a direct call.** (a) `new (class Named { constructor() { g(); } })()`: the class expression is a def, and the `new` is `Unresolved(no_declaration)`, so the constructor's `g` is drawn under no caller (found by the coding session). (b) `(function wrapped() { g(); }).call(this)` and `.apply`: the site resolves to the external `Function.call`, and `wrapped`'s `g` is in no tree under the caller and **in no counter**. `uncovered_call_apply` counts a name before `.call`, not a function written there, so this is a rule 8 gap. Anonymous forms are fine: their calls are the enclosing body's. **Recommended:** (a) is D40 for a class: when the class expression of a `new`, under parentheses and assertions, is a def, the site is a `Construct` through `walk::construction`, like `new C()`. This is the owner's call (D40 names function expressions only). (b) is counted in `uncovered_call_apply`, as a name is, with no new decision (D22 step 5: call/apply is counted, not drawn) | a fixture for each, (a) in both forms, `new (class N {})()` and `new (class {})()`; V-NOC and postcss goldens unchanged |
| H13 | **found by the owner, 2026-11-19; decided (D43); not built.** a call that resolves to a **method or accessor signature in a type literal** (`type Ops = { m(): void }`), or to an accessor signature of an interface, panics in a debug build and is `Unresolved(decl_not_in_outline)` in a release build: the outline has no def for it. §6.1 | §6.1 (c): one fixture per declaration kind |

**Rust note (H1).** A `std::sync::Mutex` locked twice by one thread does not return: the documentation leaves it at "will not return
(it might panic or deadlock)". There is no error to catch, and `Cancel` is never checked, because the thread is waiting inside `lock`.
A `Cell<bool>` costs one byte and turns the wait into a message with a line number, in the builds the tests run.

### 6.1 H13: a method signature in a type literal (found by the owner, 2026-11-19; decided, D43; not built)

**What happened.** The owner ran the debug `typescript-cli calls` on a real file and it panicked:
`the engine's declaration in …/fsOperations.ts is not a def of the outline` (`sem/resolve.rs:488`). The file declares
`type FsOperations = { existsSync(path: string): boolean; … }` and calls `fs.existsSync(p)` on a value of that type.

**Why.** Two rules must agree on what a def is: the engine's (the declaration a call resolves to) and the outline's (what
`typescript-syntax` lists). For this call the engine answers the **method signature inside the type literal**. The outline lists the alias
(`Type FsOperations`) and nothing inside it. The line that panicked is a `debug_assert!` put there for exactly this disagreement: a debug
build stops, a release build counts the call as `Unresolved` with the reason `decl_not_in_outline` (read from the code, not run in release).

**Run by the reviewer** (debug CLI, 28 throwaway one-call projects, each a neighbour of the owner's case):

| The call resolves to | Answer today |
|---|---|
| a method signature of an **interface** (`interface Ops { m(): void }`) | a def, `Ops.m` (policy row `interface`) |
| a property of function type (`{ m: () => void }`), a call or construct signature, a function type alias | `Unresolved(function_typed_value)` (D16) |
| an abstract method, a method of a `declare class`, an interface in `declare global` | a def |
| an index signature, `Record<string, () => void>` | `Unresolved(no_declaration)` |
| **a method signature in a type literal**: under an alias, a parameter's type, a nested member, a return type, a generic argument, a `declare const`, a class property's type, a constraint (`T extends { m(): void }`), an intersection, through `Pick`, optional (`m?()`), with overloads, called as a template tag | **panic** (14 of the 28 cases) |
| **an accessor signature**: `get v(): number` or `set v(x)` in a type literal, **and in an interface** | **panic** (3 cases; 17 of the 28 in all) |

**Why no test found it.** The tests come from two lists: the policy table, whose rows were written from the kinds of call *site* and of
callee expression, and the sweep over two real projects. The table has the interface's method and the function-typed property, the two
neighbours, and no row for the same member written in a type literal. Neither V-NOC nor postcss calls through one (the whole-project runs
of both are debug builds and never stopped). Nothing listed the kinds of *declaration* the engine can answer with.

**(a) The rule. Decided (D43, 2026-11-19; the owner asked the reviewer to decide):**

- a type alias whose type **is** an object type (`type Ops = { … }`): its method and accessor signatures are defs, named as an
  interface's are (`FsOperations.existsSync`, Method). Such an alias is an interface in all but the keyword, and its members carry
  JSDoc as an interface's do. Rule 10 allows it: a kind of def is added, no existing name changes;
- a type literal anywhere else (a parameter's type, a nested member, a return type, a part of an intersection): it has no name of its
  own, so the call is `Unresolved(function_typed_value)`, as the same member written `m: () => void` already is (D16);
- an accessor signature of an interface: a def, as its method signature is (`G.v`), and as a class's accessor is.

The choice not taken is `function_typed_value` for every one of them. It is simpler, and on the owner's file it gives some thirty
`Unresolved` lines for calls whose names are written in the file. "An alias directly around it" means the alias's type is the object
type itself: in `type A = { m(): void } & B` or `type A = Readonly<{ m(): void }>` the literal is a part of something else and has no
name. `languages/typescript/docs/qualnames.md` and `policy.md` gain their rows when this is built.

**(b) No panic on a user's code, in any build.** The `debug_assert!` at `resolve.rs:488` goes; the count stays. The tests keep their
tripwire another way: the helper every graph test goes through fails when `decl_not_in_outline` is counted and the test did not say it
expects it. Rule 8 ("never fail") then holds in a debug build too. `[check]` the two other `debug_assert!` of `sem` (`resolve.rs:141`,
`mod.rs:854`) for whether a user's text alone can reach them; they look like checks of our own code, which may stay.

**(c) A grid by declaration.** One policy fixture for each row of the table above (the 28 cases), each with its expected answer, and
`policy.md` gains the rows. The list to fill is the engine's own: every node kind that can carry a signature (function, method,
constructor, accessor, function expression, arrow, method signature, call signature, construct signature, index signature, function
type, constructor type, JSDoc callback) against every place it can be written (class, class expression, interface, type literal,
object literal, namespace, `declare`).

**(d) A sweep anyone can point at a project.** An `#[ignore]` test driven by a variable (as `TS_STITCH_*`): the graph of every def of
every file of `Project::files()`, the total breakdown printed, a failure when `decl_not_in_outline` is not 0. The reviewer runs it on
each new real project before the owner tries it.

**The corpus (D43).** By style, not by size, each a checkout outside the repo that is only read: first the private codebase on the
owner's machine that found H13 (about 1,900 `.ts`/`.tsx` files, no config and no packages: type-literal tables, `using`, async
generators); then **hono** (type literals and generics) and **nest** (classes, decorators, interfaces). A bare checkout is enough for the
sweep, since a disagreement between the engine and the outline is about project code. The unresolved counts need the packages installed
(without them a call into a package is `any`), so those are taken on V-NOC and on one more project, chosen when the sweep has run.

**(e) The count of `function_typed_value`, split by what holds the function** (a parameter or prop, a binding made from a call's result,
a property, a variable): `Stats.language` keys beside the total. It is what says, per project, how much parameters in the key (C3 of
[`08`](08-later-context-sensitive.md)) would answer; the design of C3 waits for it.

## 7. The stack

**Rust note.** A stack overflow is not a panic. A panic unwinds and can be caught at a thread's edge; an overflow hits the guard page
and the **process aborts**. A driver that overflows on one file is gone for every file.

Measured (§1): at 2 MiB, which is what `std::thread::spawn` gives a thread, 10 000 levels of nesting abort, in `outline` alone too; at
8 MiB (a main thread on Linux and macOS) they pass. A `+` chain is the realistic shape: the parser nests it one level per term, and
generated files have long ones. Which layer overflows first (the engine's parser, or our walk over its tree) was not found; the fix is
the same. tsrs gives every thread that parses or checks 512 MiB (`CHECKER_STACK_SIZE`, `WORKER_STACK_SIZE`; the memory is reserved, not
used, until the stack grows).

| Where | What |
|---|---|
| `vnoc`, the driver | every thread that calls an analyzer function is made with `std::thread::Builder::new().stack_size(…)`. Row 9 of 07 §7 says so |
| here, H7 (**built, `dc1bc9a`**) | a fixture with a 10 000-term `+` chain and a 10 000-call method chain; `outline` and a call graph of each, **on a thread of the size the driver is told to use**, and `[measure]` the smallest size that passes in a release build, written in `ENGINE.md` with the number the driver is given (a margin above it) |

**Measured (H7, release build, three runs agree):** at 10 000 levels a `+` chain passes at 4 MiB and aborts at 3.5 MiB (outline and call graph);
a method chain passes at 8 MiB and aborts at 7 MiB. **The driver gives each analyzer thread 32 MiB** (four times the larger): reserved
address space, not memory used until the stack grows. The test `tests/stack.rs` runs all four on a 32 MiB thread (`DRIVER_STACK`), and
`TS_STACK_KB=<n>` reruns them at another size; at 2 MiB all four abort, so the test can fail. If the stack grows linearly with the depth (not measured), 32 MiB holds about 40 000 levels of a method chain; tsrs gives its own threads 512 MiB, so a larger reservation costs nothing either.

## 8. Not scheduled

- **An anonymous class constructed in place** (found at H12, run): `new (class { constructor() { g(); } })()` is `Unresolved(no_declaration)`,
  and `g` is in no tree under the caller. The class is no def, and its constructor is an outline def of its own, a top-level `constructor`
  (`typescript-cli outline`: `Method constructor`), so the walk stops at it. Rule 8 holds at the site (an `Unresolved` edge, counted), and
  the constructor's own entry draws `g`. 0 on V-NOC and postcss (no `new (class` in either). A fix needs a rule, not a lookup: a `Call`
  to that constructor def, or a construction of a class with no def. Kept here until a project needs it.

- **A marker over a statement with two declarators** (found at D1's review, run): `/** ID: PAIR */ export const p = () => …, q = () => …;`
  gives `p` and `q` no marker at all, so the `ID:` line is in the file and on no def. A question for the marker rule (TS2, `markers.md`),
  not a bug of D1: whether the outline should say so (a marker that names nothing), and whether `plan_marker` can ever write one there.

- **`MarkerStyle<'static>` in `Options`** (finding 7). Both languages have it. A driver that uses one label for its whole life leaks
  one string once. Change both or neither.
- **The quadratic work per file** (finding 10). 0.1 s for 8 000 defs in one file, in a debug build. D1 computes one thing per file
  once (who owns a marker); `occurrence` can move next to it then, if a number ever asks for it.
