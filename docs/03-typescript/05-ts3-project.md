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
pub enum OpenError { NoSuchFile, ConfigError(String) }
```

This is the Python crate's `open_project` for TypeScript. `OpenError` is one of the two places an analyzer may return an error (rule 6); a
project that loads with type errors is **not** an error.

## 2. Which files, which options

A TypeScript "project" is a `tsconfig.json`: it lists the files and the compiler options. A repository can have many (a monorepo with one per
package; a `tsconfig.json` and a `tsconfig.test.json` side by side).

| Situation | What `open` does |
|---|---|
| a `tsconfig.json` (or `jsconfig.json`) is found above `file`, and it includes `file` | load that one. This is the same upward search the editor does; `tsrs_lint/src/engine.rs` (`find_tsconfig`) is the code to follow |
| the nearest tsconfig does **not** include `file` (it has `"files": []` and `"references"`, a "solution" config) | follow its `references` to the one that does **[check: tsrs has project-reference support in `tsrs_compiler`; see how `tsrs_lint` handles it]** |
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
let mut opts = tsrs_compiler::ProgramOptions::new(config, host);
opts.checker_regions = true;                 // the checkers' memory can be given back (see §4)
let program: &'static tsrs_compiler::Program = tsrs_compiler::new_program(opts);
```

Three options to set on purpose, each with a line in `ENGINE.md` saying why:

| Option | Value | Why |
|---|---|---|
| `checker_regions` | `true` | without it the CLI "leaks its checkers" by design (the comment on the field says so) |
| `single_threaded` | on | our walk asks one question at a time (rule 11). Loading and parsing may still be parallel **[check what the flag covers]** |
| the number of checkers (`CompilerOptions::checkers`) | 1 | one checker sees every file; no question of which checker owns a node |

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
        // sem's own Drop calls tsrs_compiler::free_program and releases the loader regions
    }
}
```

The rules:

1. **A `Project` owns its program.** Dropping the `Project` frees it. Nothing else may keep a tsrs pointer.
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

If S-TS3 shows that `free_program` does not return the memory in a long-running process, the fallback is honest and simple: the driver runs
call-tree work in a **child process per project**, restarted when the project changes. The analyzer's API does not change.

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
| 4 | `is_project_code`, `package_of` | a fixture with a fake `node_modules/pkg` and a `.d.ts` |
| 5 | `Drop`, the lifetime on handles, the memory loop test | the test of §4.4 |
| 6 | solution configs and project references | a fixture with `references`. May be deferred to after TS4 if S-TS3 shows it is rare in your target projects |
| 7 | `environment_inputs` | returns `**/tsconfig*.json`, `**/jsconfig.json`, `**/package.json`, the lock files |
| 8 | `typescript-cli files <dir> <file>` | prints the loaded files and the config that was chosen: the debugging tool for "why is this call unresolved" |

Next: [`06-ts4-calls.md`](06-ts4-calls.md)
