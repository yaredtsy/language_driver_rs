# 05.04 The wire: a fix to the plan

```
the wire
├── 1. what the plan got wrong
├── 2. the decision (D17)
├── 3. the design
├── 4. what the wire holds, field by field
├── 5. what changes in the plan, file by file
├── 6. the steps, each with a proof
└── 7. to confirm
```

A correction decided on 2026-11-11. Until step W5 below is done, the other files still say "the wire is not moved": this file wins.

## 1. What the plan got wrong

The plan treats the context graph as the product and the wire as something old:

| Where | What it says |
|---|---|
| D11, row PA5, Q-PA3 | "no old web-driver parity, `merge_for_output` is dropped"; `interp/wire.rs` stays in the pylspt archive |
| [`02-what-moves-where.md`](02-what-moves-where.md) §4 | "`call_wire` is gone from the public API … the wire shape is the driver's" |
| [`../02-tree-rules/01-graph-and-tree.md`](../02-tree-rules/01-graph-and-tree.md) §4 | "V-NOC's own merge stays in V-NOC, when saving" |

The owner's answer: **the wire is the output that is used.** The graph is how it is computed, not what is consumed. And the merge
(`call_count`) must be available from the analyzer, as an option.

What already went wrong because of it (commit `8363c23`, PY5's first half): `wire.rs` was copied in, then `target_id` and
`merge_for_output` were cut out of it to satisfy rule 1, and the merge test lost its second half. Step W1 puts them back.

## 2. The decision (D17)

1. **The wire tree is a public output of every analyzer**, next to the graph. `python-cli calls --wire` prints it; the library has a function
   that returns it.
2. **Merging is an option**: `merged: false` gives one node per call site with `call_count: 0` (today's default); `merged: true` gives
   pylspt's `merge_for_output`. This replaces "dropped" in D11 and answers Q-PA3 with "yes".
3. **Rule 7 does not change.** The wire is a *view of the graph*, like the tree: `graph → render → Node → wire`. Nothing returns a wire
   that did not come from a `Graph`, so the graph stays the one source and two views cannot disagree.
4. **Rule 1 does not change.** The words `FunctionSchema/` and `ClassSchema/` do not enter the crates: they are **parameters**, the way the
   marker label `ID` is (`MarkerStyle`).

## 3. The design

```rust
// one module, written only against analyzer-core types (Graph, Node, DefInfo, ContextId)

/// How an id is written on the wire. The prefixes are the caller's, never ours.
pub struct WireStyle<'a> {
    pub function_prefix: &'a str,     // V-NOC passes "FunctionSchema/"
    pub class_prefix:    &'a str,     // V-NOC passes "ClassSchema/"
}

pub struct WireOptions<'a> {
    pub style:  WireStyle<'a>,
    pub merged: bool,                 // false: one node per call site · true: siblings with one target become one, call_count summed
    pub render: RenderOptions,        // budget, depth, only_marked: the tree's own options
}

#[non_exhaustive]
pub struct Frame {                    // pylspt's FrameJson, field for field (§4)
    pub target_qname: String,
    pub target_id:    String,
    pub call_count:   u32,
    pub call_site:    Option<LineSpan>,
    pub contexts:     Vec<ContextId>,
    pub marks:        Vec<Mark>,      // omitted from JSON when empty
    pub children:     Vec<Frame>,
}

pub fn wire(graph: &Graph, entry: &ContextId, sources: &dyn Sources, opts: &WireOptions<'_>) -> Frame;
pub fn merge(root: &Frame) -> Frame;  // public on its own too: a consumer that holds an unmerged wire can merge it later
```

- `wire` calls `render(graph, entry, &opts.render)`, turns each `Node` into a `Frame`, and merges when asked. It has no tree rules of
  its own: recursion, cuts and the budget are the renderer's.
- `Sources` gives the text of a file by its project-relative path. The wire needs it for one thing: a call site in the graph is bytes
  relative to the function (`Site`), and the wire wants a line and column. `python-analysis` implements it from its database.
- **Where the module lives.** `python-analysis/src/wire.rs` now. It names no Python and no engine, so at TS4, when TypeScript needs the
  same wire, the file moves to `analyzer-core` unchanged (rule 5, the rule of two). Do not put it in core before that.

> **🦀 Rust note: a view is a function, not a field.** It is tempting to store the wire inside the answer (`Answer { graph, wire }`). Then
> every caller pays for both, and the two can drift. A function `wire(&graph, …)` costs nothing until someone calls it, and it *cannot*
> disagree with the graph because the graph is its only input. `&dyn Sources` is a trait object: `wire` does not care whether the text
> comes from a database, a map in a test, or the disk.

## 4. What the wire holds, field by field

The target is: **`calls --wire` on the Jarvis project gives `cli-wire.json` again** (195 nodes; 170 function ids, 25 class ids).

| Field | From | Same as pylspt's wire? |
|---|---|---|
| `target_qname` | `DefInfo::qname` | **to confirm (§7)**: pylspt writes `jarvis.CallGraphGenerator.do_pass`, with the module. [`02-tree-rules/04`](../02-tree-rules/04-the-defs-table.md) §7 says Python's `qname` has no module |
| `target_id` | `StableDef::Marked(id)` → prefix + id, the prefix chosen by `DefInfo::kind` (class or not). `Path` → `""` | yes, with V-NOC's two prefixes |
| `call_count` | `0`; with `merged`, `Σ call_count + k − 1` over `k` merged siblings | yes |
| `call_site` | `Site` + the def's `range.start` → line and column through `LineIndex` | yes: `[line, column]` pairs, both 1-based, the column counted in characters (what `sem.line_col` gives today) |
| `contexts` | `Node::contexts` | **one rename**: the JSON key `Node` is now `Marked` (`{"func": {"Marked": "…"}}`) |
| `marks` | `Node::marks` | written in snake case as before (`recursive`, `cut`, `incomplete`); the two new marks only appear with `show_external` / `show_unresolved` |
| `children` | `Node::children`, in site order | yes |

The merge rule, unchanged from pylspt: siblings with one `target_qname` become one node; `contexts` and `marks` are unioned; the children
are concatenated and merged the same way.

## 5. What changes in the plan, file by file

| File | Change |
|---|---|
| [`../07-decisions-and-questions.md`](../07-decisions-and-questions.md) | new row D17 (§2); D11's "`merge_for_output` is dropped" struck; Q-PA3 answered "yes" |
| [`01-what-changes.md`](01-what-changes.md) §4, §5 | the `interp/wire.rs` row: "moves, as `wire.rs`"; PA5: `interp/wire.rs` leaves the "stays in the archive" list |
| [`02-what-moves-where.md`](02-what-moves-where.md) §1, §4, §5, §7 | the same; the public surface gains `wire`, `merge`, `WireOptions`, `WireStyle`, `Frame`; `python-cli calls [--graph \| --wire] [--merged]` |
| [`03-steps.md`](03-steps.md) | PY5 and PY6 rows name the wire steps below; PY7's proof compares `--wire` with `cli-wire.json`, and `--wire --merged` with the merged golden file |
| [`../02-tree-rules/01-graph-and-tree.md`](../02-tree-rules/01-graph-and-tree.md) §4, §5 | "V-NOC's merge stays in V-NOC" becomes "the merge is an option of the wire view"; the `FrameJson` row says `Frame`, moved |
| [`../03-typescript/06-ts4-calls.md`](../03-typescript/06-ts4-calls.md), [`../04-rust/02-phases.md`](../04-rust/02-phases.md) | one line each: the CLI prints `--wire`; the module moves to core here (TypeScript) and is reused (Rust) |
| `README.md` | the Python row says what `calls --wire` gives |

`CLAUDE.md` needs no rule change. One sentence is added under "Decisions already made": the wire is a public view of the graph, merged
on request, with the id prefixes as parameters.

## 6. The steps

They fit inside PY5–PY7; no step is added to the roadmap. Each is a commit.

| # | When | Do | Proof |
|---|---|---|---|
| W1 | **done** (`e027756`) | Put back what `8363c23` cut, on the **old** renderer: `target_id` built from a `WireStyle` in `Options` (no product word in the crate), `merge_for_output`, and the second half of the merge test (`p05_02_merge_matches_v_noc`, renamed without the product name) | `cargo test -p python-analysis`: 49 pass (the 46 moved tests and three new ones), and that test checks `children.len() == 1` and `call_count == 1` after the merge; `scripts/check-all.sh --static` passes |
| W2 | **done** | The golden baseline, once, from a worktree of `670ae79a15` (D14): `calls --wire` and `calls --wire --merged` on the Jarvis project, kept outside git. `cli-wire.json` is checked against the first | two files; the unmerged one equals `cli-wire.json` |
| W3 | PY5, after the graph is core's | `wire.rs`: `wire(&graph, …)` and `merge`, built on `Graph` + `render` as in §3. A test on every fixture: the new wire equals the old renderer's wire, after the renames of §4 | the comparison passes on all 41 fixture tests' programs; it is PY6's renderer comparison, one level up |
| W4 | PY6 | Delete the old renderer and the old `FrameJson` with it; `wire.rs` is the only wire | `cargo test -p python-analysis` passes with the same count |
| W5 | PY7 | `python-cli calls --wire [--merged] --function-prefix … --class-prefix …`; the doc edits of §5; delete the sentence under this file's title | `calls --wire` on Jarvis equals W2's file and `--merged` equals the merged one, after the renames of §4; `grep -rn "not moved" docs` finds no line about the wire |

**W2 result (2026-11-11).** Built from `git archive 670ae79a15` (not a worktree: no metadata is added to pylspt's `.git`). On the Jarvis project,
entry `CallGraphGenerator.do_pass`: `wire.json` has **195 nodes and is the same JSON as `cli-wire.json`** (only the pretty-print layout differs);
`wire-merged.json` has **64 nodes and its `call_count` sums to 131** (= 195 − 64). Both are in `out/golden/` (git-ignored), with a checksum file and
the exact commands, so W3 and W5 compare against them. The count of tests is by source, not by running pylspt's suite: 46 tests moved, and the
16 `#[test]` that stayed are in files that stayed. The golden was made with the owner's uncommitted `run-jarvis.sh` (`jarvis.py`), and the pin
reproduces it, so the two agree on that project.

W1 is small and can be done today. It gives back a working, complete wire in this repo before the graph work starts, so nothing later is
judged against a wire that was already cut.

## 7. To confirm

| # | Question | Recommendation |
|---|---|---|
| 1 | `target_qname`: with the module (`jarvis.CallGraphGenerator.do_pass`, as today) or without (`CallGraphGenerator.do_pass`, as the outline)? | **keep the module on the wire**, so what reads the wire today keeps working. The outline's qualname stays without it. It means the wire reads the module from `StableDef::Path` or a new `DefInfo` field, which is decided in W3 |
| 2 | `target_id`: prefixes passed in (`FunctionSchema/…` comes out as today), or the bare id plus a `kind` field and V-NOC adds the prefix? | **prefixes passed in**: the wire stays byte-compatible, and rule 1 holds |
| 3 | The `Node` → `Marked` rename inside `contexts` | **accept it**: the type is core's and is shared with the graph. If something parses that key today, say so and the wire keeps `Node` with a serde rename |

Next: [`../06-roadmap.md`](../06-roadmap.md)
