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
2. Take the **nearest `/** … */` block** among the comments above it (`/**/` is not one). **Nothing between the block and the def detaches
   it**: not a blank line, not a `//` or `/* */` comment. This is the block TypeScript applies to the def, and the only one it applies
   (checked with `tsc` 5.1.6: a `@param {number}` in a block two blank lines and a `//` note above a function still types its parameter).
   One block is not the def's: **the file's opening comment**, which is the comments from the top of the file (after a `#!` line) down to the
   first blank line, when that blank line comes before the def. It is a licence or an overview: the file's, whatever stands below it.
3. In the block, the first line that, after its leading `*` and spaces, reads `<label>:<spaces><text>` is the marker. The text is returned as
   written, trimmed. `IDENT: x` and `see ID: x` are not it. A marker under a block tag (`@returns`) is read.

Only `/**` is read. `/* ID: x */` and `// ID: x` above a def are not markers (`marker_plain_comment.ts`). With two blocks above a def only the
nearest counts. A marker is read across a blank line and through the comments below its block (`marker_jsdoc_blank_line.ts`,
`marker_comment_between.ts`). A marker line in the file's opening comment is not the first def's.

Where "above the def" is: the `export const` statement for `export const f = () => {}`; the member's own first line for a class member or an
object-literal method; the **first overload signature** for an overloaded function (a marker written above the implementation is not read).

### Planning (`plan_marker`)

| Above the def | The edit |
|---|---|
| a multi-line block with no block tags | ` * <label>: <text>` as the last line before the closing ` */` |
| a multi-line block with block tags (`@param`, `@returns`, …) | the line **before the first block-tag line**, so that it ends the description. A blank ` *` line before the tags stays above it |
| a one-line block (`/** Parses. */`) | the comment rewritten as `/**`, ` * Parses.`, ` * <label>: <text>`, ` */`. A block that is only a tag (`/** @type {X} */`) gets the marker first, then the tag. An empty block (`/** */`, `/***/`) gets just the marker |
| no block of its own (nothing, a plain `/* */`, a `//` comment, or only the file's opening comment) | a new one-line block `/** <label>: <text> */` directly above the def's first line, **below** any plain comment and **above** decorators |

A block with a blank line or a comment between it and the def is edited like any other block; what stands between stays where it is. A new block
is never written below a block that is the def's: TypeScript uses only the last block above a def, so the user's own would stop showing, and in
a checked `.js` file its `@param` types would stop applying (error TS7006).

The file's opening comment is never written into. The first def below it gets a new block of its own, unless the opening comment carries block
tags: then neither way is safe, and the answer is `FileCommentAbove`.

The inserted text copies the block's own indentation and ` * ` prefix (or the def's indentation for a new block), and the file's line ending (the
first one found; `\n` when there is none). A byte-order mark stays first. The one-line case is the only edit that changes existing bytes; the old
words stay unchanged inside the new block.

### When nothing is written (`Skip`)

Every `Skip` carries its reason: match on the variant to act on it, and call `to_string()` for a sentence a person can read (an editor shows it
next to the def that was not marked). The sentences are in `src/lib.rs` and one test checks that every variant has its own.

| Reason | When |
|---|---|
| `NotOnOwnLine` | something other than blanks stands before the def on its first line: `const a = () => 1, b = () => 2;` (the second), `class C { m() {} }` (the method), `/* c */ function f() {}`. Never reformat the user's code to make room |
| `SyntaxError` | a parse error overlaps the def or the block above it |
| `AlreadyMarked { existing }` | the def already has a marker (read by the rule above) |
| `UnsafeDocComment` | the def's block has no closing line of its own (` * text */`), a line without its `*`, or a tag on its opening line (`/** @param x`). A new block is **not** stacked below it: TypeScript uses only the last block, so the user's own would stop applying |
| `FileCommentAbove` | the nearest block above the def is the file's opening comment **and it carries block tags** (`@param`, `@type`, `@license`, …). It is the file's, so it is not written into; but TypeScript applies it to the def, and a new block below it would switch its tags off for the def (in a `.js` file they may be its types). A def that has a block of its own below the opening comment is planned as usual |
| `DirectiveAbove` | the def has no block to write in, and a comment above it is a directive that points at the def's line: a new block between them would move its target. Two kinds. **A tool's**, directly above the def: `eslint-disable-next-line`, `oxlint-disable-next-line`, `tslint:disable-next-line`, `biome-ignore`, `rome-ignore`, `deno-lint-ignore`, `prettier-ignore`, `dprint-ignore`, `istanbul ignore next/if/else`, `c8 ignore next`, `v8 ignore next`, `node:coverage ignore next`, `noinspection`, `$FlowFixMe`, `$FlowExpectedError`, `$FlowIgnore`, `flowlint-next-line`, `cspell:disable-next-line`, `nosemgrep`. **The compiler's**, `@ts-ignore` and `@ts-expect-error`: these reach the def through blank lines and `//` comments (a block comment cuts them off), so they are looked for that far up. Also: a one-line block that is itself a directive (`/** @ts-ignore */`), which would stop being one as several lines |
| `InvalidText` | the marker text is empty, starts or ends with a blank (reading trims, so it would not come back the same), has a line break, or contains `*/` (it would close the comment) |
| `PlacementNotSupported` | the style asks for a placement other than `Docstring` |
| `UnknownDef` | the text has no def with this qualname and range: the def comes from the outline of another text (an older version of the file). Its offsets are not trusted; outline the text again. `marker` returns `None` for such a def |

`FileCommentAbove`, `DirectiveAbove`, `InvalidText` and `UnknownDef` are not in the plan's first list; they were added for the hazards above. A directive is matched as a whole word at the
start of the comment. A comment for a whole file or a range (`/* eslint-disable */`, `// @ts-nocheck`, `// deno-lint-ignore-file`) is a plain comment:
the block goes below it. So does any comment that is not on the list. When the def has a JSDoc block, a directive never matters: the line goes
into the block and nothing between the directive and the def moves.

## Where the file's marker lives (`header_marker`, `plan_header_marker`)

A **`// <label>: <text>` comment at the top of the file**: the first such line among the comments before the first token. Only a line comment
counts, and only before code.

It is written **as the last line of the file's opening comment**, so that a licence keeps the first line (a licence checker and a test runner's
docblock both look there):

1. after a `#!` line, if there is one;
2. after the comments at the top of the file, whatever their style (`/* */`, `/** */`, `//` lines), down to the first blank line or the first
   token. `// Copyright …` lines followed by a blank line, or glued to an `import`, get the header as their last line;
3. but it stops **above** each of these, which must stay with what is below them:
   - a `/// <reference path="…" />` line or a `// @…` pragma (`// @ts-check`, `// @flow`): they only need comments before them, and a comment
     is what we add;
   - a directive for the next line (the `DirectiveAbove` list: `// eslint-disable-next-line`, `// @ts-ignore`, …);
   - **the first def's own JSDoc** (by the reading rule above). A JSDoc above anything that is not a def (an `import`, `"use strict"`) documents
     nothing, so it is the file's and the header goes below it;
   - a block comment with code after it on its line;
4. and it is always above `"use strict"` and every other statement.

An empty file gets `// ID: x`. A file whose last line has no line break gets one first. `AlreadyMarked` and `InvalidText` apply as for defs.

Known and accepted: a plain `//` note glued to the first statement (`// helper` above `function f`) is taken for the opening comment, so the
header goes between the note and the statement. A licence written as a JSDoc and glued to the first **def** (no blank line) is that def's block
by every rule, so the header goes above it. The header does not know the dialect; it finds the first def with the `.ts` grammar, which also
finds it in `.tsx` and `.jsx` files (tested).

## Not decided here

- A comment-above placement (`Placement::CommentAbove`, plan §8) is not built. The `_ =>` arms return nothing / `PlacementNotSupported`.
- A marker above the **implementation** of an overloaded function is not read: the def's JSDoc is the first signature's.
- Generated files, and files V-NOC must never touch, are the core's decision, not the analyzer's.
