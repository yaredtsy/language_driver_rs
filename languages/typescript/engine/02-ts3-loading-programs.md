# TS3: loading programs

Part of `languages/typescript/ENGINE.md` (split out on 2026-11-24, text unchanged). Sections in the order they were written.

## TS3: what `sem.rs` found when it loaded programs

Measured and read on 2026-11-11, macOS, `typescript-analysis` at the pin above.

### Freeing a program: the region alone is not enough

`tests/memory.rs` opens and drops a three-file project 100 times (RSS by `memory-stats`, peak over rounds 11–100 against round 10).

| `Sem::drop` | RSS at round 1 | round 10 | round 60 |
|---|---|---|---|
| region only, no `free_unshared_program` | 20 MB | 115 MB | 349 MB (the 100-round test: 115 → 440 MB) |
| region entered, then `free_unshared_program` | 12 MB | 19 MB | 24 MB |

Why, from the source (`tsrs_compiler/src/program.rs`): `new_program` does `Box::leak(Box::new(program))`, and the file loader leaks the processed files, the
project-reference mapper and the resolution hosts the same way. They sit on the **global heap**, so a region does not hold them. `free_program` drops the
`Program` and its resolution host, and dropping the `Program` drops its checker pool, which frees the leaked slice of checkers (`PoolState::drop`);
`free_unshared_program` also frees the processed files and the mapper (`SharedProgramData::free`). None of the `Drop` impls in `tsrs_compiler` closes a
file or stops a thread (read: `grep "impl.*Drop for"`), so the cost of skipping the call is memory. Which of those boxes holds most of the 5 MB per
open was not measured.

So the plan (`docs/03-typescript/05-ts3-project.md` §4) was right that `Drop` frees the program, and the lint was in its way: `free_unshared_program` is an
`unsafe fn`, `unsafe_code = "forbid"` cannot be lifted locally. The TypeScript workspace is `deny`, with one `#[allow(unsafe_code)]` on `Sem::drop`
(`scripts/check-all.sh` fails on that attribute anywhere but a `sem.rs`). tsrs has no safe wrapper; its own users (`tsrs_lint`, `tsrs_execute`,
`tsrs_project`, `tsrs_api`) call the `unsafe fn` the same way. The call is made with the program's region entered, as in S-TS3.

### Loading a tsconfig

- A `tsconfig.json` with a **syntax error still parses**: `get_parsed_command_line_of_config_file` returns no diagnostics for it. Its errors live on
  `config.config_file.source_file.diagnostics()`, which `tsrs_lint`'s `find_tsconfig` also checks. `Sem::open` reports both as `OpenError::ConfigError`.
- The upward search for `tsconfig.json` (then `jsconfig.json`, per directory) **stops at the root** the caller gave. `tsrs_lint`'s `find_tsconfig` goes to the
  filesystem root; ours must not pick up a stray config outside the repository, and `files()` is relative to the root.
- A program loaded from a tsconfig that does not include the file has no source file for it. `open` says so (`ConfigError`) rather than answering for a
  file the program does not hold. A solution config (`"files": []` plus `references`) is TS3 step 6.

### Program options `Sem::open` sets on purpose

| Option | Value | Why, and what was measured (2026-11-11) |
|---|---|---|
| `checkers` (`CompilerOptions`, set on the parsed config) | `Some(1)` | one checker sees every file, so no question of which checker owns a node, and the same project answers the same way every time. Checked by `sem::tests::the_program_has_one_checker` (`program.options().checkers`); `CheckerPoolImpl::new` reads it |
| `checker_regions` | `true` | the checkers' memory goes back with the program (S-TS3) |
| `regions` | one `ProgramRegions` per `Sem` | the loaders' memory goes back with the program (S-TS3) |
| `single_threaded` | **not set** | it implies one checker, but it also makes the file loader sequential (`fileloader.rs`). On a Next.js app (1,549 files in the program, `node_modules` included), warm: **about 45 ms to open by default, about 160 ms with `single_threaded`**. Parallel loading is kept; `sem::tests::the_program_lists_its_files_in_the_same_order_every_time` opens the fixture 31 times and checks the program's file order never changes |

The plan (`docs/03-typescript/05-ts3-project.md` §3) listed `single_threaded` and `checkers = 1` as two options and left "what the flag covers" as a
`[check]`: it covers file loading and the checker pool, and `checkers = 1` is the part we need. Cold opens vary a lot (the same program: 126 to 444 ms
on a first run), so quote warm numbers.

### The inferred project (no config above the file)

Built with `tsrs_tsoptions::new_parsed_command_line` and the file as the only root; its imports and `require` calls pull in the rest (fixtures `plain-js`,
`plain-js-require`: an unimported sibling is not in the program). Options, from `tsrs_lint`'s set for files no config claims (`engine.rs`, the `unmatched`
branch), with the plan's changes: `allow_js` on, `check_js` off, `no_emit` on, `module` ESNext, `module_resolution` bundler, `target` ES2022, `jsx` react-jsx
(so JSX elements are call sites in a `.jsx` file), `allow_importing_ts_extensions`, `es_module_interop`, `resolve_json_module`. Left out of `tsrs_lint`'s set:
`source_map` (nothing is emitted), `strict_null_checks` / `strict_function_types` (not set; whether they ever change which signature a call resolves to was not checked), and
`allow_non_ts_extensions`: without it a `.txt` root is ignored, so `open` says `NotInProject("(inferred project)")` instead of reading prose as TypeScript.
Not checked: that the options above change a resolved call target compared with `tsc`'s own inferred-project defaults (TS4's fixtures will show it).

### Which files are the project's, and what package a file belongs to

`Sem::is_project_code` is true for source code **under the root** (D33, step R2): a TypeScript or JavaScript file (`Dialect::from_path`) that is not in
`node_modules` (or a file the program found by searching it: `is_source_file_from_external_library`), not a declaration file
(`tspath::is_declaration_file_name`: `.d.ts`, `.d.mts`, `.d.cts`; this includes the project's own `src/types.d.ts`, as the plan said), not a bundled library
(`is_source_file_default_library`; these live outside the root, and there are 91 of them in a one-file program), and whose path `relative_path` finds under
the root. The first four are `is_source_code`; a source file that fails only the last is `is_outside_root_source`: the program pulled it in by a relative
import (`../../shared/lib`), it is not walked, and a call into it is an `External` named by `place_of` (`../shared/lib.ts`) and counted in
`Stats.language["outside_root"]`. A `.d.ts` out there is `External` too and is not counted: it has no body. `Project::files()` lists project code only,
relative to the root. Fixture `outside-root`, `tests/outside_root.rs`.

Two checks keep that true at the door (step R3): `Sem::entry` answers `None` unless the file is project code (a file outside the root and a file in
`node_modules` have no entry, so `describe`'s `expect` holds), and `Sem::open` answers `OpenError::OutsideRoot` for a file that is not under the root once
`join` and `canonicalize` have resolved any `..`. Both use `project::under_root`, the one rule `relative_path` uses (the root's engine spelling, then a
`/`), so open and the analysis cannot disagree. Fixture `node-modules-source` holds a `.ts` file under `node_modules` that the program loads
(`"types": "index.ts"`), for the entry test.

`Sem::package_of` names the package for an `External` edge: the first segment after the **last** `node_modules/` (pnpm nests them), `@scope/pkg` with
its scope, **`@types/x` as `x` and `@types/scope__x` as `@scope/x`** (the package a types package describes: a policy, not an engine fact), and
`typescript:lib` for every bundled library. `None` for the project's own files. Fixture `with-node-modules` covers a plain package, a scoped one and an
`@types` one; the pnpm and `__` cases are unit-tested on strings only, not on a real pnpm tree.

A file with no package (not in `node_modules`, not a bundled library) is named by `Sem::place_of`: its path relative to the root with `/` when it is
under it (`src/types.d.ts`), else the path from the root with `..` (`../shared/api.d.ts`), never the machine's absolute path (D33, step R1). The `..`
spelling is `tspath::get_relative_path_from_directory` (`path.rs:979` at the pin), through the `tspath` re-export `sem` already imports
(`tsrs_core::tspath`, `pub use path::*`): **`[check]` answered, yes, it is there and callable.** It panics (an `assert!`) when exactly one of its two paths
is rooted; both are absolute here, so it cannot. A file on another Windows drive has no path from the root, and the function answers with the file's own
absolute path (`D:/shared/lib.ts`); `place_between` keeps that, and a unit test pins it on strings (not run on Windows). Names are compared as spelled,
with `use_case_sensitive_file_names: true`, as `relative_path` compares them.

**`[check]` letter case in `relative_path` (D33, 10-review-fixes §3.3):** the engine's file names do not come from asking the disk for their case. A file
under the root is named by the root string `sem` passes as the engine's cwd (`to_engine_path` of the `canonicalize`d root) plus the text of the import or
of the config's `include`, so the root prefix matches by construction and `relative_path` cannot fail on letter case for such a file. Run on macOS
(case-insensitive disk): every existing fixture finds its files under the root. Not run on Linux or Windows: CI's three systems are the check. A file the
engine reaches by its **real path through a symlink** (a package linked into `node_modules`, a symlinked folder under the root) is spelled by the engine
as it resolved it, which may not start with the root: that is left to the monorepo work.

### Two defs of one file with one marker (D34, step D1)

`FileDefs::owns_marker` says, once per file and marker style, which defs own their marker: the first def in source order (by where it starts) with each
text. `Sem::describe` gives `StableDef::Marked` to an owner and the `Path` id (file, qualname, occurrence) to a later def with the same text, so `b` is
named `b` and not `a`. The rule reads the file, not the request, so a def has one id whatever else the request reaches (`tests/duplicate_marker.rs`,
`b` asked for alone). The outline is unchanged: both report the marker. `TypeScript::describe` notes each such def's id, and `finish` counts the ones the graph
holds in `Stats.language["duplicate_marker"]` (only when above 0; `Breakdown.duplicate_marker`). Known: a copy above the original owns the marker. Across
files nothing is renamed (D2, `core/`).

### The file's own code as an entry (D32, M step 2)

`Entry::File` is a request for the **module**: a `Func` whose `at` is `At::Module`, not an index of the outline (rule 10: the outline has no row for it).
`Func` is `{ file, at: At }` with `enum At { Module, Def(usize) }`, both private, so no place can index `outline.defs` without saying what it does for the
module: `Func::outline_index()` is an `Option`. The lookups that are the same for both go through `Sem::node_of` (the def's node, or the source file's),
`kind_of` (`DefKind::Module` for the module) and `range_of` (the whole file); `Sem::def` is `None` for the module. `Sem::file_entry(file)` answers the
module, and only for project code (D33): a file outside the root, in `node_modules`, a `.d.ts` or not code has no file entry.

Its row (`describe`): kind `Module`, `qname` the file's path, `qualname` empty, occurrence 0, range the whole file (so a site's offset is its position in the
file), id `StableDef::Path { module: file, qname: "", nth: 0 }`. The wire root is `target_kind` `Module` with empty `target_qualname` and `target_id`; no
change in core.

Its body is the source file's statements, walked by the same `Collector` down to the next def (`body_of`, `Kind::SourceFile`): top-level calls, the calls in
anonymous functions at the top, and hand-overs. A side effect, as in a function that declares a class: the **computed member names** of a class at the top
are read by the module's walk, so they are drawn under the module (`top-class.ts`: one site moved from counted to drawn). The counters of a class's
definition code were untouched at this step: a file entry that declared a class also counted its decorators as
`uncovered_local_class_decorator_sites`. M step 4 (below) draws all of it and retires the counters.

The coverage test enters every project file with `Entry::File` too (`Sem::project_modules`); `top-level` and `top-level-anonymous` left `KNOWN_GAPS`, and
the per-file numbers of the table above moved from missed to drawn (`top-level.ts` 5, `iife.ts` 9, `jsx.tsx` 3, `tagged.ts` 4). A new outcome,
`Counted("merged_defs")`, is a site whose def lost its row to another def with the same id (D34 across files): `tests/fixtures/duplicate-marker-across`.
The sweep read 595 nodes in 42 programs before (with the new `file-entry` fixture, the test not yet entering files) and 599 in 43 after.

### Definition code in the declaring body (M step 4, `449dfb4`, `c6dbfd7`)

**The rule** (`09-module-def.md` §3.3, accepted 2026-11-17): code that runs when a class or an enum is **defined** belongs to the body that
declares it. That is the module for a top-level class or enum, the function (or method) for one declared there, and the namespace for one in a
namespace. It is drawn at its source position among that body's other sites. `Collector::collect` already stopped at a nested def (`def_of`)
and read a class's computed member names there. At the same stop it now also calls `class_definition_code`, the one function that reads what
runs at definition: the class's decorators and its members', the decorators of the constructor's and the methods' parameters, the `extends`
expression, the **static** field initializers and the static blocks. `body_of`'s class arm calls the same function and then adds the
**instance** field initializers. Those run at construction, so a construction still counts them in `uncovered_construct_initializer_sites`
(D25); the class's own body is read only for that (`initializer_sites`), so nothing is drawn twice. At an enum's stop the walk reads each
member's initializer. An enum entry draws nothing (`E` → `E`, pinned): its initializers are the declaring body's.

**Retired** with their `Uncovered` variants, their noting (`walk::construction`, `handed.rs`) and the span functions that fed only them:
`uncovered_class_definition_sites`, `uncovered_computed_member_name_sites`, `uncovered_local_class_decorator_sites`. They are not qualnames, so
rule 10 does not freeze them, but `typescript-v0.2.0`'s `policy.md` names two of them, `uncovered_computed_member_name_sites` and
`uncovered_local_class_decorator_sites` (the v0.3.0 tag notes say so); `uncovered_class_definition_sites` was added at TS5 step 3 and was in no tag. A class entry now counts only
`uncovered_construct_initializer_sites` and the `inherited_constructor_*` notes. Policy snapshots: six lose a counter line;
`counter-class-definition` (`newLocal ▸ make`) and `counter-local-decorators` (`declaresDecorated ▸ dec`) gain one node each. Fixture
`definition-code`, 6 tests in `tests/definition_code.rs`.

**Shapes checked by the reviewer** (a probe project, CLI, 2026-11-17). A static field arrow (`static h = () => g()`) is its own def,
`C.h`, so `g` stays in `C.h`, not in the module. A class expression in a static initializer is defined with its class: in `deep`,
`class L1 { static inner = class L2 { static w = d(); } }` draws `d`. Both a method that declares a class and a namespace draw that class's
definition code. `declare class` and `const enum` draw nothing. **An anonymous class expression** (`use(class { static s = a(); x = b(); m() {} })`)
is not a def: the walk goes through it as through an anonymous function, so the enclosing body draws `a` **and** `b`, its instance initializer
too. The methods of such a class are defs and keep their calls. This is older than M step 4 and follows the phase-1 rule for anonymous
functions; it is not pinned by a fixture yet.

**A class handed over in a decorator.** `@UseGuards(guard)` with `guard` a function draws `guard [Passed]` in the declaring body.
`@UseGuards(AuthGuard)` with `AuthGuard` a class draws `UseGuards`, the `Unresolved(decorator_result)` of its result, and counts
`uncovered_class` 1, as anywhere a class is named where a value is handed over (D22 step 5).

**`Breakdown`'s line** (`c6dbfd7`, no behaviour change): `Uncovered::label()` sits next to `counter()`, both exhaustive matches, and
`Uncovered::ALL` lists the 11 kinds in the order the line prints them, held to the enum by a test. The table of labels in `Display` is gone, so
a new kind cannot be left out of the line without a word.

**Measured** (reviewer, 2026-11-17, dev profile): TypeScript 285 pass, 5 ignored. V-NOC `TaskDetailPanel` is unchanged (141 call,
2 construct, 235 external, 86 unresolved, 18 passed, 90 contexts). The golden function entries (V-NOC `TaskDetailPanel`, postcss (a)
`Processor.process` and `Parser.parse`, (b) `Parser.parse` and `MapGenerator.generate`) are byte-identical in tree, wire, merged wire and
breakdowns. Their `.graph` dumps gain only `frontier: 0` (TS5 step 1) and `merged_defs: 0` (D2). `measure_coverage` is in the row 3c section.

### The proof that a `File<'p>` cannot outlive its `Sem`

`File<'p>` is `P<SourceFile>` plus `PhantomData<&'p Sem>`; `Sem::drop`'s `unsafe` rests on no `File` outliving it. The proof is a `compile_fail` doctest
(`typescript_analysis::proof`, `#[doc(hidden)]`): `drop(sem)` while a `File` is used later. A doctest is a separate crate, so the real `Sem` and `File` are
re-exported there (a wrapper function would prove only its own signature).

- **A first version used a wrapper (`Project::handle_for_doctest`) and was too weak:** with `Sem::file` changed to `fn file<'p>(&self, ..) -> File<'p>` (an unbound
  lifetime, the realistic mistake) the wrapper's own `-> FileHandle<'_>` re-tied the borrow, and the doctest still passed. On the real types the same mutation
  makes it fail, which is the check that it protects something.
- **`compile_fail,E0505` is not enforced by stable rustdoc**: pinning `E0499` instead still passed. With `RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS=-Zunstable-options cargo test --doc`
  the code is checked (the error is `E0505`, "cannot move out of `sem` because it is borrowed"; `E0499` failed). On stable the guard against "fails for the wrong reason" is
  the **twin** doctest, which differs only in the order of the last two lines and must compile.
- A `PhantomData<&'p ()>` in place of `PhantomData<&'p Sem>` also passes: it binds the borrow just the same through `Sem::file`'s signature, so it is not a bug.

### Thread safety of the handles (checked 2026-11-11)

Before this step the compiler said: `File<'static>` is `Send` and `Sync`, and so is `Sem` (so `Project` is). Nothing was wrong yet, because no handle can be used
without a `&Sem` and no method touches the checker, but the design is one checker answering one question at a time and tsrs enters its regions per thread, so
"a handle may cross threads" was a claim nobody had made. `File` now carries a `PhantomData<*const ()>` (neither `Send` nor `Sync`), and two `compile_fail`
doctests with compiling twins pin it (`typescript_analysis::proof`, run with `--features __proof` by `scripts/check-proofs.sh`; they failed before the marker was added and
fail again with it removed). **`Sem` (so `Project`) is `Send` but not `Sync`: decided at the start of TS4 and pinned** (a `Cell` marker and a proof pair; moving a `Project` between threads stays allowed, `tests/threads.rs`). Asking a `Sem` questions runs the
checker, and a shared `&Sem` would let two threads call into one checker. TS5 revisits it only if the driver needs parallel questions.

The proof module is behind the feature `__proof` because a `compile_fail` doctest is a separate crate that can only name public items. With the feature off rustdoc
finds no proof doctest at all and reports success, hence the script and its count file. Cargo unifies features across a build, so a crate that enables `__proof`
exposes `Sem` to everything built with it.

### Which config owns a file: solution configs and references (TS3 step 6)

`find_owner` ports `tsrs_lint`'s `find_tsconfig` (`engine.rs`), bounded by the root: the nearest config, then the configs it references (`resolved_project_reference_paths`),
then the config above, and so on; the first whose file list holds the file wins (`!file_names().is_empty()`, `file_names_by_path` contains it; a `composite` config must also match
by its own patterns, `possibly_matches_file_name`). `disableSolutionSearching` on the starting config stops the climb. Nothing holds it: `NotInProject` naming the nearest config.

- **The compiler API `tsrs_lint` uses is enough**: no project-reference support had to be written. The `[check]` in the plan (does `tsrs_compiler` support references?) is answered:
  yes, through the parsed config's references plus `ProgramOptions::use_source_of_project_reference`.
- **`use_source_of_project_reference = true` for a config program** (as `tsrs_lint` sets it; off for an inferred one). Without it a referenced project's files drop out of the program (the
  `references` fixture's `packages/lib/src/greet.ts` is gone), because a referenced project is expected to be built and its `.d.ts` output read instead; that output is often not there.
  With it on, a call from `app` into `lib` should reach `lib`'s source (a def, not an `External`): TS4's fixtures must confirm the resolved target.
- **Error policy (mine, not tsrs's):** a syntax error in the **nearest** config is a `ConfigError`; one in a config only reached through a reference or a climb is skipped, as `tsrs_lint`
  does (it may have nothing to do with this file). Skipping can hide a broken referenced config that did own the file; the person then sees `NotInProject`.
- **Real layout checked** on a Vite project on this machine (`tsconfig.json` with `"files": []` and references to `tsconfig.app.json` and `tsconfig.node.json`):
  `src/main.tsx` and `src/app.tsx` load the app config's program (`src/vite-env.d.ts` is left out of `files()`: a declaration file), `vite.config.ts` loads the node config's, and `tsconfig.json`
  itself is `NotInProject`. The fixture `solution` has the same shape. Not checked: the V-NOC frontend itself (its path is not in this repo), a pnpm workspace, `extends` chains.

### What a reload must watch, and what moves across threads (TS3 steps 7–8)

- **`ParsedCommandLine::extended_source_files()` lists the config files an `extends` pulled in**, so `Project::environment_inputs` can name `config/shared.json` exactly. The fixed
  globs alone would miss it (it is not named `tsconfig*.json`). Every config the owner search read goes in too, a skipped broken one included: fixing it changes the answer.
  Not covered: a file a config reads in some other way (`tsrs` content mappers), and `package.json` fields other than through the glob (a `types` entry is covered by `package.json`).
- **A `Project` opened on one thread and dropped on another works** (`tests/threads.rs`: 20 move-and-drop rounds, four projects alive at once, four threads opening at once, 100 rounds
  of open-on-one drop-on-another with flat memory; the memory test fails, 116 to 282 MB, with the free removed). So the free does not depend on the thread that made the program.
  `Project` is `Send` and, since TS4, not `Sync` (pinned).
- **A skipped config is reported, not silent** (`OpenError::NotInProject { config, skipped }`). It is not reported when `open` succeeds: a skipped broken config that would also have
  held the file is invisible then, except through `environment_inputs`. If that matters, `Project` needs a `skipped_configs()`.
- **The V-NOC frontend, opened through `Project::open`** (2026-11-11, `src/frontend` of the V-NOC repo, read only; debug build): `tsconfig.json` has `"files": []`, references to
  `tsconfig.app.json` and `tsconfig.node.json`, and its own `paths`. `src/main.tsx` and every other file under `src` get `tsconfig.app.json` (`"include": ["src"]`); `vite.config.ts` gets
  `tsconfig.node.json`; `shiki.bundle.ts` and `tailwind.config.js` are in neither config, so `NotInProject`, which is what `tsc` says too. Open takes 0.15 s warm (the whole `typescript-cli files`
  run, `node_modules` included). The program holds 397 files: 395 TypeScript (+ `src/vite-env.d.ts`, the one declaration file) and **two imported `.json` fixtures, which `files()` wrongly
  listed as project code**: fixed, `is_project_code` now needs a TypeScript or JavaScript file (`Dialect::from_path`), fixture `json-import`.
- **Not checked, still:** a pnpm workspace, and Windows paths in `find_owner` (`relative_to` and the visited set compare engine strings, which are `/`-separated everywhere; the
  `canonicalize` of the root is not).
