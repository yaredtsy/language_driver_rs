# Contributing

Read `CLAUDE.md` for the rules and `docs/README.md` for the plan. This file is the practical part.

## Setup

You need `rustup` and `git`. Nothing else for `core/` and the stub languages (Python's vendored ruff and the engines
that come later may need more; each `languages/<lang>/ENGINE.md` says).

`rust-toolchain.toml` at the root pins the compiler (1.99.1, with `rustfmt` and `clippy`). The first `cargo` command in
any folder of the repo installs it. A crate **outside** the repo does not see that file and uses your default toolchain.

There is no `Cargo.toml` at the repo root, on purpose: always `cd` into `core/` or `languages/<lang>/` first.

## A smaller clone

Python's vendored ruff is large, and a Rust contributor does not need it:

```sh
git clone --filter=blob:none --sparse https://github.com/<owner>/language_driver_rs
cd language_driver_rs
git sparse-checkout set --cone core scenarios scripts docs languages/typescript
```

Add `languages/_template` if you will run `scripts/new-language.sh`. The top-level files (`rust-toolchain.toml`,
`CLAUDE.md`, ...) are always checked out. Change the set later with the same command.

## The test loop

```sh
cd core && cargo test                                 # the shared crates
cd languages/<lang> && cargo test                     # one language
cd languages/<lang> && cargo test -p <lang>-syntax    # the fast crate only: must not compile the type checker
scripts/check-all.sh --static                         # cheap repo checks, seconds
scripts/check-all.sh [<lang>]                         # what CI runs: static, core, then every (or one) language
```

Before you push, on every crate you touched:

```sh
cargo fmt
cargo clippy -p <crate> --all-targets -- -D warnings
```

Pass `-D warnings` per crate, never for a whole workspace: the vendored ruff would fail it.

A change under `core/` must keep **every** language green. CI runs every language when `core/` changes, and so should you
(`scripts/check-all.sh`).

## Adding a fixture

One file per case under `languages/<lang>/crates/<lang>-syntax/tests/fixtures/`. Every row of a language's policy table
(`languages/<lang>/docs/policy.md`) has a fixture, and every tree rule has a scenario.

Snapshots (outline snapshots, `scenarios/*/expected.*.tree`) are compared, never rewritten, unless you ask:

```sh
ANALYZER_BLESS=1 cargo test -p <lang>-syntax        # writes the missing or changed snapshots
git diff                                            # then read every changed snapshot: this is the review
```

A snapshot you did not read is not a test. Never bless to make a failing test pass before you know why it failed.

## Adding a scenario

`scenarios/<name>/` holds the same program once per language (`python/`, `typescript/`, `rust/`) and the tree it must
give: `expected.default.tree` (the engine's default resolver) and, where context sensitivity changes the answer,
`expected.precise.tree`; or a single `expected.tree` where they agree. Write a `README.md` that names the rule it pins.
Write the default tree **by hand**, from the rule, not from the analyzer's output.

## Adding a language

```sh
scripts/new-language.sh <name>      # copies languages/_template; builds as is
```

Then fill `languages/<name>/ENGINE.md`. The folder is the registry: `scripts/list-languages.sh` and CI pick the new
language up with no other edit.

## Bumping an engine

A change of its own: only the pin in the language's `Cargo.toml`, its `Cargo.lock`, `sem.rs` and `ENGINE.md`. Update the
pinned commit in `ENGINE.md`, regenerate its list of engine items we call, and move your reading clone to the same commit
(next section) so the code you read is the code you build.

## Recreating `../language_clone/`

Upstream engines live **next to** this repo, never inside it, for reading only. Nothing is built or edited from them.
Check out each at the commit the repo pins (`languages/<lang>/ENGINE.md` is the source of truth; these are the pins at the time of writing):

```sh
mkdir ../language_clone && cd ../language_clone
git clone https://github.com/oxc-project/tsrs.git          && git -C tsrs checkout 13a31a4
git clone https://github.com/rust-lang/rust-analyzer       && git -C rust-analyzer checkout 2a362a4
```

Paths like `../language_clone/tsrs/...` in the docs are reading hints that only work with this arrangement.

## CI

`.github/workflows/ci.yml` starts on every pull request and push to `main` and skips jobs by what changed
(`scripts/changed-languages.sh`). `ci-ok` is the one required check: it passes when every job passed or was skipped.
The design and its reasons are in `docs/01-monorepo/05-ci.md`.
