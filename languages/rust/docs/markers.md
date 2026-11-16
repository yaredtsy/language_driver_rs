# Rust markers

A marker is **a line of text with a label, attached to a def or to a file**. The analyzer reads it (`Def::marker`, `marker`, `header_marker`)
and plans where one would go (`plan_marker`, `plan_header_marker`). It never chooses the text and never writes a file: the caller applies the
returned `Edit`. The label (`ID`) is a parameter: `MarkerStyle { label, placement }`. Only `Placement::Docstring` is built.

The plan is `docs/04-rust/04-default-resolver/03-rs2-markers.md`; this file says what was built. Every rule below is a test: the
`marker_*.rs` fixtures and `tests/outline.rs` (reading), `tests/plan.rs` and `plan_doc_shapes.rs` (planning), the `header_*.rs` fixtures and
`tests/header.rs` with the reviewed planned texts in `tests/snapshots/header_planned/` (the file header), and `tests/rustfmt.rs`.
`marker_round_trip` plans a marker on every def of every fixture, applies it, outlines the text again and asks for the same defs, this one
marked, and `AlreadyMarked` on a second plan.

## Where a def's marker lives

In the item's **outer doc comment**, as a line of its own:

```rust
/// Parses an order.
///
/// ID: 3RGJ8M2K
#[inline]
pub fn parse_order(raw: &str) -> Order {
```

Two side effects a user will meet:

- `rustdoc` publishes the marker line. That is the accepted cost of putting it in the docs (D6).
- An item that had no docs now has some, so `#![warn(missing_docs)]` stops warning about it.

### Reading

1. **Which comments.** The item's own outer doc comments: the `///` lines and `/** */` blocks that the engine puts in the item's node, in
   source order (`marker_kinds.rs`: every kind of def, a method of each kind of `impl`, a trait's required and provided methods, an
   associated `const`, a foreign `fn`, a `fn` and a `struct` inside a `fn`).
2. **Nothing detaches a doc line.** A blank line, a plain `//` comment, or an attribute between two doc lines or between the doc lines and
   the item changes nothing: rust-analyzer parses doc comments as part of the item, the way `rustc` does (`marker_blank_lines.rs`; the
   engine facts are in `ENGINE.md`, S-RS4). As in TypeScript, the rule is what the compiler applies, not what looks tidy.
3. **Which line.** The first line, across all those comments, that reads `<label>: <text>`:
   - in a `///` line, after the `///` and **any** blanks; in a block, after the line's leading `*` if it has one, then any blanks; the
     first line of a block is read after the `/**` (`marker_forms.rs`, `marker_block.rs`);
   - blanks may stand on either side of the `:` (`ID:x`, `ID : x`); the text is the rest of the line, trimmed; an empty text is no marker;
   - the label is case-sensitive and must start the line: `id: x`, `IDX: x`, `see ID: x` are not markers;
   - a `\r` before the line break is not part of the text (`marker_crlf.rs`).
4. **Two marker lines:** the first one wins, whether it is in a `///` line or in a block (`marker_two.rs`).

Not read (`marker_not_read.rs`):

- `#[doc = "ID: x"]`. A doc attribute counts as a doc fragment when **placing** a marker, but it is never read;
- `////`, `//`, `/* */`, `/*** */`: in Rust these are plain comments, not doc comments;
- an inner doc comment (`//!` inside a module or a body): it belongs to the item around it;
- the doc line of an `impl` block: an `impl` is not a def, so its doc gives a marker to none of its methods.

**Accepted cost of "any blanks"** (`marker_code_examples.rs`): an `ID: x` line inside a fenced code example, or an indented Markdown code line
(`///     ID: x`), is read as the marker, and before a real one further down. The other choice is worse: a marker that goes unread makes
`plan_marker` plan a second one, a new ID beside the old. Reading is tolerant; planning writes one shape (below).

Not like Python: Python reads any `<label>:` pair anywhere in the docstring, and the last one wins, because its own planner sometimes has to
write the marker after the user's text on the same line. In Rust a doc comment can always take a new line, so the planner always writes a line
of its own and the reader asks for one. Rust reads the way TypeScript and core's `comment_marker` do.

### Planning (`plan_marker`)

The edit depends on the item's **last doc fragment**, in source order: its last `///` line, its last `/** */` block, or its last `#[doc = …]`
attribute, whichever comes last (`tests/plan.rs`, `row_*` tests; one def per row in `plan_doc_shapes.rs`).

| The last fragment | The edit |
|---|---|
| an empty `///` line | `/// <label>: <text>` on the next line |
| a `///` line with text | first an empty `///` line, then the marker. In Markdown a line directly under a paragraph joins it, so without the empty line `rustdoc` would glue the marker to the user's sentence |
| a multi-line `/** */` block | ` * <label>: <text>` on a line of its own before the closing line, with the block's own indentation and ` * ` prefix, and an empty ` *` line first unless the line above is already empty or is a bare `/**`. Core's `block_marker::plan` does this text work for TypeScript and Rust; Rust passes "no block tags" (an `@` line is plain text in Rust docs) and "empty line first" (TypeScript's JSDoc does not) |
| a `#[doc = …]` attribute (`#[doc = "b"]`, `#[doc = include_str!("x.md")]`) | an empty `///` line and the marker on the line after the attribute. Put before the attribute, the marker would become the item's summary or join its first sentence. The attribute is not edited |
| none at all | one new line `/// <label>: <text>` directly above the item's first attribute or keyword |

Where the fragments are interleaved with other attributes (`/// a` · `#[inline]` · `/// b` · `#[must_use]`), the marker goes after the last
fragment, wherever it stands, and the other attributes stay where they are. `#[doc(hidden)]`, `#[doc(alias = …)]` and other `doc(…)` lists
are not fragments, nor is `#[cfg_attr(…, doc = …)]`: an item with only those has no docs and gets the new line above everything.

The new line copies the indentation of the item's first line (spaces or tabs), and the file's line ending (the first one found; `\n` when
there is none). A byte-order mark stays first. No existing byte is changed: every edit is an insert.

### When nothing is written (`Skip`)

Every `Skip` carries its reason: match on the variant to act on it, and call `to_string()` for a sentence a person can read. The sentences
are in `src/lib.rs`, and a test checks that every variant has one. The checks run in this order, as in Python and TypeScript:

| Reason | When |
|---|---|
| `PlacementNotSupported` | the style asks for a placement other than `Docstring` |
| `UnknownDef` | the text has no def with this qualname and range: the def comes from the outline of another text (an older version of the file). Outline the text again. `marker` returns `None` for such a def |
| `InvalidText` | the marker text is empty, starts or ends with a blank (it would not read back the same), has a line break, or contains `*/` (it would close a block) |
| `SyntaxError` | a parse error touches the def's range (`Def::syntax_error`); or the file has a parse error anywhere, and the edit, tried on a copy and outlined again, does not give back the same defs with the marker on this one and every other def's marker unchanged. The trial catches an unfinished item above a clean def (`struct` with no name, then `enum Complete {}`), which would take the new doc line. A file with no error needs no trial |
| `AlreadyMarked { existing }` | the def already has a marker, by the reading rule. Checked before the shape, so a marked def answers it even when its line is shared |
| `NotOnOwnLine` | something other than blanks stands before the item on its first line (`fn a() {} fn b() {}`, `mod m { fn f() {} }`), or the last fragment is a `#[doc = …]` attribute with code after it on its line. Code is never reformatted to make room |
| `UnsafeDocComment` | the last fragment is a `/** */` block that cannot be edited safely: closed on its opening line (`/** Summary. */`), a middle line without its `*`, or text before the `*/` on its closing line. TypeScript rewrites a one-line block into several lines; Rust does not, because doc blocks are rare in Rust and a rewrite changes the user's text |

Never asked, because they are not defs: items inside a macro call, enum variants, fields.

## Where the file's marker lives (`header_marker`, `plan_header_marker`)

A **plain `// <label>: <text>` comment at the top of the file**, as in every language. It is `//`, not `//!`, so `rustdoc` does not
publish it. It takes no edition. One fixture per case, listed in `tests/header.rs` with what is read from it.

### Reading

The first `// <label>: <text>` line among the plain comments before the first real token: the first item, attribute, inner attribute
(`#![…]`), outer or inner doc comment. A `/* */` block is not read, nor a `///` or `//!` line (`header_block_lookalike.rs`,
`header_doc_lookalike.rs`), nor a line after the first token (`header_after_the_first_token.rs`). With two header lines the first wins
(`header_two.rs`).

"Before the first item" is measured from the item's first token, never from where the engine's node begins. The engine puts a plain comment
directly above an item inside the item's node, and in a `\r\n` file even across a blank line, so a header there would otherwise go unread and
a second one would be planned (`header_above_item.rs`, `header_crlf_blank_then_item.rs`, `header_crlf_blank_then_doc.rs`).

### Planning

The header goes:

1. after a `#!` line, if there is one (`header_shebang.rs`);
2. as the **last line of the file's opening comment**: the first run of comments from the top, whatever their style (`//` lines, a `/* */`
   block, both), down to the first blank line or the first token. A licence keeps the first line (`header_licence_lines.rs`,
   `header_licence_block.rs`, `header_block_then_line.rs`). A comment after the blank line is not part of it (`header_run_then_blank_then_comment.rs`);
3. with an **empty `//` line** between a `//` run and the header, unless the run's last line is already an empty `//`
   (`header_run_ends_empty.rs`, `header_crlf_run_ends_empty.rs`). `rustfmt` with `wrap_comments = true` refills a `//` run as one paragraph
   when one of its lines is wider than `comment_width`, and would join the header to the text above it (`// end ID: x`), which is then not
   read. The empty line is a paragraph break for `rustfmt`. It is always written, because the width is the user's setting. Under a `/* */`
   block none is needed;
4. **above** inner attributes and inner doc comments, which may follow a comment but must come before any item
   (`header_inner_attribute.rs`, `header_licence_above_inner_attribute.rs`). Directly above a `//!` or `/*!` the header is followed by an
   empty line, so that nobody reads it as the first line of the crate docs (`header_inner_doc.rs`, `header_inner_block_doc.rs`,
   `header_shebang_licence_inner_doc.rs`);
5. above the first item, its doc comments and its attributes (`header_outer_doc_first.rs`, `header_attribute_first.rs`), and above a block
   comment that has code after it on its line (`header_code_after_block.rs`).

```rust
// Copyright (c) Someone.
// Licensed under Apache-2.0.
//
// ID: 3RGJ8M2K

//! Crate docs.
```

An empty file gets `// ID: x`. A file whose last line has no line break gets one first (`header_no_final_newline.rs`). A byte-order mark
stays first (`header_bom.rs`). Line endings are the file's. `AlreadyMarked`, `InvalidText` and `PlacementNotSupported` apply as for defs.
In a file with a parse error the edit is tried first and refused with `SyntaxError` unless the header reads back: an unterminated `/*` would
swallow it (`header_unterminated_block.rs`).

Known and accepted: a plain `//` note glued to the first item (`// About f.` above `fn f`) is taken for the opening comment, so the header
goes between the note and the item, after an empty `//` line (`header_comment_above_item.rs`). TypeScript does the same.

## `rustfmt`

`tests/rustfmt.rs` marks every def of every fixture and the file header, formats the text with the repo's `rustfmt.toml`, then again with
`wrap_comments = true`, and asks for every marker line byte for byte. A canary test keeps the case that made the empty `//` line necessary:
without it, `wrap_comments` joins the header to a wide licence line. `rustfmt` is run as a test, not a one-time check, because a `rustfmt`
update can change the answer.

## The CLI (`rust-cli marker`)

```
rust-cli marker <file> (--def <qualname> [--occurrence <n>] | --header) --text <text> [--write] [--label L] [--edition E]
```

The CLI is the only place that writes, and the text is always the caller's. Without `--write` it prints the plan and changes nothing. With
`--write` it applies the edit, writes the file, reads it back, and reports success only if the same marker reads back. A qualname shared by
several defs (D15) without `--occurrence` lists them and picks none.

| Exit | Meaning |
|---|---|
| 0 | planned; or written and read back |
| 1 | the file cannot be read or written, or the marker does not read back |
| 2 | bad usage, or twins and no `--occurrence` |
| 3 | a `Skip`, printed with its reason; the file is untouched |

A later marker command in another language uses the same four codes.

## Not decided here

- A comment-above placement (`Placement::CommentAbove`) is not built: `marker` and `header_marker` read nothing for it, the planners answer
  `PlacementNotSupported`.
- A one-line `/** Summary. */` block is not rewritten. Revisit if a project shows them.
- Generated files, and files a user wants left alone, are the caller's decision, not the analyzer's.
