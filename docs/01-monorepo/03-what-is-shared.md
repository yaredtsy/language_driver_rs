# 01.03 What is shared

```
what is shared
├── 1. the rule: share the answer, not the analysis
├── 2. analyzer-core, module by module
│   ├── text        positions and ranges
│   ├── outline     the types an outline returns
│   ├── edit        text edits · the line-comment marker
│   ├── graph       contexts and call edges (from pylspt)
│   ├── render      graph → tree (from pylspt)
│   └── solve       the flat solver, for a default resolver
├── 3. analyzer-testkit
├── 4. what is NOT shared, and why
└── 5. the rule of two
```

## 1. The rule: share the answer, not the analysis

The PA plan said "do not add a shared common crate: guessing the shared parts before the second analyzer exists gets them wrong". That warning is
still right about **analysis** code. A Python environment, a TypeScript symbol and a Rust trait have nothing in common.

What the languages do have in common is **what they hand back**. An outline is a list of definitions in any language. A call graph is contexts
and edges in any language. And you asked for one thing explicitly: every language builds the tree by the same rules. A rule that lives in three
copies is three rules.

So `analyzer-core` holds:

1. the **types of the answers** (so a driver, a test and a CLI treat every language the same way),
2. the **code that works on those types only** (rendering a graph into a tree, applying edits),
3. small things that are **not about any language** (byte offset ⇄ line and column).

It holds no parser, no type checker, no `salsa`. Its dependencies are `serde` and one hash crate.

## 2. analyzer-core, module by module

```rust
// core/analyzer-core/src/lib.rs — the whole public surface
pub mod text;      // Span, LineIndex, Position
pub mod outline;   // Def, DefKind, Outline, SyntaxError
pub mod edit;      // Edit, apply, MarkerStyle, comment_marker
pub mod graph;     // Graph, Context, Edge, EdgeTo, StableDef, ContextId, Status, Stats
pub mod render;    // render, Node, Mark, RenderOptions, to_text
pub mod solve;     // Lang, Solver, Limits, Cancel
```

### `text`

| Item | What | Comes from |
|---|---|---|
| `Span { start: u32, end: u32 }` | a byte range in one file's UTF-8 text | every engine has its own (ruff's `TextRange`, tsrs's `TextRange`, rust-analyzer's `TextRange`). Each `sem.rs` converts to this one at its edge |
| `LineIndex` | byte offset ⇄ (line, column), column in UTF-8 bytes or UTF-16 units | pylspt `lines.rs` + `positions.rs`, and the driver's `positions.rs` |

All three engines count positions in UTF-8 bytes, so `Span` is a plain copy of two numbers in each case. The UTF-16 conversion is the wire's
rule, so the *driver* calls it; it lives here so that three drivers do not write it three times.

### `outline`

```rust
#[non_exhaustive]
pub struct Def {
    pub qualname: String,          // the language's own rule; never changes once released
    pub name: String,
    pub kind: DefKind,
    pub parent: Option<usize>,     // index of the enclosing def in the same list
    pub range: Span,               // the whole definition: first decorator / attribute / doc comment → last byte
    pub name_range: Span,          // just the name, for "reveal in editor"
    pub params: Vec<String>,
    pub marker: Option<String>,    // the marker text found on this def, exactly as written
    pub syntax_error: bool,        // a parse error inside this def
}

#[non_exhaustive]
pub enum DefKind { Function, Method, Class, Struct, Interface, Trait, Enum, Module, Namespace, Type, Other(String) }

pub struct Outline { pub defs: Vec<Def>, pub file_marker: Option<String>, pub syntax_errors: Vec<SyntaxError> }
```

This is the driver protocol's `Def` (`vnoc-driver-protocol`) with the V-NOC words removed: `id` becomes `marker`, and the kinds are the open set
the V-NOC plan already asks for (`09-adding-a-language/02` §2). The list is flat, in source order, parents before children: the same contract.

### `edit`

| Item | What |
|---|---|
| `Edit { at: Span, text: String }` | one replacement, in the coordinates of the text it was planned on |
| `apply(text, &[Edit]) -> String` | for tests and the CLI. A driver sends edits to the editor instead |
| `MarkerStyle { label, placement }` | the label (`"ID"`) is the caller's (row PA6). `Placement::Docstring` is the only placement built now (D6); the enum is `#[non_exhaustive]` so a comment-above placement can be added later |
| `comment_marker::read(text, at, prefix, style)` | find a `<prefix> <label>: <text>` comment line at a place in the text |
| `comment_marker::plan(text, at, prefix, style, marker_text)` | the edit that inserts that line, copying the indentation there and the file's line ending |

The comment-marker pair is here because **every language's file header uses it**: the file's own marker is a comment at the top of the file
(`#` in Python, `//` in TypeScript and Rust). The functions know nothing about a language: they take an offset and a comment prefix. Deciding
*which offset* is each language's job. The same pair is what a later comment-above placement for defs would call.

The marker of a **def** lives in its docstring (D6), and that is per-language work: a Python docstring, a JSDoc block and a `///` run are three
different shapes ([`03-typescript/04-ts2-markers.md`](../03-typescript/04-ts2-markers.md)). The one overlap, a `/** … */` block in TypeScript
and in Rust, moves here when Rust needs it.

### `graph`

pylspt's `GraphOut`, `ContextOut`, `EdgeOut`, `EdgeTo`, `StableDef`, `StableContextId`, `Span`, `Status`, moved here with three renames. The
details and the reasons are in [`02-tree-rules/01-graph-and-tree.md`](../02-tree-rules/01-graph-and-tree.md).

### `render`

pylspt's `render.rs`, changed in one way: it reads the plain `Graph` instead of the Python solver's internal tables. Then one renderer serves
every language, and "the tree rules" are one function with one set of tests.

### `solve`

A small solver for a language that uses its engine's default resolver: memoize one summary per context, follow edges, stop at limits, build the
`Graph`. TypeScript and Rust both use it from day one. Python does **not**: it keeps its own solver and only produces the same `Graph`.
[`02-tree-rules/03-the-seam.md`](../02-tree-rules/03-the-seam.md) has the trait.

## 3. analyzer-testkit

A dev-dependency only. It turns "how a language is tested" into three function calls, so a new language gets the same checks for free.

```rust
// languages/typescript/crates/typescript-syntax/tests/syntax.rs — the whole file
use analyzer_testkit::syntax::{self, Subject};

const TS: Subject = Subject {
    outline: typescript_syntax::outline,
    plan_marker: typescript_syntax::plan_marker,
    fixtures: "tests/fixtures",
};

#[test] fn outline_snapshots() { syntax::snapshot_every_fixture(&TS); }
#[test] fn round_trip()        { syntax::marker_round_trip(&TS); }
#[test] fn never_panics()      { syntax::cut_every_50_bytes(&TS); }
```

| Testkit function | Checks | From |
|---|---|---|
| `snapshot_every_fixture` | the outline of every file in `fixtures/` equals its reviewed snapshot | PA plan `02-target/04` §1 |
| `marker_round_trip` | plan a marker on every def, apply, outline again: the marker is read back, and no byte outside the edit changed | the driver contract's round-trip and no-damage rules |
| `cut_every_50_bytes` | the outline of every fixture cut at every 50th byte returns (on a character boundary) | PA plan, "never panics" |
| `scenarios::run(lang, level)` | for every folder in `scenarios/`: build the graph, render, compare with `expected.<level>.tree` | new; [`02-tree-rules/03`](../02-tree-rules/03-the-seam.md) §5 |
| `graph::check_invariants(&Graph)` | sorted, no dangling ids, entries present, deterministic across two runs | pylspt `14-output.md`, "Determinism" |

> **🦀 Rust note: a struct of function pointers instead of a trait.** `Subject` holds plain `fn` pointers:
> ```rust
> pub struct Subject {
>     pub outline: fn(&str) -> analyzer_core::outline::Outline,
>     pub plan_marker: fn(&str, &analyzer_core::outline::Def, &str) -> Option<analyzer_core::edit::Edit>,
>     pub fixtures: &'static str,
> }
> ```
> A trait (`impl Syntax for TypeScript`) would work too. A struct of `fn` pointers is the lighter tool when there is no state and no generic code:
> the language crate needs no marker type, and the test file is a constant. Use a trait when the implementor carries data or when you want static
> dispatch through generics, as `solve::Lang` does.

## 4. What is NOT shared, and why

| Thing | Why each language keeps its own |
|---|---|
| the parser, the AST walk, "what is a def" | the whole content of a language |
| qualname rules | Python has `<locals>`, Rust has `impl` blocks and traits, TypeScript has assigned arrow functions |
| **where** a def's marker goes | a Python docstring, a JSDoc block, a Rust `///` run: three shapes, three rules |
| project loading | `pyproject.toml` + a virtual environment, `tsconfig.json` + `node_modules`, `Cargo.toml` + a sysroot |
| Python's context-sensitive solver (`interp/`) | 4,600 lines written against ty's types. It stays in `python-analysis` until a second language needs the same machinery ([`03-typescript/08`](../03-typescript/08-later-context-sensitive.md) §4) |
| the developer CLI | about 100 lines per language; a shared one would have to link every engine into one binary, which §1 of the last file rules out |
| the V-NOC wire, `FrameJson`, `"FunctionSchema/<id>"` | product; lives in `vnoc/drivers/<lang>` |

## 5. The rule of two

A piece of code moves into `core/` when **both** are true:

1. two languages have it, written and tested, and
2. the two copies differ only in types the core already has.

Until then it stays in the language. This is the PA plan's warning turned into a rule you can apply. Everything in §2 passes it today: the graph
and renderer exist in pylspt and are needed unchanged by TypeScript; the comment marker is needed by TypeScript, Rust and Python's header at the
same time.

The other direction matters as much: **a change to `core/` needs every language's tests to pass in the same pull request.** CI enforces it
([`05-ci.md`](05-ci.md) §2). That cost is what keeps `core/` small.

Next: [`04-engines.md`](04-engines.md)
