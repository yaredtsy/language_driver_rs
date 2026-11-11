# 05.02 Python: what moves where

```
what moves where
├── 1. four destinations
├── 2. to core/analyzer-core            (in B0, before any Python crate exists)
├── 3. to languages/python/crates/python-syntax
├── 4. to languages/python/crates/python-analysis
├── 5. to languages/python/crates/python-cli
├── 6. down from the V-NOC driver
├── 7. stays in the pylspt archive
└── 8. everything that is not source
```

This is the PA plan's `03-move/01-what-moves.md` with a fourth destination added: `core/`. Line counts are from pylspt commit `670ae79a15`.

## 1. Four destinations

```
pylspt/src/
├── lines, positions ───────────────────────────────► core/analyzer-core/src/text.rs
├── stable (StableDef) ─────────────────────────────► core/analyzer-core/src/graph.rs
├── interp/graph (types), canon (the id type) ──────► core/analyzer-core/src/graph.rs
├── interp/tree, render (the rules) ────────────────► core/analyzer-core/src/render.rs
│
├── defs, qname, edit, scan::id_from_body ──────────► languages/python/crates/python-syntax/
├── db, sem, attrs, mro, interp/* ──────────────────► languages/python/crates/python-analysis/
├── bin/* ──────────────────────────────────────────► languages/python/crates/python-cli/
│
└── main, nodes, scan, inject, interp/wire ─────────► stay in the archive
```

## 2. To `core/analyzer-core`

Done in **B0**, as the way the core is born: its first version is pylspt's own code with the Python removed. That is better than writing the
core from a blank page and adapting Python to it later.

| From pylspt | To | Verdict | What changes |
|---|---|---|---|
| `lines.rs` (33), `positions.rs` (91) | `text.rs` | move | one `LineIndex`; add the UTF-16 column function from the V-NOC driver's `positions.rs` (28) |
| `stable.rs`: `StableDef` (the enum only) | `graph.rs` | move | `Node(String)` → `Marked(String)` |
| `interp/canon.rs`: the `StableContextId` struct | `graph.rs` | move the **type** | renamed `ContextId`. The canonical *encoding* of a Python key stays in `python-analysis` |
| `interp/graph.rs`: `GraphOut`, `ContextOut`, `EdgeOut`, `EdgeTo`, `Span` | `graph.rs` | move the **types** | the renames and field changes of [`02-tree-rules/01`](../02-tree-rules/01-graph-and-tree.md) §5. The function `graph(&Solver, …)` stays in `python-analysis`: it reads the solver |
| `interp/tree.rs` (63): `Node`, `Mark`, `to_text` | `render.rs` | move | `Node` gains `func`, `contexts` |
| `interp/render.rs` (173): the rules | `render.rs` | **rework** | reads `Graph` instead of `Solver`; produces `Node` instead of `FrameJson`; adds `only_marked`, `show_external`, `show_unresolved` |
| `interp/mod.rs`: `Stats`, `Cuts` | `graph.rs` | move the general fields | Python-only counters (`prologue_walks`, `annotation_fallbacks`, `types_dropped`, `lifted_subclass_of`, `chain_fanout_hits`) go into `Stats.language` |
| `edit.rs`: the edit struct and `apply` | `edit.rs` | move | plus the new `comment_marker` (written for TypeScript; used by Python's header) |

New in core, not from pylspt: `outline.rs` (the types, shaped after the driver protocol's `Def`), `solve.rs` (the flat solver; Python does not
use it), and `analyzer-testkit`.

## 3. To `python-syntax`

Unchanged from the PA plan, except the return types.

| Module (lines) | Verdict | What changes |
|---|---|---|
| `defs.rs` (192) | **rework** → `outline.rs` | parse text with `ruff_python_parser` instead of the salsa query; `DefScanner` kept; **returns `analyzer_core::outline::Outline`** (flat, parents first) instead of a tree of `DefNode` |
| `qname.rs` (82) | **rework**, folded into `outline.rs` | `<locals>` names from the nesting stack during the walk |
| `edit.rs` (285) | **rework** → `ids.rs` + `edit.rs` | `plan_ids` (a closure that makes IDs) → `plan_marker` (the caller passes the text); the one `ty_python_core` import replaced; returns `analyzer_core::edit::Edit` |
| `scan.rs`: `id_from_body` only | move → `ids.rs` | the label comes from `MarkerStyle` |
| `check_positions.rs` (61) | move, as a test | |
| — | **new**: `sem.rs` | the only module that imports `ruff_python_parser` / `ruff_python_ast` / `ruff_text_size` (rule 7). pylspt has this habit in the analysis part; the syntax part gets it now |
| — | **new**: `header.rs` | from the driver's `file_id.rs`; calls `analyzer_core::edit::comment_marker` with `#` |

The public surface is the PA plan's (`02-target/02` §2) with core types:

```rust
pub use analyzer_core::outline::{Def, DefKind, Outline, SyntaxError};
pub use analyzer_core::edit::{Edit, MarkerStyle};
pub use analyzer_core::text::Span;

pub fn outline(text: &str, style: MarkerStyle<'_>) -> Outline;
pub fn marker(text: &str, def: &Def, style: MarkerStyle<'_>) -> Option<String>;
pub fn plan_marker(text: &str, def: &Def, marker_text: &str, style: MarkerStyle<'_>) -> Result<Edit, Skip>;
pub fn header_marker(text: &str, style: MarkerStyle<'_>) -> Option<String>;
pub fn plan_header_marker(text: &str, marker_text: &str, style: MarkerStyle<'_>) -> Result<Edit, Skip>;
```

These are the same five functions `typescript-syntax` and `rust-syntax` export (plus a dialect or edition argument there). That sameness is
what lets the testkit and the V-NOC drivers treat the languages alike.

## 4. To `python-analysis`

| Module (lines) | Verdict | What changes |
|---|---|---|
| `db.rs` (29) | move → `project.rs` | `Project::open`, `environment_inputs` (the same names as TypeScript and Rust) |
| `sem.rs` (607) | move | the import of the renamed patch module (`analyzer_ext`); `stable_def` returns core's `StableDef` |
| `stable.rs`: `StableType` | move | stays here; it encodes ty's types |
| `attrs.rs` (123), `mro.rs` (146) | move → `classes/` | |
| `interp/` solver files: `value`, `env`, `walk`, `bind`, `member`, `key`, `summary`, `call`, `defer`, `solver`, `entry`, `relevant`, `canon` (about 4,000 lines) | move | `require_ids` → `only_marked`; the in-memory context id keeps its meaning under a private name; imports of the moved types point at `analyzer_core` |
| `interp/graph.rs`: the function `graph()` | move | builds `analyzer_core::graph::Graph`; fills `defs`: one row for the function of every reached context and one for the class of every `Construct` target ([`02-tree-rules/04`](../02-tree-rules/04-the-defs-table.md) §7); converts line/column spans to relative byte `Site`s |
| `interp/render.rs`, `interp/tree.rs` | **replaced by core** | the old `render.rs` is kept as a private test helper until PY6's comparison passes, then deleted |
| `interp/fixture.rs` (594): the 41 tests | move | they assert on `to_text` output, which now comes from core. **The count of passing tests is the proof for the whole migration** |
| `Mode::Inline` (the earlier walk that follows every call, without summaries) | move | keep it only if the fixture tests still use it as a cross-check **[check]**; not part of the public API |

The public surface:

```rust
pub use analyzer_core::graph::{Graph, Context, Edge, EdgeTo, StableDef, ContextId, Stats};
pub use analyzer_core::render::{render, Node, Mark, RenderOptions, to_text};

pub use project::{Project, OpenError};
pub fn call_graph(project: &Project, file: &str, entry: &Entry, opts: &Options) -> Option<Graph>;
pub use classes::{mro, attributes};
pub struct Options { pub limits: Limits, pub marker: MarkerStyle<'static> }      // Limits: Python's own (fp_depth, max_rounds, …)
```

`call_tree` and `call_wire` from today's `interp/mod.rs` are gone from the public API: the tree is `render(&graph, …)` (rule 1), and the wire
shape is the driver's.

## 5. To `python-cli`

| From | To | Note |
|---|---|---|
| `bin/pylspt-dev.rs` (152) | `python-cli outline`, `python-cli calls [--graph]` | `--merged` is dropped with `merge_for_output` (PA5) unless Q-PA3 says the old parity is still needed |
| `bin/mro.rs` (90), `bin/attrs.rs` (134) | `python-cli mro`, `python-cli attrs` | |
| `bin/ast.rs` (31), `show.rs` (62) | `python-cli ast`, if you still use it (Q-PA5) | |

The subcommand names `outline` and `calls`, and their output format (`to_text` for the tree, JSON for `--graph`), are the same in
`typescript-cli` and `rust-cli`.

## 6. Down from the V-NOC driver

Unchanged from the PA plan (`03-move/01` §3), with one more destination:

| Driver file (lines) | The part that is not V-NOC's | Goes to |
|---|---|---|
| `outline.rs` (115) | parsing, the `<locals>` rule, syntax-error ranges, the flat walk | `python-syntax::outline` |
| `file_id.rs` (119) | "the header is the leading run of comment lines" | `python-syntax::header` + `analyzer_core::edit::comment_marker` |
| `positions.rs` (28) | byte offset → line + UTF-16 column | **`analyzer_core::text::LineIndex`**: every Rust-written driver needs it, so it is written once. The driver still decides *that* the wire uses UTF-16 |
| `plan_ids.rs` (96) | finding a def's body in the tree | `python-syntax::edit` |

## 7. Stays in the pylspt archive

Unchanged (PA5): `main.rs` (201), `nodes.rs` (30), `scan.rs` minus one function (199), `inject.rs` (138), `bin/inject.rs` (47),
`interp/wire.rs` (69); and `plan/00-orientation`, `plan/02-mapping`, `plan/04-build`, all of `experience/`, `.claude/`.

## 8. Everything that is not source

| Thing | Goes to | Note |
|---|---|---|
| `vendor/ruff/` | `languages/python/vendor/ruff/` | copied with `git archive playground-final vendor \| tar -x` (the 171 quoted paths); trimmed of unused test fixtures if [measure] says it is worth it; patch renamed |
| `Cargo.toml` | `languages/python/Cargo.toml` | the workspace root; keeps the two comments about salsa and `zstd`; adds `exclude = ["vendor"]` and the `analyzer-core` path dependency |
| `Cargo.lock` | `languages/python/Cargo.lock` | start from this exact file (PA plan's Rust note: then the only thing that changed is your code) |
| `rust-toolchain.toml` | not moved | the repo root has one |
| `plan/03-call-tree/` (16 files) | `languages/python/docs/call-tree/` | the solver's design. `14-output.md`'s general half is now [`02-tree-rules/`](../02-tree-rules/01-graph-and-tree.md); leave a pointer |
| `plan/01-crates/`, `plan/05-reference/`, `tutorial/` | `languages/python/docs/ruff-and-ty/`, curated (PA7) | |
| `run-jarvis.sh` | `languages/python/scripts/run-on-project.sh <dir> <file> <entry>` | |
| `cli-wire.json` | stays; used once as the golden output | |
| `pylspt/fable_plan/` | stays | this folder (`05-python-migration/`) is its successor for the destination |

Next: [`03-steps.md`](03-steps.md)
