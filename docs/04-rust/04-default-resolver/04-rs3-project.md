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
    /// What this load could not do as asked, for the driver to show the user (§6, D37). Sorted, each once.
    pub fn warnings(&self) -> &[Warning];
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
   finds the workspace root above a member, and that root **may** be above the manifest we found. **Above `root` it is loaded but not
   project code** (decided 2026-11-16, before step 2, as D33 for TypeScript): cargo needs the whole workspace to load a member, and reading
   it writes nothing, but `root` stays the limit of what is the user's. So `files()` lists only files under `root`, a def in a sibling
   member above `root` is `External` (§4's "under `root`"), and `manifest()` is the manifest we found, never a path with `..`. The lockfile
   rule (§5) looks beside the workspace's root manifest wherever it is;
3. no manifest below `root`: the file alone is the project, a one-file crate with its `mod` children (§6, the ladder's last rung), with
   the `NoManifest` warning. Its outline and markers work as always (they never needed a project).

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

**As built (RS3 step 3, 457a16a, reviewed 2026-11-17).** `sem/origin.rs`: `Owner::Project { file }` or `Owner::External { package, name }`,
crate-private until RS4 calls it.

- A def a `macro_rules!` of a **project** crate makes (`make!(generated)`) is in no file (its source is a macro file), so it is
  `External { package: <its crate> }`, as the table says. It has no line a marker could go on either. RS4 counts these if they matter.
- `name` is `<crate>::<modules>::<Type or Trait>::<fn>` (`alloc::string::String::len`); a method whose impl's self type is not a named type
  (`[T]`, `&T`) gets no type segment. **Provisional**: how an external target is named on the wire is RS4's to settle, and unlike a project
  def's qualname it is not stored in users' files, so it may still change.
- The `Library` row has no fixture (a registry or git source needs the network or a vendored registry); the rule is a table test.
  `fixtures/owner/` covers `Local` under the root, `Local` above the root (root at `app`), a macro-made def, and `Lang` (`String`).
- Paths are compared as engine text in one function (`paths::relative`): `/` separators, no `\\?\` prefix, the drive letter upper case.
  The engine also treats `C:/x.rs` and `c:/x.rs` as one path (`ra_ap_vfs` `vfs_path.rs`, `paths_encoding`). Python and TypeScript strip the
  prefix and do not fold the letter. **Not yet run on Windows**: the `windows-latest` job is the proof.

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

**Recommendation: B, with A as the fallback** for a toolchain older than 1.97, reported by the `LockfileWritten` warning (§6) so the
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

#### Measured: a load without dependencies (owner, 2026-11-16; measured in step 1, D38)

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

**Result (RS3 step 1, 2026-11-16, `ENGINE.md` "In-project edges by load mode"): D loses every cross-package edge** (45 of 45 on `core/`,
16 of 16 on `typescript/`, 7 of 7 on the outside project) and some `std`-typed same-crate ones (13, 57, 0). The [check] above is answered:
a call from member A into member B is `Unresolved` under `no_deps`. Worse than the table above says, `CargoConfig::no_deps` also reaches
the sysroot's own `cargo metadata`, so `std`'s crates lose their links too. **So B, with A as the fallback (D38)**; D is not built.

#### As built (RS3 step 1b, ccb699e, reviewed 2026-11-16)

- **Which lock.** Cargo keeps one `Cargo.lock` beside the workspace's root manifest, not a member's. `sem::load` asks
  `cargo locate-project --workspace` for it (one fast `cargo` call per load) and falls back to the manifest's folder when cargo cannot say.
- **The version.** `cargo --version` in the manifest's folder, with `RUSTUP_AUTO_INSTALL=0` (`toolchain.rs`, reused by step 5). A
  pre-release counts as older than its release, as in semver: `1.97.0-nightly` gets A, `1.98.0-nightly` B; a `+build` suffix is ignored;
  an unreadable version gets A.
- **Either guess is safe.** Whether cargo wrote a lock is decided after the load on every path (`!had_lock && lock.exists()`), so a
  toolchain that ignores the variable on B, or honours it on A, ends in the same place: no lock, or a lock that is reported
  (`LockfileWritten`, step 5). The version rule only avoids a useless variable.
- **The temporary folder** is a small `TempDir` (no `tempfile` crate) whose `Drop` deletes it when `load` returns, on every path. A folder
  that cannot be made falls back to A.
- Tested on 1.99.1 only: a no-lock fixture and a lock made stale at run time are byte-identical after `open`, and no temporary folder is
  left. The stale case passes either way, because the engine's own copy already protects a project that has a lock. The A path
  (`wrote_lockfile`) needs a toolchain below 1.97 and is not run here; it is covered by the version rule's table test, and by §7's
  old-toolchain measurement in step 5.

## 6. Errors and warnings (D37)

**The rule (owner, 2026-11-16): if the user's Rust is complete and recent, load normally; if not, load with what there is and say so.**
A missing piece is a **warning** the driver shows the user (a banner in V-NOC), not a failure. Only a file that is not there is
an error: with no Rust at all, the crate graph is read from `Cargo.toml` by us (the ladder, below). This replaces the first plan's "a load without `std` is never returned as a success": it still is never returned *silently*.

```rust
#[non_exhaustive]
pub enum OpenError {
    NotUnderRoot,               // the file does not exist, cannot be read, or is not inside `root`: there is nothing to load
}

#[non_exhaustive]
pub enum Warning {
    NoStdSource { toolchain: String },                              // no rust-src: loaded without the standard library
    OldToolchain { found: String, minimum: String },                // older than the measured minimum, nothing newer installed
    ToolchainSubstituted { wanted: String, used: String, why: Why },// Why::NotInstalled | Why::TooOld
    DependenciesMissing { message: String },                        // `cargo metadata` failed; loaded with `--no-deps`
    LockfileWritten,                                                // cargo wrote a Cargo.lock into the project (§5, Q-RS6)
    NoToolchain,                                                    // no cargo: the crate graph was read from Cargo.toml by us (the ladder, rung 3)
    ManifestUnreadable { message: String },                         // cargo and our reader both failed on a Cargo.toml (rung 4)
    NoManifest,                                                     // no Cargo.toml up to the root: the file alone (rung 4)
}

impl Warning {
    pub fn code(&self) -> &'static str;   // "no_std_source", "old_toolchain", …: stable, for the driver and Stats
}
```

| What the user has | What `open` does | What the user is told | What is missing from the tree |
|---|---|---|---|
| a complete, recent toolchain | loads | nothing | nothing |
| no `cargo` at all (no Rust installed) | loads from **our own reading of the `Cargo.toml` files**, with no `std` (the ladder below, rung 3) | `NoToolchain`: "Rust is not installed: only calls that do not go through the standard library are shown" | every call whose answer needs `std` or a dependency (as `NoStdSource` and `DependenciesMissing` together). What stays: functions called by path (`helper(x)`, `crate::db::save()`, `Order::new()`), and methods on a value whose project type is known without `std` (`self.validate()`, `Order::new().submit()`, `let o: Order = …; o.submit()`), across the workspace's own crates |
| no `Cargo.toml` up to the root, or one nobody can read | the file alone, as a one-file crate with its `mod` children (rung 4) | `NoManifest` or `ManifestUnreadable` | calls into other files of the package that this file does not reach by `mod` |
| no `rust-src` | loads without `std` | `NoStdSource`: "install the Rust source (`rustup component add rust-src`) to see all calls" | project method calls whose receiver's type comes through `std` (`parse()?.submit()`, `for x in v.iter() { x.run() }`). A project function called by path still resolves |
| a toolchain older than the minimum (§7's measurement), a newer one installed | loads with the **newer** toolchain's `cargo` and `std` (`RUSTUP_TOOLCHAIN` in `extra_env` and for `sem::load`'s own `rustc --print sysroot`) | `ToolchainSubstituted { why: TooOld }`: "your project uses Rust 1.74; calls were read with 1.99" | little for in-project calls: `std`'s types change slowly. **[check]** in the §7 measurement |
| the same, nothing newer installed | loads with the old one | `OldToolchain`: "Rust 1.74 is older than 1.82: some calls may be missing" | whatever the measurement found for that version |
| `rust-toolchain.toml` names a toolchain that is not installed | loads with an installed one (the default first) | `ToolchainSubstituted { why: NotInstalled }`: "Rust 1.50 is not installed; calls were read with 1.99" | as above. **Never installs anything** (the engine sets `RUSTUP_AUTO_INSTALL=0`; §5's rule against `Sysroot::discover` stays) |
| offline, or a dependency that cannot be fetched | the engine's `--no-deps` retry | `DependenciesMissing`: cargo's message | project methods on values whose type comes from a dependency, and **every call between the workspace's own crates** (measured under `no_deps` in step 1: all cross-package edges; the retry uses the same `--no-deps` metadata, `std` stays). A single-package project loses only the first |
| no `Cargo.lock`, and the lock could not be redirected | loads | `LockfileWritten`: "a Cargo.lock was created in your project" | nothing |

- Each warning is also copied into every graph's `Stats.language` as `warning_<code>: 1`, the way `skipped()` is (§3), so a tree with
  holes says why even when it is read without the `Project` (rule 8: every limit that cuts is marked and counted).
- `Display` of each gives the sentence of the table; the driver can show it as it is or by `code()`.
- How a warning reaches the screen is the driver's (`vnoc/drivers/rust/`); this crate only returns the data (rule 1, rule 2).

### The ladder: load with what there is (D37)

`open` tries each rung in order and stops at the first that loads. Each rung below the first adds its warning.

| Rung | When | How | Engine call |
|---|---|---|---|
| 1 | `cargo` runs and `cargo metadata` succeeds | as §2–§5 | `ProjectWorkspace::load` |
| 2 | `cargo` runs, `cargo metadata` fails, `--no-deps` works | the engine's own retry | the same (S-RS3) |
| 3 | no `cargo`, or `cargo` cannot read the manifest but the TOML parses | `sem::load` reads the `Cargo.toml` files itself and builds the crate graph: each member's crate roots by Cargo's documented layout (`src/lib.rs`, `src/main.rs`, `src/bin/*.rs`, `[lib] path`, `[[bin]] path`, `autobins`), its edition (with `edition.workspace = true`), the path dependencies between members, `cfg(test)` on, the default features as `cfg(feature = …)`. No sysroot. What the reader does not understand (a target layout it does not know, `[patch]`, a `build.rs`'s generated module) is counted, `layout_skipped` | `ProjectWorkspace::load_inline(ProjectJson, …)`, sysroot `None` |
| 4 | no `Cargo.toml` up to the root, or none that parses | the requested file is the crate root; its `mod` children come with it | the same, one crate |

Rung 3 is an approximation of what cargo would say, written from Cargo's documented rules, so it never guesses a name: a member it cannot
lay out is skipped and counted, never invented (rule 8). It needs a TOML parser in `rust-analysis` (the `toml` crate; not an engine crate,
so rule 4 does not apply).

**To check before building it** (RS3 step 7): **[check]** `load_inline` with sysroot `None` and **no `rustc` on `PATH`**: the engine asks
`rustc` for the target's `cfg` and data layout (`QueryConfig::Rustc`); does it fall back or fail? **[check]** that `self.m()`,
`Self::new()`, `Order::new().submit()` and a call into another member resolve with no `std` (expected: yes; the receiver's type is known
without `std`). **[measure]** in-project edges at rung 3 against rung 1 on the three projects: a fourth column of §5's table. S-RS3's only
number so far (no `std`, on `core/`, all calls): 634 of 978 calls and 74 of 914 method calls resolved.

**How each condition is found** (S-RS3, `ENGINE.md`): `sem::load` never uses `RustLibSource::Discover` (it runs `rustup component add
rust-src` by itself). It reads the version with `cargo --version` in the manifest's folder, finds the sysroot with `rustc --print sysroot`
and passes it as `RustLibSource::Path` (built in step 1: the engine then looks for `lib/rustlib/src/rust/library` itself, without
`rustup`, and a missing one is `sysroot.error()`; `sysroot_src` stays `None`); it loads through `ProjectWorkspace::load`, then reads
`sysroot.error()` and `ProjectWorkspaceKind::Cargo { error }` before `load_workspace`. The installed toolchains for a substitution come from
`rustup toolchain list` (**[check]** the form without `rustup`: then no substitution, and `OldToolchain`, or rung 3 of the ladder when there is no `cargo`).

These are the only errors `rust-analysis` has. Everything after `open` returns `Option` (rule 8).

## 7. Tests

| Test | Proves |
|---|---|
| `open` on `core/` of this repo, from a file in `analyzer-core/src/` | a virtual workspace loads; `files()` is exactly the `.rs` files under `core/*/src` and `tests` |
| `open` with `root` = the repo root and a file in `languages/rust/` (and one in `core/`) | the right one of five workspaces is chosen; `manifest()` says which. The Rust workspace also lists `core/`'s files: `analyzer-core` is a path dependency under the root, so project code (§2's table). Not `languages/typescript/`: its `cargo metadata` needs `tsrs`'s git repository, so Rust's CI would fetch another language's engine (decided 2026-11-16, step 2 review) |
| a fixture with a workspace, a member, a path dependency inside and one outside `root` | §4, row by row |
| a fixture with `build.rs` and one with a derive | `skipped()` counts them |
| a lone file (no `Cargo.toml`): loads as a one-file crate, `NoManifest` | §2, §6 rung 4 |
| `NoToolchain` (rung 3) and `NoStdSource` as warnings on a successful load: with `PATH` and `RUSTUP_TOOLCHAIN` set for the test's child process. **No `cargo` needs an empty `HOME` as well**: the engine falls back to `~/.cargo/bin/cargo` (S-RS3) | the errors of §6. Marked `#[ignore]` if they cannot run on CI's images, and then run by hand and the result written in `ENGINE.md` |
| a fixture with no `Cargo.lock`, and one with a stale lock: after `open`, the folder is byte-identical (under B; under A's fallback, `warnings()` holds `LockfileWritten`) | §5, the lockfile; rule 2 |
| **the old-toolchain measurement**, once, by hand: a small project (a project method reached through `Option::unwrap` and through `Vec::iter`) loaded with each installed toolchain (1.74.1, 1.94.1, 1.98.0, 1.99.1): loads?, `std` found?, both calls resolved?; and the same project loaded with 1.74 substituted by 1.99. The table goes in `ENGINE.md`; it sets the minimum. Afterwards the old toolchains are uninstalled (about 4 GB) | the minimum of §6, and that a substitution loses nothing in-project |
| the minimum check: a table test of the version comparison (`1.81.0` → warn, `1.82.0` → none, a version cargo prints with a suffix, an unreadable one) | pure code; needs no old Rust |
| one test per row of §6's table that CI can make (no `rust-src` with a `RUSTUP_TOOLCHAIN` that lacks it; a toolchain that is not installed; offline with a missing dependency) | the warning and its `code()`; `Stats.language` holds `warning_<code>` |
| load twice, drop, in a loop of 20: memory is flat **[measure]** | ordinary ownership should make this trivially true; it is one cheap test that says so |

Loading is slow (seconds). The tests of this crate share loads: one `#[test]` per fixture workspace, many checks inside it.

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | the engine dependencies (again `cargo update -p unicode-properties --precise 0.1.4`, `ENGINE.md` S-RS1); `sem::load`; `Project::open` on a manifest path; the in-project measurement of §5 (B, D, no `std`), then the lockfile rule Q-RS6 picks | `core/` loads; the table in `ENGINE.md`; the lockfile test. **Done but the lockfile rule** (f4064e4; table in `ENGINE.md`): the measurement picked B (D38) |
| 1b | the lockfile rule (§5, D38): `cargo --version` in the manifest's folder (step 5 reuses it); from 1.97 and only when the project has no `Cargo.lock`, B (`CARGO_RESOLVER_LOCKFILE_PATH` in `extra_env` to a temporary folder, deleted before `open` returns); below 1.97, A. A's `LockfileWritten` warning comes with `Warning` in step 5; until then the load records that it wrote one | §7's lockfile test: a fixture with no lock and one with a stale lock are byte-identical after `open` (on the installed 1.99.1); a table test of the version rule (`1.96.x` → A, `1.97.0` → B, a suffixed version, an unreadable one → A). **Done** (ccb699e, as built in §5) |
| 2 | finding the manifest from a file; the root limit | the five-workspaces test. **Done** (0364d27; UTF-8 fix 98cf4d3): `open(root, file)` without `opts` until step 4; errors are `OpenError::Load` until steps 5 and 7; pure path functions in `paths.rs` |
| 3 | `files`, relative paths, project code (§4) | the fixture of §4; green on Windows in CI. **Done but Windows** (457a16a; test scope 193402f; as built in §4): the Windows job has not run, nothing is pushed |
| 4 | `skipped`, `LoadOptions`, `environment_inputs` | fixtures |
| 5 | `OpenError` and `Warning` (§6, D37), from S-RS3's texts; the old-toolchain measurement and the minimum; the substitution | §7's error and warning tests; the table in `ENGINE.md` |
| 7 | the ladder's rungs 3 and 4 (§6): our own `Cargo.toml` reader into `ProjectJson`, the one-file crate; its checks first | a fixture workspace loaded with no `cargo` and no `rustc` on `PATH` and an empty `HOME`: the in-project calls of §6's row resolve, `NoToolchain` is set; a lone `.rs` file; §5's table with rung 3 |
| 6 | `ENGINE.md`: what `sem::load` calls, the `attach_db` rule, the measured load times | reviewed |

Q-RS3 (`sem` as a folder) is answered before step 1: this is the phase where `sem` gets its second kind of work.

Next: [`05-rs4-calls.md`](05-rs4-calls.md)
