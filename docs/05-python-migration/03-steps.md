# 05.03 Python: the steps

```
the steps
├── 1. the order, and what it is tied to
├── 2. PY0–PY8
├── 3. PY2 and PY6 in more detail
├── 4. what "done" means
└── 5. what can go wrong
```

These are the PA plan's steps 0–8 (`03-move/03-steps.md`) with the new destination. Each still ends with both repos building and a proof you can
run. `vnoc` keeps its old pin on pylspt until PY4 changes it on purpose.

## 1. The order

```
PY0  pin pylspt at 670ae79a15 (no freeze, D14) ─────────────────┐ baseline: test count + golden output
B0   the repo skeleton + analyzer-core (born from pylspt's types)│      ← shared with every language; see 06-roadmap.md
PY1  languages/python skeleton + vendored ruff + CI row          │
PY2  python-syntax ──────────► PY3  tag python-v0.1.0            │
                               PY4  vnoc's Python driver switches ◄── V-NOC 0.1 is unblocked here
PY5  python-analysis (ty + the solver), old renderer kept        │
PY6  switch to the core renderer; compare on all 41 fixtures     │
PY7  python-cli + the golden run  ◄──────────────────────────────┘ compared with the baseline
PY8  docs; tag python-v0.2.0; archive pylspt
```

Two rules decide it, both from the PA plan:

- **Fast crate first.** V-NOC 0.1 needs only `python-syntax`.
- **Stop anywhere.** If you stop after PY4, nothing is half-done: the old repo still holds the solver, working, at its tag.

PY5–PY8 can wait until V-NOC 0.2 needs call trees. But **PY6 is worth doing before TypeScript's TS4**, because it is the test of the shared
renderer against 41 real cases ([`../06-roadmap.md`](../06-roadmap.md) §3).

## 2. PY0–PY8

| Step | Repo | What | Proof |
|---|---|---|---|
| **PY0** | pylspt | **not a freeze (D14): pylspt stays a live playground.** The source of the migration is the commit **`670ae79a15`**, always read by its hash: `git -C ../pylspt archive 670ae79a15 <path> \| tar -x`, never the working tree. The baseline (the test count, and the golden output in **both** forms, `calls` and `calls --graph`) is taken from a worktree of that commit (`git -C ../pylspt worktree add <dir> 670ae79a15`), when PY5 first needs it | nothing copied into this repo differs from `git show 670ae79a15:<path>`; the baseline is one number and two files, kept outside git |
| **B0** | new | the repo skeleton and `analyzer-core` ([`../06-roadmap.md`](../06-roadmap.md) §2). The core's types are moved out of pylspt here ([`02-what-moves-where.md`](02-what-moves-where.md) §2) | `cd core && cargo test`; `cargo tree` shows no engine |
| **PY1** | new | `languages/python/`: `Cargo.toml` (workspace, `exclude = ["vendor"]`), the old `Cargo.lock`, the vendored ruff (one commit: the copy; next commit: the patch rename + `ENGINE.md`), an empty `python-syntax` that depends on `ruff_python_parser` and `analyzer-core` | the Python CI row is green on three operating systems, on toolchain 1.99.1; `git diff <copy commit> <patch commit> -- languages/python/vendor` shows only the patch |
| **PY2** | new | `python-syntax`: `sem`, `outline`, `ids`, `edit`, `header`; the facade; fixtures; the three testkit tests | `cargo test -p python-syntax` passes **and** `cargo tree -p python-syntax` lists no `salsa`, `ruff_db` or `ty_*` |
| **PY3** | new | README section for Python with a real example; `languages/python/docs/` started (qualnames, markers); **tag `python-v0.1.0`** | a fresh clone builds and tests `python-syntax` with the README's one command |
| **PY4** | vnoc | the Python driver depends on `python-syntax` by tag; the `pylspt` line and the three `ruff_*` lines go; `outline.rs`, `file_id.rs`, `positions.rs`, `plan_ids.rs` shrink to conversions (PA plan `03-move/04`) | the driver's tests and the conformance test pass; build time and binary size are lower **[measure both, before and after]** |
| **PY5** | new | `python-analysis`: `project`, `sem`, `classes`, `interp/` moved, emitting `analyzer_core::graph::Graph`; the renames (`Marked`, `only_marked`, `analyzer_ext`); the 41 fixture tests, **still rendered by the old renderer** (kept as a private module); one test per patch wrapper | `cargo test -p python-analysis` passes with the same count as PY0, minus the tests of files that stayed |
| **PY6** | new | render through `analyzer_core::render`; a test renders every fixture both ways and compares; fix what differs; delete the old renderer; turn on the scenarios at `Level::Precise` | the comparison test passed on all 41 before it was deleted (keep that commit's CI run as the record); `scenarios` passes |
| **PY7** | new | `python-cli` with subcommands; `scripts/run-on-project.sh` | `calls --graph` on the Jarvis project equals PY0's golden graph, after the documented renames; `calls` equals the golden tree |
| **PY8** | both | `languages/python/docs/architecture.md`, the curated `call-tree/` and `ruff-and-ty/`; **tag `python-v0.2.0`**; pylspt gets a README pointer and is archived on GitHub | no link in `languages/python/docs/` points outside the repo or to a missing file; the old repo page says "archived" |

## 3. Two steps in more detail

### PY2: the only step with design in it

The PA plan's order (`03-move/03` §2, "Step 2 in a little more detail") holds. With the core in place it is:

1. `sem.rs`: parse with `ruff_python_parser`; expose only what `outline` and `edit` need.
2. `outline(text, style)`: `DefScanner` from `defs.rs` + the qualname stack and syntax-error collection from the driver's `outline.rs`,
   producing core's flat `Outline`. Snapshot the fixtures.
3. `ids`: `id_from_body`, with the label from `MarkerStyle`.
4. `edit`: `doc_slot` and `id_edit` from `edit.rs`; its 9 tests move with it and are the proof that nothing changed.
5. `header`: from the driver's `file_id.rs`, on top of `comment_marker` with `#`.
6. the three testkit tests (`snapshot_every_fixture`, `marker_round_trip`, `cut_every_50_bytes`).
7. `lib.rs`: the facade. Make every module private; fix what the compiler complains about (the PA plan's Rust note on `unreachable_pub`).

Python is the **first user of the testkit**. If a testkit function is awkward for Python's docstring markers (it was designed with comment
markers in mind), fix the testkit now, before TypeScript copies the pattern.

### PY6: the renderer comparison

```rust
// languages/python/crates/python-analysis/tests/render_parity.rs — lives for one step, then is deleted
#[test]
fn core_renderer_matches_the_old_one() {
    for fixture in all_solver_fixtures() {
        let (solver, root) = solve(&fixture);
        let old = old_render::render_root(&solver, root).to_node();          // today's code, kept private for this step
        let graph = graph::graph(&solver, &[root]);                          // → analyzer_core::graph::Graph
        let new = analyzer_core::render::render(&graph, &graph.entries[0], &options_like_old());
        assert_eq!(to_text(&old), to_text(&new), "fixture {}", fixture.name);
    }
}
```

Read every difference before fixing it. There are three possible outcomes for each, and the third is the valuable one:

| The difference is | Then |
|---|---|
| a bug in the new core renderer | fix core; add a core test with a hand-written `Graph` that shows it |
| information the old renderer took from the solver and the `Graph` does not carry | add the field to `Graph` (that is what seam one is for), or decide the tree does not need it |
| **a rule the old renderer followed that was never written down** | write it into [`../02-tree-rules/02-the-rules.md`](../02-tree-rules/02-the-rules.md), add a scenario for it. This is the point of doing PY6 before TS4 |

> **🦀 Rust note: keeping old code alive for one step.** Put the old renderer in `tests/support/old_render.rs` (or behind
> `#[cfg(test)] mod old_render;`), not in the public crate. `#[cfg(test)]` means "compile this only when building tests", so it cannot leak into
> the driver's binary, and deleting it later is deleting one file and one `mod` line. If the old code needs private items of the crate, the
> `#[cfg(test)] mod` form inside `src/` can see them; a file under `tests/` cannot (integration tests only see the public API).

## 4. What "done" means

- [ ] `vnoc/drivers/python/Cargo.toml` names `python-syntax` (and later `python-analysis`) from `language_driver_rs`, and no ruff crate.
- [ ] `grep -ri "vnoc\|schema/\|/Users/" languages/python/crates languages/python/scripts` finds nothing.
- [ ] No `uuid` and no `fs::write` in `python-syntax` or `python-analysis`.
- [ ] `grep -l "ruff_\|ty_" languages/python/crates/*/src/*.rs` prints only `sem.rs` files (and `interp/` files that hold ty handles in
      values: see §5, last row).
- [ ] The Python CI rows are green on Linux, macOS and Windows.
- [ ] The 41 fixture tests pass, rendered by the core. The scenarios pass at `Level::Precise`.
- [ ] The golden run matches.
- [ ] pylspt is archived, with a pointer to the new repo.

## 5. What can go wrong

The PA plan's list (`03-move/03` §4) holds. What the monorepo and the core add:

| Risk | How you notice | What to do |
|---|---|---|
| the vendored ruff does not build on 1.99.1, or floods the log with new warnings | PY1's CI | warnings: ignore them for `vendor/` (deny warnings per crate only). Errors: this would force either a ruff bump first (a job of its own, before PY2) or holding the repo toolchain at 1.98 until TypeScript starts |
| `languages/python/Cargo.toml` and `vendor/ruff/Cargo.toml` fight over who is the workspace root | PY1: "current package believes it's in a workspace when it's not" | `exclude = ["vendor"]` in ours. pylspt avoided this by having no `[workspace]` at all; a real workspace needs the exclude |
| core's `Outline` is flat and pylspt's `DefNode` is a tree; something downstream wanted the tree | PY2 or PY5 does not compile | `analyzer_core::outline` offers `children_of(&outline, index)`; the solver uses ty's own definitions, not the outline, so this should only touch `python-syntax` |
| the core renderer and the old one differ | PY6's test | §3 above. This is expected work, not an accident |
| byte `Site`s versus the old line/column `Span` | the golden graph differs in every edge | expected: compare after converting the golden file's spans with a throwaway script, or compare the **tree** form (which has no spans) and spot-check ten edges by hand |
| the rule "only `sem.rs` imports the engine" does not fit `interp/` | `scripts/check-all.sh --static` fails for Python | pylspt's values and keys hold ty handles (`Definition<'db>`, `Type<'db>`) throughout `interp/`. Do **not** refactor the solver to hide them: that is a rewrite, and the PA plan promises a move. Python gets a written exception in the check: `interp/` may *name* engine types that `sem` re-exports (`use crate::sem::{Definition, Type}`), and may not call engine *functions*. That is already how pylspt is written ("`sem/` is the only code that imports the vendored ty crates") |
| both repos change while the move is under way | pylspt keeps changing (D14): an experiment, or a fix | everything is read from the hash of PY0, so an experiment cannot leak in. A fix you want is made in this repo; a pylspt commit is brought over only on purpose, as a change of its own that names the new hash |

Next: [`../06-roadmap.md`](../06-roadmap.md)
