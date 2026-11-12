# Engine: python

ruff and ty, **vendored** (a folder in this repo, not a git dependency). It is a copy with one small patch.

- Upstream URL: https://github.com/astral-sh/ruff
- Pinned commit / version: `ac201b86aa37da5140ed716ebd4eca36d1fe32da` (2026-09-02, ruff 0.16.5). Vendored on 2026-11-11, **byte for byte from pylspt's
  commit `670ae79a15`** (`git archive 670ae79a15 vendor`), not from upstream again. The pylspt hash is the one to read when asking "what did we start from".
- Engine crates used: the 26 crates in `vendor/ruff/crates/` (the closure of `ty_project` and `ty_python_semantic`).
  `python-syntax` (fast): `ruff_python_parser` only. `python-analysis` (slow, PY5): the ty crates through `sem.rs`.
- Patches: **one**, see below.
- How to bump: see below. An engine bump is a change of its own.
- Licence notice: ruff and ty are MIT, Copyright (c) 2022 Charles Marsh and the ruff contributors (`vendor/ruff/LICENSE`).
  `ruff_annotate_snippets` carries its own `LICENSE-APACHE` and `LICENSE-MIT`, and the bundled typeshed has
  `ty_vendored/vendor/typeshed/LICENSE`. All three stay in the tree unchanged. A binary that links `python-analysis` embeds typeshed.

## What was removed from the copy

The copy is the commit's `vendor/` tree minus four directories of test data (42 MB to 23 MB, 3,733 files to 1,747). Nothing else was removed or
edited by the copy commit (`git diff` of the copy commit against a plain `git archive` shows exactly these):

| Removed | What it was |
|---|---|
| `crates/ruff_python_parser/resources/` | the parser's valid / invalid / inline test programs |
| `crates/ruff_python_parser/tests/snapshots/` | the parser's `insta` snapshots |
| `crates/ty_python_semantic/resources/mdtest/` | ty's Markdown tests |
| `crates/ty_python_semantic/resources/corpus/` | ty's corpus test programs |

**Kept on purpose:**

- every `tests/*.rs`. The parser's `[[test]] fixtures` and ty's `[[test]] mdtest` have no `path`, so Cargo looks for `tests/fixtures.rs` and
  `tests/mdtest.rs` when it reads the manifest;
- `crates/ty_python_semantic/resources/lint_docs/`: 132 `#[doc = include_str!("../resources/lint_docs/…")]` attributes (one per lint) read it **at build time**.
  It looks like test data and is not.

The vendored crates' own tests are not run here (their fixtures are gone). The proof that the engine works is our tests, not ruff's.

## The patch

Rust privacy is why there is one: ty's most useful functions are `pub(crate)`, so no code outside the crate can call them.

| Edit | Where | Why |
|---|---|---|
| 1. NEW, 203 lines | `crates/ty_python_semantic/src/types/analyzer_ext.rs` | `pub` wrappers over private ty accessors (MRO, member lookup, the callable's parts, type arguments). Its header states the rules: one wrapper per need, one internal call each, only public types in signatures |
| 2. EDIT, one line | `crates/ty_python_semantic/src/types.rs` | `pub mod analyzer_ext;`, next to the other `pub mod` lines |

It was `types/vnoc.rs` in pylspt. The rename is the second PY1 commit, so `git diff <copy commit> <rename commit> -- languages/python/vendor` is
the patch and nothing else. The compiler catches a wrapper whose signature broke after a bump; only a test catches one whose behaviour
changed, so PY5 adds one test per wrapper. They are in `crates/python-analysis/src/sem.rs` (`attrs_tests`), not in a file of `tests/`: the repo's rule 4 says only `sem.rs` names the engine, and these tests must. Where each is: `class_mro` (the `mro` tests), `member` and `instance_of` (`member_*`), `widen`, `type_of_definition`, `bound_method_self`, `type_args` and `callable_parts` (one test each, on `tests/fixtures/classes/wrappers.py`). `callable_definition` is tested only for its `None` cases: its positive case, a callable ty made from a function such as `property(f)`, is exercised only by the fixture programs, so a changed answer there would show in the 41 tests and not in a direct one. A bump re-runs all of them first.

## Pins that must match, and two traps

When `python-analysis` starts depending on the engine (PY5), its entries in `[workspace.dependencies]` come from pylspt's `Cargo.toml` at `670ae79a15`
(`git show 670ae79a15:Cargo.toml`). Two comments there must come with them, because the failure is silent:

- **salsa**: copy the `salsa` line (and `rustc-hash`, `thin-vec`, `uuid`, `serde`, `serde_json`, `anyhow`, `smallvec`) from `vendor/ruff/Cargo.toml`, never
  retype it, so Cargo's feature unification keeps exactly one salsa in the graph. Two copies are two incompatible `Db` traits.
- **zstd**: `ty_project` is `default-features = false, features = ["zstd"]`. `default-features = false` is what ruff's own root table says, and it is safe
  there only because ruff's `ty` binary re-enables zstd. We are that binary, so we re-enable it. Without it, decompressing the vendored typeshed breaks silently.

`ruff_db` is `default-features = false, features = ["os"]`.

## The lockfile

`Cargo.lock` started as pylspt's, at `670ae79a15`, so that the only thing that changed is our code. Cargo drops the entries nothing depends on yet:
pylspt's has 287 packages, this one has 90 (the parser's closure plus our five crates). Every one of the 90 third-party packages has the version pylspt's lock
has.

**At PY5, in this order**, so that no version is chosen again:

1. Put pylspt's lock back first: `git -C ../pylspt show 670ae79a15:Cargo.lock > Cargo.lock` (read by hash, never the working tree).
2. Add the ty dependencies to `[workspace.dependencies]` and build. Cargo keeps every locked version that still satisfies the requirements, and only
   adds our own crates and drops what is unused.
3. Compare the packages of the new lock with pylspt's: any third-party package whose version differs is a drift to explain, not to accept. The
   comparison should come out empty.

If the order is reversed (add the dependencies to the 90-package lock), Cargo has no pins for the ~197 dropped packages and takes the newest compatible
version of each, so drift becomes the expected result.

## The project root (D26)

`ProjectMetadata::discover(dir, system)` (`ty_project/src/metadata.rs`, `discover_with_uv_workspace`) walks **up** from `dir` through every
parent folder and takes as the root, in this order: the closest folder with a `ty.toml` or a `pyproject.toml` that has `[tool.ty]`; the uv
workspace root, if uv is on; the closest folder with any `pyproject.toml`; else `dir`. A config in a parent therefore moves the root, and
`Project::file("app.py")` then looks under the parent.

What `open_project` (`sem.rs`) does: it calls `discover`, and uses the answer only when `metadata.root() == dir`. When the root is a parent,
or `discover` fails, it uses `ProjectMetadata::new(<name of dir>, dir)`: the project `discover` itself returns when no folder has a config.

- **Spelling.** Checked in R1: `root()` is one of the `ancestors()` of the `dir` given, so it is cut from the same text and is equal to it when
  it is `dir` (`SystemPath` compares by components, so a trailing separator does not matter either). A parent is never equal to the longer path,
  so no `canonicalize` is needed in `open_project`. `Project::open` still canonicalizes once, before it.
- **An empty `ty.toml` is a valid config** and makes its folder the root (`tests/fixtures/09-root/above-ty`: red before the fix).
- **Known limit.** `dir` has a `pyproject.toml` without `[tool.ty]` and a parent has a `ty.toml` (or a `[tool.ty]` pyproject): the engine prefers
  the parent, we fall back to the default project, and `dir`'s `requires-python` is not read. No public function reads one folder only
  (`discover_in` is private). Rare.
- **Known limit.** A virtual environment in a parent folder (`<repo>/.venv`, with `services/api` opened) is no longer found: library types do not
  resolve. R1b is the step that gives it back.
- **uv.** `UseUv::from_system` (`ty_project/src/uv.rs`) reads the variable `TY_UV`: `1` or `true` is on, `scripts` is scripts only, anything else
  (and unset) is off. Off, `discover` runs no `uv` process. On, it runs `uv` once per `discover`, and the V-NOC driver opens the project on every
  request. A uv workspace root above `dir` is a parent root, so it is ignored like any other. Read, not run.

## Keeping the project open (PY9.0)

Measured 2026-11-12 at `33d65ca`, **debug build** (the `dev` profile: `opt-level = 1` for this workspace's crates, `opt-level = 3` for every
dependency, so the vendored engine is optimized and our own frames are not). A temporary `#[ignore]` test in the sem module
(`docs/05-python-migration/11-driver-needs.md` §7, PY9.0), run from the built test binary, each run twice and the second run kept (the OS
file cache warm); deleted after. Times in ms. "Jarvis" is `CallGraphGenerator.do_pass` in `jarvis.py`; "post" and "get_files" are
`FilesResource.post` and `get_files` in `app/api/file.py` of the Flask service. One request is `call_wire(&project, file, &entry,
&Options::default(), false)`.

**(a) Cold: a new `Project` for each request**, as the V-NOC driver does today.

| Entry | open 1 / 2 / 3 | solve 1 / 2 / 3 |
|---|---|---|
| Jarvis | 1.3 / 0.8 / 0.8 | 88.9 / 85.9 / 84.5 |
| post | 1.2 / 0.9 / 0.9 | 185.7 / 174.5 / 173.4 |
| get_files | 1.4 / 0.9 / 0.9 | 107.4 / 94.0 / 119.5 |

**(b) Warm: one `Project`, three requests.** The three answers are equal (the serialized frames compared byte for byte).

| Entry | solve 1 / 2 / 3 | wire bytes | cold (median) ÷ warm (2 and 3) |
|---|---|---|---|
| Jarvis | 85.6 / 5.8 / 5.6 | 99 837 | ≈ 15× |
| post | 229.0 / 9.1 / 9.9 | 131 379 | ≈ 18× |
| get_files | 101.2 / 2.6 / 2.5 | 5 300 | ≈ 37× |

The first request on a kept project costs what a cold one does: what is saved is the engine's memoized work (parsing, imports, types),
and that exists only after one request.

**(c) The refresh, nothing changed:** `Files::sync_all` on the warm project of (b), three times, then one more request.

| Entry | `sync_all` 1 / 2 / 3 | the request after it | its answer |
|---|---|---|---|
| Jarvis | 0.10 / 0.05 / 0.05 | 5.7 | equal to (b) |
| post | 2.21 / 1.33 / 1.28 | 9.3 | equal to (b) |
| get_files | 2.30 / 1.63 / 1.50 | 2.8 | equal to (b) |

The project stays warm after `sync_all`. It costs more on the Flask service, which has more files the engine has looked up (its cost is one
`stat` per path the engine knows, `ruff_db/src/files.rs`); the number of paths was not printed.

**(c′) The refresh after one change**, on a copy of `tests/fixtures/06-wire/python/` in the temp folder: ask `main` of
`ids_and_classes.py`, rewrite the file so that `unmarked` calls a new `added`, ask again without `sync_all`, then `sync_all` and ask.
The same in all three runs (the largest numbers shown).

| Step | time | `added` in the answer |
|---|---|---|
| first request (a fresh project: builtins and the file are read) | 20.5 | no |
| the file rewritten, no `sync_all` | 0.1 | **no**: the old answer, from memory |
| `sync_all` | 0.05 | |
| the request after it | 0.4 | **yes** |

So the engine never reads the disk again on its own: without a refresh a kept project answers from the old text. With `sync_all`, only what
read the changed file is recomputed (0.4 ms, against 20 ms for the first request). A file created after the first request was not tried here
(PY9.2 (b), `11-driver-needs.md` §10 claim 2).

**Through the CLI (PY9.4).** `python-cli calls <dir> <file> --entry <entry> --wire --timing --repeat 3`, the debug binary, run twice on 2026-11-13
at the commit of PY9.4 (the second run is in the second line of each cell; refresh is `Project::refresh`, which includes `sync_all`). Times in
ms; the three answers were equal (the CLI checks it and exits 1 if not).

| Entry | refresh 1 / 2 / 3 | solve 1 / 2 / 3 |
|---|---|---|
| Jarvis | 0.04 / 0.16 / 0.13; 0.03 / 0.16 / 0.12 | 128.4 / 5.8 / 5.8; 88.5 / 5.7 / 5.5 |
| post | 0.05 / 2.19 / 2.19; 0.05 / 2.20 / 2.31 | 279.3 / 9.3 / 9.5; 167.7 / 9.7 / 9.0 |

They agree with (b) and (c) (solve 5.6–5.8 and 9.1–9.9). Refresh 1 is before the first request, when the engine knows no file yet, so it is
the cheapest; later ones are about as (c) says (Jarvis a little over it, 0.12–0.16 against 0.05–0.10).

**Memory.** Resident size from `ps` inside the test after (b), and the peak of the whole run from `/usr/bin/time -l`. The test starts at
5.8 MiB. The size after (b) is an upper bound of one warm project: the three cold projects of (a) were dropped before it, and the allocator
may keep their pages.

| Entry | resident after (b) | peak of the run |
|---|---|---|
| Jarvis | 60.0 MiB | 60.6 MiB |
| post | 78.4 MiB | 87.5 MiB |
| get_files | 61.5 MiB | 62.5 MiB |

**The stack.** The built `python-cli calls <dir> <file> --entry <entry>` on the main thread, its stack set with `ulimit -s` in a subshell; an
overflow ends it with exit 134 ("has overflowed its stack"). The chain programs are `def f0(): f1()` … `def fN(): pass`, `def main(): f0()`, in
a temp folder; the 256-level one reaches `max_stack` (the CLI prints `cut max_depth: 1`, core's name for `Cut::Stack`).

| Program | fails at | passes at |
|---|---|---|
| chain of 256 levels (to `max_stack`) | 2048 KB | **2052 KB** |
| chain of 128 levels | 1072 KB | 1088 KB |
| Jarvis | 192 KB | 224 KB |
| post | 272 KB | 288 KB |
| get_files | 128 KB | 256 KB (not narrowed) |

One level of the solver's recursion takes **about 7.5 KB** in this build ((2052 − 1088) ÷ 128); 2052 ÷ 256 ≈ 8.0 KB is the upper bound with
the fixed part included. A request that reaches `max_stack` needs **just over 2 MiB**, so it overflows a thread with Rust's default 2 MiB
(`std::thread::spawn`), and an overflow aborts the whole process, not the thread. The driver asks for **8 MiB**
(`std::thread::Builder::stack_size(8 << 20)`): 2 MiB rounded up to the next power of two is 4 MiB, and doubled for a margin, because a level of
real code (nested expressions in a body) can be larger than a one-line body's. A release build uses less.

**The longest stretch without a check** (`11-driver-needs.md` §10 claim 5). A temporary timer in `Solver::cost_limit` (reverted) kept the
longest time between two calls of it in one request, and the time from the last call to the end of `call_wire` (the render and the wire,
which no check covers). These runs were about twice as slow overall as the ones above (the machine was busier), so the numbers are high.

| Entry | longest gap, cold (4 requests) | longest gap, warm | after the last check, cold / warm |
|---|---|---|---|
| Jarvis | 34.4 / 35.0 / 34.8 / 95.6 | 1.05 / 1.77 | ≤ 36.1 / ≤ 11.7 |
| post | 14.0 / 14.7 / 63.1 / 13.0 | 0.32 / 0.31 | ≤ 27.4 / ≤ 6.7 |
| get_files | 4.6 / 4.8 / 4.6 / 4.9 | 0.29 / 0.28 | ≤ 2.6 / ≤ 1.7 |

A cancel checked in `cost_limit` would be seen within about 0.1 s at worst on a cold project, and within 2 ms on a warm one, plus the render.

## Environment inputs (PY9.3)

`Project::environment_inputs` lists the files whose change means "answers may differ" (`docs/05-python-migration/11-driver-needs.md` §5).
The environment among them is the one **the engine uses**, found with the engine's own functions (paths in `vendor/ruff/crates/`):

1. The config's `environment.python`, if any (`ty_project/src/metadata/options.rs` `to_program_settings`, line 214 is the `Ok(None)` arm that
   falls through to step 2). `MergedOptions::python_environment` (`metadata.rs:654`, public) makes the path absolute with
   `RelativePathBuf::absolute` (`metadata/value.rs:65`: against the config's folder, `~` and `$VAR` expanded) and calls
   `PythonEnvironment::new` (`options.rs:315-340`). We call that function, not a copy. If `new` fails (no `pyvenv.cfg`, a bad one), the
   engine continues without an environment; we still list the configured path, made absolute the same way.
2. Else `PythonEnvironment::discover(Some(root), system)` (`ty_site_packages/src/lib.rs:286`): `VIRTUAL_ENV` (299), a child `CONDA_PREFIX`
   (308), `<root>/.venv` (315), a base conda (335), then `python3` / `python` on `PATH` (340). We list the result only if
   `<sys_prefix>/pyvenv.cfg` is a file: a system Python has none.
3. `sys_prefix()` (`lib.rs:374`) is canonical, so an entry outside the root is the canonical path (on macOS `/private/var/...`, not `/var/...`).

**`.python-version` is not read.** `grep` for `.python-version` as a file name in `ty_project`, `ty_site_packages`, `ty_python_semantic`,
`ty_module_resolver`, `ruff_db` finds nothing: the only hits are the TOML key `environment.python-version` in messages and tests. The Python
version comes from that key, from `requires-python`, or from the environment's `pyvenv.cfg`.

**`TY_UV` is off unless set** (`ty_project/src/uv.rs:49`: only `1`, `true` or `scripts` turn it on). `discover` takes the setting from the
process environment, and `open_project` accepts no root but the one asked for, so D26 holds either way. `VIRTUAL_ENV`, `CONDA_PREFIX`,
`PATH` and `TY_UV` are variables of the driver's process, fixed for its life: not inputs.

The list is computed at open and at each reopen, not per request: `discover` looks `python3` up on `PATH`.

## A def found past an unknown base (D28)

The engine writes "found past a base it cannot see" as an intersection (`Type::Intersection`) whose positive parts are the def it found and
`Unknown`: `Model.find`, with `class Model(Hidden, Mixin)` and `Hidden` an unresolved import, is
`(bound method <class 'Model'>.find() -> Unknown) & Unknown` (R2b test, `sem.rs`). `Sem::likely_def` reads it with
`IntersectionType::iter_positive`, which is public (`ty_python_semantic::types`); nothing is added to the copy. `iter_negative` is
`pub(crate)`, so negative parts cannot be read from here: they are ignored. A method the class defines itself, before the unknown base,
is typed exactly (`bound method Model.own() -> Unknown`).

## Class summaries, CS2: objects coming back

Measured 2026-11-13 with the debug CLI, before = `99c60b3` (CS2a) and after = CS2b, the second of two runs, `--graph --timing`; nodes are the
unmerged wire's. Jarvis is `CallGraphGenerator.do_pass`, Flask is `FilesResource.post` in `app/api/file.py`, FastAPI is `APIRouter.include_router`
in `fastapi/routing.py`.

| Project | nodes | contexts | walks | unresolved | `ref_unresolved` | solve ms |
|---|---|---|---|---|---|---|
| Jarvis | 207 → 207 | 55 → 55 | 55 → 55 | 30 → 30 | 0 → 0 | 89 → 88 |
| Flask service | 266 → 266 | 70 → 70 | 70 → 70 | 81 → 81 | 3 → 0 | 170 → 170 |
| FastAPI | 436 → 458 | 68 → 70 | 68 → 70 | 27 → 29 | 14 → 0 | 138 → 132 |

The golden of Jarvis is `equal`. All 22 nodes FastAPI gains are one getter, `fastapi._compat.v2.ModelField.type_`, under callers that were already in
the tree: 20 under `v1.is_pv1_scalar_field` and `v1.is_pv1_scalar_sequence_field`, 2 directly under `analyze_param`.

**Inline mode draws them too.** `PYTHON_DUMP_GAP=1` on this entry (the `dump_graph` test, which has inline mode) gives summary 458 nodes, inline 1019,
`0 extra`: every node summary mode draws, these 22 included, is in inline mode's tree. Both modes draw them for the same reason, a branch the walk does
not narrow:

- `fastapi/utils.py` `create_model_field` returns a v1 `ModelField` (line 113) or, under `elif PYDANTIC_V2`, a v2 one (line 123), so the field the
  walk holds is either.
- `fastapi/_compat/main.py:187-192` `is_scalar_field` sends it to `v1.is_scalar_field` under `isinstance(field, may_v1.ModelField)`. The walk does not
  narrow by `isinstance`, so the v2 object goes down that branch too, to `_compat/v1.py:282` and `is_pv1_scalar_field`, whose reads `field.type_`
  (`v1.py:201-204`) are calls of the property at `_compat/v2.py:88-89`.
- The 2 under `analyze_param` are `field.type_` at `dependencies/utils.py:555`, a real path when pydantic v2 is installed.

So the 20 are edges Python does not take (a v2 field never reaches `v1.is_pv1_scalar_field`), and both modes agree on them. They are not new
imprecision: the `v1.is_scalar_field` caller was already in the tree before CS2b. They are what "no narrowing" costs.

## Toolchain

`vendor/ruff/rust-toolchain.toml` says 1.98.0 and ruff's `rust-version` is 1.96. It is ignored here (the repo root's `rust-toolchain.toml` decides,
1.99.1). Measured on 2026-11-11, macOS, `cargo build -p python-syntax`, cold target: **52 s**, no warnings, on 1.99.1.
The full ty closure has not been built in this repo yet (PY5).

## How to bump

An engine bump is a change of its own: only `vendor/`, `Cargo.lock`, `sem.rs`, and this file.

1. Clone upstream at the new commit into a temporary folder.
2. Compute the closure: every crate `ty_project` and `ty_python_semantic` depend on (`cargo metadata`). Replace `vendor/ruff/crates/` with those crates
   and copy upstream's `Cargo.toml`, `LICENSE` and `rust-toolchain.toml`.
3. Remove the four test-data directories above (and check that no new `include_str!` reads a directory you are about to remove:
   `grep -rn 'include_str!\|include_bytes!' vendor/ruff/crates --include='*.rs'`).
4. Re-apply the patch: copy `analyzer_ext.rs` back and re-add the one `pub mod` line. Keep the copy and the patch as two commits.
5. Copy the `salsa` line (and its siblings) from the new `vendor/ruff/Cargo.toml` into `[workspace.dependencies]`.
6. `cargo test`, then `cargo test -p python-syntax` alone: `cargo tree -p python-syntax` must still list no `salsa`, `ruff_db` or `ty_*`.
7. Re-check `sem.rs` against the new source and fix `sem.rs` only. Write the new commit, its date and ruff version here.

If upstream's `rust-toolchain.toml` moved, that is a separate step: bump the repo's toolchain first, and tell the driver repo (one toolchain).
At most once per release of the analyzer: ty changes weekly.
