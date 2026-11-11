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
