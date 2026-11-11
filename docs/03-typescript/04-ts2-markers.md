# 03.04 TS2: markers

```
TS2
├── 1. what a marker is, for an analyzer
├── 2. the decision: the marker lives in the JSDoc (D6)
├── 3. the rule, exactly
│   ├── reading
│   ├── planning
│   └── when nothing is written (Skip)
├── 4. the public functions
├── 5. the file header
├── 6. fixtures and tests
├── 7. steps
└── 8. the later option: a comment above the def
```

## 1. What a marker is, for an analyzer

V-NOC writes a small ID into the code so that a doc finds its function again after a rename (tier 2). For the analyzer this is only: **a line of
text with a label, attached to a def**. The analyzer reads it (in the outline's `marker` field) and plans where one would go. It never chooses
the text and never writes a file (rule 2). The label, `ID`, is a parameter (row PA6).

## 2. The decision

**D6 (2026-11-11): the marker lives in the docstring, in every language, for now.** A configurable placement comes later.

| Language | "Docstring" means | Looks like |
|---|---|---|
| Python | the docstring (F1, unchanged) | `"""Parses an order.` / `ID: 3RGJ…` / `"""` |
| TypeScript, JavaScript | the JSDoc block above the def | `/**` / ` * Parses an order.` / ` * ID: 3RGJ…` / ` */` |
| Rust | the doc comment above the item | `/// Parses an order.` / `///` / `/// ID: 3RGJ…` |

What this choice costs, so that nobody is surprised: the marker line is **rendered** wherever documentation is rendered (hover tooltips, TypeDoc,
rustdoc). That was accepted with the decision, and it is the same in Python today.

How the later option stays cheap: `MarkerStyle` carries the placement from the first day.

```rust
// analyzer-core::edit
#[derive(Clone, Copy)]
pub struct MarkerStyle<'a> {
    pub label: &'a str,              // "ID"
    pub placement: Placement,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Placement {
    Docstring,                       // the only one built now
    // CommentAbove,                 // §8: added later, as a new variant
}
```

> **🦀 Rust note: leaving room for a variant.** `#[non_exhaustive]` on the enum means code *outside* `analyzer-core` cannot write a `match` on
> `Placement` without a `_ =>` arm. So when `CommentAbove` is added, every language crate still compiles; each one's `_ =>` arm returns
> `Skip::PlacementNotSupported` until that language implements it. Without the attribute, adding a variant would be a breaking change for every
> crate that matches on it.

## 3. The rule, exactly

### Reading

```
1. start at the first byte of the def's `range` (TS1 §4): its first decorator, modifier or keyword
2. take the nearest JSDoc block (`/** … */`) above it: only whitespace and comments between, and NO blank line
3. inside it, the first line that, after its leading `*` and spaces, reads   <label>:<spaces><text>   is the marker
4. <text> is returned exactly as written, trimmed
```

- A blank line between the block and the def detaches it: the comment belongs to the file, not to the def.
- A one-line block (`/** ID: x */`) is read the same way.
- A plain block comment (`/* ID: x */`) or a line comment (`// ID: x`) is **not** read. Only `/**`.
- With several JSDoc blocks above a def, only the nearest one counts. That is the one TypeScript itself uses.
- A `//` or `/* */` comment between the block and the def does **not** detach it. (Changed in the TS2 review: TypeScript attaches the block
  through comments and uses only the last block, so a new block written below the comment hid the user's own. In a checked `.js` file that
  switched its `@param` types off.)
- A marker line that sits under a block tag (after `@param`, `@returns`) is still **read**. It is only never *written* there (below).

### Planning

Where the new line goes depends on what is above the def:

| Above the def | Edit |
|---|---|
| a multi-line JSDoc with **no** block tags | insert ` * <label>: <text>` as the last line before ` */` |
| a multi-line JSDoc **with** block tags (`@param`, `@returns`, …) | insert the line **before the first block-tag line**, so that it ends the description. A line after `@returns` would become part of that tag's text in every tool |
| a one-line JSDoc (`/** Parses. */`) | rewrite that one comment as three lines: `/**`, ` * Parses.`, ` * <label>: <text>`, ` */` |
| no JSDoc (nothing, or a plain `/* */` or `//` comment) | insert a new one-line block `/** <label>: <text> */` directly above the def's first line, **below** any plain comment |

In every case the inserted text copies the block's own indentation and ` * ` prefix (or the def's indentation for a new block) and the file's
line ending.

```ts
/**
 * Parses an order.
 * ID: 3RGJ8M2K…                 ← inserted here: ends the description, before the tags
 * @param raw the wire text
 */
@logged
export async function parseOrder(raw: string) {
```

The one-line case is the only edit that changes existing bytes (it splits one comment into lines). The no-damage test (§6) allows exactly that:
the old comment's text must appear unchanged inside the new block.

Where "above the def" is, for defs that are not a statement of their own:

| Def | The JSDoc is looked for above |
|---|---|
| `export const f = () => {}` (one variable) | the `export const` statement. That is where TypeScript attaches it |
| a class member, an object-literal method | the member's own first line |
| an overloaded function | the **first overload signature** (the def's `range` starts there, TS1 §4) |

### When nothing is written (`Skip`)

| Case | Reason given |
|---|---|
| several defs on one line (`const a = () => 1, b = () => 2;`, a minified file) | `NotOnOwnLine`. Never reformat the user's code to make room |
| a syntax error overlaps the def or its JSDoc | `SyntaxError` |
| the def already has a marker | `AlreadyMarked { existing }` |
| the JSDoc block is malformed (no closing `*/` on its own, mixed prefixes we cannot copy safely) | `UnsafeDocComment`. A new block is **not** stacked on top of it: two JSDoc blocks would hide the user's own |

Generated files and files V-NOC must never touch are the core's decision (V-NOC plan `09/04`), not the analyzer's.

## 4. The public functions

```rust
// typescript-syntax/src/lib.rs (added in TS2)
pub use analyzer_core::edit::{Edit, MarkerStyle, Placement};

/// The marker on this def, if one is written. The outline already fills `Def::marker` with it.
pub fn marker(text: &str, dialect: Dialect, def: &Def, style: MarkerStyle<'_>) -> Option<String>;

/// The edit that puts `marker_text` on this def, or why it cannot be done.
pub fn plan_marker(text: &str, dialect: Dialect, def: &Def, marker_text: &str, style: MarkerStyle<'_>) -> Result<Edit, Skip>;

#[non_exhaustive]
pub enum Skip { NotOnOwnLine, SyntaxError, AlreadyMarked { existing: String }, UnsafeDocComment, PlacementNotSupported }
```

`marker.rs` does the work in two layers:

```
sem.rs       the JSDoc comment range directly above a node            (tsrs: get_leading_comment_ranges for the range; node.jsdoc(file) only to know one is attached; ENGINE.md, S-TS4)
marker.rs    lines of the block · find the label · find the first block-tag line · build the Edit
```

The second layer is plain text work on one comment: it takes the block's text and returns an offset and a string. Keep it free of tsrs types.
Rust's `/** … */` doc comments have the same shape, so by the rule of two this layer may move to `analyzer-core` when `rust-syntax` needs it
(RS2). Not before.

`outline` takes the style too, so that it knows which label to read: `outline(text, dialect, style)`. Decide this signature in TS1 even though
`Def::marker` stays `None` until TS2; changing a public signature after the first tag is a breaking change.

> **🦀 Rust note: `Result<Edit, Skip>` instead of `Option<Edit>`.** `Option` would say "no edit" without saying why, and the driver contract
> requires a reason a person can read (`skipped` entries). An enum of reasons is better than a `String`: the driver can turn each variant into its
> own message, and a test can match on the variant without comparing text.

## 5. The file header

The file's own marker (`FileID` in V-NOC; here just "the header marker") is **a comment at the top of the file**, in every language. D6 does not
change it: a file has no docstring to put it in (your earlier decision: "file ID = comment at the top of the file").

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

Both functions call `analyzer_core::edit::comment_marker::{read, plan}` with the prefix `//`. Python calls the same pair with `#`, Rust with
`//`. That helper is the one piece of marker code all three languages share today.

## 6. Fixtures and tests

Marker fixtures are the outline fixtures: the testkit's `marker_round_trip` runs on every def of every file in `tests/fixtures/`, so every case
of TS1 §6 is also a placement test. It checks, per def:

1. `plan_marker` returns an edit or a `Skip`;
2. apply the edit, outline again: that def's `marker` is the text and every other def is unchanged;
3. **no damage**: removing the inserted marker line from the new text gives back the old text, except in the one-line case, where the old
   comment's words must appear unchanged inside the new block;
4. the new text still parses with the same number of syntax errors, and the def's JSDoc is still attached to it;
5. plan again on the marked text: `Skip::AlreadyMarked`.

Files that exist only for markers:

```
marker_jsdoc_multi.ts          description only → the line goes last
marker_jsdoc_tags.ts           @param / @returns present → the line goes before the first tag
marker_jsdoc_oneline.ts        /** Parses. */ → becomes three lines
marker_no_jsdoc.ts             a new one-line block
marker_plain_comment.ts        `/* x */` or `// x` above the def → a new JSDoc goes BELOW it; `/* ID: x */` is not read
marker_jsdoc_blank_line.ts     a block separated by a blank line is NOT the def's
marker_comment_between.ts      a block with a `//` or `/* */` comment below it IS the def's; a blank line on the way detaches it
marker_under_tag.ts            a hand-written marker after @returns is read
marker_decorators.ts           the JSDoc sits above the decorators
marker_const_arrow.ts          JSDoc on the `export const` statement
marker_overloads.ts            JSDoc on the first overload signature
marker_indent_tabs.ts          indentation is copied byte for byte
marker_crlf.ts                 inserted lines end with \r\n
one_line_many_defs.js          Skip::NotOnOwnLine
header_shebang.js  header_license.ts  header_reference.ts  header_use_strict.js
```

## 7. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `analyzer_core::edit`: `MarkerStyle { label, placement }`, `Placement`, and `comment_marker::{read, plan}` for headers (in `core/`, with its own tests: indentation, line endings) | `cd core && cargo test` |
| 2 | `sem.rs`: the JSDoc range directly above a node; the blank-line rule | unit tests on four small inputs |
| 3 | `marker.rs` reading; fill `Def::marker` in the outline | snapshots now show markers |
| 4 | `marker.rs` planning: the four cases of §3, then `Skip` | `marker_round_trip` passes on every fixture |
| 5 | `header.rs` | the four header fixtures |
| 6 | `languages/typescript/docs/markers.md` from §3 and §5 | — |
| 7 | README example; **tag `typescript-v0.1.0`** | a fresh clone builds and tests `typescript-syntax` with one command |
| 8 | in `vnoc`: `drivers/typescript`, tier 1–2 ([`01-monorepo/06`](../01-monorepo/06-versions-and-vnoc.md) §4) | the driver conformance test passes |

Step 8 is `vnoc` work and is listed so that the tag has a consumer. It also needs the V-NOC-side items the plan already lists for a second
language: drivers chosen by file extension and an open `DefKind` (`06-drivers.md` §3).

## 8. The later option: a comment above the def

Not built now. Recorded so that the design has room for it and nobody has to rediscover the rule.

`Placement::CommentAbove` would put the marker on its own `//` line directly above the def, and never touch the JSDoc:

```ts
/** Parses an order. */
// ID: 3RGJ8M2K…
@logged
export async function parseOrder(raw: string) {
```

| | Docstring (built now) | Comment above (later) |
|---|---|---|
| shows in hover tooltips and generated docs | yes | no |
| a def with no doc comment | a block is created | one line |
| edits an existing comment | yes | never |
| one rule for TypeScript and Rust | no (JSDoc and `///` differ) | yes |

What adding it needs: the new `Placement` variant; a reading rule ("the first `//` comment directly above the def, no blank line, that reads
`<label>: <text>`"); a planning rule (one line, the def's indentation); and one check first, which is why spike S-TS4 stays on the list as
optional: a `//` line between a JSDoc block and a declaration must not detach the JSDoc, in tsrs and in VS Code. `comment_marker` in the core
already does the text work, because the file header uses it.

A file should use one placement. When both exist on one def, the reader for the configured placement wins and the other is ignored.

Next: [`05-ts3-project.md`](05-ts3-project.md)
