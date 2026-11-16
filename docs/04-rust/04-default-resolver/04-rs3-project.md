# 04.04.04 RS3: project loading

```
RS3
├── 1. the public type
├── 2. which workspace owns a file
├── 3. the three switches, and saying what was skipped
├── 4. what is project code
├── 5. inside: the database, a request, the handles, the lockfile
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
    /// The load had to let cargo write a `Cargo.lock` into the project (§5, the lockfile; Q-RS6).
    pub fn wrote_lockfile(&self) -> bool;
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

### The lockfile: a load must not write into the project (Q-RS6)

S-RS2 saw a load write a `Cargo.lock` into a fixture folder. Read in `ra_ap_project_model-0.0.358` (`cargo_config_file.rs`,
`make_lockfile_copy`; `cargo_workspace.rs`, `FetchMetadata`), that happens in one case only:

| The project | What the engine's `cargo metadata` does |
|---|---|
| has a `Cargo.lock`, toolchain ≥ 1.82 | the engine copies the lock into a temporary folder and points cargo at the copy (`--lockfile-path` from 1.82, `CARGO_RESOLVER_LOCKFILE_PATH` with `-Zlockfile-path` from 1.95, the variable alone from 1.97). The project's lock is not touched, even when it is stale (checked 2026-11-16, 1.99.1 and 1.94.1) |
| has a `Cargo.lock`, toolchain < 1.82 | no copy: cargo may rewrite a stale lock |
| has **no** `Cargo.lock` | the copy fails (it copies a file that is not there), so there is no copy, and cargo resolves and **writes a new `Cargo.lock` into the project** |

The sysroot is loaded apart, with `--locked` (the engine's comment: "Make sure we never attempt to write to the sysroot").

Rule 2 says a library crate never writes a file. The third row is not rare: a library that does not commit its lock, freshly cloned, and
every fixture of ours. The options:

| | What | Cost |
|---|---|---|
| **A** | accept: cargo writes the file, as every `cargo` command and every editor running rust-analyzer would | a stated exception to rule 2; a new untracked file can appear in the user's project |
| **B** | redirect: when the project has no `Cargo.lock` and its toolchain is ≥ 1.97, `sem::load` sets `CARGO_RESOLVER_LOCKFILE_PATH=<temporary folder>/Cargo.lock` in `CargoConfig::extra_env`. Cargo writes the new lock there, and the folder is deleted before `open` returns | a temporary file, of the same kind the engine's own copy already makes; three **[check]**s below |
| **C** | refuse: pass `--locked`. With no lock, `cargo metadata` fails, the engine retries with `--no-deps`, and every call into a dependency is `Unresolved` | a quiet half answer (rule 8) for a reason the user cannot see |

**Recommendation: B, with A as the fallback** for a toolchain older than 1.97, reported by `Project::wrote_lockfile() -> bool` so the
driver can tell the user. Two things need your yes (Q-RS6): that a temporary file deleted before `open` returns is not a "write" in rule 2's
sense (the rule is about the user's tree and the results), and the fallback.

To check before step 1, with S-RS2's loader:

**All three checked on 2026-11-16 (`ENGINE.md`, S-RS2, "Cargo.lock"):**

1. **Yes.** With `CARGO_RESOLVER_LOCKFILE_PATH` in `extra_env` on 1.99.1 the sysroot still loads (no error, 26 packages), and a project *with* a lock still uses the
   engine's copy: its lock stayed byte-identical (also when stale) and the redirect path stayed empty, so the engine's own value wins on that command.
2. **Yes.** With no lock, cargo created `<temp>/Cargo.lock` (1.99.1 and 1.98.0) and the project had no `Cargo.lock` afterwards.
3. **The project's toolchain**, from `cargo --version` run in the manifest's folder, so a `rust-toolchain.toml` there counts. 1.98.0 honours the variable, **1.94.1
   ignores it** (the lock was written into the project, also when `rust-toolchain.toml` chose 1.94.1 under another loader toolchain); 1.74.1 (< 1.82) makes no copy and rewrote a
   stale lock. The boundary 1.97 itself was not tested (not installed); `sem::load` reads `cargo --version` itself and applies B from 1.97.

#### Parked: measure a load without dependencies first (owner, 2026-11-16)

The owner's goal for the call tree is **calls inside the project**; calls into a dependency are not needed. That opens a fourth option:

| | What | Cost |
|---|---|---|
| **D** | load with `CargoConfig::no_deps = true`: `cargo metadata --no-deps` resolves nothing, needs no network, and **never writes a lockfile** (the engine's comment in `cargo_workspace.rs`), so Q-RS6 does not arise | the dependencies' types are unknown, so a project method called on a value whose type came from a dependency is unresolved. And the links **between the workspace's own crates** may be lost too: the engine builds a crate's dependencies from `metadata.resolve` (`cargo_workspace.rs`, the loop over `meta.resolve` nodes), which `--no-deps` leaves empty. **[check]**: a call from member A into member B, under D |

What each piece gives in-project calls, to say why the measurement is the one below:

| Piece | Needed for in-project calls? |
|---|---|
| dependencies downloaded | mostly not: only where a project value's type comes from a dependency (`let c = toml::from_str(s)?; c.validate()`) |
| the standard library (`rust-src`) | **yes**: types flow through `std` in project code (`?`, `for x in v.iter()`, `Box::new(p).run()`); without it those method calls are unresolved. A free function by path (`crate::utils::save()`) resolves without it |
| `cargo` | **yes**: which files form which crate, each crate's edition, which member depends on which |

**The measurement (RS3 step 1, before Q-RS6 is answered).** Load `core/`, `languages/typescript/` and the 232-package project of S-RS2
three ways: full (with B), `no_deps = true` (D), and without `std` (the S-RS3 case), and count, for each, **only the call edges whose
target is project code**, the cross-crate ones of a workspace separately. Write the table in `ENGINE.md`. Then:

- D loses no in-project edge, or only ones a counter can name: **D is the default** (faster, offline, no lockfile), and A/B are not built;
- D loses cross-crate or `std`-typed project edges: **B** as written above, and D stays what the engine already does offline (S-RS3, and a counter).

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

**S-RS3 answered (2026-11-16, `ENGINE.md`): it does not fail**, and two more things follow. (1) `RustLibSource::Discover` without `sysroot_src` runs
`rustup component add rust-src` on the user's machine; `sem::load` must not use it. It runs `rustc --print sysroot`, checks
`lib/rustlib/src/rust/library` itself (`NoSysroot` if absent) and passes it as `sysroot_src`. (2) The load also succeeds when a dependency is not
downloaded and `cargo metadata` fails (the engine retries with `--no-deps`); the message is only in `ProjectWorkspaceKind::Cargo { error }`. So
`sem::load` goes through `ProjectWorkspace::load` and reads `sysroot.error()` and that `error` before `load_workspace`. What to do with the second
(an error, or a counter in `Stats`) is a decision for RS3 step 1: the load did work, but every call into a dependency is `Unresolved`.

These are the only errors `rust-analysis` has. Everything after `open` returns `Option` (rule 8).

## 7. Tests

| Test | Proves |
|---|---|
| `open` on `core/` of this repo, from a file in `analyzer-core/src/` | a virtual workspace loads; `files()` is exactly the `.rs` files under `core/*/src` and `tests` |
| `open` with `root` = the repo root and a file in `languages/rust/` | the right one of five workspaces is chosen; `manifest()` says which |
| a fixture with a workspace, a member, a path dependency inside and one outside `root` | §4, row by row |
| a fixture with `build.rs` and one with a derive | `skipped()` counts them |
| `NoManifest` on a lone file | §2 |
| `NoCargo`, `NoSysroot`: with `PATH` and `RUSTUP_TOOLCHAIN` set for the test's child process. **`NoCargo` needs an empty `HOME` as well**: the engine falls back to `~/.cargo/bin/cargo` (S-RS3) | the errors of §6. Marked `#[ignore]` if they cannot run on CI's images, and then run by hand and the result written in `ENGINE.md` |
| a fixture with no `Cargo.lock`, and one with a stale lock: after `open`, the folder is byte-identical (under B; under A's fallback, `wrote_lockfile()` is `true`) | §5, the lockfile; rule 2 |
| load twice, drop, in a loop of 20: memory is flat **[measure]** | ordinary ownership should make this trivially true; it is one cheap test that says so |

Loading is slow (seconds). The tests of this crate share loads: one `#[test]` per fixture workspace, many checks inside it.

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | the engine dependencies (again `cargo update -p unicode-properties --precise 0.1.4`, `ENGINE.md` S-RS1); `sem::load`; `Project::open` on a manifest path; the in-project measurement of §5 (B, D, no `std`), then the lockfile rule Q-RS6 picks | `core/` loads; the table in `ENGINE.md`; the lockfile test |
| 2 | finding the manifest from a file; the root limit | the five-workspaces test |
| 3 | `files`, relative paths, project code (§4) | the fixture of §4; green on Windows in CI |
| 4 | `skipped`, `LoadOptions`, `environment_inputs` | fixtures |
| 5 | `OpenError`, from S-RS3's texts | §7's error tests |
| 6 | `ENGINE.md`: what `sem::load` calls, the `attach_db` rule, the measured load times | reviewed |

Q-RS3 (`sem` as a folder) is answered before step 1: this is the phase where `sem` gets its second kind of work.

Next: [`05-rs4-calls.md`](05-rs4-calls.md)
