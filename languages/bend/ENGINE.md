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
