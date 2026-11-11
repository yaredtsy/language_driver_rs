# 05.01 Python: what changes against the earlier plan

```
what changes
├── 1. the earlier plan in five lines
├── 2. what stays exactly as planned
├── 3. what changes because the destination is a monorepo
├── 4. what changes because of analyzer-core
└── 5. the PA rows, one by one
```

The earlier plan is `pylspt/fable_plan/` (the **PA plan**, 13 files, written 2026-11-11). It moves pylspt into a new repo called
`python-analyzer`. This folder is a **migration guide on top of it**: read the PA plan for the reasons and the detail, and this folder for where
things go now. I did not edit the PA plan.

## 1. The earlier plan in five lines

1. pylspt was a playground; the polished code moves to a clean repo; the old repo is frozen and archived.
2. Nothing enters unpolished: six rules (no product words, never writes files, chosen API, tests in CI, no personal paths, builds from the README).
3. Three crates: `python-syntax` (fast), `python-analysis` (ty + the solver), a CLI.
4. Fast crate first, so that V-NOC 0.1 can depend on it early.
5. Steps 0–8, each with a proof.

All five still hold. The destination is `language_driver_rs/languages/python/` instead of a repo of its own.

## 2. What stays exactly as planned

| PA plan file | Still the reference for |
|---|---|
| `01-why/01-playground-to-product.md` | the six rules; why a clean start and not a clean-up in place |
| `02-target/02-crates-and-public-api.md` | the two library crates, their `lib.rs`, the functions of `python-syntax`, taking V-NOC words out |
| `02-target/03-vendored-ruff.md` | the vendored folder, the patch rename, how to update |
| `02-target/04-tests-ci-docs.md` §1, §3 | which tests move; which notes move and which stay in the archive |
| `03-move/01-what-moves.md` | the verdict (move / rework / stay) for every module |
| `03-move/02-history-and-first-commit.md` | clean history; the first commit names the pylspt commit it came from |
| `03-move/04-switching-vnoc.md` | how the V-NOC driver changes its dependency |

## 3. What changes because the destination is a monorepo

| PA plan | Now | Why |
|---|---|---|
| a new repo `python-analyzer` | the folder `languages/python/`, a Cargo workspace of its own | your decision: one repo for all languages |
| root `Cargo.toml` is the workspace | `languages/python/Cargo.toml` is the workspace, with `exclude = ["vendor"]` | [`01-monorepo/02`](../01-monorepo/02-workspaces-and-toolchain.md) |
| `vendor/ruff/` at the repo root | `languages/python/vendor/ruff/` | the engine belongs to its language |
| `vendor/ruff/PATCHES.md` | `languages/python/ENGINE.md` (same content, the shared headings) | every language has the same file |
| CLI crate `python-analyzer-cli`, binary `python-analyzer` | crate and binary `python-cli` | the pattern `<lang>-cli` |
| tags `v0.1.0`, `v0.2.0` | `python-v0.1.0`, `python-v0.2.0` | one repo, several version lines |
| toolchain 1.98.0 | 1.99.1, the repo's one toolchain **[check ruff builds on it: PY1]** | tsrs needs 1.99 |
| CI: three jobs in the repo's own workflow | the same three jobs as rows of the shared workflow, started by path | [`01-monorepo/05`](../01-monorepo/05-ci.md) |
| `docs/` at the root: architecture, `call-tree/`, `ruff-and-ty/` | `languages/python/docs/`: the same three | repo-level `docs/` is for what all languages share |
| `scripts/update-ruff.sh`, `run-on-project.sh` | `languages/python/scripts/` | |
| "the history starts at 10–15 MB" | the vendored ruff is added to a repo that other languages share | the one real cost; see [`00-start-here/01`](../00-start-here/01-what-this-repo-is.md) §5 |

One thing to decide before PY1, because it is permanent in git history: **trim the vendored tree before the first commit.** The PA plan already
raises it ("decide whether vendored *test fixtures* are needed at all: if the vendored crates' own tests are never run, their `resources/test/`
folders can be left out"). In a repo of its own that was a nice-to-have. In a repo that TypeScript and Rust contributors also clone, do it:
**[measure]** the size with and without, and record what was left out in `ENGINE.md`.

## 4. What changes because of analyzer-core

Four groups of types that pylspt defines today come from `analyzer-core` instead. Python is their origin, so this is mostly "the definition
moves, the code keeps compiling".

| pylspt today | After | Work |
|---|---|---|
| `positions.rs`, `lines.rs` (line index) | `analyzer_core::text::LineIndex` | move the code **into core** in B0; `python-syntax` uses it |
| the outline's `DefNode` (a tree with `children`) and the driver's flat `Def` | `analyzer_core::outline::{Def, Outline}` (flat, parents first) | `python-syntax::outline` builds the core type directly. The PA plan already reworks `defs.rs` into `outline(text)`; this only fixes the return type |
| `edit.rs`'s edit type; the driver's header-comment logic | `analyzer_core::edit::{Edit, apply, MarkerStyle, comment_marker}` | the **docstring** marker logic stays Python's (`ids.rs`, `edit.rs`); only the edit type and the `#` header comment use core |
| `stable.rs` `StableDef`; `interp/canon.rs` `StableContextId`; `interp/graph.rs` `GraphOut`, `ContextOut`, `EdgeOut`, `EdgeTo`, `Span` | `analyzer_core::graph::*` | move the definitions **into core** in B0 with the renames of [`02-tree-rules/01`](../02-tree-rules/01-graph-and-tree.md) §5. `StableType` and the canonical key encoding stay in `python-analysis` |
| `interp/tree.rs` `Node`, `Mark`, `to_text` | `analyzer_core::render::*` | move into core in B0 |
| `interp/render.rs` (renders from the `Solver`) | `analyzer_core::render::render` (renders from the `Graph`) | **the one real rework.** See below |
| `interp/wire.rs` `FrameJson`, `merge_for_output` | not moved (PA5) | — |

### The renderer: the one real rework

Today `render_root(&solver, root_ctx)` reads the solver's internal tables and produces `FrameJson`. The shared renderer reads a `Graph` and
produces `Node`. The rules are the same ones (they *are* pylspt's rules). What can differ in practice:

| Risk | Check |
|---|---|
| child order: today "the evaluation order of the first context's sites"; core sorts by `site` (source order) | pylspt's `14-output.md` already flags this with **[check]**. Run the 41 fixture tests through both renderers and diff |
| recursion check: today by qname; core by `StableDef` | the same for every function with one definition; differs for `nth > 0` twins. Diff |
| "shown" filter (`require_ids`): today the solver decides; core's `only_marked` reads `StableDef::Marked` | diff |
| the `Graph` holds only contexts reachable through final edges; the old renderer walks the solver's slots | should be equal by construction (`graph()` does the same reachability). Diff |

So the migration keeps **both** renderers for one step (PY6): the old one stays as a private test helper, a test renders every fixture both ways
and compares, and when they agree on all 41 the old one is deleted. If they disagree, the disagreement is either a core bug or a rule that was
never written down; either way it is found before TypeScript depends on it.

**Python does not use `analyzer_core::solve`.** Its solver stays whole in `python-analysis/src/interp/`
([`02-tree-rules/03`](../02-tree-rules/03-the-seam.md) §4).

## 5. The PA rows, one by one

| Row | PA plan | Status now |
|---|---|---|
| **PA1** | a new repo `python-analyzer`; pylspt frozen at a tag and archived | **changed**: the destination is `language_driver_rs/languages/python/`. Freezing and archiving pylspt stands |
| **PA2** | clean history | **stands**: nothing of pylspt's 157 MB history comes along. The first Python commit names the pylspt commit |
| **PA3** | crates `python-syntax`, `python-analysis`, `python-analyzer-cli` | **stands**, except the CLI is `python-cli` |
| **PA4** | fast crate first; nothing enters before the six rules | **stands** |
| **PA5** | no `legacy/`; `nodes.rs`, `scan.rs`, `inject.rs`, `interp/wire.rs`, `main.rs` stay in the archive | **stands** |
| **PA6** | the marker label is a parameter | **stands**, and is now `analyzer_core::edit::MarkerStyle`, shared by all languages |
| **PA7** | docs move curated | **stands**; they land in `languages/python/docs/` |
| **PA8** | one repo per language analyzer; no shared repo, no common crate | **reversed by your decision.** What it warned about is answered by one workspace per language and by sharing only the answer ([`00-start-here/01`](../00-start-here/01-what-this-repo-is.md) §5, [`01-monorepo/03`](../01-monorepo/03-what-is-shared.md)) |
| **PA9** | later languages: stage A (tree-sitter inside vnoc) → B (borrow a language server as a child process) → C (own analyzer) | **changed**: TypeScript and Rust skip A and B. The engines are linked as libraries, which gives a real parser for tier 1–2 and the engine's own resolver for tier 3. C/C++ may still follow A → B (clangd as a child process); nothing here decides that |

The open questions Q-PA1 to Q-PA5 are still open and are carried into [`../07-decisions-and-questions.md`](../07-decisions-and-questions.md).

Next: [`02-what-moves-where.md`](02-what-moves-where.md)
