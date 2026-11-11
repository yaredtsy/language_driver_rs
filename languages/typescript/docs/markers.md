# TypeScript and JavaScript markers

A marker is **a line of text with a label, attached to a def or to a file**. The analyzer reads it (`Def::marker`, `header_marker`) and plans where
one would go (`plan_marker`, `plan_header_marker`). It never chooses the text and never writes a file: the caller applies the returned `Edit`. The
label (`ID`) is a parameter: `MarkerStyle { label, placement }`.

Every example below is a test: `tests/markers.rs` (defs) and `tests/header.rs` (files) hold the exact before and after texts, and
`marker_round_trip` runs the planner on every def of every fixture.

## Where a def's marker lives

In the **JSDoc block** directly above the def, as a line `<label>: <text>`:

```ts
/**
 * Parses an order.
 * ID: 3RGJ8M2K…
 * @param raw the wire text
 */
@logged
export async function parseOrder(raw: string) {
```

### Reading

1. Start at the first byte of the def's `range`: its first decorator, modifier or keyword. The def must **own its line**: only blanks (and a
   byte-order mark) before it on that line. Otherwise it has no marker to read.
2. Take the **last comment** before it. It must be a `/** … */` block (`/**/` is not one), with only whitespace between it and the def and **no
   blank line** (a line of blanks is a blank line). A `//` or `/* */` comment between the block and the def detaches the block; a blank line
   detaches it too (the comment then belongs to the file).
3. In the block, the first line that, after its leading `*` and spaces, reads `<label>:<spaces><text>` is the marker. The text is returned as
   written, trimmed. `IDENT: x` and `see ID: x` are not it. A marker under a block tag (`@returns`) is read.

Only `/**` is read. `/* ID: x */` and `// ID: x` above a def are not markers (`marker_plain_comment.ts`). With two blocks above a def only the
nearest counts (`marker_jsdoc_blank_line.ts`).

Where "above the def" is: the `export const` statement for `export const f = () => {}`; the member's own first line for a class member or an
object-literal method; the **first overload signature** for an overloaded function (a marker written above the implementation is not read).

### Planning (`plan_marker`)

| Above the def | The edit |
|---|---|
| a multi-line block with no block tags | ` * <label>: <text>` as the last line before the closing ` */` |
| a multi-line block with block tags (`@param`, `@returns`, …) | the line **before the first block-tag line**, so that it ends the description. A blank ` *` line before the tags stays above it |
| a one-line block (`/** Parses. */`) | the comment rewritten as `/**`, ` * Parses.`, ` * <label>: <text>`, ` */`. A block that is only a tag (`/** @type {X} */`) gets the marker first, then the tag. An empty block (`/** */`, `/***/`) gets just the marker |
| no block (nothing, a plain `/* */`, a `//` comment, or a block detached by a blank line) | a new one-line block `/** <label>: <text> */` directly above the def's first line, **below** any plain comment and **above** decorators |

The inserted text copies the block's own indentation and ` * ` prefix (or the def's indentation for a new block), and the file's line ending (the
first one found; `\n` when there is none). A byte-order mark stays first. The one-line case is the only edit that changes existing bytes; the old
words stay unchanged inside the new block.

### When nothing is written (`Skip`)

| Reason | When |
|---|---|
| `NotOnOwnLine` | something other than blanks stands before the def on its first line: `const a = () => 1, b = () => 2;` (the second), `class C { m() {} }` (the method), `/* c */ function f() {}`. Never reformat the user's code to make room |
| `SyntaxError` | a parse error overlaps the def or the block above it |
| `AlreadyMarked { existing }` | the def already has a marker (read by the rule above) |
| `UnsafeDocComment` | the block has no closing line of its own (` * text */`), a line without its `*`, or a tag on its opening line (`/** @param x`). A new block is **not** stacked on top of it: two blocks would hide the user's own |
| `DirectiveAbove` | the comment directly above the def is a directive that applies to the next line: it starts with `@ts-`, `eslint`, `prettier-ignore`, `istanbul`, `c8 `, `v8 `, `biome-ignore`, `tslint:`, `jshint` or `jscs`. A block put between them would move what it points at |
| `InvalidText` | the marker text is empty, has a line break, or contains `*/` (it would close the comment) |
| `PlacementNotSupported` | the style asks for a placement other than `Docstring` |

`DirectiveAbove` and `InvalidText` are not in the plan's list; they were added for the two hazards above. The directive list is short on purpose:
a comment that is not on it is treated as a plain comment, and the block goes below it.

## Where the file's marker lives (`header_marker`, `plan_header_marker`)

A **`// <label>: <text>` comment at the top of the file**: the first such line among the comments before the first token. Only a line comment
counts, and only before code.

It is written, in order:

1. after a `#!` line, if there is one;
2. after the leading block comments that are not the first def's own JSDoc: a licence block, or a `/** */` followed by a blank line. A block comment
   with code after it on its line is not skipped;
3. **before** any `//` comment (`/// <reference path="…" />`, `// @ts-check`), `"use strict"` and every other directive or statement, because those only need to
   be preceded by comments, and a comment is what we add;
4. the first def's own JSDoc (directly above it, no blank line) stays attached to it: the header goes above it, not between them.

An empty file gets `// ID: x`. A file whose last line has no line break gets one first. `AlreadyMarked` and `InvalidText` apply as for defs.

## Not decided here

- A comment-above placement (`Placement::CommentAbove`, plan §8) is not built. The `_ =>` arms return nothing / `PlacementNotSupported`.
- A marker above the **implementation** of an overloaded function is not read: the def's JSDoc is the first signature's.
- Generated files, and files V-NOC must never touch, are the core's decision, not the analyzer's.
