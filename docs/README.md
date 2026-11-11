# The language repo: plan

Written 2026-11-11, after reading:

- `vnoc_playground/fable_plan/` (the V-NOC plan: `06-drivers.md`, `09-adding-a-language/`, `11-git-and-release/`),
- `pylspt/` (`src/`, `plan/03-call-tree/`, and `fable_plan/`, the earlier plan to move it to a repo of its own),
- `../language_clone/tsrs` at commit `13a31a4` (version 0.24.0, a Rust port of the TypeScript 7.1 compiler),
- `../language_clone/rust-analyzer` at commit `2a362a4`.

This is the plan for **one repo that holds the analyzer of every language** V-NOC supports: Python (moved from pylspt), TypeScript / JavaScript and
Rust. The repo is **`language_driver_rs`** (your name). The upstream clones stay in `../language_clone/` as reading resources; see
[`00-start-here/02-the-name.md`](00-start-here/02-the-name.md).

## The short version

1. **One repo, one Cargo workspace per language.** Python sits on ruff and ty, TypeScript on tsrs, Rust on rust-analyzer. They cannot share a
   lockfile (two of them pin different `salsa` versions), so each language is its own workspace inside the repo, and a small `core` workspace holds
   what they share.
2. **What is shared is the answer, not the analysis.** Every language returns the same call graph type, and one renderer turns it into the tree.
   That is the part of pylspt that was never about Python.
3. **TypeScript and Rust start with the engine's own call resolver.** No context sensitivity yet. In the shared graph that is simply "one context
   per function". Context sensitivity later means "more contexts per function": the graph type, the renderer, the CLI, the tests and the V-NOC
   driver do not change.
4. **Each language is built in the same phases:** spikes → outline → markers → project loading → call graph → what the driver needs (budget, cancel,
   cache). A language is useful to V-NOC after the first two.
5. **pylspt moves in as the Python workspace.** The earlier move plan still holds almost line by line; only the destination and four types change.

## The plan, top-down

Read this tree from the root. Each branch has a file.

```
language_driver_rs
│
├── START HERE ────────────────────────────────────── 00-start-here/
│   ├── what this repo is          layers · what it never does · what phase 1 means ── 01-what-this-repo-is.md
│   └── the name                   my suggestion, and why it is cheap to change ───── 02-the-name.md
│
├── THE MONOREPO ──────────────────────────────────── 01-monorepo/
│   ├── layout                     every folder at the root ───────────────────────── 01-layout.md
│   ├── workspaces and toolchain   why one workspace per language · one toolchain ─── 02-workspaces-and-toolchain.md
│   ├── what is shared             analyzer-core · analyzer-testkit · the rule of two  03-what-is-shared.md
│   ├── engines                    ruff vendored · tsrs by git rev · ra_ap crates ──── 04-engines.md
│   ├── CI                         jobs by path · one required check · nightly ─────── 05-ci.md
│   └── versions and vnoc          tags per language · how a driver pins ──────────── 06-versions-and-vnoc.md
│
├── THE TREE RULES (from pylspt, for every language) ─ 02-tree-rules/
│   ├── graph and tree             the answer is a graph; the tree is a view ──────── 01-graph-and-tree.md
│   ├── the rules                  12 rules every language follows ────────────────── 02-the-rules.md
│   ├── the seam                   how phase 1 leaves the door open ───────────────── 03-the-seam.md
│   └── the defs table             a change: definitions get their own table ──────── 04-the-defs-table.md
│
├── TYPESCRIPT / JAVASCRIPT ───────────────────────── 03-typescript/
│   ├── what tsrs gives            exported · needs our work · risks ──────────────── 01-what-tsrs-gives.md
│   ├── TS0  spikes                four questions to answer before building ───────── 02-ts0-spikes.md
│   ├── TS1  outline               what a def is · qualnames · the crate ──────────── 03-ts1-outline.md
│   ├── TS2  markers               reading and planning the ID comment ────────────── 04-ts2-markers.md
│   ├── TS3  project loading       tsconfig · program · memory ────────────────────── 05-ts3-project.md
│   ├── TS4  call graph            the walk · the default resolver · the policy ───── 06-ts4-calls.md
│   ├── TS5  what the driver needs budget · expand · cancel · cache · unsaved text ── 07-ts5-driver-needs.md
│   └── later: context-sensitive   what changes, file by file ─────────────────────── 08-later-context-sensitive.md
│
├── RUST ──────────────────────────────────────────── 04-rust/
│   ├── what rust-analyzer gives   exported · needs our work · risks ──────────────── 01-what-rust-analyzer-gives.md
│   └── RS0–RS5                    the same phases, shorter ───────────────────────── 02-phases.md
│
├── PYTHON: MIGRATING pylspt ──────────────────────── 05-python-migration/
│   ├── what changes               against the earlier `python-analyzer` plan ─────── 01-what-changes.md
│   ├── what moves where           every module: move · rework · stay ─────────────── 02-what-moves-where.md
│   └── PY0–PY8                    the steps, each with a proof ───────────────────── 03-steps.md
│
├── THE ROADMAP ───────────────────────────────────── 06-roadmap.md
│   └── B0 bootstrap, then three tracks; what blocks what
│
└── WHAT NEEDS YOUR YES ───────────────────────────── 07-decisions-and-questions.md
```

## Reading order

| If you want… | Read |
|---|---|
| the folder tree of the new repo | [`01-monorepo/01-layout.md`](01-monorepo/01-layout.md) |
| what code is shared between languages, and what is not | [`01-monorepo/03-what-is-shared.md`](01-monorepo/03-what-is-shared.md) |
| how "no context sensitivity now" avoids a rewrite later | [`02-tree-rules/03-the-seam.md`](02-tree-rules/03-the-seam.md) |
| what tsrs and rust-analyzer already do for us | [`03-typescript/01-what-tsrs-gives.md`](03-typescript/01-what-tsrs-gives.md), [`04-rust/01-what-rust-analyzer-gives.md`](04-rust/01-what-rust-analyzer-gives.md) |
| the TypeScript work list | [`03-typescript/README.md`](03-typescript/README.md) |
| how pylspt comes in | [`05-python-migration/03-steps.md`](05-python-migration/03-steps.md) |
| the order of everything | [`06-roadmap.md`](06-roadmap.md) |
| what I need you to decide | [`07-decisions-and-questions.md`](07-decisions-and-questions.md) |

## Conventions in these files

- Every file starts with a small tree of what is inside it, then goes branch by branch.
- **🦀 Rust note** boxes explain a concept the first time it matters. Concepts the earlier plans already explain (virtual workspace, git
  dependencies, `[patch]`, `#[non_exhaustive]`, `insta` snapshots) are linked, not repeated.
- **[check]** means I believe it but did not run it. **[measure]** means a number to measure before relying on it.
- **M1, M2…** are draft decision rows. **Q-M1…** are questions only you can answer. Nothing is decided until you say yes.
- Phases are named by track: **B** (bootstrap), **TS**, **RS**, **PY**. I avoided `R1…` because your release plan already uses it.
- "The earlier plans" are `vnoc_playground/fable_plan/` (called **V-NOC plan**) and `pylspt/fable_plan/` (called **PA plan**). I did not edit
  either.
- I did not build tsrs, rust-analyzer or pylspt for this plan. Signatures quoted here were read from the source at the commits above.
