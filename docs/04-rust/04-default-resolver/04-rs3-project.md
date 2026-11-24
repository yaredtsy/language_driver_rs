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
├── 8. steps
└── 9. after the tag: the reader against cargo  (proposed, not scheduled)
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
| a file no crate of the workspace includes (not reachable by `mod`) | `open` succeeds; `call_graph` on a def of that file is `None` (the engine has no body for it). **`Project::files` lists only files in some crate's module tree**, on every rung (decided 2026-11-17, RS3 step 7 review; built in f1a35de with `Semantics::file_to_module_def`): the engine loads whole folders, so "loaded" would list a stray `.rs` file that no crate has, and a lone file's whole folder. This is what the driver needs: `files()` says which edits mean "reload" (as TypeScript's program files do), and an edit to a file no crate has changes no graph. Two consequences, accepted: a file under a `#[cfg(…)]` module that is off on this machine is not listed (it changes nothing here; turning the cfg on edits a listed file or a manifest, which are watched); and the first `files()` call costs seconds (2 to 3 s on `core/`), because the engine builds the crates' module maps, the same work the first call graph needs, so it moves the cost earlier and adds none |
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

**As built (RS3 step 4, 888c228, reviewed 2026-11-17).**

- `LoadOptions { build_scripts, proc_macros }`, `#[non_exhaustive]`, `Default` both off; `open(root, file, &opts)`.
- **`proc_macros` runs the build too**: the proc-macro dylibs come out of the build run (`cargo check`), so a derive cannot expand without
  it. With `proc_macros` on, `build_scripts_skipped` is not counted.
- **Where the build run writes: `<target-dir>/rust-analyzer`** (`TargetDirectoryConfig::UseSubdirectory`), the folder rust-analyzer in an
  editor uses, not the user's own `target/` build and its lock. It runs under the lockfile redirect (D38). This is the one place a load
  writes inside the user's tree, and only when the caller asks for a switch; the default load writes nothing. Rule 2 is about the source
  tree and the results; a build cache under cargo's target folder is what any cargo command makes. Rejected: a temporary folder (a full
  rebuild on every load).
- **Code that exists only at build time is its own plan, later (D39, owner 2026-11-17).** Generated code (a `build.rs`'s `OUT_DIR`, proc-macro
  output) is in no file of the user's tree, so no marker, doc or edit reaches it: it is treated like an external library (`External`, counted).
  The switches stay as built, off by default; where their build run writes is decided in that plan, not now.
- **A counter is listed only when it cut something** (absent when 0, as `Stats.language` counters are elsewhere, D28). `build_scripts_skipped`
  counts the **local** packages (`CrateOrigin::Local`: members and path dependencies, the project code of §4) that have a build script.
  `proc_macros_skipped` is 1 when the graph has any proc-macro crate and none could expand.
- **A build run that fails never fails the load**: the load goes on, counted as not run (`build_scripts_skipped`, and `proc_macros_skipped`
  when proc macros were asked for), and step 5 turns its message into a warning.
- `environment_inputs()`: the five globs, `**/build.rs` when the build ran, then the exact files no glob covers: the manifests of the local
  packages, the workspace's root manifest and its lock, when outside the root, spelled as the engine spells them (absolute, `/`), as
  TypeScript's `config_files` are. **Not** copied into `Stats.language` (corrected 2026-11-17: it holds counts, and these are strings;
  TypeScript does not copy its `config_files` either). `skipped()` is copied (RS4 §2).

### Which `cfg` set a load reads (D48, owner, 2026-11-20)

**Now:** `test` (since RS3, S-RS2) and **`debug_assertions`** are on, `miri` is off: the code a default `cargo build` / `cargo test`
compiles. On the cargo rung through `CargoConfig::cfg_overrides` (`CfgOverrides { global: CfgDiff::new(enable, disable) }`, the field
rust-analyzer fills from its `rust-analyzer.cargo.cfgs` setting; the engine applies it to every crate, as a dev build compiles the
dependencies with debug assertions too; `CfgDiff` comes from `ra_ap_cfg`, a direct dependency since then); on D37's reader and the lone
file through `manifest::host_cfgs()`, which **already** pushed `debug_assertions` (since RS3 step 7, 5fb73d8): before D48 the cargo rung
and the reader disagreed, and only the cargo rung changed (fa2f1e0, reviewed 2026-11-20; `tests/cfg.rs` on all three rungs: the two
live functions are defs with edges, the two that are off are `None`, and the entry's calls to them are `unknown_callee`). Not a
`LoadOptions` field yet.

**Later, not scheduled (look at it again when a user asks to choose):**

| Choice | What it would show | What it takes |
|---|---|---|
| what a debug build compiles | the default above | built |
| what a release build compiles | `debug_assertions` off: debug-only functions are no defs, debug-only statements no sites | a `LoadOptions` field; the same `CfgOverrides` with `debug_assertions` disabled |
| `miri` on / off | code written for the Miri interpreter, in place of its `not(miri)` twins | a `LoadOptions` switch; `miri` in the override's enable list |
| show all | every branch of every `cfg`: the Linux and the Windows version of a function, debug and release | **one load cannot give it**: the engine reads one `cfg` set, and code under a `cfg` that is off has no body for it (no def to enter, no site to resolve: `cfg_off.rs`). Either one load per `cfg` set, with the graphs joined (memory and time times the number of sets), or the off branches drawn from the syntax alone, unresolved and marked. A design of its own |

A change of the default changes which defs exist (a marker on a debug-only function), never a qualname, so it is not frozen by a tag;
it is a policy change (`policy.md`, row `cfg-off`).

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
  prefix and do not fold the letter. **Green on Windows** (the `windows-latest` CI job, reported by the owner 2026-11-17).

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
- Tested on 1.99.1 (and, since step 5a, A on 1.94.1: the lock is written and `LockfileWritten` said, §6): a no-lock fixture and a lock made stale at run time are byte-identical after `open`, and no temporary folder is
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
    BuildScriptsFailed { message: String },                         // a switch asked for the build run and it failed (§3; added in RS3 step 5a review)
    CargoFailed { message: String },                                // cargo is there and failed; our reader built the graph (rung 3; added in RS3 step 7 review)
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

**The checks, answered (RS3 step 7, 2026-11-17, `ENGINE.md` "Rung 3").** `load_inline` with no `rustc`, no `cargo` and an empty `HOME`
loads (the engine falls back to an empty cfg list and keeps the data-layout error); without `std` every call on a project type resolves,
across crates too; rung 3 loses 13% of `core/`'s in-project edges and 5% of `languages/rust/`'s against rung 1 (path first, RS4's
policy), all of them receivers typed through `std`. **So rung 3 is built**, and the reader writes, for each package:

| What | Rule |
|---|---|
| crates | one per target: the lib (`src/lib.rs` or `[lib] path`), `src/main.rs`, `src/bin/*.rs`, `[[bin]] path`, `tests/*.rs`. The lib's crate name is `[lib] name` if given, else the package name with `-` → `_` (read, not skipped: a wrong name breaks every path call into it). Bin and test crates may share names; that is fine |
| dependencies | the path dependencies between the project's packages, by their `package =` rename or key; a lib also gets its **dev**-dependencies (its own `#[cfg(test)]` code uses them); a `tests/*.rs` crate gets the package's lib and its dev-dependencies |
| edition | `edition`, or the workspace's with `edition.workspace = true`; 2015 when absent, as Cargo |
| source | absolute `root_module`; `source.include_dirs` = the whole package folder, `exclude_dirs` = its `target/` (a `#[path]` module outside `src/` is otherwise not loaded) |
| cfg | `test` comes from `set_test`. The **host cfgs**, from the analyzer's own build (`std::env::consts::{OS, ARCH, FAMILY}`, `target_pointer_width`, `target_endian`, `unix` or `windows`), and `debug_assertions`: with no `rustc` the engine has none, and `#[cfg(unix)]` code would be inactive. An approximation (the project is assumed to build for this machine), covered by `NoToolchain`'s sentence. The default features as `feature = "…"` |
| skipped and counted (`layout_skipped`) | `[patch]`, a registry or git dependency (no source to load), a `build.rs`'s generated module, a target layout the reader does not know. Never invented |

**As built (RS3 step 7, 5fb73d8, reviewed 2026-11-17).** `manifest.rs` (the reader; no engine type, `toml` crate) follows the table above.
It also reads `workspace = true` dependencies, optional ones the default features turn on, members globs and `exclude`; an unknown
edition skips its package. **A dependency without `package =` is named in code by its lib name, not by its key** (found while writing the
fixture). Counted as `layout_skipped`: `[patch]`, `[replace]`, registry and git dependencies, target-specific dependency tables,
`build.rs`, examples and benches, a `**` member pattern, a target whose file is missing, a package it cannot read. `sem/load.rs` asks cargo
first; on a failure the reader's crates go to `load_inline` (rung 3, sysroot `None`); if the reader fails too, the file alone (rung 4,
edition 2024 as `rust-syntax`'s fallback, the file's folder as its source). `OpenError::Load` is left only for what no rung gets past: a
path the engine cannot be given (not UTF-8), and the engine's own `load_workspace` failing on a description it accepted. With no manifest,
`Project::manifest()` is `""`.

- **`CargoFailed { message }`**, code `cargo_failed`: "cargo could not read the project, so it was read from Cargo.toml; the standard
  library and dependencies are not shown". Proposed by the coding session, accepted in review: cargo present and failing is not
  `NoToolchain` ("Rust is not installed" would be wrong), and `NoStdSource`'s advice (install `rust-src`) does not help. Without it the
  load would be silent about having no `std` (rule 8).
- **Known cost of rung 4**: the engine loads the lone file's whole folder, recursively, so a lone file at the top of a large tree with no
  `Cargo.toml` anywhere loads every `.rs` file under it. `files()` lists only the module tree (§2); the load time is not limited. Rare;
  revisit if a user meets it.
- **Later**: at rung 3 with `rustc` and `rust-src` present (cargo failed for another reason), a sysroot could be given in the
  `ProjectJson`, and `std` would resolve. Not built: `CargoFailed` says what is missing.
- Tests: ten reader table tests on an in-memory tree; `tests/ladder.rs` (a lone file with a `mod` child, a `Cargo.toml` that is not TOML,
  one only cargo refuses); a worker with no `cargo` and no `rustc` on `PATH` and an empty `HOME` that resolves §6's calls across two crates
  (one called by its `[lib] name`), the host's `#[cfg(unix)]` or `#[cfg(windows)]` call and a `#[path]` module outside `src/`, with
  `NoToolchain`; it keeps `SystemRoot`, `TMP`, `TEMP` for Windows. Green on Windows CI (2026-11-17).

**Every tool is found the way the engine finds it** (fix asked in the RS4 step 4 review, ENGINE.md "Finding a tool"): `cargo`, `rustc`
and `rustup` by the engine's `ra_ap_toolchain::Tool::prefer_proxy()` order with `PATH` moved before the variable (`$CARGO_HOME/bin`,
`PATH`, environment variable; 2c9e685, for a rustup whose proxies live only on `PATH`, as Homebrew's: ENGINE.md), asked through `sem`
(rule 4), never by the bare name. `prefer_proxy()` is the engine's rule whenever it has a sysroot (`Sysroot::tool`), so in every load with `std`; `Tool::path()`
(variable first) is its rule only without one, and under `cargo run` / `cargo test` the variable `CARGO` names a binary that reads no
`rust-toolchain.toml`. Otherwise a driver started with a short `PATH` loads with the engine's `cargo` and without `std`, and says "install
`rust-src`". Tests: a worker with `PATH` set to an empty folder and `HOME`, `CARGO_HOME`, `RUSTUP_HOME` kept loads at rung 1 with `std`
and no warning; the `toolchains.rs` workers keep `CARGO` and `RUSTC` as `cargo test` sets them. Built with `path()` in 8fb062c; changed to
`prefer_proxy()` after its review.

**How each condition is found** (S-RS3, `ENGINE.md`): `sem::load` never uses `RustLibSource::Discover` (it runs `rustup component add
rust-src` by itself). It reads the version with `cargo --version` in the manifest's folder, finds the sysroot with `rustc --print sysroot`
and passes it as `RustLibSource::Path` (built in step 1: the engine then looks for `lib/rustlib/src/rust/library` itself, without
`rustup`, and a missing one is `sysroot.error()`; `sysroot_src` stays `None`); it loads through `ProjectWorkspace::load`, then reads
`sysroot.error()` and `ProjectWorkspaceKind::Cargo { error }` before `load_workspace`. The installed toolchains for a substitution come from
`rustup toolchain list`, run in a neutral folder with `RUSTUP_AUTO_INSTALL=0` (in a folder pinned to a missing toolchain it installs it;
ENGINE.md "rustup facts") (**[check]** the form without `rustup`: then no substitution, and `OldToolchain`, or rung 3 of the ladder when there is no `cargo`).

These are the only errors `rust-analysis` has. Everything after `open` returns `Option` (rule 8). `OpenError` has `Display` and
`std::error::Error`, as Python's and TypeScript's do.

**As built (RS3 step 5a, b9e48bf, reviewed 2026-11-17).**

- `OpenError::NotUnderRoot` for a file that is not there, not a file, outside the root, or a root that cannot be used. `Load(String)` stays
  for what 5b and 7 replace: no `Cargo.toml` up to the root (rung 4), and a load the engine refuses (no `cargo`, a toolchain that is not
  installed, a manifest nobody can read).
- Filled in: `NoStdSource` (the engine's `sysroot.error()`, with the toolchain folder's name), `DependenciesMissing` (the engine's
  `--no-deps` retry, `ProjectWorkspaceKind::Cargo { error }`, cargo's message kept), `LockfileWritten`. The others are types until 5b and 7.
- **`BuildScriptsFailed { message }`**, code `build_scripts_failed`: "the build scripts failed, so code they generate and derives are not
  shown: {message}". Proposed by the coding session, accepted in review: a switch the caller turned on that did not work is a thing the
  user must be told, and §3's counters alone do not say why.
- **A sysroot nobody can find is `NoStdSource` too** (`toolchain: "unknown"`) when the load still succeeds: `rustc --print sysroot`
  failing must not give a load without `std` and no warning (rule 8). 5b and 7 then give the case its precise warning.
- Tests: `fixtures/offline/` (`[net] offline = true` in its `.cargo/config.toml`, a dependency in no cache) loads with
  `DependenciesMissing`, `std` resolves and the call into the sibling member does not (ENGINE.md, "In-project edges by load mode").
  `NoStdSource` is asserted against what the disk has. Two `#[ignore]` tests run a worker process with the toolchain in its environment
  (`cargo test` sets `RUSTUP_TOOLCHAIN` itself): a toolchain without `rust-src` (a copy of 1.99.1 without `lib/rustlib/src`) gives
  `NoStdSource`, and **1.94.1 writes the lock and gives `LockfileWritten`** (run by the reviewer too). That is §5 option A's first run.
- **The minimum is 1.82** (RS3 step 5b review, `ENGINE.md` "Old toolchains"): no in-project call was lost on 1.74.1, 1.94.1 or 1.98.0,
  so the minimum is set by the lockfile, not by calls. Below 1.82 cargo rewrites a stale committed `Cargo.lock` in place; a substitution
  to an installed toolchain from 1.82 avoids that. Not 1.97 (the redirect's line): it would substitute most users' stable toolchain on
  every load (this machine's default is 1.94.1) to avoid a file that `LockfileWritten` already reports.
- **`LockfileWritten` also covers a lock that existed and changed** (asked in the 5b review): with `OldToolchain` (below 1.82, nothing
  newer installed) cargo may rewrite a stale lock, and "a lock appeared" does not see that. The lock's bytes are compared before and after.
- **CI needs `rust-src`.** `rust-toolchain.toml` installs `rustfmt` and `clippy` only, so on CI every test that needs `std` (step 3's
  `String` rows, the offline test, all of RS4) would see no standard library. `rust-src` goes into its `components`.

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
| load twice, drop, in a loop of 20: memory is flat **[measure]** | ordinary ownership should make this trivially true; it is one cheap test that says so. **Measured** (fd06501, `tests/memory.rs`, `#[ignore]`, about 80 s, Unix): open `core/`, `files()`, drop, twenty times; RSS after drop 351 MB at run 2, 357 MB at run 20, 311 to 378 MB over all runs with no trend |

Loading is slow (seconds). The tests of this crate share loads: one `#[test]` per fixture workspace, many checks inside it.

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | the engine dependencies (again `cargo update -p unicode-properties --precise 0.1.4`, `ENGINE.md` S-RS1); `sem::load`; `Project::open` on a manifest path; the in-project measurement of §5 (B, D, no `std`), then the lockfile rule Q-RS6 picks | `core/` loads; the table in `ENGINE.md`; the lockfile test. **Done but the lockfile rule** (f4064e4; table in `ENGINE.md`): the measurement picked B (D38) |
| 1b | the lockfile rule (§5, D38): `cargo --version` in the manifest's folder (step 5 reuses it); from 1.97 and only when the project has no `Cargo.lock`, B (`CARGO_RESOLVER_LOCKFILE_PATH` in `extra_env` to a temporary folder, deleted before `open` returns); below 1.97, A. A's `LockfileWritten` warning comes with `Warning` in step 5; until then the load records that it wrote one | §7's lockfile test: a fixture with no lock and one with a stale lock are byte-identical after `open` (on the installed 1.99.1); a table test of the version rule (`1.96.x` → A, `1.97.0` → B, a suffixed version, an unreadable one → A). **Done** (ccb699e, as built in §5) |
| 2 | finding the manifest from a file; the root limit | the five-workspaces test. **Done** (0364d27; UTF-8 fix 98cf4d3): `open(root, file)` without `opts` until step 4; errors are `OpenError::Load` until steps 5 and 7; pure path functions in `paths.rs` |
| 3 | `files`, relative paths, project code (§4) | the fixture of §4; green on Windows in CI. **Done** (457a16a; test scope 193402f; as built in §4); the Windows job green, reported by the owner 2026-11-17. **RS3 is complete** |
| 4 | `skipped`, `LoadOptions`, `environment_inputs` | fixtures. **Done** (888c228; as built in §3), with two fixes asked in review: a failed build run must not fail the load, and `build_scripts_skipped` counts local packages |
| 5a | `OpenError` and `Warning` (§6, D37), from S-RS3's texts; the warnings CI can make | §7's error and warning tests. **Done** (b9e48bf; as built in §6), with fixes asked in review: `BuildScriptsFailed`, `NoStdSource` for a sysroot nobody can find, `OpenError`'s `Display`, `rust-src` in `rust-toolchain.toml` |
| 5b | the old-toolchain measurement and the minimum; the substitution (`ToolchainSubstituted`, `OldToolchain`); then the old toolchains are uninstalled | §7's measurement table in `ENGINE.md`; the substitution test. **Done** (21f38e6, fixes fa4b54c 6cb8dce 42b3087 59eede1 8e9d313; table and rustup facts in `ENGINE.md`). **Minimum 1.82**, set from the measurement: no call is lost down to 1.74.1, so the line is the lockfile's. One fix asked in review: a rewritten existing lock is `LockfileWritten` too |
| 7 | the ladder's rungs 3 and 4 (§6): our own `Cargo.toml` reader into `ProjectJson`, the one-file crate; its checks first | a fixture workspace loaded with no `cargo` and no `rustc` on `PATH` and an empty `HOME`: the in-project calls of §6's row resolve, `NoToolchain` is set; a lone `.rs` file; §5's table with rung 3. **Done** (checks b658059, code 5fb73d8; as built in §6), with two changes asked in review: `CargoFailed`, and `files()` = the module tree (§2) |
| 6 | `ENGINE.md`: what `sem::load` calls, the `attach_db` rule, the measured load times | reviewed. **Done** by the reviewer as the steps went ("The engine items we call", S-RS2's rules, "RS3: loading a workspace"; 95b2d12 … this commit) |

Q-RS3 (`sem` as a folder) is answered before step 1: this is the phase where `sem` gets its second kind of work.

**RS3 is complete (2026-11-17, last code commits f4cedf4 `CargoFailed`, f1a35de `files()` = the module tree, fd06501 the memory test), except the proof that needs CI: the `windows-latest` job has not run** (path comparison, drive-letter fold, the no-tools worker's `SystemRoot`/`TMP`/`TEMP`).

## 9. After the tag: the reader against cargo (proposed 2026-11-24, not scheduled)

Rung 3's reader (`manifest.rs`) is "written from Cargo's documented rules" (§6), and its fixtures were written from the same reading.
So a rule we read wrongly is wrong in the reader **and** in its fixture, and both stay green. `layout_skipped` counts what the reader
knows it cannot do; it cannot count what the reader gets wrong without knowing. Nothing compares the reader with cargo itself.

The idea is from another project, [blaze](https://github.com/indexable-inc/blaze) (a Rust build tool that reads `Cargo.toml` itself; read
2026-11-24, its `docs/resolve.md`). Its reader was held to the text of `cargo metadata --no-deps` on 300 crates and 72 repositories. That
is its own claim, not ours to repeat; the method is what we take.

| # | Do | Proof |
|---|---|---|
| 8 | a test, `#[ignore]` like the sweeps (it needs `cargo` and real workspaces): for each workspace given, run `cargo metadata --no-deps --format-version 1` and the reader on the same folder, and compare what both describe: the member packages (name, manifest path), each one's lib, bin and test targets (crate name, root file, edition), and the path dependencies between members (with their `package =` renames). What the reader skips on purpose (registry and git dependencies, examples, benches, build scripts) is left out on both sides, and its count printed | the workspaces of this repo, and the copies of terminusdb and ruff the sweeps used: no difference, or each difference is a reader bug fixed with a fixture, or a new row of §6's "skipped and counted" list. `--no-deps` resolves nothing and writes no lockfile (§5), so it is safe on a copy |

> **🦀 Rust note: why `--no-deps` is the right thing to compare with.** `cargo metadata` normally also *resolves*: it picks a version for
> every dependency, which needs the registry and writes `Cargo.lock`. `--no-deps` skips that and prints only what the manifests say: the
> packages, their targets, and the dependencies each one **declares**. That is exactly the part our reader re-implements, so the two
> outputs describe the same thing and can be compared field by field.

**A crate for the field reading: `cargo-util-schemas`** (the owner's rule, 2026-11-23: an existing crate before hand-written code). It
is the cargo team's own serde types for `Cargo.toml`, published apart from the `cargo` crate, and blaze reads manifests through it. It
would replace the part of `manifest.rs` that picks fields out of a `toml::Table`. It would **not** replace the layout rules
(`src/bin/*.rs`, `autobins`, the lib's name), which live in cargo itself and not in that crate, so most of the reader stays either way.
It is a new dependency (not in `languages/rust/Cargo.lock` today), and its version follows cargo's. So: step 8 first; the crate only if
step 8 finds bugs in how fields are read. **[check]** before proposing it: that it builds on 1.99.1 and what it adds to the lockfile.

Next: [`05-rs4-calls.md`](05-rs4-calls.md)
