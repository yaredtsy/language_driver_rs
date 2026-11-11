# 03.05 TS3: loading a project

```
TS3
├── 1. the public type
├── 2. which files, which options: tsconfig
├── 3. building the program
├── 4. memory: who owns what, and when it is freed
├── 5. what `sem` offers after TS3
└── 6. steps
```

## 1. The public type

```rust
// typescript-analysis/src/lib.rs
pub struct Project { /* private */ }

impl Project {
    /// Load the TypeScript / JavaScript project that owns `file`, searching upward from it for a tsconfig.
    pub fn open(root: &Path, file: &Path) -> Result<Project, OpenError>;
    /// The files of the loaded program, relative to `root`.
    pub fn files(&self) -> Vec<String>;
    /// File globs whose change means "load again": tsconfig files, package.json, lock files.
    pub fn environment_inputs(&self) -> Vec<String>;
}

#[non_exhaustive]
pub enum OpenError {
    NoSuchFile,
    ConfigError(String),        // no config found, or it cannot be read (a syntax error in it counts)
    NotInProject(String),       // the config loaded fine, but its program does not hold `file`; carries the config's path
}
```

This is the Python crate's `open_project` for TypeScript. `OpenError` is one of the two places an analyzer may return an error (rule 6); a
project that loads with type errors is **not** an error.

## 2. Which files, which options

A TypeScript "project" is a `tsconfig.json`: it lists the files and the compiler options. A repository can have many (a monorepo with one per
package; a `tsconfig.json` and a `tsconfig.test.json` side by side).

| Situation | What `open` does |
|---|---|
| a `tsconfig.json` (or `jsconfig.json`) is found above `file`, and it includes `file` | load that one. This is the same upward search the editor does, **but it stops at `root`**: a stray config above the repository is never picked up, and `files()` is relative to `root` (decided in TS3 step 2; `tsrs_lint` searches to the filesystem root); `tsrs_lint/src/engine.rs` (`find_tsconfig`) is the code to follow |
| the nearest tsconfig does **not** include `file` (it has `"files": []` and `"references"`, a "solution" config) | follow its `references` to the one that does; until step 6 is built `open` returns `NotInProject`. **Not optional**: the V-NOC frontend is this layout (`tsconfig.json` with `"files": []` and references to `tsconfig.app.json`), so without step 6 opening a file of it fails **[check: tsrs has project-reference support in `tsrs_compiler`; see how `tsrs_lint` handles it]** |
| no tsconfig at all (a plain JavaScript folder) | an **inferred project**: `allowJs: true`, `checkJs: false`, `noEmit: true`, module resolution `bundler`, root files = `file` alone; imports pull in the rest. Built with `tsrs_tsoptions::new_parsed_command_line` |
| `node_modules` is missing (the user did not install) | load anyway. Imports from packages do not resolve; calls into them become `Unresolved`. `Project` remembers this so that `Stats` can say why |

One `Project` is one program. A call that crosses into another package of the same monorepo resolves through `node_modules` or project
references to that package's **source** when the config says so (`use_source_of_project_reference`), otherwise to its `.d.ts`, which is
`External`. Phase 1 accepts that. A "workspace of several programs" is a TS5 or later question.

## 3. Building the program

Following `tsrs_lint/src/engine.rs`, lines 465–490 at the pinned commit:

```rust
// sem.rs — sketch; names are tsrs's
let config   = tsrs_tsoptions::get_parsed_command_line_of_config_file(&config_path, None, None, &host, None);   // (config, diagnostics)
let host     = tsrs_compiler::new_compiler_host(cwd, fs, &tsrs_vfs::bundled::default_lib_path(), None, None, None);
// all of it inside an entered Region::new_scratch(1 << 20), kept until the program is freed (§4, ENGINE.md S-TS3)
let mut opts = tsrs_compiler::ProgramOptions::new(config, host);
opts.checker_regions = true;                 // the checkers' memory can be given back (see §4)
opts.regions = Some(Arc::clone(&loaders));   // loaders: Arc<tsrs_compiler::ProgramRegions>, one region per loader thread
let program: &'static tsrs_compiler::Program = tsrs_compiler::new_program(opts);
```

Options set on purpose, each with a line in `ENGINE.md` ("Program options `Sem::open` sets on purpose") saying why and what was measured:

| Option | Value | Why |
|---|---|---|
| `checker_regions` | `true` | without it the CLI "leaks its checkers" by design (the comment on the field says so) |
| `regions` | one `ProgramRegions` per `Sem` | the loaders' memory goes back with the program |
| the number of checkers (`CompilerOptions::checkers`, set on the parsed config) | 1 | one checker sees every file; no question of which checker owns a node; same answers every time |
| `single_threaded` | **not set** | it implies one checker, but it also loads files one at a time: about 160 ms against 45 ms to open a 1,549-file program (warm). Parallel loading is kept; a test checks the program's file order is stable |

Type-checking the whole program is **not** needed to resolve calls: the checker computes types on demand for the nodes we ask about. So `open`
loads and binds, and does not call anything like "get all diagnostics".

## 4. Memory: who owns what

This is the part of TS3 that needs design, because of fact (a) in [`01-what-tsrs-gives.md`](01-what-tsrs-gives.md) §5.

```rust
pub struct Project {
    root: PathBuf,
    sem: sem::Sem,              // holds the &'static Program and the regions it was built in
}

impl Drop for Project {
    fn drop(&mut self) {
        // sem's own Drop drops its checker handles, calls tsrs_compiler::free_unshared_program,
        // then drops the loader regions and the program's region, in that order
    }
}
```

The rules:

1. **A `Project` owns its program.** Dropping the `Project` frees it: `Sem::drop` calls `tsrs_compiler::free_unshared_program`, an `unsafe fn`, with
   the region entered. **The region alone is not enough** (measured in step 2: 60 opens reach 349 MB without the call and stay at 24 MB with it; the
   `Program` and its processed files are `Box::leak`ed outside any region). So the TypeScript workspace sets `unsafe_code = "deny"` and `sem.rs` has the
   one `#[allow(unsafe_code)]`, on that `Drop` (`01-monorepo/02-workspaces-and-toolchain.md` §4). Nothing else may keep a tsrs pointer.
2. **`sem::Func` (and every other handle) is only valid while its `Project` lives.** The public API never returns one: `call_graph` takes
   `&Project` and returns a `Graph`, which is plain owned data.
3. **Inside the crate, handles carry the project's lifetime**, so the compiler checks rule 2 for us:

> **🦀 Rust note: giving a lifetime to something that has none.** tsrs's `P<Node>` is `'static`; the compiler will let it live forever. We wrap
> it and *add* a lifetime with `PhantomData`:
> ```rust
> use std::marker::PhantomData;
>
> #[derive(Clone, Copy, PartialEq, Eq, Hash)]
> pub(crate) struct Func<'p> {
>     node: tsrs_ast::P<tsrs_ast::Node>,          // really 'static, and unsafe to use after the program is freed
>     _project: PhantomData<&'p Sem>,             // "pretend I borrow from the Sem"
> }
>
> impl Sem {
>     pub(crate) fn entry<'p>(&'p self, file: &str, qualname: &str) -> Option<Func<'p>> { … }
> }
> ```
> `PhantomData<&'p Sem>` stores nothing (zero bytes). It only tells the borrow checker that a `Func<'p>` is tied to a borrow of the `Sem` it came
> from. Now `drop(project)` while a `Func` is still alive is a **compile error**, exactly as if `Func` held a real reference. This turns tsrs's
> "be careful" into the ordinary Rust guarantee, at the wall where it enters our code. pylspt gets the same thing for free, because ty's handles
> already carry `'db`.

4. **One test proves it:** open a project, build a graph, drop the project, 200 times; the process's memory at round 20 and round 200 must be
   about equal **[measure the tolerance in TS0]**.

S-TS3 answered it (`ENGINE.md`): `free_unshared_program` alone returned almost nothing (+28 MB per load, 528 MB after 15 loads); built inside
an entered region with `ProgramOptions::regions` set, as `tsrs_lint` does, memory was flat at 145–153 MB from load 10 to load 60 (about 40 MB
stays after the first free). So `Sem` owns a region, and the tolerance for test 4 is "no growth after round 10". The child-process fallback
(the driver runs call-tree work in a **child process per project**) is not needed; it stays the answer if a later tsrs pin regresses.

## 5. What `sem` offers after TS3

Only what TS4 needs; nothing speculative:

```rust
impl Sem {
    pub(crate) fn open(root: &Path, file: &Path) -> Result<Sem, OpenError>;
    pub(crate) fn files(&self) -> impl Iterator<Item = File<'_>>;
    pub(crate) fn file(&self, relative: &str) -> Option<File<'_>>;
    pub(crate) fn is_project_code(&self, file: File<'_>) -> bool;          // not node_modules, not .d.ts, not a bundled lib
    pub(crate) fn package_of(&self, file: File<'_>) -> Option<String>;     // "lodash", "typescript:lib" — for External
}
```

`typescript-analysis` depends on `typescript-syntax`, and uses its outline rule on the **already parsed** file so the two crates cannot
disagree about what a def is or what it is called:

```rust
// typescript-syntax: a second entry point for a caller that already has a tree (the PA plan does the same for Python)
pub fn outline_of(parsed: &sem::Parsed<'_>, style: MarkerStyle<'_>) -> Outline;      // pub, but #[doc(hidden)]: for typescript-analysis only
```

**[check]** whether `typescript-syntax`'s `Parsed` (made by its own `with_parsed`) and the program's `SourceFile` are the same tsrs type, so
that the walker can run on both. They should be: both come from `tsrs_parser`.

## 6. Steps

| # | Do | Proof |
|---|---|---|
| 1 | add the heavy tsrs crates to the workspace at the same `rev`; `typescript-analysis` skeleton with `sem.rs` | it builds; `typescript-syntax` still does not pull them in |
| 2 | `Sem::open` for the simple case: one `tsconfig.json` above the file | a fixture project of three files loads; `files()` lists them |
| 3 | the inferred project (no tsconfig) | a plain-JS fixture folder loads |
| 4 | `is_project_code`, `package_of`. **`File<'p>` is the first handle `sem` hands out, so it carries the lifetime (`PhantomData<&'p Sem>`) from the day it exists**, not as a later retrofit | a fixture with a fake `node_modules/pkg` and a `.d.ts` |
| 5 | prove the lifetime on handles (the lifetime itself lands with the first handle, step 4; `Func` in TS4 gets it too). `Drop` and the memory loop test were done early, with step 2: `tests/memory.rs`. **Must be green before TS4 adds `Func`**: `Sem::drop`'s `unsafe` rests on "no handle outlives the `Sem`", and only this makes the compiler check it | `drop(project)` while a handle is alive does not compile (a `compile_fail` doctest) |
| 6 | solution configs and project references | a fixture with `references`, shaped like the V-NOC frontend (`"files": []`, references to `tsconfig.app.json`). **Before TS4, not after**: it is the common Vite layout |
| 7 | `environment_inputs` | returns `**/tsconfig*.json`, `**/jsconfig.json`, `**/package.json`, the lock files |
| 8 | `typescript-cli files <dir> <file>` | prints the loaded files and the config that was chosen: the debugging tool for "why is this call unresolved" |

Next: [`06-ts4-calls.md`](06-ts4-calls.md)
