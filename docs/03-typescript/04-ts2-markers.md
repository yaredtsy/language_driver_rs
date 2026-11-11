# 03.04 TS2: markers

```
TS2
├── 1. what a marker is, for an analyzer
├── 2. the choice: where a TypeScript marker lives
├── 3. the rule, exactly
├── 4. the public functions
├── 5. the file header
├── 6. fixtures and tests
└── 7. steps
```

## 1. What a marker is, for an analyzer

V-NOC writes a small ID into the code so that a doc finds its function again after a rename (tier 2). For the analyzer this is only: **a line of
text with a label, attached to a def**. The analyzer reads it (in the outline's `marker` field) and plans where one would go. It never chooses
the text and never writes a file (rule 2). The label, `ID`, is a parameter (row PA6).

In Python the marker is a line in the docstring (your decision F1). F1 also says "a comment above the def comes later as an option". TypeScript
and Rust have no docstrings, so **this is where that option gets built**.

## 2. The choice

| | A. a line in the JSDoc block | B. a line comment above the def (recommended) |
|---|---|---|
| looks like | `/** Parses an order.`<br>` * ID: 3RGJ… */` | `// ID: 3RGJ…`<br>`export function parseOrder() {` |
| a def with no JSDoc | a JSDoc block must be created for it | one line |
| shows in hover tooltips and generated API docs | **yes**: JSDoc is rendered | no |
| a formatter (Prettier) may move or rewrap it | JSDoc bodies are sometimes rewrapped by plugins | a `//` line directly above a declaration stays put |
| edits an existing comment | yes: must parse JSDoc and keep its style | never touches existing text |
| the same rule works for Rust | no (`///` is rendered by rustdoc; a different rule) | **yes**: `// ID: …` above the item |
| closest to Python's docstring style | yes | no |

**Recommended: B.** It is one rule for TypeScript, JavaScript and Rust; it is the smallest possible diff; and it is invisible to every tool that
renders documentation. It is what users commit, so it is your decision: **Q-M3**. The rest of this file assumes B; A would change §3 only.

## 3. The rule, exactly

**Reading.** The marker of a def is found like this:

```
1. start at the first byte of the def's `range` (TS1 §4): its first decorator, modifier or keyword
2. look at the comments directly above it, nearest first, with only whitespace between them and the def
3. the first `//` comment whose text is   <label>:<one or more spaces><text>   is the marker; <text> is returned exactly as written, trimmed
4. stop at the first blank line, or at anything that is not a comment
```

So a JSDoc block may sit above or below the marker line; both are read. A marker separated from the def by a blank line is **not** read: a
blank line means the comment belongs to the file, not to the def.

**Planning** (where a new marker goes): directly above the first line of the def's `range`, on its own line, with the def's indentation and the
file's line ending.

```ts
/** Parses an order. */
// ID: 3RGJ8M2K…                 ← inserted here: below the JSDoc, above the decorator
@logged
export async function parseOrder(raw: string) {
```

If S-TS4 shows that a line between the JSDoc and the def detaches the JSDoc (in tsrs or in VS Code), the placement becomes "above the topmost
comment that is directly attached to the def" instead. The reading rule already accepts both, so that is a change in `plan_marker` only.

Three cases where the def does not start its own line:

| Case | Example | Rule |
|---|---|---|
| a method in an object literal or class, on its own line | `  load() {` | above it, indented like it |
| `export const f = () => {`, one variable | | above the `export const` |
| several things on one line | `const a = () => 1, b = () => 2;` or a minified file | **skipped**, with the reason "the definition does not start its own line". Never reformat the user's code to make room |

**Not touched, ever** (V-NOC plan `09/04`): a file with a syntax error overlapping the def; generated files are the core's decision, not ours.

## 4. The public functions

```rust
// typescript-syntax/src/lib.rs (added in TS2)
pub use analyzer_core::edit::{Edit, MarkerStyle};

/// The marker on this def, if one is written. The outline already fills `Def::marker` with it.
pub fn marker(text: &str, dialect: Dialect, def: &Def, style: MarkerStyle<'_>) -> Option<String>;

/// The edit that puts `marker_text` on this def, or why it cannot be done.
pub fn plan_marker(text: &str, dialect: Dialect, def: &Def, marker_text: &str, style: MarkerStyle<'_>) -> Result<Edit, Skip>;

#[non_exhaustive]
pub enum Skip { NotOnOwnLine, SyntaxError, AlreadyMarked { existing: String } }
```

Both are thin: they compute "the first byte of the def" and "is it alone on its line", then call
`analyzer_core::edit::comment_marker::{read, plan}` with the prefix `//`. The Rust analyzer calls the same two core functions.

`outline` takes the style too, so that it knows which label to read: `outline(text, dialect)` becomes `outline(text, dialect, style)`. Decide
this signature in TS1 even though the field stays `None` until TS2; changing a public signature after the first tag is a breaking change.

> **🦀 Rust note: `Result<Edit, Skip>` instead of `Option<Edit>`.** `Option` would say "no edit" without saying why, and the driver contract
> requires a reason a person can read (`skipped` entries). An enum of reasons is better than a `String`: the driver can turn each variant into its
> own message, and a test can match on the variant without comparing text. `#[non_exhaustive]` lets a later version add a reason without breaking
> the driver's `match` (it must already have a `_ =>` arm).

## 5. The file header

The file's own marker (`FileID` in V-NOC; here just "the header marker") is a comment at the top of the file. The analyzer answers two questions:

```rust
pub fn header_marker(text: &str, style: MarkerStyle<'_>) -> Option<String>;
pub fn plan_header_marker(text: &str, marker_text: &str, style: MarkerStyle<'_>) -> Result<Edit, Skip>;
```

Where it may go in a TypeScript or JavaScript file:

```
#!/usr/bin/env node                 1. after a shebang line, if there is one (tsrs_scanner::get_shebang)
/* Copyright … */                   2. after the leading licence comment block, if there is one
// FileID: 3RGJ…                    ← here
/// <reference path="…" />          3. BEFORE triple-slash directives and `// @ts-check` pragmas:
// @ts-check                           they only need to be preceded by comments, and a comment is what we add
"use strict";                       4. before any directive or code
import …
```

Python's version of this lives in the V-NOC driver today (`file_id.rs`) and moves down in the Python migration. Both languages end up calling the
same core helper with a different comment prefix.

## 6. Fixtures and tests

Marker fixtures are the outline fixtures: the testkit's `marker_round_trip` runs on every def of every file in `tests/fixtures/`, so every case
of TS1 §6 is also a placement test. It checks, per def:

1. `plan_marker` returns an edit or a `Skip`;
2. apply the edit, outline again: that def's `marker` is the text, every other def is unchanged, and the byte difference between old and new
   text is exactly the inserted line (the **no-damage rule**);
3. plan again on the marked text: `Skip::AlreadyMarked`.

A few files exist only for markers:

```
marker_with_jsdoc.ts        JSDoc above · marker between · both orders are read
marker_blank_line.ts        a marker-looking comment separated by a blank line is NOT the marker
marker_block_comment.ts     `/* ID: x */` is not a marker (only `//`)
marker_indent_tabs.ts       indentation is copied byte for byte
marker_crlf.ts              the inserted line ends with \r\n
one_line_many_defs.js       Skip::NotOnOwnLine
header_shebang.js  header_license.ts  header_reference.ts  header_use_strict.js
```

## 7. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `analyzer_core::edit::comment_marker::{read, plan}` (in `core/`, with its own tests: indentation, line endings, the blank-line rule) | `cd core && cargo test` |
| 2 | `marker.rs`: first-byte and own-line logic; fill `Def::marker` in the outline | snapshots now show markers |
| 3 | `plan_marker` + `Skip` | `marker_round_trip` passes on every fixture |
| 4 | `header.rs` | the four header fixtures |
| 5 | `languages/typescript/docs/markers.md` from §3 and §5 | — |
| 6 | README example; **tag `typescript-v0.1.0`** | a fresh clone builds and tests `typescript-syntax` with one command |
| 7 | in `vnoc`: `drivers/typescript`, tier 1–2 ([`01-monorepo/06`](../01-monorepo/06-versions-and-vnoc.md) §4) | the driver conformance test passes |

Step 7 is `vnoc` work and is listed so that the tag has a consumer. It also needs the V-NOC-side items the plan already lists for a second
language: drivers chosen by file extension, an open `DefKind`, and the comment `style` capability (`06-drivers.md` §3).

Next: [`05-ts3-project.md`](05-ts3-project.md)
