# language_driver_rs

One repo holding the **analyzer** of every language V-NOC supports: Python (ruff + ty), TypeScript/JavaScript (tsrs), Rust (rust-analyzer).
The plan is in `docs/`. Read `docs/README.md` first, then the file for the phase you are working on. The plan is the source of truth; if code
and plan disagree, say so instead of silently picking one.

## Words

- **driver**: the JSON-RPC program in `vnoc/drivers/<lang>/` (another repo). Not built here.
- **analyzer**: a language's crates in this repo (`languages/<lang>/crates/`).
- **engine**: the compiler front end an analyzer sits on (ruff/ty, tsrs, rust-analyzer). We never write parsers or type checkers.

## Layout

```
core/                     workspace 0: analyzer-core, analyzer-testkit. NO engine, no salsa.
languages/<lang>/         one workspace per language: <lang>-syntax (fast), <lang>-analysis (slow), <lang>-cli
languages/<lang>/ENGINE.md   the engine pin, the engine items we call, how to bump
scenarios/                the same program in every language + expected.default.tree / expected.precise.tree
docs/                     the plan
```

There is **no `Cargo.toml` at the repo root**, on purpose (ruff and rust-analyzer pin different `salsa`). Never add one. A language never
depends on another language, only on `core/` and its engine.

Upstream source for reading only: `../language_clone/tsrs`, `../language_clone/rust-analyzer`. Never build from or edit those.

## Commands

```sh
cd core && cargo test                          # the shared crates
cd languages/<lang> && cargo test              # one language
cd languages/<lang> && cargo test -p <lang>-syntax    # fast crate only: must not compile the type checker
scripts/check-all.sh --static                  # cheap repo checks
scripts/check-all.sh                           # everything CI runs
```

Toolchain: 1.99.1 (`rust-toolchain.toml`). Run `cargo fmt` and `cargo clippy -p <crate> -- -D warnings` on crates you touched. Pass
`-D warnings` per crate, never for a whole workspace (the vendored ruff would fail it).

## Rules that are not optional

1. **No product words.** No `vnoc`, `FileID`, `Schema/`, no ID format in `core/` or `languages/*/crates`. The marker label (`"ID"`) is a
   parameter (`MarkerStyle`).
2. **Text in, data out.** A library crate never writes a file and never invents an ID. No `fs::write`, no `uuid` outside `*-cli`.
3. **Chosen public API.** Modules are private; `lib.rs` exports by name. Returned structs and enums are `#[non_exhaustive]`.
4. **One engine module per crate.** Only `sem.rs` imports the engine (`tsrs_*`, `ruff_*`, `ty_*`, `ra_ap_*`). Everything else calls `sem`.
   Exception, Python only: `interp/` may *name* engine types that `sem` re-exports, and may not call engine functions.
5. **`core/` stays engine-free and small.** Code moves into `core/` only when two languages have it, written and tested (the rule of two).
   A change in `core/` must keep every language's tests green.
6. **No personal paths, no commented-out code, no output files in git.**
7. **The answer is a graph.** An analyzer's public call function returns `analyzer_core::graph::Graph`; a tree is always
   `analyzer_core::render::render(&graph, …)`. Never return a tree directly.
8. **Never guess, never fail.** Fallible analysis steps return `Option`, not `Result`. An unknown call target is an `Unresolved` edge, never
   dropped and never guessed. Every limit that cuts is marked and counted in `Stats`.
9. **Same input, same output.** Never iterate a hash map to produce output; sort by stable ids, or by (file, offset).
10. **Qualname rules are frozen at a language's first tag.** They are stored in users' files. Adding a kind of def is fine; changing how an
    existing def is named is not.

## Decisions already made (do not reopen; see `docs/07-decisions-and-questions.md`)

- **The marker lives in the docstring in every language**: Python docstring, a `ID: …` line inside the JSDoc block for TypeScript/JavaScript
  (before the first `@tag`), a `/// ID: …` doc line for Rust. A comment-above placement is a later option: keep `MarkerStyle.placement`, build
  only `Placement::Docstring`. The **file header** marker is a plain comment in every language.
- **Follow the engine.** If the engine's default type checker / call resolver resolves something, we use its answer. So JSX elements and
  accessor access are call sites, and object-literal methods are defs.
- **TypeScript and Rust are not context-sensitive yet**: one context per function (`type Key = ();`, digest zeros) on core's flat solver.
  Keep the seam: `resolve(site, &env)` with an empty `Env`, `Key` used by name, modules named `sem` / `walk` / `env` / `key` / `calls`.
  Python keeps its own context-sensitive solver and does not implement `Lang`.
- In phase 1, calls inside an anonymous function belong to the enclosing def.
- Order: bootstrap (B0) → Python syntax (PY1–PY4) → TypeScript (TS0 spikes first) → the rest. See `docs/06-roadmap.md`.
- Drivers stay in `vnoc`. Tags are per language (`typescript-v0.1.0`); `core/` is never tagged.

## How to work here

- **Nothing in the plan was built or run when it was written.** Every `[check]` and `[measure]` in `docs/` is a claim to verify before you
  depend on it; the list is in `docs/07-decisions-and-questions.md` §5. When you verify one, write the answer where the plan says
  (usually `languages/<lang>/ENGINE.md`) and fix the doc if it was wrong.
- Do the spikes before the phases that depend on them (`docs/03-typescript/02-ts0-spikes.md`, RS0 in `docs/04-rust/02-phases.md`). Spike code
  goes in `languages/<lang>/spikes/` as its own tiny workspace and is deleted when the answer is written down.
- Each phase ends with the proof named in its doc. Do not call a phase done without running that proof; report what actually passed.
- Every behaviour has a fixture: one file per case under `tests/fixtures/`, every policy-table row a fixture, every tree rule a scenario.
  Snapshots are reviewed, not just accepted.
- tsrs memory: its pointers (`P<T>`, `&'static Program`) are arena pointers the compiler cannot check. Parse inside a region via
  `sem::with_parsed`, convert to owned `analyzer-core` data before returning, and never let a tsrs type cross out of `sem.rs`
  (`docs/03-typescript/01-what-tsrs-gives.md` §5).
- Engine bumps are a change of their own: only the pin, the lockfile, `sem.rs` and `ENGINE.md`.
- The owner is mid-level in Rust and wants explanations in a teaching style: when you introduce a non-obvious Rust concept, explain it briefly.
- Commit only when asked.
