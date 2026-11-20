# Bend markers

This is the marker rule of `bend-syntax`: where a def's marker and the file's marker live, how they are read, how an edit that adds one is
planned. It is part of the contract that freezes at `bend-v0.1.0`: a marker is stored in users' files, and a placement rule that moves would
lose them. Every row is pinned by a fixture and its snapshot in `crates/bend-syntax/tests/` (`snapshots/*.outline` show what is read,
`snapshots/planned/*` show the text after a plan is applied). The plan it was written from is `docs/08-bend/03-phase-2-defs-and-calls.md`
§1.3 and Q-BD1 (answered yes, 2026-11-20: D49). Written 2026-11-20 from the snapshots of BD2 step 2 (01f622e, 692dfa0).

## The one comment form

Bend has `#` to the end of the line, and nothing else: no docstring, no block comment, no doc comment. So both markers are a comment line

```
# <label>: <text>
```

where `<label>` is the caller's (`MarkerStyle`, `"ID"` in the examples; rule 1) and `<text>` is read after the colon, trimmed. Blanks are
allowed between `#` and the label and around the colon. An empty text is no marker. `bend` ignores comments, so a marker never changes what the
compiler, the checker or a proof sees (proved on the corpus: 13,468 markers planned and applied, every file's declarations unchanged).

What tells a def's marker from the file's is **where the line is**.

## A def's marker

| Rule | Fixture |
|---|---|
| The marker is the `# <label>: <text>` line in the comment block **directly above** the declaration (no blank line between): the block that is the def's doc and the start of its range (`qualnames.md`, range). It is read from that block only | `def_marker_not_file_marker.bend` |
| A marker line a blank line above the declaration is **not** the def's (it is a banner's or nobody's), and `plan` does not join it | `above_does_not_join_a_block_a_blank_line_away` (core) |
| Above `@unsafe`: the decorator is the declaration's first token, and the doc block (with the marker) sits above it | `def_unsafe.bend` |
| A law and its proof share a qualname; each has its own block, so a marker on one says nothing about the other. The marker is what tells them apart for good | `marked_law_unmarked_proof.bend` |
| Several marker lines in one block: the **last** wins (the one nearest the declaration, the one a tool wrote most recently). `plan` never writes a second one: a def that has a marker is `Skip::AlreadyMarked { existing }` | `marker.rs` tests |
| A test's trailing `#| expected output` lines are comments and never a marker: after the `#` comes `|`, not the label | `marker.rs` tests |

**Planning** (`plan_marker(text, &def, style, marker_text) -> Result<Edit, Skip>`): one inserted line, `# <label>: <text>` plus the file's line
ending, placed **immediately above the declaration's first token** and so below any existing doc block; the declaration's indentation (always
zero in Bend) is copied, as the other languages do. Nothing else moves; the round trip (plan on every def, apply, outline again) reads the
marker back on every def of every fixture and of the corpus. Refusals are the `Skip` enum, the same set as Rust's: `AlreadyMarked`,
`NotOnOwnLine`, `SyntaxError`, `InvalidText` (a text with a line break), `UnknownDef`, `PlacementNotSupported` (a `Docstring` style asked of
Bend).

## The file's marker

| Rule | Fixture |
|---|---|
| Read among the comments **before the first `import` line**, or, in a file with no imports, before the first declaration's range (its doc block included: that block is the declaration's). `book_load` skips blank and `#` lines before the first import, so a header there is legal Bend | `file_marker_before_imports.bend`, `file_marker_after_licence.bend` |
| A marker after the first import is not the file's (it is nobody's: it sits above an import) | `file_marker_after_first_import.bend` |
| A marker directly above the first declaration of a file with no imports is **that def's**, not the file's | `def_marker_not_file_marker.bend` |
| Written at **offset 0**, past a byte-order mark (`header_slot(text)`). 127 corpus files start with code on their first line; when the first declaration sits at the insertion point, the plan adds one blank line after the marker so that it does not become that def's | `planned/def_marker_not_file_marker.bend` |
| Refusals: `Skip::AlreadyMarked` when a header is there | `header.rs` tests |

The header is the one marker that core's `comment_marker` reads and plans for every language (with the prefix `#` or `//`); Bend adds
nothing to it but the rule above about which comments are the file's.

## What core gained for this (D49)

`Placement::Above` (a new variant of the `#[non_exhaustive]` enum) and `MarkerStyle::above(label)`. `comment_marker::plan` aimed at the
declaration's first token already put the line in the right place (below an existing block, copying indentation and line ending, keeping a
missing final newline missing); the variant is what lets a style say so. Reading a block of lines and choosing the last is Bend's alone and
stays in `bend-syntax` (rule of two).

## Known cost

`plan_marker` outlines the whole file on each call (as the other languages' do). A driver plans one marker per request, so this is nothing;
a tool that marks every def of a 70,000-def generated file at once would pay a quadratic cost, which the corpus test avoids by sampling.
