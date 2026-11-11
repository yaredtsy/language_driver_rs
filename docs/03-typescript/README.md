# 03. TypeScript and JavaScript

```
typescript
├── what tsrs gives ─────────────── 01-what-tsrs-gives.md
├── TS0  spikes ─────────────────── 02-ts0-spikes.md
├── TS1  outline ────────────────── 03-ts1-outline.md
├── TS2  markers ────────────────── 04-ts2-markers.md        → tag typescript-v0.1.0 → V-NOC tier 1–2
├── TS3  project loading ────────── 05-ts3-project.md
├── TS4  call graph ─────────────── 06-ts4-calls.md          → tag typescript-v0.2.0 → V-NOC tier 3, whole tree
├── TS5  what the driver needs ──── 07-ts5-driver-needs.md   → tag typescript-v0.3.0 → V-NOC tier 3, lazy
└── later: context-sensitive ────── 08-later-context-sensitive.md
```

One language id, `typescript`, covers `.ts .tsx .mts .cts .js .jsx .mjs .cjs`. JavaScript is not a second analyzer: tsrs parses and checks it
with the same code, as the TypeScript compiler does.

## The phases at a glance

| Phase | Builds | Crate | Depends on | Proof it is done | Size |
|---|---|---|---|---|---|
| **TS0** | four throwaway experiments | none (a scratch folder) | B0 | four numbers and four yes/no answers, written into `ENGINE.md` | S |
| **TS1** | `outline(text, kind) -> Outline` | `typescript-syntax` | TS0 | snapshot tests on the fixtures; `cargo tree` shows no checker | M |
| **TS2** | `marker`, `plan_marker`, `header_slot` | `typescript-syntax` | TS1 | the round-trip and never-panics tests pass | S |
| | **tag `typescript-v0.1.0`** | | | the V-NOC driver reaches tier 2 with it | |
| **TS3** | `Project::open(root)` | `typescript-analysis` | TS0 | a real project loads; memory returns after `drop` | M |
| **TS4** | `call_graph(project, file, entry)` | `typescript-analysis`, `typescript-cli` | TS3, B0's solver | every scenario matches `expected.default.tree`; the policy table has a fixture per row | L |
| | **tag `typescript-v0.2.0`** | | | | |
| **TS5** | budget, expand, cancel, cache, unsaved text | `typescript-analysis` | TS4 | the driver's tier-3 conformance checks | M |
| | **tag `typescript-v0.3.0`** | | | | |
| later | context sensitivity | `typescript-analysis` | TS5, and a decision | the scenarios match `expected.precise.tree` | XL |

S, M, L are relative sizes, not times. **[measure]** I did not build anything.

TS1 and TS2 need only the parser. TS3 does not depend on TS1/TS2, so one person can go TS0 → TS1 → TS2 → TS3 → TS4 in a line, or two people
can split after TS0.

## What V-NOC gets, and when

```
typescript-v0.1.0   .ts/.js files appear in the tree and on the canvas · docs attach · docs survive renames
typescript-v0.2.0   call trees, computed whole
typescript-v0.3.0   call trees that open one level at a time, can be cancelled, and are fast the second time
```

The first tag is the valuable one. A language can stay there for a long time (PA plan: "stage A alone is useful").

## The rule for every phase

Each phase ends with:

1. the tests named in "proof it is done" running in CI on three operating systems,
2. the public API of the crate still a short `lib.rs` (rule 3),
3. `ENGINE.md`'s list of engine items regenerated,
4. the phase's decisions copied into `languages/typescript/docs/` (the qualname rule after TS1, the marker rule after TS2, the policy table
   after TS4). Those three documents are the language's contract with its users.
