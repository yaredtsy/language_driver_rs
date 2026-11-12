# 04.04.04 RS3: project loading

```
RS3
├── 1. the public type
├── 2. which workspace owns a file
├── 3. the three switches, and saying what was skipped
├── 4. what is project code
├── 5. inside: the database, a request, the handles
├── 6. errors
├── 7. tests
└── 8. steps
```

## 1. The public type

```rust
// rust-analysis/src/lib.rs
pub struct Project { /* the database, the Vfs, the workspace's facts */ }

impl Project {
    /// The Cargo workspace that owns `file` (relative to `root`, or absolute), searching no higher than `root`.
    pub fn open(root: &Path, file: &Path, opts: &LoadOptions) -> Result<Project, OpenError>;
    /// The project's own `.rs` files under the root, relative to it with `/`, sorted.
    pub fn files(&self) -> Vec<String>;
    /// The workspace's root manifest, relative to the root.
    pub fn manifest(&self) -> &str;
    /// What, if it changes, means "load again".
    pub fn environment_inputs(&self) -> Vec<String>;
    /// What this load did not do (§3). Copied into every graph's `Stats.language`.
    pub fn skipped(&self) -> &[(&'static str, u64)];
}

#[non_exhaustive]
pub struct LoadOptions { pub build_scripts: bool, pub proc_macros: bool }     // both false by default
```

The shape is `typescript-analysis`'s `Project`, on purpose: the driver's code for "open the project of this file" is then the same for
both languages.

## 2. Which workspace owns a file

The first plan had `open(root)`: "the workspace at or above `root`". This repo is the counter-example: its root has **no** `Cargo.toml`
and holds five workspaces (`core/`, one per language). So, as TypeScript finds the nearest `tsconfig.json`:

1. from `file`'s folder, climb to the nearest `Cargo.toml`, no higher than `root`;
2. hand it to the engine (`ProjectManifest::from_manifest_file`, then `ProjectWorkspace::load`), which asks `cargo metadata`; Cargo itself
   finds the workspace root above a member, and that root **may** be above the manifest we found but is never accepted above `root`;
3. no manifest below `root`: `OpenError::NoManifest`. A lone `.rs` file has no call graph in layer 1; its outline and markers work
   (they never needed a project).

| Case | Answer |
|---|---|
| a file in `src/bin/`, `examples/`, `tests/`, `benches/` | the package's manifest; each is a crate of its own in the engine, all loaded |
| a file no crate of the workspace includes (not reachable by `mod`) | `open` succeeds; `call_graph` on a def of that file is `None` (the engine has no body for it). `Project::files` lists only files the engine loaded |
| a `Cargo.toml` with `[workspace]` and no `[package]` (a virtual manifest) | fine: that is `core/` here |
| a path dependency outside the workspace but under `root` | loaded by the engine as a local crate; **project code** (§4) |

`environment_inputs`: `**/Cargo.toml`, `**/Cargo.lock`, `**/rust-toolchain`, `**/rust-toolchain.toml`, `**/.cargo/config.toml`,
`**/build.rs` when build scripts are on. Globs first, then exact files no glob covers, as TypeScript does.

## 3. The three switches

`LoadCargoConfig { load_out_dirs_from_check, with_proc_macro_server, prefill_caches, .. }`, all off by default
([`../01-what-rust-analyzer-gives.md`](../01-what-rust-analyzer-gives.md) §5c). What that costs must be visible (rule 8):

| Off | What is missing | Counted as |
|---|---|---|
| build scripts | code under `include!(concat!(env!("OUT_DIR"), …))`; `cfg`s a `build.rs` sets | `build_scripts_skipped`: the number of packages of the workspace that have a build script |
| proc macros | derives and attribute macros do not expand: no `impl` from `#[derive(…)]`, so a call to a derived method (`x.clone()` on a project type) has no project target | `proc_macros_skipped`: 1 when the workspace's dependency graph has any proc-macro crate |
| cache prefill | nothing; the first request is slower | not counted |

`Project::skipped()` holds them, and `call_graph` copies them into `Stats.language`, so a tree that is missing a derive's call says why.
`LoadOptions` lets the driver turn the first two on; the default stays off until S-RS2's numbers argue otherwise.

## 4. What is project code

| A def is project code when | Else |
|---|---|
| its crate's origin is `CrateOrigin::Local` (a workspace member, or a path dependency), **and** its file is under `root`, **and** the def is in the text (not inside a macro expansion) | `External { package: <crate name>, name: <path> }` |

`CrateOrigin::Library` (registry and git dependencies), `Lang` (`std`, `core`, `alloc`) and `Rustc` are external. "Under `root`" is the
same extra test TypeScript has: a file relative to the root is what a `DefInfo` holds.

## 5. Inside

```
Project                owns: RootDatabase (salsa) · Vfs (path ↔ FileId) · manifest · skipped
   │  borrowed for one request
   ▼
Sem<'p>                holds: &'p Project · hir::Semantics<'p, RootDatabase> · a per-request cache (file → Outline)
   │  hands out
   ▼
Func<'p>, RawSite      Copy handles (../02-architecture.md §4)
```

> **🦀 Rust note: a database and a borrow.** `RootDatabase` is rust-analyzer's whole knowledge of the project: file texts go in, and
> everything else (parse trees, name resolution, types) is computed on demand and remembered. `Semantics::new(&db)` is a thin helper that
> borrows it; it is cheap to make and holds caches of its own for the syntax trees it has handed out. One request makes one `Sem`, uses
> it, and drops it. Because `Sem<'p>` *borrows* the `Project`, Rust's ordinary rule applies: while a `Sem` is alive nobody can get
> `&mut Project`, so nobody can change a file under a running request. In RS5, "apply an edit" needs `&mut Project`, and the compiler
> forces the session to finish or drop the request first. That is the whole concurrency design of layer 1, and the borrow checker
> enforces it for free.

- `sem::load` is the only place that names `ra_ap_load_cargo`, `ra_ap_project_model`, `ra_ap_vfs`.
- Paths: the Vfs speaks absolute paths; `sem` converts to root-relative with `/` in one function, tested on Windows (TS3's verbatim-prefix
  fix, commit `8fe3f66`, is the bug to not repeat: compare paths as the engine spells them).
- Loading prints progress through a callback; `Project::open` takes none in layer 1 and passes a no-op. RS5 may add one.

## 6. Errors

```rust
#[non_exhaustive]
pub enum OpenError {
    NoManifest,                 // no Cargo.toml between the file and the root
    NoCargo,                    // `cargo` cannot be run
    NoSysroot,                  // no rust-src: the standard library's source is missing
    Toolchain(String),          // the toolchain a rust-toolchain file names is not installed
    Metadata(String),           // `cargo metadata` failed: the message, as cargo printed it
}
```

Each has a `Display` a user can act on ("run `rustup component add rust-src`"). The exact conditions come from S-RS3. If a missing
sysroot does not fail the engine's load, `sem::load` checks for it itself and returns `NoSysroot`: a load without `std` is never returned
as a success.

These are the only errors `rust-analysis` has. Everything after `open` returns `Option` (rule 8).

## 7. Tests

| Test | Proves |
|---|---|
| `open` on `core/` of this repo, from a file in `analyzer-core/src/` | a virtual workspace loads; `files()` is exactly the `.rs` files under `core/*/src` and `tests` |
| `open` with `root` = the repo root and a file in `languages/rust/` | the right one of five workspaces is chosen; `manifest()` says which |
| a fixture with a workspace, a member, a path dependency inside and one outside `root` | §4, row by row |
| a fixture with `build.rs` and one with a derive | `skipped()` counts them |
| `NoManifest` on a lone file | §2 |
| `NoCargo`, `NoSysroot`: with `PATH` and `RUSTUP_TOOLCHAIN` set for the test's child process | the errors of §6. Marked `#[ignore]` if they cannot run on CI's images, and then run by hand and the result written in `ENGINE.md` |
| load twice, drop, in a loop of 20: memory is flat **[measure]** | ordinary ownership should make this trivially true; it is one cheap test that says so |

Loading is slow (seconds). The tests of this crate share loads: one `#[test]` per fixture workspace, many checks inside it.

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | the engine dependencies; `sem::load`; `Project::open` on a manifest path | `core/` loads |
| 2 | finding the manifest from a file; the root limit | the five-workspaces test |
| 3 | `files`, relative paths, project code (§4) | the fixture of §4; green on Windows in CI |
| 4 | `skipped`, `LoadOptions`, `environment_inputs` | fixtures |
| 5 | `OpenError`, from S-RS3's texts | §7's error tests |
| 6 | `ENGINE.md`: what `sem::load` calls, the `attach_db` rule, the measured load times | reviewed |

Q-RS3 (`sem` as a folder) is answered before step 1: this is the phase where `sem` gets its second kind of work.

Next: [`05-rs4-calls.md`](05-rs4-calls.md)
