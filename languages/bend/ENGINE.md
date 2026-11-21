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

S-BD1, run 2026-11-20 on macOS (Darwin 23.6.0) with `scripts/bend-corpus.sh ../language_clone/bend`. **`bend` is not installed**; `bun 1.3.2` is, so the
oracle is `bun <clone>/bend2/main.ts` (the pinned clone, Bend 2.0.40: the grammar's source of truth), not the `#|` fallback. Nothing was fetched
(telemetry off, hub is a closed port, `BEND_LIB` under `spikes/out/`), nothing under `~/.bend` was touched. Output lists are in
`languages/bend/spikes/out/` (ignored by git).

| What | Count |
|---|---|
| corpus: every `.bend` under `tests/` (1,879), `demos/` (56), `bench/` (22), plus `bend2/base.bend` | **1,958** (the plan's 1,982 is the whole repo: + `evals/` 20, `tools/` 4, not in the corpus) |
| declared negative by their own `#|` lines (first line `SOME PROOFS FAIL` or `Error:`, the test of `gates/test.ts`) | 549 |
| `--check-only` **accepts** (parse, names, type check) | 1,252 |
| `--check-only` refuses | 706 (none timed out) |
| **the loader accepts** (`book_load`: imports, `parse_book`, names; no checker) | **1,744** |
| the loader refuses | 214 (75 in `tests/parse`, 41 `tests/check`, 32 `tests/flatten`, 20 `tests/import`, rest in 7 more folders) |
| check-accepted but not loader-accepted | 0 |
| files with a hub import (`0x<hash>/…`, `name@ver/…`) | 0 |
| col-0 code lines in an accepted file other than `def`/`type`/`law`/`@unsafe`/`import`/a closing bracket | 0 |
| accepted files with an indented `import "./x.c"` (foreign def); with a col-0 one | 113; 0 |
| declaration (`def`, `type`, `law`, `ctr`) entries Bend sees in the accepted files | 176,229 (78,754 def, 74,427 law, 7,735 type, 15,313 ctr) |

What these say about the plan:

- **The list a parser must accept is the loader's, not `--check-only`'s.** `--check-only` refuses every program that relies on `@unsafe` or foreign code
  ("N defs rely on unsafe or foreign code"): 98 of `tests/io`, 16 of `tests/run`, 4 of `demos/io_rollback_netcode` and more, and those are exactly the files
  that exercise `@unsafe`, `def f?` and `import "./x.c"`. `parse-accepted.txt` is therefore the corpus test's list; `check-accepted.txt` is kept for BD4.
- **`tests/parse/` is not all positives**: 86 of its 173 files expect `Error:` or `SOME PROOFS FAIL`. Of its 173, the loader accepts 98. (A plan line says
  "`parse/` holds 173 positive parse tests".)
- The loader also refuses forms that *are* syntactically fine but fail inside `parse_book` after reading: `body_flatten`'s "a match cannot scrutinize a
  non-parameter", pattern arity, "'a' can't be matched in this position", duplicate declarations. Our parser accepts those: the loader-refused list is a
  superset of the syntax errors, so it is not a list of "files we must reject".
- `bend.ts` offsets (`Span.beg`) are **UTF-16 code units** (JS string indices), not byte offsets: a column from `bend`'s own error text differs from ours on
  any line with a non-ASCII character before it. `parse-refused.txt` offsets are in those units.
- The two files whose first code line is not an import or a declaration are both loader-refused negatives (`tests/parse/legacy_brace_datatype.bend`,
  `legacy_data_annotation.bend`): in the accepted corpus every file starts with comments, then imports, then declarations. 127 files have code as their first non-blank line
  (an `import` or a declaration): the header marker's slot is "offset 0" for them (`03` §1.3).

### BD1 step 3: the outline-shaped parser against the corpus

2026-11-20, `cargo test -p bend-parser --release --test corpus -- --ignored` (needs `BEND_CLONE` and the lists above). Every file of `parse-accepted.txt`
(1,744; 13,715,509 bytes) parses with **zero `ParseError`s**, and the declarations found are Bend's own, in order, for every file: the same kind and name,
the same number of parameters (a def's, a type's, a constructor's fields) and, for a def with a `->`, of templates. **176,229 declarations** (78,754 defs,
74,427 laws, 7,735 types, 15,313 constructors), the loader's count exactly. Parsing alone took 57–76 ms over all of it with bodies skipped (release).

- A def with no `->` fills a law, and its template count is the law's (`for ~k`), so the test does not compare it: 32 files showed that, all of them
  law-filling defs. A def that fills a law of another file is named `<Alias>.<name>` in the text and `<namespace>:<name>` by Bend; the test accepts
  that pair when the alias is one of the file's own.
- The test is not vacuous: breaking one rule of the term skipper (`>` glued never ends a term) makes 372 declarations disagree.
- In step 3 parameter and return types and bodies were **skipped, not read** (a throwaway `stopgap.rs`); step 4 replaced every one with the term or body it
  covers and deleted the file. These numbers held unchanged through that.

### BD1 step 4: terms and bodies

2026-11-20, same test. Every file of `parse-accepted.txt` parses with **zero `ParseError`s** with every body read, the declarations still match Bend's (176,229),
and every node of every tree passes the span invariants (a term inside its parent, siblings apart, a name that is its span, a call that runs from its callee to its
`)`, an operator from its left to its right). Cutting the 1,733 accepted files of at most 20 kB at every 53rd byte (37,641 cuts) never panics and the invariants hold
on every cut. Breaking the newline-ends-the-spine rule makes 176 files fail, so the test sees terms.

**Time (BD1 step 5):** with bodies read, parsing all 1,744 files (13.7 MB, release, five runs) takes 189–262 ms, median 195 ms, parse only (no file reading), on the
owner's machine, 2026-11-20. The expectation in 02 §4 was "well under a second for all 1,982 files": met.

The term nodes of the accepted corpus, a first picture of what BD4 walks (1,729,369 terms; a reply body is counted as its term):

| Term kind | Count |
|---|---|
| Name | 973,069 |
| Ctr | 255,703 |
| Call | 250,479 |
| TypeApp | 78,469 |
| Annot/Eq/Refl | 48,829 |
| Tuple | 27,140 |
| Lit | 24,409 |
| Op | 18,658 |
| Lam | 16,029 |
| Rewrite | 14,239 |
| Type/Kind/Quant | 13,743 |
| Group | 4,068 |
| Plus | 1,619 |
| Do | 871 |
| Arrow | 853 |
| List | 550 |
| All | 407 |
| Index | 80 |
| LamMatch | 78 |
| ArrayNew | 71 |
| Hole | 5 |

- **Calls:** 433,747 arguments over 250,479 calls; 881 are `~` arguments; 80 calls are `f!(..)`.
- **Operators** (the 18,658 `Op` terms): . `&` is nearly all of it: the pair type.
- **Bodies:** 32,378 `match` (66,033 cases), 8,037 lets, 4 array-write statements.
- **Do statements** (871 blocks): .
- Not in the corpus at all: nothing the grammar list G1–G30 has; every row's form is exercised by a fixture even where the corpus has none.

Rows G31–G36, found reading these functions again, are in the grammar table below.

Choices in the tree where the surface could be read two ways are listed in the BD1 step 4 report, not here: they are decisions, not facts about `bend.ts`.

## BD2

### The comment block above a declaration (05 §4 [check] 5)

2026-11-20, `cargo test -p bend-syntax --release --test corpus -- --ignored --nocapture`. Over the accepted corpus (1,744 files), the outline finds **160,916 defs**: the
176,229 declarations less the 15,313 constructors, in Bend's order, with Bend's names and kinds (a def that fills a law of an imported file is named through its
alias, `M.name`, where Bend says `<namespace>:name`).

For each declaration, what is above it (the def's range starts at its doc, so "doc" is read from the range):

| Folder | Declarations | `#` block ending on the line directly above (the doc) | block, exactly one blank line between | the same, not counting a block at the very top of the file | two blank lines between |
|---|---|---|---|---|---|
| `bench/` | 147,301 | 175 | 19 | 19 | 0 |
| `bend2/` (`base.bend`) | 541 | 55 | 23 | 22 | 0 |
| `demos/` | 1,208 | 581 | 54 | 54 | 0 |
| `tests/` | 11,866 | 325 | 7 | 5 | 0 |
| **all** | **160,916** | **1,136** | **103** | **100** | **0** |

The second number is **not near zero**: 103 against 1,136 (8% of the declarations with a comment block just above or one blank line above are in the second group); outside the generated `bench/`, 84
against 961. What they are: 61 of the 100 are a **section banner**, a title and an underline (`# Array` / `# -----`, then a
blank line, then the first def of the section; counted over all accepted files, bench included, 61 of the 100 blocks end in a rule line). A banner is not the doc of the def under it,
so **the range rule of 03 §1.2 (a blank line between makes the block not the def's) is right as written** and nothing has to change before the tag; a rule that took "the
block that ends on the line above, blank or not" would put a section title into the first def's range. Two blank lines between never occur.

### BD2 step 2: markers and the header

2026-11-20, `cargo test -p bend-syntax --release --test corpus -- --ignored --nocapture`. A marker (`# ID: m<i>`) planned on the defs of every accepted file, the file's own marker
(`# ID: FILE`) planned too, all the edits applied together to the text, and the text outlined again: **1,744 files, 1,744 file markers read back, 13,468 def markers read back**,
none refused, none already marked in the corpus, every file's defs still Bend's own list (same names and kinds, in order) and no syntax error added. `plan_marker` outlines
the whole file on each call, so the test plans **every def of a file of at most 300 defs, and about 40 spread over a larger one** (the generated `bench/` files have up to 70,000):
13,468 of the 160,916 defs were planned, not all of them.

## BD3

### What `book_load` and the tables do that the plan did not say (read from `bend.ts`, 2026-11-20)

- **Bend's own offsets are into the text with its `import` lines emptied.** `book_load` parses `body.join("\n")` where each import line was set to `""`, so every `Span` of a
  `Ref` (and of every other term) is before the file's own offset by the length of each import line above it. `scripts/bend-refs.ts` puts that length back; a comparison of
  our spans with Bend's that forgets it is off by 12 (`import Base`) or more on every line after the imports.
- **The entry file's namespace is `""`, the same as Base's**, not its file name: `book_read` calls `book_load(book, file, "", seen)`. A bare name in `tlds` is therefore the entry's or
  Base's. Every other file is `ns:name` with the namespace relative to the entry file's folder (`../shared/lib` for a file above it).
- **`book.order` lists a law's name twice**, once for the law and once for the def that fills it (`parse_def` pushes the same key again); `tlds` has **one** entry, the def's body replacing
  the law's `null`, and its `m` stays the law's namespace. A proof is named `<law's namespace>:<name>` though it is written in another file: `DeclEntry::qualified` follows that.
- **A proof is the def with no `->` whose resolved name is a law of the book** (`tld.v === null`, not Base's, not foreign). A second proof of one law finds a def there, not a law, and is
  a duplicate (we count it; the first stays).
- **Duplicates across files cannot happen by namespace alone**: `a:f` and `b:f` are two names. `parse_fresh` refuses `nm in tab` (the bare spelling: Base's, or the entry's), a name already
  taken in this namespace, and a name whose first segment is an alias. So the duplicates a book can have are: a name twice in one file, a second proof of a law (two files), and a name Base has.
- `parse_reso` with an alias whose file lacks the name tries the whole name as Base's (`List.map` with an alias `List`); with the alias *and* the whole name both in the table it refuses
  ("the alias shadows"). We never refuse: the alias wins. A file's own def written with the alias's name (refused by `bend`) is found rather than called unknown.
- Base's names are a regex over `base.bend` (`scripts/bend-base-names.sh`), checked against Bend's own loader on `bend2/base.bend`: the same **517 distinct top-level names** (541
  declarations: 24 laws that Base proves itself, so a name twice) and the same **50 constructors**. `src/base_names.rs` carries the pin in its first lines; the ignored test
  `base_names_are_current` runs the script and fails when the file differs.

### The Ref oracle (S-BD4) against the book

`bun scripts/bend-refs.ts ../language_clone/bend languages/bend/spikes/out` loads every `.bend` directly under `demos/*/` as an entry (56 files, 16 demos) with `book_load`, lowers each def's
body with `term_lower` (which opens the closures of Bend's higher-order terms), and prints one line per distinct `Ref`: `<file> <def ns:name> <callee ns:name> <beg>-<end>`, spans in
UTF-16 units of the file as it is on disk. About 3 s for all 56. It is not run by any test by itself: `every_ref_the_oracle_prints_resolves` (ignored, `--test corpus`) reads its output.

| | |
|---|---|
| entries, Refs | 56, **32,805** |
| found again by the name written at the span | 19,773 |
| made up by sugar (`T.add`, `Array.get`, `Nat.add`, `Pair`, … : the span is the whole sugar) | 13,032; resolved by the callee's name without its namespace |
| ours: `Project` (and `qualified` equals Bend's `ns:name`) / `Base` | 13,827 / 18,978 |
| `Unknown`, or a different def | **0** |

The oracle names a law and its proof alike (`LAWS:sorted`), so it cannot tell the two `DeclId`s apart: the fixture `law_alias` does. A Ref is also printed for names in **type positions** of a
body (annotations): the list is every def reference, not the call sites; BD4 decides which are sites (BD-D9).

### The corpus against the book

`every_accepted_entry_opens_with_bends_names` (ignored): each of the 1,744 files the loader accepts, opened with its own folder as the root.

- the **160,916 declarations of the entry files** have Bend's names (`ns:name`, with a proof named in its law's namespace) and Bend's kinds, in Bend's order, and the **15,313 constructors** are Bend's;
  1,897 files were opened in all (the corpus is flat: a book of one file nearly always);
- none of them counts a cycle, a missing import, a hub import, an import with no alias, or a duplicate: what Bend accepts, we accept;
- the 214 files the loader refuses open all the same: `import_missing` 13 (all in `tests/import`: a file that is not there: `cycle_terminates` and `diamond_dedup` name `/cycle/other_half.bend`, `/diamond/top.bend`),
  `duplicate_declaration` 17 over the 214 (three of them in `tests/import`: `duplicate_name`, `shadow_lib`, `shadow_base`, where a name Base has is declared again). **`tests/import/` has no cycle**: the flat corpus cannot hold the second
  file of one (`cycle_terminates` says so in its own comment), so cycles are covered by the fixture `cycle` only. The corpus has no hub import (`bend-corpus.sh` counted 0).

### Measurement (`measure`, release)

2026-11-20, macOS, the owner's machine, `cargo test -p bend-analysis --release --test corpus measure -- --ignored --nocapture`. Median of 30 opens (100 for `base.bend`); time covers reading the files, both
parses (the outline and the book's facts), and the tables. **Bytes are from lengths and `size_of`, not from an allocator** (the crate forbids unsafe code, so no counting allocator): texts are exact, outlines and
tables are the strings and structs counted, without the allocator's rounding or the maps' nodes. Each demo is opened at the entry file with the largest book (`PROOF.bend`; `walkers_test.bend` for the netcode).

| book | files | defs | text bytes | outline bytes | table bytes | open, ms |
|---|---|---|---|---|---|---|
| demos/app_pong_game_2d/PROOF.bend | 3 | 25 | 7,471 | 6,395 | 3,287 | 0.39 |
| demos/app_ray_tracer_3d/PROOF.bend | 3 | 39 | 10,720 | 9,508 | 5,074 | 0.51 |
| demos/app_slash_boss_3d/PROOF.bend | 5 | 514 | 216,051 | 140,011 | 69,691 | 8.51 |
| demos/app_triangle_2d/PROOF.bend | 4 | 20 | 5,337 | 5,762 | 2,485 | 0.28 |
| demos/app_win_is_bug_2d/PROOF.bend | 4 | 73 | 23,954 | 17,360 | 9,009 | 0.82 |
| demos/io_hello_world/PROOF.bend | 3 | 3 | 614 | 546 | 349 | 0.14 |
| demos/io_http_fetch/PROOF.bend | 3 | 20 | 7,583 | 4,685 | 2,590 | 0.35 |
| demos/io_http_server/PROOF.bend | 3 | 12 | 3,499 | 2,656 | 1,504 | 0.23 |
| demos/io_rollback_netcode/walkers_test.bend | 3 | 172 | 48,870 | 45,684 | 24,854 | 2.05 |
| demos/io_tcp_echos/PROOF.bend | 3 | 12 | 3,230 | 2,656 | 1,494 | 0.19 |
| demos/proof_insertion_sort/PROOF.bend | 4 | 19 | 6,203 | 4,592 | 2,316 | 0.64 |
| demos/proof_numerics/PROOF.bend | 3 | 28 | 7,120 | 6,861 | 3,257 | 0.40 |
| demos/proof_typed_eval/PROOF.bend | 3 | 19 | 6,935 | 4,164 | 2,580 | 0.34 |
| demos/pure_hvm5_mini/PROOF.bend | 3 | 220 | 69,062 | 55,623 | 32,667 | 2.62 |
| demos/pure_par_sort/PROOF.bend | 4 | 20 | 6,654 | 4,895 | 2,393 | 0.35 |
| demos/pure_par_sum/PROOF.bend | 4 | 11 | 3,551 | 2,374 | 1,312 | 0.24 |
| **largest book of the corpus by file count**: demos/app_slash_boss_3d/PROOF.bend | 5 | 514 | 216,051 | 140,011 | 69,691 | 8.52 |
| bend2/base.bend alone | 1 | 541 | 90,291 | 133,129 | 68,397 | 3.29 |

- **The whole of `tests/` is not one book** (no single entry): the corpus's books are nearly all one file, and the largest by file count is a demo (5 files, 514 defs, 216 kB, 8.5 ms). The plan's "a few
  hundred files in milliseconds" (03 §4) holds with room: the cost is parsing, and parsing all 1,744 accepted files together was 195 ms (BD1 step 5).
- A book is two to three times its text in memory (the text, an outline about two thirds of it, tables about a third); `base.bend` alone, a dense file of 541 short defs, is the one whose outline is larger than its text.
- A refresh that rebuilds a book of this size (BD5) is under 10 ms for every demo: `Session::refresh` can drop the book and open it again.

## BD4

### What Bend's own `Ref`s say about the walk (2026-11-20)

`every_ref_the_oracle_prints_is_accounted_for` (ignored, `--test corpus`) takes the 32,805 Refs `scripts/bend-refs.ts` prints for the 56 demo entries and finds, for each, the place of its def's body that the walk
read (`walked`): a site, a term skipped as a type, a counted sugar, or data. **None falls in no place; none is a site that names another def than Bend's.**

| Bucket | Refs |
|---|---|
| a `Call` to a def of the book (`qualified` is Bend's `ns:name`) | 12,547 |
| `External` (Base's, by the name as written) | 4,300 |
| a name handed over (`passed`) | 156 |
| not a site, counted `sugar_base` (`(a + b : U32)`, `&&`, `++`, `a[i]`, `do IO<..>`, `&`, `\|`) | 12,116 |
| inside a term skipped as a type, counted `type_position` | 2,652 |
| a `type` handed over, counted `type_passed` (`App.run(~U32, ..)`) | 261 |
| a bare project def name below the arguments, counted `passed_deep` | 65 |
| data: `3n+p`, `{a != b : T}`, a do block's closing value, a Base name as a value | 708 |
| in none | **0** |

How the oracle's spans and ours meet (found by running it): Bend's `Ref` of a sugar is at the **operator token** for an infix (`Ref(f, op === "&" ? s : t)`), at the whole sugar for `a[i]` and `3n+p`, and at a do statement's
span **ended after the white space that follows it** (`parse_span` after `parse_skip`): the statement's end is not ours. So the test compares **starts** for a statement, takes the innermost place that holds a Ref's start,
and of those the one whose end is nearest (a do block's final value has two Refs that start together: the monad's, over the whole value, and the call's inside it). Since BD4 step 5 `Op` has an `op_span` (the token) and
an infix sugar's site is that token: **11,399 operator Refs of the oracle (every one whose text has no letter and no space) are at exactly our site's bytes, 0 differ**; the 46 `Array.get` / `Array.set` Refs are at exactly the
bytes of our site, the whole `a[i]` (for `a[i] <- v` too: 22 of the 46), so an index stays the whole expression. A do statement stays the statement: Bend's end differs and cannot be matched.

### What the walk counted over every def of each demo's largest book (`demos_counters`)

Over the 16 books (all their defs, walked once each): **3,679** calls to defs of the book, **1,185** constructs, 1,172 externals, 46 passed edges, **68 `function_typed_value`**, 0 `unknown_name`, 0 `unknown_constructor`;
counted and not drawn: **6,667 `type_position`**, **3,072 `sugar_base`**, **15 `passed_deep`**, 84 `type_passed`, **0 `open_law`** (every law of a demo has its proof), 0 `external_passed`. Per demo, the largest are
`app_slash_boss_3d` (1,998 calls, 3,552 `type_position`, 22 `function_typed_value`, 13 `passed_deep`) and `pure_hvm5_mini` (656 calls, 590 constructs, 21 `function_typed_value`, 2 `passed_deep`).

The two oracle rows and these counts are those after the `passed-do` fix (a do bind's, exec's and step's bare def is a passed edge): before it, `passed` was 117 Refs and `passed_deep` 104, and 33 passed edges and 28 `passed_deep` over the books; 39 Refs and 13 edges moved from the one to the other, nothing else changed (0 in none, 0 differ).

### The sweep (BD4 step 5)

`scripts/bend-sweep.sh <root> <entry>` builds `bend-cli` in release and runs `bend-cli calls <root> <entry> --all` under `/usr/bin/time -l`. `--all` enters **every def of every file of the book once, at depth 1**, so each
site and each body's counters are seen once (the same way Rust's sweep was run: a def's own body), and sums them (the book's own counters once). 2026-11-20, macOS, the owner's machine. The 16 entries are each demo's
`main.bend`, except `io_rollback_netcode` (no `main.bend`: its largest book, `walkers_test.bend`); the last row is the largest book of the corpus, entered at `PROOF.bend` (510 entered, 4 laws with a proof skipped: `call_graph`
enters the proof by that name). Columns: edges `call`, `construct`, `external`, `passed`, `unresolved` (all of them `function_typed_value`: **no `unknown_name`, no `unknown_constructor`, no `external_passed`, no `open_law` in
any of them**), then the counters, then the time of the 16 requests' loop (open excluded, ms; the slowest single request of the big book is 8 ms) and the peak memory footprint (MB, `peak memory footprint` of `time -l`; the
maximum resident set size is 2–8 MB).

| entry | files | defs | contexts | call | construct | external | passed | unresolved | `type_position` | `sugar_base` | `type_passed` | `passed_deep` | `function_typed_value` | ms | MB |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| app_pong_game_2d/main.bend | 1 | 20 | 18 | 44 | 6 | 13 | 2 | 0 | 85 | 53 | 5 | 0 | 0 | 1 | 1.3 |
| app_ray_tracer_3d/main.bend | 1 | 35 | 33 | 75 | 6 | 34 | 2 | 0 | 214 | 177 | 5 | 0 | 0 | 1 | 1.4 |
| app_slash_boss_3d/main.bend | 2 | 471 | 436 | 1954 | 444 | 697 | 22 | 20 | 3425 | 2372 | 17 | 13 | 20 | 17 | 4.9 |
| app_triangle_2d/main.bend | 1 | 12 | 12 | 34 | 0 | 10 | 2 | 0 | 61 | 38 | 0 | 0 | 0 | 0 | 1.2 |
| app_win_is_bug_2d/main.bend | 1 | 30 | 28 | 73 | 14 | 13 | 0 | 0 | 66 | 14 | 1 | 0 | 0 | 1 | 1.2 |
| io_hello_world/main.bend | 1 | 1 | 1 | 0 | 0 | 1 | 0 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 1.1 |
| io_http_fetch/main.bend | 1 | 14 | 13 | 21 | 5 | 51 | 0 | 0 | 55 | 11 | 3 | 0 | 0 | 0 | 1.2 |
| io_http_server/main.bend | 1 | 7 | 7 | 7 | 0 | 12 | 0 | 0 | 25 | 10 | 0 | 0 | 0 | 0 | 1.1 |
| io_rollback_netcode/walkers_test.bend | 3 | 172 | 160 | 276 | 56 | 175 | 4 | 13 | 886 | 195 | 16 | 0 | 13 | 4 | 2.0 |
| io_tcp_echos/main.bend | 1 | 8 | 8 | 9 | 0 | 12 | 0 | 1 | 32 | 9 | 0 | 0 | 1 | 0 | 1.1 |
| proof_insertion_sort/main.bend | 2 | 10 | 10 | 13 | 0 | 2 | 0 | 0 | 29 | 4 | 0 | 0 | 0 | 0 | 1.1 |
| proof_numerics/main.bend | 1 | 7 | 7 | 9 | 0 | 0 | 0 | 0 | 21 | 1 | 0 | 0 | 0 | 0 | 1.1 |
| proof_typed_eval/main.bend | 1 | 13 | 11 | 21 | 28 | 2 | 0 | 0 | 36 | 0 | 0 | 0 | 0 | 0 | 1.2 |
| pure_hvm5_mini/main.bend | 1 | 188 | 161 | 611 | 590 | 60 | 14 | 21 | 931 | 132 | 34 | 2 | 21 | 5 | 2.8 |
| pure_par_sort/main.bend | 1 | 8 | 8 | 18 | 0 | 11 | 0 | 0 | 32 | 0 | 0 | 0 | 0 | 0 | 1.1 |
| pure_par_sum/main.bend | 1 | 3 | 3 | 5 | 0 | 5 | 0 | 0 | 6 | 0 | 0 | 0 | 0 | 0 | 1.1 |
| **the 16 `main` entries** | 20 | 999 | 916 | 3,170 | 1,149 | 1,098 | 46 | 55 | 5,905 | 3,016 | 81 | 15 | 55 | 29 | 4.9 (max) |
| **app_slash_boss_3d/PROOF.bend** (the largest book) | 5 | 514 | 475 | 1,998 | 444 | 708 | 22 | 22 | 3,524 | 2,385 | 17 | 13 | 22 | 21 | 4.9 |

Contexts are the distinct contexts of the union of the graphs: the defs entered and the defs, types and leaves they reach. Everything is milliseconds and a few MB: a book of these sizes is rebuilt, and every def of it walked, in less than
a frame (BD5's `refresh` can drop the book).

**The sweep found a cost the tests did not.** The first run of `app_slash_boss_3d/main.bend` took **2,333 ms** (471 requests, about 5 ms each): a request made a new `Bend`, which read the file it touched again (the places of the bodies,
and the markers with the caller's style: two parses of a 216 kB file). The book now keeps both (`OnceLock` per file, markers per label behind a `Mutex`): **34 ms**, and 17 ms for the table's run.

**What the 28 `passed_deep` were** (the table above is after the `passed-do` fix, which drew the 13 as passed edges: `pure_hvm5_mini` passed 1 → 14, `passed_deep` 15 → 2; the 15 that remain are the types) (`app_slash_boss_3d` 13, `pure_hvm5_mini` 15; each read at its site): **15 are project type names**, operands of `->` or `&` in a value position (`@+f: F -> Cam`, the type argument `Sound & Audio` of
`IO.pure(…)`): a type written where the walk does not know it is a type (an arrow's or a pair's operands are walked, as the plan says). **13 are one thing: the value of a do bind or exec** (`w : String <- name`, `Unit <- pop`,
`e : Bool <- eof`, `h : Hc <- peek`, all in `pure_hvm5_mini`'s parser), a bare project def used as the **value argument of the sugar's `P.bind`**: a def **handed over** to `M.bind`, which the rule for a direct argument
(D22) would draw as a passed edge, and which the walk reads as a nested term because it sees the statement and not the call it stands for. So the thirteen are a policy row owed (`do-project`: a bare def as a bind's value is
handed over to `M.bind`), not BC-level work; the fifteen are types and owe a row only if Q-BD5 changes what a type position is.

## BD5

`Session` (`bend-analysis/src/session.rs`): a book kept open, followed to the disk by stamps (D41), lost when the entry is gone (D46), no `set_text` (D42). Measured 2026-11-20, macOS, the owner's machine, release,
`cargo test --release --test corpus -- --ignored session_measure` (the demo's folder is copied to a scratch folder: the clone is only read), medians of 30 runs (300 for the refresh that changes nothing), on
`demos/app_slash_boss_3d/PROOF.bend` (5 files, 514 defs, 216 kB). The request is `Laws.live_after`, the def of that file with the largest tree (115 contexts). Two runs:

| | median, ms |
|---|---|
| `Session::open` (cold: the files read, the tables built) | 7.8 – 9.6 |
| the first request on a fresh session (the bodies of the files read) | 8.8 – 9.6 |
| `refresh`, nothing changed (one stat call per watched path) | **0.007** |
| `refresh`, the entry file changed (the book built again) | 8.0 – 8.2 |
| the first request after that `refresh` | 9.2 |

A change costs about 17 ms end to end (the load, then the bodies read again by the first request), and nothing costs 7 µs: the driver can call `refresh` before every request. The rebuild is the open of BD3 (8.5 ms, the table
above): there is no partial reparse because there is no need of one.

**What is stamped** (`stamps.rs`; the length and the modified time of each, `Missing` for a path that is not there; a folder only for a path an import asked for that turned out to be one): every file the load read, by its
real path, and every path an **import asked for**, as written from the importing file's folder, found or not. The second is what sees a file created where an import looked. The entry
file's path as the session was given it is stamped too, so a session that is lost (no book, so nothing else to watch) waits on that one. Hub imports are never looked up and so never stamped. A path is stamped **when the
loader asks for it, before the read**: a file saved while the load runs keeps a stamp older than the text the book holds and is read once more by the next `refresh` (TypeScript's `Stamps::after_build` does the
same thing from outside the load, because its engine reads the files; here the loader is ours and stamps each path itself).

**Limits, written down:** a file saved to the same length within the file system's clock tick of the last look is not seen by `refresh` (tested: `a_stamp_is_the_length_and_the_modified_time`); `reload` sees it. A file the
editor holds unsaved is not seen (D42).

## BC0

The measurement of `docs/08-bend/04-phase-3-context-layer.md` §2: for every `function_typed_value` site, how the variable got its function. Run 2026-11-21 by a spike (`languages/bend/spikes/bc0`, its own workspace, deleted with this
section's numbers): it reads each file with `bend-parser`, keeps a scope with the **origin** of every binder (a copy of the layer-1 walk with an origin on each name), and classifies each site where `bend-analysis`'s own
walk puts one. **Checked two ways:** its site list equals `walked()`'s `function_typed_value` spans def by def (0 mismatches over 78,814 defs, so no site is missed or invented), and its totals are the sweep's (3,170 calls,
1,098 externals, 1,149 constructs, 46 passed, 55 unresolved on the 16 books); a file with one case of each class (a to f) lands each in its class.

**Two inputs, and what each is.** *The demos' books*: the sweep's 16 entries, every file of each book (20 files, 916 defs), callers read over the whole book. *The corpus*: the 1,744 files the loader accepts, **each opened as its own
book, and only the entry file's own defs counted** (so no file is counted twice); callers are read over that book (the file and what it imports, never its importers). Read the corpus with three cautions: **`bench/`** is 22
generated programs of 70,623 defs (it makes the totals large and the rate small: 2 sites); **`tests/`** is 1,665 small files that probe the checker (`hurkens_paradox`, `beta_ann_lambda`: forms written to be odd), 440 of the 545
sites; and a **library file opened alone shows "no caller"** (`bend2/base.bend`, `bend3d.bend`: their callers are in files that import them), so the caller columns mean something only for the demos' whole books.

### The demos' books: 55 sites in 4,323 call sites

Call sites with a head: 3,170 calls to project defs + 1,098 externals + 55 unresolved = 4,323; **`function_typed_value` is 1.3%** of them (1.7% of the calls to project code and the unresolved). The 55 sit in 39 defs, all reachable
from `main`. No other unresolved reason occurs.

| class | sites | share |
|---|---|---|
| **a.** a template parameter `~f` of the def | 16 | 29.1% |
| **b.** a plain function-typed parameter of the def | 32 | 58.2% |
| &nbsp;&nbsp;with an arrow type / whose type is not written as an arrow (`m: P<A>`, a CPS alias) | 31 / 1 | |
| **c.** a closure from a call's result | 3 | 5.5% |
| &nbsp;&nbsp;called directly, `f(a)(k)` / bound by a let, `add2 = adder(2)` | 3 / 0 | |
| **d.** a variable from a constructor field (a `case K{f}`) | 3 | 5.5% |
| **e.** a value from a do bind | 0 | 0% |
| **f.** a lambda written in place and called | 0 | 0% |
| **g.** anything else: a lambda's own binder (`k => s => k(x, s)`) | 1 | 1.8% |
| **total** | **55** | |

87% of the sites (48) call one of the def's own parameters, and 91% (50) sit in a def that has a `~` parameter or an arrow-typed one: resolving them needs the caller. **What the callers of those 48 parameters pass, followed up
through callers' own parameters until a def name or a lambda** (a def with several callers needs all of them to end that way):

| the function can be named | a (templates) | b (parameters) |
|---|---|---|
| yes: every path up ends at a def name or a lambda | 16 of 16 | 12 of 32 |
| no: callers apply fewer arguments than the def has parameters (partial application: `Unit <- drop(2n)`, the continuation is supplied by the `P.bind` the do stands for) | 0 | 10 |
| no: the def is handed over as a value (the parser primitives `eof`, `peek`, `pop` that a `do P<..>` gives to `P.bind`, which applies them to a continuation) | 0 | 4 |
| no: a caller passes a lambda's own binder (a continuation) | 0 | 3 |
| no: no caller in the book (`P.bind`, which the do sugar calls; `name.go`) | 0 | 3 |

**The 17 parameter sites of the last three rows with the partial applications (10 + 4 + 3) are all in one program, `pure_hvm5_mini`'s continuation-passing parser monad**; the 3 passed a lambda's binder are not checked for where they are. Of the 16 template sites, **15 are in a def whose callers pass their own `~` parameter along** and only 1 has every caller passing a def name or a lambda: a key made at a call site by `t(~g, a)` resolves 1 of the 16 on its own.

**`~` arguments.** 120 of the 4,165 calls with a name head (2.9%) have one; 57 of the 916 defs have a `~` parameter. Their 387 `~` arguments: a bare def name 22, a lambda 2, a type 18 (`~U32`), **a call 10
(`~World()`, `~Walkers.game()`: a nullary def called), a constructor 3 (`~App{Fly.view, Fly.tick}`), another form 2**, and **330 are the caller's own `~` parameter passed along** (120 of them types, 210 values). Calls with a def name
or a lambda among their arguments: 37 to project defs (27, and 10 through a `~`), 11 to Base (9, and 2 through a `~`).

### The corpus: 545 sites in 145,406 call sites

| part | books | defs | calls (project) | externals | `function_typed_value` | a | b | c | d | e | f | g |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| all | 1,744 | 78,814 | 132,372 | 12,401 | 545 (0.4%) | 80 | 252 | 70 | 63 | 0 | 37 | 43 |
| `tests/` | 1,665 | 6,687 | 8,016 | 9,335 | 440 (2.5%) | 44 | 194 | 66 | 58 | 0 | 37 | 41 |
| `demos/` (the 56 entries' files) | 56 | 1,064 | 3,682 | 1,172 | 68 (1.4%) | 16 | 44 | 4 | 3 | 0 | 0 | 1 |
| `bend2/base.bend` (what every program imports) | 1 | 440 | 785 | 0 | 35 (4.3%) | 20 | 12 | 0 | 2 | 0 | 0 | 1 |
| `bench/` | 22 | 70,623 | 119,889 | 1,894 | 2 | 0 | 2 | 0 | 0 | 0 | 0 | 0 |

The detail rows, over the same columns (all / `tests/` / `demos/` / `base.bend` / `bench/`): b with an arrow type 116 / 64 / 41 / 11 / 0, **b whose type is not an arrow 136 / 130 / 3 / 1 / 2**; c called directly 56 / 52 / 4 / 0 / 0, c bound
by a let 14 / 14 / 0 / 0 / 0; d from a project constructor 48 / 43 / 3 / 2 / 0, from a Base constructor 9 / 9 / 0 / 0 / 0, from a tuple 5 / 5, from `<>` 1 / 1; f an annotated lambda `{(x => x) : T}(1)` 18 / 18 / 0 / 0 / 0, a bare one 19 / 19; g a
lambda's own binder 27 / 25 / 1 / 1 / 0, a let bound to a lambda 9 / 9 / 0 / 0 / 0, a let bound to a name 3 / 3, an `@` binder 2 / 2, an annotated name called 2 / 2. **No site is a do bind (e) in any file**, and none is an operator or a do
on a bound type name.

| | all | `tests/` | `demos/` | `base.bend` | `bench/` |
|---|---|---|---|---|---|
| sites in a def that has a `~` parameter or an arrow-typed one | 216 (39.6%) | 122 (27.7%) | 60 (88.2%) | 34 (97.1%) | 0 |
| sites whose callee is one of the def's own parameters (a + b) | 332 (60.9%) | 238 (54.1%) | 60 (88.2%) | 32 (91.4%) | 2 |
| defs with at least one `~` parameter | 170 | 81 | 57 | 32 | 0 |
| calls with a `~` argument (of the calls with a name head) | 339 (0.2%) | 184 (1.1%) | 120 (2.5%) | 35 (4.6%) | 0 |
| their `~` arguments: def name / lambda / type | 88 / 80 / 117 | 66 / 78 / 99 | 22 / 2 / 18 | 0 / 0 / 0 | |
| ... a call / a constructor / a literal / other (and a field, a let: 1 and 1 in `all` and in `tests/`) | 15 / 10 / 22 / 37 | 5 / 7 / 22 / 35 | 10 / 3 / 0 / 2 | 0 | |
| ... the caller's own `~` or plain parameter passed along | 434 | 23 | 330 | 81 | |
| a + b: the function can be named by following the callers up | 204 of 332 | 174 of 238 | 23 of 60 (the 16 whole books: 28 of 48) | 5 of 32 (the 20 templates have no caller in their own book) | 2 of 2 |

The last row's corpus columns are the caution above at work: of the 128 corpus parameters that cannot be named, 78 have **no caller in their own book** (42 templates, 36 parameters: libraries, entry points), 14 are partial applications or handed over (10 + 4, all in the CPS parser), 22 are passed a call's
result, 6 a let, 3 a lambda's binder, 3 a do bind or a constructor field, 2 something else.

### Which of BC1, BC2 and BC3 the numbers argue for

*The context layer is the last 1.3% of the sites, and a finished BC1 + BC2 names the function for 28 of the 55 demo sites: 0.65 points of the 4,323 call sites.* That is the size of the prize, and the plan's own words ("about the last
1%") are right. Within it the numbers are not close. **Templates and parameters are 87% of the demo sites, 91% of Base's, and 54% of the test suite's (where c, d and f are the checker probes): BC1 and BC2 pay and BC3 does not.** They are
one mechanism and should be built as one step: **15 of the 16 demo template sites sit in a def whose callers pass their own `~` parameter along, so a key made only at `t(~g, a)` (BC1 as §3 words it) resolves 1 of them; the rule
that the caller's key flows down (§4's "unless the caller's key knows it") is what resolves the other 15, and it is the same rule that resolves 12 of the 32 parameters.** Build the key with the flow-down from the first commit,
templates first inside it (every one of the 16 is exact and named by the compiler's own rule), then plain parameters (the same code with a weaker guarantee). **BC3 (values through lets and returns) is 3 of 55 in the demos and none
in Base:** all three are a call's result called directly, `f(a)(k)`, none a let (`add2 = adder(2)`), and the corpus's 70 are 66 test-suite probes; it is not worth building until a program of the owner's has an `add2 = adder(2)`. Neither is the constructor-field case (d: 3 of 55 in the
demos, 2 in Base) nor the lambda in place (f: 0 outside the test suite) nor the do bind (e: 0 in 1,744 files). What none of the three resolves is the parameter sites the table names, and **17 of the 20 are one program**: `pure_hvm5_mini`'s CPS parser, where 10 are a def applied to fewer arguments than it has parameters (`Unit <- drop(2n)`: the continuation arrives later, through `P.bind`), 4 are primitives (`eof`, `peek`, `pop`) handed to `P.bind`, and 3 have no caller but the sugar's. Tracking an unfinished call through `M.bind` is a different mechanism from a key, for one program; I would leave it a limit of §6's list and add it there.

**What in §1 and §2 the numbers show to be wrong or incomplete.** (1) §3 and §1 say a template argument is closed, "top-level defs and lambdas only": of the demos' 385 `~` arguments 22 are def names and 2 lambdas, and **13 are neither a
def, a lambda nor a type**: `~World()` and `~Walkers.game()` (a nullary def **called** at compile time) and `~App{Fly.view, Fly.tick}` (a constructor of defs). `Known` as §1 writes it (a def or a lambda) cannot hold those; a template
parameter fed by them is a value that contains functions, which is class d's problem, not a def's. (2) §1's table has four ways; the sites show **two more**: a lambda's own binder (`k => s => k(x, s)`, the continuation of a CPS
monad; 27 in the corpus, 1 in the demos, 1 in Base) and a parameter the callers do **not pass at all** (partial application: 10 of the demos' 32 parameter sites), and §1's "the caller, when it passes a name or a lambda" does not cover the second.
(3) §2's go/no-go reasoning, "user code calls Base's higher-order defs far more than it writes its own", is the other way round in the demos: 11 calls pass a def name or a lambda to Base and 37 pass one to a project def (the
count takes a bare lowercase or dotted Base name for a def). (4) §2's prior holds: 55 of 4,323 on the 16 mains (68 of 4,922 on the 56 entries) against "64 of about 5,900", and 35 of 820 in Base against "39 of about 1,000". (5) §1's "one kind of site is unresolved"
holds: no operator and no do is on a bound type name anywhere (the `Sugar::Bound` case of the walk occurs 0 times in 1,744 files).

## BC1+BC2

The context layer of `docs/08-bend/06-context-layer-design.md`, built 2026-11-21 in nine commits (89a07ba, 1030b7f, 7bc5520, 4628b69, 14248b7, 325066c, 03270d3, b334658, 2c03f3b), each reviewed with its proofs
reproduced. It is **off by default** (`Options::precision`, `bend-cli calls --entry <q> --precise`); every number of the sections above is `Precision::Default` and did not move. Measured by
`context_layer_on_the_demos` (ignored, `--test corpus`, release), which pins every count below and prints the times; rerun by the reviewer with the same counts.

    BEND_CLONE=<the bend clone> cargo test -p bend-analysis --release --test corpus -- --ignored --nocapture context_layer_on_the_demos

**The input** is BC0's: the sweep's 16 books, each asked whole at its `main`, at `Default` and at `Precise`. Under `Default` they have 55 unresolved sites, all `function_typed_value`: `app_slash_boss_3d` 20,
`io_rollback_netcode` 13, `io_tcp_echos` 1, `pure_hvm5_mini` 21; the other 12 books have none. A **site** is a def and a span; under `Precise` a def can have several contexts, so each site is read in every context
of its def. In one context it is **drawn** (a `Call` of a def), a **lambda** (`Unresolved(anonymous_function_called)`: the parameter is known to be a lambda, whose calls are drawn where it is written, BC-D6), or
**unresolved** (`function_typed_value`, as before).

### The 55 sites

| class | `~` parameter | plain parameter | not a parameter | all |
|---|---|---|---|---|
| drawn in every context | 16 | 0 | 0 | **16** |
| a lambda in every context | 0 | 12 | 0 | **12** |
| drawn in some, a lambda in others | 0 | 0 | 0 | 0 |
| named in some, unresolved in others | 0 | 3 | 0 | **3** |
| unresolved in every context | 0 | 17 | 7 | **24** |
| the def is not reached | 0 | 0 | 0 | 0 |
| all | 16 | 32 | 7 | 55 |

By book (drawn / lambda / named in some / unresolved): `app_slash_boss_3d` 16 / 4 / 0 / 0; `io_rollback_netcode` 0 / 7 / 3 / 3; `io_tcp_echos` 0 / 1 / 0 / 0; `pure_hvm5_mini` 0 / 0 / 0 / 21.

**BC0's forecast holds to the site: 28 named in every context, 16 at a template parameter and 12 at a plain one.** What BC0 could not say is what they are named as. **The 16 templates are all a def, and are drawn; the 12 plain
parameters are all a lambda, and are not.** So the tree of a demo gains 16 callees, every one in `app_slash_boss_3d`'s `bend3d.bend` (the library's hooks, `~cam`, `~shade`, `~unit`, `~at`, handed down chains of `~`
parameters from `main.bend`, which names its own `Stage.*`, `Sky.*` and `Mesh.of`; `bend3d.bend` has defs of some of the same names, and the target is the one the naming file means), and each of those defs has
the one context it had, now with a key:

| book | file | def | at | the call | is |
|---|---|---|---|---|---|
| `app_slash_boss_3d` | `bend3d.bend` | `Blk.at` | 1241:7 | `at` | `Sky.at` |
| `app_slash_boss_3d` | `bend3d.bend` | `Cloth.move` | 804:61 | `wind` | `Stage.wind` |
| `app_slash_boss_3d` | `bend3d.bend` | `Cloth.patch` | 901:3 | `mesh` | `Mesh.of` |
| `app_slash_boss_3d` | `bend3d.bend` | `Cloth.rope` | 918:53 | `cam` | `Stage.cam` |
| `app_slash_boss_3d` | `bend3d.bend` | `Cloth.rope` | 919:3 | `mesh` | `Mesh.of` |
| `app_slash_boss_3d` | `bend3d.bend` | `Grid.go` | 641:61 | `cam` | `Stage.cam` |
| `app_slash_boss_3d` | `bend3d.bend` | `Mesh.of` | 712:56 | `cam` | `Stage.cam` |
| `app_slash_boss_3d` | `bend3d.bend` | `Mesh.vert` | 532:25 | `cam` | `Stage.cam` |
| `app_slash_boss_3d` | `bend3d.bend` | `Mesh.vert` | 532:37 | `shade` | `Stage.shade` |
| `app_slash_boss_3d` | `bend3d.bend` | `Tile.b4` | 1407:5 | `col` | `Sky.col` |
| `app_slash_boss_3d` | `bend3d.bend` | `Tile.b4` | 1407:23 | `col` | `Sky.col` |
| `app_slash_boss_3d` | `bend3d.bend` | `Tile.go` | 1464:10 | `of` | `Sky.of` |
| `app_slash_boss_3d` | `bend3d.bend` | `Tile.go` | 1466:31 | `flat` | `Sky.mixed` |
| `app_slash_boss_3d` | `bend3d.bend` | `Tile.pick` | 1448:11 | `at` | `Sky.at` |
| `app_slash_boss_3d` | `bend3d.bend` | `Unit.fit` | 681:7 | `size` | `Stage.size` |
| `app_slash_boss_3d` | `bend3d.bend` | `Unit.go` | 187:7 | `unit` | `Stage.unit` |

The 12 lambdas, and the 3 that are a lambda in all contexts but one (the last column: the contexts where the site is a lambda):

| book | file | def | at | the call | a lambda in |
|---|---|---|---|---|---|
| `app_slash_boss_3d` | `main.bend` | `Play.pace` | 2736:7 | `k` | 1 of 1 |
| `app_slash_boss_3d` | `main.bend` | `Play.pace` | 2739:76 | `k` | 1 of 1 |
| `app_slash_boss_3d` | `main.bend` | `Play.serve` | 2750:5 | `rest` | 1 of 1 |
| `app_slash_boss_3d` | `main.bend` | `Play.turn` | 2786:23 | `rest` | 1 of 1 |
| `io_rollback_netcode` | `netcode.bend` | `Client.sync.go` | 680:9 | `rest` | 1 of 1 |
| `io_rollback_netcode` | `netcode.bend` | `Post.insert.go` | 155:12 | `rest` | 1 of 1 |
| `io_rollback_netcode` | `netcode.bend` | `Post.next.go` | 170:7 | `rest` | 1 of 1 |
| `io_rollback_netcode` | `netcode.bend` | `Post.next.go` | 172:7 | `rest` | 1 of 1 |
| `io_rollback_netcode` | `netcode.bend` | `String.word.go` | 372:7 | `rest` | 15 of 15 |
| `io_rollback_netcode` | `walkers_demo.bend` | `World.input.go` | 103:12 | `rest` | 1 of 1 |
| `io_rollback_netcode` | `walkers_test.bend` | `Bot.more` | 74:7 | `rest` | 2 of 2 |
| `io_tcp_echos` | `main.bend` | `echo_sent` | 25:7 | `next` | 1 of 1 |
| `io_rollback_netcode` | `netcode.bend` | `String.word` | 378:7 | `k` | 14 of 15 |
| `io_rollback_netcode` | `netcode.bend` | `String.word.go` | 370:7 | `k` | 14 of 15 |
| `io_rollback_netcode` | `netcode.bend` | `String.word.go` | 372:22 | `k` | 14 of 15 |

Each is a continuation written at the call (`Play.pace(pace, w => b => fr => Play.go(…))`, `Post.insert.go(…, u => Post.insert(P, p, t))`, `String.word(…, room => from => Msg.join(room, from))`). Its own calls are in
the tree already, under the def that writes it. **The three `String.word` sites** are BC0's "a caller passes a lambda's own binder": 14 callers hand `String.word` a lambda, and its own recursion goes through
`k2 => String.word(R, t, k2)`, whose call belongs to the def it is written in and passes the binder `k2`, so it makes the open context of `String.word`, where `k` is unknown. BC0 asked that every caller end at a def or
a lambda and filed the three under "no"; per context they are named in 14 of 15.

**The 24 unresolved in every context are BC0's classes, none in between**, each pinned by a limit row of `docs/policy.md`: 17 plain parameters of `pure_hvm5_mini`'s CPS parser (10 partial applications, 4 primitives
handed to the monad's bind, 3 with no caller but the sugar: rows `limit-partial-application`, `limit-handed-to-bind`); 3 calls of a call's result (row `closure-call`); 3 variables from a constructor's fields, `Game.read`,
`Game.show`, `Game.step` (row `limit-constructor-of-defs`); 1 lambda's own binder, `P.pure`'s `k` (row `limit-lambda-binder`). By def: `Game.read` 255:3 `read`; `Game.show` 259:3 `show`; `Game.step` 251:3 `step`; `P.bind` 524:13 `m`, 524:20 `f`, 524:20 `f(a)`; `P.pure` 521:13 `k`; `bindc.go` 617:7 `k`, 620:7 `k`, 623:7 `k`, 626:7 `k`; `bindv` 604:3 `k`; `drop` 538:3 `k`; `eof` 704:7 `k`, 706:7 `k`; `index.go` 734:3 `k`; `look.go` 690:7 `k`; `main` 1916:3 `prog()`; `name.go` 548:3 `k`; `opt.go` 565:7 `k`, 567:7 `k`; `peek` 533:3 `k`; `pop` 646:7 `pop.go(U32.is_eq(u, 1), nm, u)`, 648:7 `k`.

### What it costs

| book | contexts | unresolved edges | `Default` ms | `Precise` ms |
|---|---|---|---|---|
| `app_slash_boss_3d` | 424 → 424 | 20 → 4 | 3.06 | 3.70 |
| `io_rollback_netcode` | 130 → 160 | 13 → 70 | 0.43 | 0.68 |
| `pure_hvm5_mini` | 157 → 157 | 21 → 21 | 0.95 | 1.13 |
| `io_tcp_echos` | 8 → 8 | 1 → 1 | 0.03 | 0.03 |

Median of 20 requests, release, the owner's machine, the coder's run; the other 12 books are under 0.25 ms in both and gain nothing but the option's check. The reviewer's run, on a busier machine, gave the same
counts and 1.1 to 1.4 times the `Default` time for `Precise` on the three large books. **The cost is the keys, not more bodies**: 15 books walk exactly the bodies they walked. Only `io_rollback_netcode` has more contexts,
30 more in four defs: `String.word` and `String.word.go` have 15 each (one per lambda a caller writes, and the open one), `Bot.frame` and `Bot.more` 2 each. **Every key that multiplies a def holds a lambda; no def of
the 16 books is reached under two different defs.** A lambda's context differs from the open one only in the reason its sites give, and in what it passes down. That is the price of knowing it, and of `Known::Lambda`
being there for the day a lambda's calls are drawn under its caller.

**Count sites, not edges.** `Stats.unresolved` and the reasons in `Stats.language` are per edge, and an edge is per context: `io_rollback_netcode` goes from 13 unresolved edges to 70 (64 `anonymous_function_called`,
6 `function_typed_value`) while its unresolved sites stay 13 and 10 of them become known. A dashboard that shows `Stats.unresolved` would say the layer made things worse.

### The counters, and what they say about the unbuilt phases

`key_slot_*` count, per context walked, a parameter the callee **calls** that the caller did not name, by why (`docs/08-bend/06` §4). Over the 16 books: `key_slot_missing` 14 and `key_slot_open` 5, all in
`pure_hvm5_mini` (the partial applications of its parser, and their parameters handed on); `key_slot_local` 15 and `key_slot_open` 1 in `io_rollback_netcode`; `key_slot_value` 0 everywhere. **The 15 locals are one argument**, the
binder `k2` in `String.word(R, t, k2)`, counted once in each of `String.word`'s 15 contexts: a lambda's own binder, not a `let`. So in the demos:

- **no `let` holds a function that is then passed** (BC3's `g = work; each(g, 1)`: 0 sites), and no Base def is passed to a parameter that is called;
- a call's result called is 3 sites, a constructor's field 3 (BC0's c and d, unchanged);
- **the one unbuilt thing that would change the demos' trees is drawing a lambda's calls under the function that calls it** (TypeScript's TC7, Q-TC1 and Q-RC1's other answer; Python does it for a nested `def` and not for a `lambda`): the 12 sites, and the 3 of
  `String.word` whose binder would then have a value. 15 of 55, against 16 drawn by this step. It is a tree rule shared by every language, so it was asked as Q-BD9; **answered yes the same day (D54), and designed in `docs/08-bend/07-value-layer-design.md`**.

## grammar

S-BD2 (G1–G30, step 2; G31–G36 found in step 4): every form `bend.ts` reads that the BNF comment (lines 8–160) does not name, or names differently. Read 2026-11-20 from `parse_book` (2593), `parse_def`
(2535), `parse_tele` (2487), `parse_term_base` (1818), `parse_term_ops` (2101), `parse_term_all` (2241), `parse_term_tup` (2260), `parse_term_num` (2278),
`parse_term_do_stmt` (2329), `parse_body` (2376), `parse_block` (2466), `book_load` (968). Each row has its fixtures (last column), written in BD1 steps 3 and 4.

Two facts change the plan's picture before the list:

- **The parser is not purely syntactic.** Three decisions read the book's tables while parsing: `+Name` is a quantified datatype (`+D<A>`) if `Name` is a declared
  `type`, else a reusable variable (`parse_term_base` `case "+"`); `f(~g, a)` takes `~` arguments only as many as the callee def's template count
  (`parse_term_ops`, `hd.x`); and `parse_patt` asks whether a name is a constructor. Our tree keeps both readings open (`+` applies to a term; `~` is accepted
  on any call argument) and resolution decides later (BD3/BD4).
- **A name's characters are letters, digits, `_` and `.`, ASCII only** (`char_is_name` 503, `char_is_head` 498): not `-`. A `-` is a quantity mark or an operator
  (the plan's `01 §2.3` and `02 §2.3` list `-` among a name's characters: wrong).

| # | Form | Read at | The BNF says | Fixtures (all written) |
|---|---|---|---|---|
| G1 | `f!(a)`: `!` before the call parens marks an offload; only on a named def | `parse_term_ops` | nothing | `call_bang` (written) |
| G2 | `~` on call arguments: `t(~g, a)`, each `~` before the leading template arguments, or none | `parse_term_ops` | prose only (lines 136–138) | `call_template` (written) |
| G3 | `+x` as a term: a reusable variable (`Var` with the "many" mark), not only `+D<A>` | `parse_term_base` `"+"` | `Plus ::= "+" D (<…>)?` only | `plus_var` (written) |
| G4 | `(e : T)` inside any parens, not only around an operator: the `: T` names the operator namespace for the operators inside and is dropped | `parse_term_tup` | `Grp ::= "(" Body ")"`, `Ops` row | `paren_annotation` (written) |
| G5 | `\{ K: a; L: b; tail }`: any number of arms (`Name ":" Term`, `;` optional), then an optional tail term; `\{}` is the empty match, and `\{ K: a }` has an implicit empty tail | `parse_term_base` `"\\"` | `Mat` shows one arm and a tail | `lam_match` (written) |
| G6 | `K{…}` needs the `{` **glued** to the name; `K {` is a name then a stray `{`. `D<…>` and `f(…)` may be spaced | `parse_term_base` (`parse_take("{")`, no skip) | nothing | `ctr_glued`, `recover_ctr_space` (written) |
| G7 | `@ unsafe def` may have space after `@` (`parse_word` skips); `def f?(…)`: the `?` must follow the name with no space | `parse_book`, `parse_def` | `("@unsafe")? … Name "?"?` | `def_unsafe`, `recover_question_space` (written) |
| G8 | a def's foreign body: `import "path"` repeated; the path is read raw to the next `"` (no escapes) and must end `.c` or `.js`; a template cannot be foreign | `parse_def` | `["import" STRING]+` | `def_foreign`, `recover_foreign` (written) |
| G9 | a `def` with no `->` fills a law (its parameters are bare names); with `->` it types itself | `parse_def` | `("->" Term)?` | `law_and_proof` (written) |
| G10 | file-level `import` lines are read by a **regex on trimmed lines**, not by `parse_*`: `import Base`, or `import <path> as <Alias>`; a trailing `# comment` is allowed; alias is `[A-Za-z_]\w*` (no dots); only blank and `#` lines may precede; the first other line ends the import section; the import lines are blanked before `parse_book` | `book_load` 973–1000 | `Import ::= "import" "Base" \| "import" Path "as" Name` | `imports`, `recover_import` (written) |
| G11 | a declared name may not repeat an earlier one, an alias's name, or Base's bare name: errors `bend` raises while parsing, not syntax | `parse_fresh` 2525 | prose | none: we never refuse for it (written) |
| G12 | constructor list of a `type` has no terminator: it continues while the next character is a name head and the word is not `def`/`type`/`law`; a ctor's fields use `tele("}")`: no `~`, a duplicate field name is an error | `parse_book` `"type"` | `[Ctr]` | `type_plain`, `type_ctrs_flat`, `type_kinds`, `recover_ctr_fields` (written) |
| G13 | `tele`: `~` only on leading parameters of a `)` list; a bare name `x` is `-x: Quant`; a field/param separator `,` is optional | `parse_tele` | `Bind ::= Quant Name ":" Term \| "~" Name ":" Term \| Name` | `tele_marks`, `recover_tilde` (written) |
| G14 | `@Bind -> T` reads only `Quant Name ":" Term` (domain at precedence 1: stops at the first bare `->`): not `~`, not a bare name | `parse_term_all` | `All ::= "@" Bind "->" Term` | `all_binder` (written) |
| G15 | `[ ]`: `[a, b]` commas optional (`[a b]`); `[v : T * n]` needs `T` at precedence 12; `*` takes a power of two, `^` a depth | `parse_term_base` `"["` | `Array` row | `array_forms` (written) |
| G16 | index: `x[i]` / `x[i] <- v` where the value is parsed at precedence 2; the statement form `a[i] <- v` + a following statement is the let `a = a[i] <- v` | `parse_term_ops`, `term_write` | `Index` row, prose | `index_write` (written) |
| G17 | numbers: `(\d+)(n\|\.\d+([eE][+-]?\d+)?)?`; **no** `1e5`, hex or `_`; a literal glued to a name character is an error; `3n+t` takes a term after a glued `+` | `parse_term_num` | `Nat`/`U32`/`F32` rows | `numbers`, `recover_number_name` (written) |
| G18 | char and string escapes: `\n \t \r \0 \\ \' \"` and `\u{hex}`; a string or char may hold a raw newline | `parse_char` 1660, `ESCAPES` 1208 | `Chr`/`Str` rows | `chars_strings`, `recover_bad_escape` (written) |
| G19 | the operator table and its precedence (`INFIX` 2049): `->` 0 · `&` `\|` 1 (right) · `\|\|` 2 · `&&` 3 · `< <= > >=` 4 · `<>` `++` `<&>` 5 (right) · `.\|.` 6 · `.^.` 7 · `.&.` 8 · `<<` `>>` 9 · `+ -` 10 · `* / %` 11; `%` needs whitespace after it (it also heads a rewrite); `-`/`+` glued to `>` or a name head is no operator; a glued `>`-headed operator never fires | `parse_term_ops` | prose + `Ops` row | `ops_precedence`, `ops_glued` (written) |
| G20 | `<` after a name: argument at precedence above comparison, then `>` or `,` decides type arguments (`D<A, B>`) or less-than; a compound first argument needs parens (`F<(A & B)>`); `>` closers stack (`List<List<A>>`) | `parse_term_ops` | prose (lines 115–126) | `type_args`, `juxt_lt_decision`, `juxt_compound_first_arg`, `juxt_glued_gt`, `recover_compound_type_arg` (written) |
| G21 | layout: a newline before `(` or `[` ends a spine; `parse_body(col)` — a `match`'s `case` rows sit at the first row's column, which must be deeper than the body's; a do-block statement is a step when a `;` or a line at the block's first column follows | `parse_body`, `parse_more`, `parse_term_ops` | prose | `layout_match_cols`, `layout_do_steps`, `juxt_newline_ends_spine`, `recover_newline_ends_spine` (written) |
| G22 | a body is zero or more lets, then a reply: `-x = v` (erased), `x : T = v` (typed; a `: T` with no `=` rewinds), `x y z = a b c` (parallel), destructuring patterns (a term turned into a pattern: `K{a, b}`, literals, `+x`) | `parse_body` | `Local ::= (Quant Name \| Term)+ "=" Term+ ";"? Body` | `let_forms`, `let_typed_rewind`, `juxt_plus_binder` (written) |
| G23 | `match a b:` / `match a, b:` (commas optional); `case p q:` needs as many patterns as scrutinees; `match` heads a body only, never a term (parens hold a body: `(match …)`) | `parse_terms`, `parse_body` | `Match ::= "match" [Term] ":" [Case]` | `match_multi`, `match_in_parens`, `recover_match_arity` (written) |
| G24 | a lambda binder is parsed as a **term** first, then turned into one name (`x => b`, `+x => b`, `_ => b`); only at precedence 0; the body is a `parse_block` (lets and matches allowed) | `parse_term_ops` | `Lam ::= Name "=>" Body` | `lambda_forms`, `recover_lambda_binder` (written) |
| G25 | rewrite `%e : P; body` / `%e@E : P; body` (`@` binds the equation name; `;` optional), the tail is a body | `parse_term_base` `"%"` | `Rwt` | `rewrite` (written) |
| G26 | `do M<ts>:` heads a **term**; its statements: `x : T = v`, `t <- v`, `x : A <- v`, `t` as a step, `return t`, a final value; `do_end` rejects a stray `=` ("write x : T = …") | `parse_term_do_stmt` 2329 | `DoBlock`, `DoStmt` | `do_forms`, `layout_do_steps`, `recover_do_end` (written) |
| G27 | law clauses: `for [-\|+\|~]x: T [where W]` and `exs x: T [where W]` (no quantity on `exs`); `~` only on leading clauses; the claim after the clauses is a body | `parse_book` `"law"` | `Clause`, `Law` | `law_clauses`, `recover_law` (written) |
| G28 | `?Name` hole (`?TODO` counts as incomplete: semantic); `Type`, `Data`, `Quant`, `Kind(t)` are term heads; `&0 &1 &2` are quantities; `match`/`case`/`return`/any keyword at term start is an error | `parse_term_base` | `Hol`, `Typ`, `Qnt`, `Qua` | `hole_kinds`, `recover_keyword_term` (written) |
| G29 | `#` starts a comment anywhere whitespace may be (`parse_skip`), including inside a term; there is no block comment; `#` inside a string or char is not one | `parse_skip` 1595 | nothing | `comments` (written) |
| G30 | a dotted name: every `.` must be followed by a letter or `_` (`a.1` and `a.` are errors); one name, the dot means nothing | `parse_lexeme` 1636 | prose | `name_dots`, `recover_name_dot` (written) |
| G31 | **a parenthesised group is the term inside it** in `bend.ts` (`parse_term_tup` returns the body), so a binder may be written `(x : T) => b`; the `: T` is only an operator namespace and is dropped. The tree keeps it (`Lam.annot`) | `parse_term_tup` 2260, `parse_bind` 1764 | not in the BNF (or differs) | `lambda_forms` (written) |
| G32 | `D<>` is **refused**: after `<` a first argument is required, though the BNF writes `"<" [Term ","?] ">"`. (`do M<>:` is a separate path and takes none.) | `parse_term_ops` 2185 | not in the BNF (or differs) | `recover_type_args_empty` (written) |
| G33 | in a do block `t <- v` with `t` any term (not a name) is `Exec`: `t` is the type and the binder `_`; a name followed by `:` is typed (`Let` with `=`, `Bind` with `<-`); a term followed by `;` or a line at the block's column is a `Step` | `parse_term_do_stmt` 2329 | not in the BNF (or differs) | `layout_do_steps`, `do_forms` (written) |
| G34 | a **def's body** is read with `parse_body(col 0)`, a law's claim, a lambda body and a rewrite tail with `parse_block` (the column of their first token): a `case` may sit at its `match`'s own column in a def body | `parse_def` 2588, `parse_block` 2466 | not in the BNF (or differs) | `layout_match_cols` (written) |
| G35 | a string (and a foreign import's path) may span lines, and one that never closes runs to the end of the text; recovery restarts at its opening quote and reads strings per line, so the declarations below it survive | `parse_term_base` 2024 | not in the BNF (or differs) | `recover_unterminated`, `recover_foreign` (written) |
| G36 | a typed let is tried with a memo (`p.ann`): `x : T` with no `=` after it rewinds, and the position is remembered so that nested annotations are not read twice at each level | `parse_body` 2411 | not in the BNF (or differs) | `let_typed_rewind` (written) |
| G37 | **a byte-order mark** at the start of a file: `book_load` trims one off an `import` line (`String.trim` strips U+FEFF) but `parse_skip` does not skip one before a declaration, so `bend` refuses a BOM file that starts with a `def`. `bend-parser` reads it as whitespace at the start of the text (a file with no outline at all is the worse answer), and the file marker goes after it | `book_load` 973, `parse_skip` 1595 | not in the BNF | `bom`, `bom_doc` (parser), `bom` (syntax) (written) |
