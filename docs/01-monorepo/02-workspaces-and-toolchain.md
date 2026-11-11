# 01.02 Workspaces and the toolchain

```
workspaces and toolchain
├── 1. why not one workspace for the whole repo
├── 2. one workspace per language: how it works
│   ├── the language workspace
│   ├── the core workspace, used by path
│   └── what a driver in vnoc writes
├── 3. one toolchain
└── 4. profiles and lints
```

## 1. Why not one workspace for the whole repo

A Cargo workspace has **one `Cargo.lock`**. Every crate in it must agree on one version of each dependency (within a semver range). The three
engines do not agree:

| | ruff + ty (vendored, at `ac201b8`) | rust-analyzer (at `2a362a4`) | tsrs (at `13a31a4`) |
|---|---|---|---|
| `salsa` | `0.28.2`, features `compact_str, macros, salsa_unstable, inventory` | `0.28.5`, features `rayon, salsa_unstable, macros, inventory, triomphe` | does not use it |
| minimum Rust | 1.98 | 1.98 | **1.99** (pins 1.99.1) |
| how we get it | a folder in the repo | crates.io | a git commit |

`salsa` is the problem. `0.28.2` and `0.28.5` are "compatible" for Cargo, so one lockfile would pick **one** of them and turn on the **union** of
both feature lists for both engines. pylspt's `Cargo.toml` already warns that ruff only builds against the salsa version it is locked to, and the
V-NOC Python driver is its own workspace for exactly this reason. Two engines that each track `salsa` closely, forced onto one copy, would break
on the day either one moves.

There are softer reasons too:

- `cargo test` at the root would build ty, the TypeScript checker and rust-analyzer together: tens of minutes from nothing.
- A contributor to one language would wait for, and could be broken by, the other two.
- Bumping one engine would change a lockfile every language shares.

So: **no workspace at the root** (row M2).

> **🦀 Rust note: what a workspace really is.** A workspace is a set of crates that share three things: one `Cargo.lock`, one `target/` folder, and
> the `[workspace.dependencies]` / `[workspace.lints]` tables. That is all. A git repo can hold several workspaces side by side; Cargo finds "the"
> workspace by walking up from the crate's folder until it sees a `Cargo.toml` with a `[workspace]` table. With no `Cargo.toml` at the repo root,
> `languages/python/` and `languages/typescript/` are two unrelated workspaces that happen to share a git history.

## 2. One workspace per language: how it works

### The language workspace

```toml
# languages/typescript/Cargo.toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
edition = "2024"
license = "Apache-2.0"
publish = false

[workspace.dependencies]
# shared, by path, out of this workspace
analyzer-core    = { path = "../../core/analyzer-core" }
analyzer-testkit = { path = "../../core/analyzer-testkit" }

# the engine: ONE commit, written once (ENGINE.md records why this one)
tsrs_parser   = { git = "https://github.com/oxc-project/tsrs", rev = "13a31a4…" }
tsrs_ast      = { git = "https://github.com/oxc-project/tsrs", rev = "13a31a4…" }
# … the other tsrs crates, the same rev

# this workspace's own crates
typescript-syntax   = { path = "crates/typescript-syntax" }
typescript-analysis = { path = "crates/typescript-analysis" }
```

Each crate then writes `tsrs_parser = { workspace = true }`. The commit hash exists in one file.

### The core workspace, used by path

`core/` is a small workspace of its own, so it can be built and tested alone (`cd core && cargo test`), with its own lockfile. A language reaches
it with a **path dependency that points outside its own workspace**.

> **🦀 Rust note: a path dependency into another workspace.** This is allowed. When you build `languages/typescript`, Cargo compiles
> `analyzer-core` as an ordinary dependency: it uses the **typescript** lockfile and the **typescript** `target/` folder, and ignores
> `core/Cargo.lock`. The core's own lockfile only matters when you build inside `core/`. Two consequences:
>
> 1. `analyzer-core` is compiled once per language workspace. It is small and has no engine in it, so that costs seconds.
> 2. Its dependency versions are resolved per language. So keep the core's dependencies few and boring: `serde`, `xxhash-rust`. Never `salsa`.
>
> Inside `core/`, `{ workspace = true }` lines resolve against `core/Cargo.toml`, wherever the build started. **[check]** with a two-crate
> experiment before B0 is called done: a member of `core/` that inherits a workspace dependency, built from `languages/typescript/`.

### What a driver in vnoc writes

```toml
# vnoc/drivers/typescript/Cargo.toml
typescript-syntax = { git = "https://github.com/<owner>/language_driver_rs", tag = "typescript-v0.1.0" }
```

Cargo clones the repo at that tag, searches it for a package named `typescript-syntax`, finds it under `languages/typescript/crates/`, and resolves
that crate's path dependency on `../../core/analyzer-core` inside the same clone. The driver's own `Cargo.lock` decides the versions. For local
work on both repos at once, the `[patch]` trick from the V-NOC plan (`06-drivers.md` §7) works unchanged, pointing at this folder.

**[check]** in B0: a throwaway crate outside the repo that depends on a crate in `languages/_template/` by `git` + `rev`, to prove the nested
layout resolves.

## 3. One toolchain

One `rust-toolchain.toml` at the repo root. `rustup` looks for that file in the current folder and then upward, so every workspace gets it.

```toml
[toolchain]
channel = "1.99.1"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

**Why 1.99.1:** it is what tsrs pins, and tsrs sets `rust-version = "1.99"`, so nothing older can build it. ruff and rust-analyzer ask for 1.98 or
newer, and a minimum is only a minimum.

What this changes elsewhere (row M5):

| Place | Today | After |
|---|---|---|
| V-NOC plan row R9, "one toolchain, 1.98.0" | 1.98.0 | **1.99.1**. The idea (one version everywhere) stands; the number moves |
| `vnoc/rust-toolchain.toml` | 1.94.1 | unchanged until `vnoc` itself needs more. Each driver is its own workspace with its own toolchain file, so `vnoc/drivers/typescript/` pins 1.99.1 without touching the core |
| `vnoc/drivers/python/rust-toolchain.toml` | 1.98.0 | may stay, or move to 1.99.1 with the Python migration |

**[check]** in B0: the vendored ruff builds on 1.99.1. A newer compiler can add warnings; ruff's crates are path dependencies here, and Cargo
shows warnings for path dependencies (it hides them only for registry and git ones). That is noise, not failure, as long as CI does not turn
warnings into errors for `vendor/`. So CI's "deny warnings" is applied per crate (`-p python-syntax -p python-analysis`), never to the whole
workspace.

Upgrading the toolchain is a pull request of its own that changes one file, runs every language's tests, and says so. tsrs's `docs/RUST.md`
("Upgrading Rust") is a good model.

## 4. Profiles and lints

Cargo reads `[profile.*]` only from the workspace root being built, so each language workspace sets its own. Two lines matter in every one:

```toml
[profile.dev]
opt-level = 1                 # our code: quick to build, not painfully slow

[profile.dev.package."*"]
opt-level = 3                 # the engine: an unoptimized type checker is too slow to test against
```

pylspt already does this for ty; the same is true of the TypeScript checker and rust-analyzer.

Lints: `[workspace.lints]` cannot be inherited across workspaces, so the shared rules live in the two files Cargo and Clippy **do** read from parent
folders, `rustfmt.toml` and `clippy.toml` at the repo root, and each language's `Cargo.toml` carries the same short `[workspace.lints]` block,
copied from `_template`. `scripts/check-all.sh` fails if a language's block differs from the template's.

The block is short on purpose:

```toml
[workspace.lints.rust]
unreachable_pub = "warn"          # `pub` means "part of the public API", nothing else (PA plan, step 2)
unsafe_code = "forbid"            # our code has no reason to be unsafe; the engines are a different matter

[workspace.lints.clippy]
dbg_macro = "deny"
todo = "deny"
```

Do not copy tsrs's lint list. It is tuned for a mechanical port of a Go compiler with its own arenas; most of it would be noise here.

Next: [`03-what-is-shared.md`](03-what-is-shared.md)
