# 07. Decisions and questions

```
decisions and questions
├── 1. what you have already decided
├── 2. draft rows (M1–M18), each with my recommendation
├── 3. what they change in rows you already have
├── 4. the questions, all answered (Q-M1 … Q-M9, and the carried Q-PA ones)
└── 5. things I did not check
```

Everything here was decided on 2026-11-11: §1 lists your decisions, and D12 accepts the draft rows of §2. These are draft rows for your decision log. I did not edit your log, the V-NOC
plan or the PA plan.

Following your earlier instruction, the **structure** rows (layout, workspaces, CI, what is shared) are decided by me as drafts you can veto.
The rows that change **what users see or commit** are in §4 as questions.

## 1. What you have already decided

| | Decision | When |
|---|---|---|
| D1 | One repo for the analyzers of all languages (Python, TypeScript/JavaScript, Rust) | 2026-11-11, this request |
| D2 | The repo is named **`language_driver_rs`**; the upstream clones stay in `language_clone/` as resources | 2026-11-11 (answers Q-M1) |
| D3 | TypeScript/JavaScript and Rust start **without** context sensitivity, on the engine's default call resolver; context sensitivity is a later phase and must not need a rewrite | 2026-11-11 |
| D4 | Every language builds the tree by pylspt's rules | 2026-11-11 |
| D5 | pylspt migrates into this repo | 2026-11-11 |
| D6 | **The marker lives in the docstring, for every language, for now** (answers Q-M3). Python: a line in the docstring (as F1). TypeScript/JavaScript: a `ID: …` line inside the JSDoc block. Rust: a `/// ID: …` doc-comment line. The comment-above-the-def placement (`// ID: …`, the old recommendation) becomes a **later, configurable option**; the code keeps the placement as a parameter (`MarkerStyle`) so adding it is not a rewrite | 2026-11-11 |
| D7 | **Rule for every open question: if the engine's default type checker / call resolver already resolves it, we follow the engine; otherwise the recommendation in §4 stands** | 2026-11-11 |
| D8 | Q-M8: JSX elements (`<Button />`) and getter/setter access **are calls**, because the engine's call hierarchy resolves them (D7) | 2026-11-11 |
| D9 | Q-M6: methods in object literals **are defs**, because the engine resolves calls to them (D7) | 2026-11-11 |
| D10 | Q-M2 Python first (PY1–PY4), then TypeScript · Q-M5 `range` excludes the JSDoc (the driver may extend it for display) · Q-M7 drivers stay in `vnoc` · Q-M9 `<S as T>::m` · Q-PA4 new tree shape built from core's `Node` | 2026-11-11, recommendations accepted |
| D11 | Q-M4 an organization, public (the owner name is filled in when it is created; until then every URL is a `TODO-owner` placeholder) · Q-PA3 no old web-driver parity, `merge_for_output` is dropped · Q-PA5 keep `ast` as `python-cli ast` (it is about 90 lines) | 2026-11-11, recommendations accepted |
| D12 | Draft rows M1–M18 are accepted as written | 2026-11-11 |
| D13 | **The graph has a table of definitions** (`Graph::defs`, one `DefInfo` per function or class it mentions); a `Context` is only an analysis (`id`, `status`). Replaces the class context the core solver invented. Changes the type of M10 ([02/04](02-tree-rules/04-the-defs-table.md)) | 2026-11-11 |

## 2. Draft rows

| Row | Decision | Recommended | The other option | Where |
|---|---|---|---|---|
| **M1** | The repo holds **analyzers only**. Drivers (the JSON-RPC programs) stay in `vnoc/drivers/<lang>` (L1 unchanged). In these docs "driver" means the vnoc program and "analyzer" means this repo's crates | yes | move the drivers here too (Q-M7) | [00/02](00-start-here/02-the-name.md) §2 |
| **M2** | **One Cargo workspace per language**, plus a `core/` workspace. No `Cargo.toml` at the repo root | yes | one workspace for everything (breaks on `salsa`: ruff pins 0.28.2, rust-analyzer 0.28.5, different features) | [01/02](01-monorepo/02-workspaces-and-toolchain.md) §1 |
| **M3** | Shared code is **`analyzer-core`** (answer types, renderer, flat solver, text, edits, comment marker) and **`analyzer-testkit`**. Anything else enters `core/` only by the **rule of two** | yes | no shared crate at all (PA8); or a large shared framework | [01/03](01-monorepo/03-what-is-shared.md) |
| **M4** | Crate names: `<lang>-syntax`, `<lang>-analysis`, `<lang>-cli`. The repo name appears in no crate. All `publish = false` | yes | prefix every crate with the repo name | [00/02](00-start-here/02-the-name.md) §3 |
| **M5** | **One toolchain for the repo: 1.99.1** (tsrs needs 1.99) | yes | a toolchain file per language (they would drift) | [01/02](01-monorepo/02-workspaces-and-toolchain.md) §3 |
| **M6** | Engines: **ruff vendored** (as the PA plan); **tsrs as a git dependency pinned by `rev`**, no fork until a patch is needed; **rust-analyzer from crates.io** (`ra_ap_*`, `=` pins). One `ENGINE.md` per language | yes | vendor all three (grows the repo by 100+ MB); or fork all three | [01/04](01-monorepo/04-engines.md) |
| **M7** | **Rule 7: one module per crate (`sem.rs`) imports the engine.** Python's `interp/` may name engine types re-exported by `sem`, not call engine functions | yes | import the engine wherever convenient | [01/04](01-monorepo/04-engines.md) §6, [05/03](05-python-migration/03-steps.md) §5 |
| **M8** | CI runs **jobs by path**, with one always-reporting `ci-ok`; a change in `core/` runs every language; nightly builds everything uncached | yes | one job that builds all | [01/05](01-monorepo/05-ci.md) |
| **M9** | **Tags per language** (`typescript-v0.1.0`); `core/` is never tagged; the tag is the version | yes | one version for the whole repo | [01/06](01-monorepo/06-versions-and-vnoc.md) §2 |
| **M10** | **The contract is `analyzer_core::graph::Graph`**: contexts and edges with stable ids. A tree is always `render(&graph)`. No analyzer returns a tree directly | yes | each language returns its own tree type | [02/01](02-tree-rules/01-graph-and-tree.md) |
| **M11** | **The twelve rules** are the definition of "the tree is right" for every language; each language writes a policy table with a fixture per row | yes | per-language rules | [02/02](02-tree-rules/02-the-rules.md) |
| **M12** | Phase 1 for TypeScript and Rust = **one context per function** (`Key = ()`, digest zeros), on core's flat solver, with pylspt's module names (`sem`, `walk`, `env`, `key`) and `resolve(site, &env)` in its final form | yes | a separate "simple" design to be replaced later | [02/03](02-tree-rules/03-the-seam.md) |
| **M13** | In phase 1, **calls inside an anonymous function belong to the enclosing def** (as both engines' call hierarchies do) | yes | drop them; or show the anonymous function as a node | [02/02](02-tree-rules/02-the-rules.md) rule 3 |
| **M14** | `scenarios/`: the same program per language, with `expected.default.tree` and `expected.precise.tree`. A language declares its level | yes | per-language tests only | [02/03](02-tree-rules/03-the-seam.md) §5 |
| **M15** | TypeScript's function-like defs are **exactly the engine's call-hierarchy declarations**, so every callee is in the outline. Nested-function joiner `.<locals>.` | yes | a narrower outline with callees that have no outline entry | [03/03](03-typescript/03-ts1-outline.md) §2–3 |
| **M16** | **Spikes before building** each language (TS0: four, RS0: three); their answers go into `ENGINE.md` | yes | start with TS1 and discover on the way | [03/02](03-typescript/02-ts0-spikes.md) |
| **M17** | Python migration: the PA plan's steps with the destination `languages/python/`; the core's graph and renderer are **born from pylspt's code in B0**; Python keeps its own solver; the old renderer is kept for one step and compared on all 41 fixtures (PY6) | yes | write core fresh and adapt Python later | [05/](05-python-migration/01-what-changes.md) |
| **M18** | Budget, frontier tokens, paging and cancel live in **core** (built in TS5 steps 1–2), so Rust and later Python reuse them | yes | each language implements them | [03/07](03-typescript/07-ts5-driver-needs.md) |

## 3. What they change in rows you already have

| Existing row | Status after this plan |
|---|---|
| **L1** driver always in `vnoc`; analyzer a separate crate | unchanged (M1). The "four conditions for an own repo" are replaced by "the analyzer lives in `language_driver_rs` from the start" |
| **L2** the two-crate split (syntax / analysis) | unchanged, and now the pattern for every language |
| **L4** `PATCHES.md`; ruff updated in a pull request of its own | unchanged; the file is `languages/python/ENGINE.md`, and the rule covers every engine |
| **R9** one toolchain, 1.98.0 | the idea stands; the number becomes **1.99.1** (M5). `vnoc` itself is still on 1.94.1 |
| **R10** the tag is the version | applied here (M9) |
| **R11** `drivers/*/` is the list of languages | the same idea here: `languages/*/` is the list |
| **R12** CI by path, one `ci-ok` | applied here (M8) |
| **R13** pin by `rev` on main, tag at release; no release automation in the analyzer | unchanged; tags now carry the language |
| **R14–R16** each driver its own small extension | unchanged, and more useful: the TypeScript and Rust drivers link large engines |
| **Q-R1** Apache-2.0 | applied; a root `NOTICE` lists the engines' licences |
| **PA1** new repo `python-analyzer` | changed: `language_driver_rs/languages/python/` |
| **PA2–PA7** | stand ([05/01](05-python-migration/01-what-changes.md) §5) |
| **PA8** one repo per language, no common crate | **reversed** by D1; M2 and M3 are the conditions that make it safe |
| **PA9** stage A → B → C for later languages | changed for TypeScript and Rust: the engine is linked as a library from the start |
| `06-drivers.md` §3 (before the second driver: files by extension, open `DefKind`, `style`) | unchanged and now **required** before `typescript-v0.1.0` has a consumer |
| F1 "docstring `ID:` line; a comment above the def comes later as an option" | unchanged, and now applied to every language (D6): JSDoc in TypeScript/JavaScript, `///` in Rust. `MarkerStyle.placement` keeps room for the later option |

## 4. Questions (all answered on 2026-11-11, see D6–D11)

The table keeps the original wording. Q-M3 was answered *differently* from the recommendation: the marker goes in the docstring (D6).

| # | Question | My recommendation | Why it matters now |
|---|---|---|---|
| ~~Q-M1~~ | the repo's name | answered: `language_driver_rs` | |
| **Q-M2** | **Which goes first after B0: Python's syntax crate (PY1–PY4) or TypeScript (TS0–TS2)?** | Python's syntax crate: it is small, V-NOC 0.1 waits for it, and it is the first real test of the core | it only reorders [06](06-roadmap.md) §4; nothing else changes |
| **Q-M3** | **The marker in TypeScript, JavaScript and Rust: a line comment above the def (`// ID: …`)?** | **ANSWERED: no, docstring for now (D6).** Original recommendation: yes: one rule for three languages, invisible to doc tools, the smallest diff | it is what users commit. The alternative (a line inside JSDoc / a doc comment) is compared in [03/04](03-typescript/04-ts2-markers.md) §2 |
| **Q-M4** | GitHub owner (your account or an organization that also holds `vnoc`) and public or private (this is Q-PA1 + Q-PA2) | an organization, public | the URL goes into every driver's `Cargo.toml`; a private repo as a git dependency needs a token in `vnoc`'s CI |
| **Q-M5** | Does a def's `range` include the doc comment above it (TypeScript JSDoc)? | no in the analyzer; the driver may extend it for display | it changes what a card shows on the canvas ([03/03](03-typescript/03-ts1-outline.md) §4) |
| **Q-M6** | Are **methods in object literals** (`const api = { load() {} }`) defs in TypeScript's outline? | yes: the engine can resolve calls to them, so they need an identity | removing a kind later orphans docs; adding is safe. If unsure, say no and they appear in trees as unmarked path-named nodes |
| **Q-M7** | Should the drivers move into this repo one day? | no | it would tie this repo to V-NOC's protocol version and break rule 1 |
| **Q-M8** | TypeScript call sites: do **JSX elements** (`<Button />`) and **getter/setter access** count as calls? | JSX yes (it is how components call each other); accessors yes (it is what the engine's call hierarchy does) | it decides how busy a React component's tree is ([03/06](03-typescript/06-ts4-calls.md) §3) |
| **Q-M9** | Rust: the qualname of a trait-impl method, `<S as T>::m` (Rust's own form) or something friendlier? | `<S as T>::m` | it is shown in the UI and stored in `.vnoc/`; frozen at `rust-v0.1.0` |
| Q-PA3 | Is the old web driver parity (`FunctionSchema/<uuid>` JSON, `merge_for_output`) still needed by anything? | — | if yes, that code must live somewhere that builds (PA5 changes) |
| Q-PA4 | Does V-NOC 0.2 store call trees in the old `FrameJson` shape or a new one? | a new one, built from core's `Node` | decides what the shared conversion module in `vnoc` produces |
| Q-PA5 | Do you still use the `ast` debug subcommand? | — | decides whether it moves |

## 5. Things I did not check

I read source; I built and ran nothing.

**About the repo layout**

- A member of `core/` that inherits `[workspace.dependencies]`, compiled as a path dependency from a language workspace (B0 step 9).
- A crate outside the repo depending on a nested-workspace crate by `git` + `rev` (B0 step 9).
- That `clippy.toml` and `rustfmt.toml` at the repo root are picked up by every nested workspace.

**About tsrs** (all are TS0 spikes)

- That its crates build as git dependencies outside their own workspace, on three operating systems, and what they cost in time and size.
- That a `Region` returns memory in a long-running loop, and which constructor is the right one.
- That `free_program` with `checker_regions` returns a program's memory.
- What `resolve_call_hierarchy_declaration` returns for each row of the policy table. The table in [03/06](03-typescript/06-ts4-calls.md) §5 is
  my reading of the code, not observed output.
- How a `Context` is cancelled from outside; whether `tsrs_project::Session` can be driven without the LSP server.
- Where tsrs attaches the JSDoc for each kind of def, and how to get its byte range (S-TS4).
- Whether it runs correctly without mimalloc as the global allocator.

**About rust-analyzer** (RS0 spikes)

- Which `ra_ap_*` version matches commit `2a362a4`.
- Whether `resolve_method_call` gives the impl's method or the trait's for a trait method on a concrete type.
- Whether calls inside functions rewritten by attribute macros are found with the proc-macro server off.
- That `rustfmt` leaves a `/// ID: …` doc line unchanged.

**About Python**

- That the vendored ruff builds on toolchain 1.99.1.
- That the `Graph` carries everything the old renderer read from the solver (PY6 exists to find out).
- pylspt's test count and the golden output: PY0 produces them.
- Everything the PA plan lists in its own "things I did not check".

**Sizes.** Every S / M / L in these files is a guess. TS0 and PY2 give the first real numbers; update [06](06-roadmap.md) §4 then.

Back to: [`README.md`](README.md)
