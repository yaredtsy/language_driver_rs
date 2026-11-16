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

A library plans an edit and never writes a file or invents the marker text (rule 2). **[check] ✅ 2026-11-16:** the order matches
`typescript-syntax/src/lib.rs` (`text`, dialect/edition, `def`, marker text, `style`; the header pair takes no dialect). Name the parameter
`marker_text` as TypeScript does, not `marker`, which is also the name of the reading function. One shape across languages is worth more
than a nicer one here.

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
| which line | the first line that reads `<label>: <text>` after the `///` and **any** blanks (in a block: after its leading `*`, if the line has one, and any blanks; the first line after the `/**`). Blanks may stand on either side of the `:` (`ID:x`, `ID : x`); `text` is the rest of the line, trimmed; an empty text is no marker. The label is case-sensitive and must start the line (`see ID: x` is not one). This is TypeScript's and core's `comment_marker` rule (decided 2026-11-16, RS2 step 1 review) |
| why any blanks, not one | a marker that goes unread makes `plan_marker` plan a second one, a new ID beside the old; that is worse than the opposite case. Reading only one space would not keep code examples out anyway: an `ID: x` line inside a fenced (` ``` `) example is read under either rule. What any-blanks adds is a Markdown indented code line (`///     ID: x`); accepted, as in TypeScript. Planning stays strict (§4, §5 `UnsafeDocComment`): read tolerantly, write one shape |
| why not Python's reader | Python (`python-syntax/src/ids.rs`) is looser still: any `<label>:` word pair anywhere in the docstring, the value one word, the **last** one wins. It must be: Python's own planner (`python-syntax/src/edit.rs`, the single-quoted docstring and the one-line `def f(): """doc"""`) writes the marker **after the user's text on the same line**, because a new line would not parse there, so a line rule would miss markers Python itself wrote. A `///` comment can always take a new line, so Rust's planner always writes a line of its own and its reader can ask for one. Rust keeps the line rule above and TypeScript's "first wins" |
| a blank line between the doc lines and the item, or between two doc lines | whatever the engine's node holds is the item's. S-RS4 answered: a blank line does **not** detach a `///` block (`ENGINE.md`). TypeScript's lesson (D15, point 4): follow what the compiler applies, not what looks tidy |
| an outer doc comment above the item that a plain comment and a blank line separate from the next doc line: `/// a` · `// plain` · blank line · `/// b` · item | **S-RS4 answered (2026-11-16, `ENGINE.md`): the engine's node holds both.** Doc comments are not trivia in `ra_ap_parser 0.0.358`; the grammar eats them as `DOC_COMMENT` attributes, so `/// a`, the `// plain` (a `COMMENT` child) and the blank line are all inside the item's node, in LF and in CRLF. `rustc`'s count and the engine's agree. The first reading of `n_attached_trivias` (a doc line outside the node) was wrong: that function only sees plain comments |
| `#[doc = "…"]` | not read |
| inner doc comments (`//!` inside a module or a body) | belong to the enclosing item. Never read as this def's marker |
| `////`, `//`, `/* */`, `/***/` | not doc comments in Rust. Not read |
| two marker lines | the first wins; `plan_marker` answers `AlreadyMarked` with it |

## 4. Planning an edit

| The item has | The edit |
|---|---|
| `///` lines, the last one empty (`///`) | insert `/// <label>: <text>` after it |
| `///` lines, the last one not empty | insert an empty `///` line, then the marker. In Markdown a line directly under a paragraph joins it, so without the empty line rustdoc would glue the marker to the user's sentence |
| a `/** */` block as its **last** doc comment | the marker goes on a line of its own before `*/`, with the block's own ` * ` prefix. TypeScript's JSDoc code does this text work; this is its second user, so the helper moves to `analyzer-core::edit` now (rule of two), with TypeScript's tests moving with it. **An empty ` *` line goes first**, for row 2's reason (rustdoc would glue the marker to the sentence above), unless the line above is already empty (` *`, blanks only) or is a bare `/**`. TypeScript keeps its shape (no empty line, as tagged): the choice is a parameter of `block_marker::plan`. Decided 2026-11-16 (RS2 step 3 review) |
| a one-line block as the last doc comment (`/** Summary. */`) | `UnsafeDocComment` (§5). TypeScript rewrites such a block into several lines; Rust does not, because doc blocks are rare in Rust and a rewrite edits the user's text. Revisit if a project shows them |
| doc comments and attributes interleaved (`/// a` · `#[inline]` · `/// b`) | after the **last doc fragment**, wherever it stands |
| a `#[doc = …]` attribute as the last doc fragment (`/// a` · `#[doc = "b"]`, or only `#[doc = include_str!("x.md")]`) | after it, an empty `///` line first. A `#[doc = …]` attribute is a doc fragment for **placing** (rustdoc joins it with the `///` lines, in source order) but never **read** (§3). Put before it, the marker would be the item's rustdoc summary (`include_str!`) or glue to its first sentence (`ID: x` · `b` is one Markdown paragraph). `#[doc(hidden)]` and other `doc(…)` lists are not fragments. The attributes are not edited. Changed 2026-11-16 (RS2 step 2 review; the step-2 code put the line above the first attribute) |
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
| `SyntaxError` | a parse error overlaps the item or its doc comments (`Def::syntax_error`); or the file has any parse error and the edit, applied to a copy and outlined again, does not give back the same defs with the marker on this one and every other def's marker unchanged. Found in step 2: an unfinished item with no name above a clean def (`struct` · `enum Complete {}`) takes the new doc line, though `Complete` touches no error. The trial costs one parse, only in a file with errors; a clean file needs none (the round trip proves it) |
| `UnsafeDocComment` | the last doc fragment is a `/** */` block we cannot edit safely: closed on its opening line, a middle line without its `*`, or text before the `*/` on the closing line (core's `block_marker::plan` answers `None`) |
| `InvalidText` | the text is empty, starts or ends with a blank, or has a line break, or `*/` |
| `UnknownDef` | the `Def` is not one this text's outline has (a stale outline) |
| `PlacementNotSupported` | any placement but `Docstring` |

`DetachedDocComment` was in this table until RS2 step 3 and is deleted: S-RS4 found no shape that produces it in 0.0.358 (an outer doc line
above an item is always the item's child, §3), and no fixture of steps 1–3 did either.

**Order of the checks**, as in Python (`python-syntax/src/edit.rs`) and TypeScript: `PlacementNotSupported`, `UnknownDef`, `InvalidText`,
`SyntaxError`, `AlreadyMarked`, then the shape (`NotOnOwnLine`, `UnsafeDocComment`). `AlreadyMarked` carries a fact the caller uses (the
existing marker), so a marked def answers it even when its line is shared (`struct A; /// ID: x` · `fn b() {}`).

Not skips, because they are not defs and so are never asked about: items inside a macro call, enum variants, fields.

## 6. The file header

A plain comment in every language: `// <label>: <text>`, through core's `comment_marker` with the prefix `//`.

| Point | Rule |
|---|---|
| where | after a leading licence comment (a `//` run or a `/* */` block at byte 0, whatever its style: D15 point 5) and a `#!` shebang line, and **before** inner attributes and inner doc comments (`#![…]`, `//!`). Those must come before any item but may follow a plain comment |
| a file that starts with `//!` | the header goes above it, followed by an empty line, so it cannot be read as part of the crate docs (it is `//`, not `//!`, so rustdoc ignores it anyway) |
| the header joins a `//` run (a licence, or a note glued to the first item) | an **empty `//` line** goes between the run and the header, unless the run's last line is already an empty `//`. Found in RS2 step 4: `rustfmt` with `wrap_comments = true` refills a `//` run as one paragraph when one of its lines is wider than `comment_width`, and joins a header that is the run's last line to the text above (`// end ID: x`), which is then not read, and a second header is planned. An empty `//` line is a paragraph break for `rustfmt`, as an empty `///` is for rustdoc (§4 row 2); measured safe. Always written, not only under a wide line: the width is the user's setting. A header after a `/* */` block needs none (`rustfmt` does not join a `//` line to a block). Not done: a blank line before the header (it would part the header from the licence it ends) or the header above the licence (D15 point 5). Python and TypeScript need no such line: their formatters do not refill comments. Decided 2026-11-16 (RS2 step 4 review) |
| reading | the first `// <label>: …` line before the first item, inner attribute or inner doc comment |
| "before the first item" | measured from the item's start as RS1's `range` gives it (its first doc comment or attribute, else its first token that is not a comment or whitespace), **never** from the start of the engine's node. The node can begin with plain comments: a header written directly above the first item is inside the first item's node in any file, and in a CRLF file it is inside even across a blank line (S-RS4). Measured from the node, such a header would not be found and a second one would be planned |

## 7. Tests

| Test | What it proves |
|---|---|
| `analyzer_testkit::syntax::marker_round_trip` on every def of every fixture | plan → apply → outline again → the same defs, each with its marker; a second plan is `AlreadyMarked` |
| the cut-every-50-bytes test, now also planning a marker on each def | never panics |
| one fixture per row of §3, §4, §5 and §6 | the tables are the tests |
| **`rustfmt` leaves it alone**: every marked fixture is formatted with the repo's `rustfmt.toml`, and again with `wrap_comments = true`, and the marker line is byte-identical | the first plan's **[check]**. A test, not a spike, because a `rustfmt` update can change the answer |
| **`rustc` reads it as docs**: `cargo doc` is not run; instead the marked text is parsed again and the marker line is among the item's doc comment children | the marker did not detach the docs from the item or attach itself to the next one. **Covered by the round trip** (step 2 review): reading takes only the item's own `DOC_COMMENT` children (§3), and the round trip asks for the same defs with the marker on this one, so no separate test |
| line endings: one `\r\n` fixture | the edit keeps them |
| the header directly above the first item; the header, a blank line and the first item in a `\r\n` file | §6's "before the first item": found, and a second plan is `AlreadyMarked` |
| `rustfmt` with `wrap_comments = true` on a header under a `//` run with a line wider than `comment_width` | the header line comes out unchanged and is read (§6, the empty `//` line). The canary that showed the join stays as the reason |

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `marker`: reading, §3 | fixtures |
| 2 | `plan_marker` for `///` and no-doc items; `Skip` | the round trip on those fixtures |
| 3 | the `/** */` helper moves to core, and with it `own_line` and `fits_a_line` (written and tested twice now: TypeScript's `marker.rs` and Rust's; one function, with the `*/` check TypeScript makes at its call site); `plan_marker` for blocks | core's tests and TypeScript's stay green (rule 5); the round trip on every fixture; §7's `rustfmt` test on every marked fixture |
| 4 | the header | fixtures; the round trip |
| 5 | `rust-cli marker` (plan and, in the CLI only, apply) | a marker written into a scratch copy of a file of this repo, and read back |
| 6 | `languages/rust/docs/markers.md`; `ENGINE.md`'s list of engine items | reviewed |
| | **tag `rust-v0.1.0`** | the qualname rule is frozen from here |

Next: [`04-rs3-project.md`](04-rs3-project.md)
