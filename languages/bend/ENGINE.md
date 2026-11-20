# Engine: bend

Bend has no engine to sit on: there is no Rust implementation and no parsing library for it. The parser is ours, `bend-parser` (row BD-D1), and what
this file pins is the **grammar** (a Bend commit), not a crate.

- Upstream URL: https://github.com/bendlang/bend (read from `../language_clone/bend`; never built from, never edited)
- Pinned commit / version: grammar pin `0e2f38f` (Bend 2.0.40 and after). The pinned clone is the source of truth for the grammar, even if an installed `bend` is newer.
- Engine crates used: none. `bend-parser` is ours (BD-D1); it depends on `analyzer-core` only (for `text::Span`). `sem` in `bend-syntax` and `bend-analysis` is the only module that imports it.
- Patches: none
- How to bump: a bump is a change of its own (only the pin, `bend-parser`, `ENGINE.md`), as an engine bump is elsewhere. Re-run the corpus script, read
  `bend.ts`'s diff since the pin, fix the parser, move the pin.
  - the Rust functions mirror `bend.ts`'s (`parse_skip` → `skip`, `parse_name` → `name`, `parse_tele` → `tele`, `parse_term` → `term`, `parse_body` → `body`,
    `parse_book` → `book`), each with the `bend.ts` line in its doc comment: a diff of `bend.ts` names the Rust functions to touch.
- Licence notice: Bend is Apache-2.0, Copyright 2026 HigherOrderCO. `bend-parser` is a Rust port of the parser in `bend2/bend.ts` (a derivative work), so
  a binary that links `bend-parser` ships Bend's `LICENSE` (the clone's, unchanged).

## corpus

(to be filled by `scripts/bend-corpus.sh`, below)

## grammar

(to be filled by S-BD2, below)
