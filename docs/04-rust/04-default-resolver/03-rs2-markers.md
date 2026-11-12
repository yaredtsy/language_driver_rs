# 04.04.03 RS2: markers

```
RS2
├── 1. the public functions
├── 2. where the marker lives
├── 3. reading
├── 4. planning an edit
├── 5. when no edit is planned: Skip
├── 6. the file header
├── 7. tests
└── 8. steps, and the tag
```

## 1. The public functions

The same five as `typescript-syntax`, with `Edition` where it has `Dialect`:

```rust
pub fn marker(text: &str, edition: Edition, def: &Def, style: MarkerStyle<'_>) -> Option<String>;
pub fn plan_marker(text: &str, edition: Edition, def: &Def, marker: &str, style: MarkerStyle<'_>) -> Result<Edit, Skip>;
pub fn header_marker(text: &str, style: MarkerStyle<'_>) -> Option<String>;
pub fn plan_header_marker(text: &str, marker: &str, style: MarkerStyle<'_>) -> Result<Edit, Skip>;
```

A library plans an edit and never writes a file or invents the marker text (rule 2). **[check]** the exact parameter order against
`typescript-syntax/src/lib.rs` when writing it: one shape across languages is worth more than a nicer one here.

## 2. Where the marker lives

D6: in the doc comment. The label is the caller's (`MarkerStyle.label`); only `Placement::Docstring` is built.

```rust
/// Parses an order.
///
/// ID: 3RGJ8M2K                 ← the marker: the last line of the item's doc comment
#[inline]
pub fn parse_order(raw: &str) -> Order {
```

## 3. Reading

| Point | Rule |
|---|---|
| which comments | the item's own **outer** doc comments: `///` lines and `/** */` blocks that are children of the item's node (S-RS4) |
| which line | the first line that reads `<label>: <text>` after the `///` and one optional space (or after the leading `*` in a block). `text` is the rest of the line, trimmed at the end |
| a blank line between the doc lines and the item, or between two doc lines | whatever the engine's node holds is the item's (S-RS4 says which). TypeScript's lesson (D15, point 4): follow what the compiler applies, not what looks tidy |
| `#[doc = "…"]` | not read |
| inner doc comments (`//!` inside a module or a body) | belong to the enclosing item. Never read as this def's marker |
| `////`, `//`, `/* */`, `/***/` | not doc comments in Rust. Not read |
| two marker lines | the first wins; `plan_marker` answers `AlreadyMarked` with it |

## 4. Planning an edit

| The item has | The edit |
|---|---|
| `///` lines, the last one empty (`///`) | insert `/// <label>: <text>` after it |
| `///` lines, the last one not empty | insert an empty `///` line, then the marker. In Markdown a line directly under a paragraph joins it, so without the empty line rustdoc would glue the marker to the user's sentence |
| a `/** */` block as its **last** doc comment | the marker goes on a line of its own before `*/`, with the block's own ` * ` prefix. TypeScript's JSDoc code does this text work; this is its second user, so the helper moves to `analyzer-core::edit` now (rule of two), with TypeScript's tests moving with it |
| doc comments and attributes interleaved (`/// a` · `#[inline]` · `/// b`) | after the **last doc comment**, wherever it stands |
| only `#[doc = "…"]` attributes | a new `///` line above the item's first attribute. The attributes are not edited |
| no doc at all | one new line `/// <label>: <text>` directly above the item's first attribute or keyword, with the item's indentation |

Indentation is copied from the item's first line. Line endings are copied from the file (`\r\n` stays `\r\n`): core's edit helpers do it.

Two side effects to state in `languages/rust/docs/markers.md`, because users will meet them:

- the marker line is published by `rustdoc`. That is the accepted cost of D6;
- an item that had no docs now "has docs" for `#![warn(missing_docs)]`.

## 5. When no edit is planned

`Skip` is `#[non_exhaustive]`, every variant has a reason a person can read (`Display`), as D15 point 3 asks.

| `Skip` | When |
|---|---|
| `AlreadyMarked { existing }` | §3 found one |
| `NotOnOwnLine` | something other than blanks stands before the item on its first line (`fn a() {} fn b() {}`, generated code). Never reformat the user's code to make room |
| `SyntaxError` | a parse error overlaps the item or its doc comments |
| `UnsafeDocComment` | the last doc comment is a `/** */` block we cannot edit safely (closed on its opening line, a line without its `*`) |
| `InvalidText` | the text is empty, starts or ends with a blank, or has a line break, or `*/` |
| `UnknownDef` | the `Def` is not one this text's outline has (a stale outline) |
| `PlacementNotSupported` | any placement but `Docstring` |

Not skips, because they are not defs and so are never asked about: items inside a macro call, enum variants, fields.

## 6. The file header

A plain comment in every language: `// <label>: <text>`, through core's `comment_marker` with the prefix `//`.

| Point | Rule |
|---|---|
| where | after a leading licence comment (a `//` run or a `/* */` block at byte 0, whatever its style: D15 point 5) and a `#!` shebang line, and **before** inner attributes and inner doc comments (`#![…]`, `//!`). Those must come before any item but may follow a plain comment |
| a file that starts with `//!` | the header goes above it, followed by an empty line, so it cannot be read as part of the crate docs (it is `//`, not `//!`, so rustdoc ignores it anyway) |
| reading | the first `// <label>: …` line before the first item, inner attribute or inner doc comment |

## 7. Tests

| Test | What it proves |
|---|---|
| `analyzer_testkit::syntax::marker_round_trip` on every def of every fixture | plan → apply → outline again → the same defs, each with its marker; a second plan is `AlreadyMarked` |
| the cut-every-50-bytes test, now also planning a marker on each def | never panics |
| one fixture per row of §3, §4, §5 and §6 | the tables are the tests |
| **`rustfmt` leaves it alone**: every marked fixture is formatted with the repo's `rustfmt.toml`, and again with `wrap_comments = true`, and the marker line is byte-identical | the first plan's **[check]**. A test, not a spike, because a `rustfmt` update can change the answer |
| **`rustc` reads it as docs**: `cargo doc` is not run; instead the marked text is parsed again and the marker line is among the item's doc comment children | the marker did not detach the docs from the item or attach itself to the next one |
| line endings: one `\r\n` fixture | the edit keeps them |

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `marker`: reading, §3 | fixtures |
| 2 | `plan_marker` for `///` and no-doc items; `Skip` | the round trip on those fixtures |
| 3 | the `/** */` helper moves to core; `plan_marker` for blocks | core's tests and TypeScript's stay green (rule 5); the round trip on every fixture |
| 4 | the header | fixtures; the round trip |
| 5 | `rust-cli marker` (plan and, in the CLI only, apply) | a marker written into a scratch copy of a file of this repo, and read back |
| 6 | `languages/rust/docs/markers.md`; `ENGINE.md`'s list of engine items | reviewed |
| | **tag `rust-v0.1.0`** | the qualname rule is frozen from here |

Next: [`04-rs3-project.md`](04-rs3-project.md)
