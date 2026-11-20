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
74,427 laws, 7,735 types, 15,313 constructors), the loader's count exactly. Parsing alone took 57–76 ms over all of it (release; this is the step 5
measure, taken early because it was free).

- A def with no `->` fills a law, and its template count is the law's (`for ~k`), so the test does not compare it: 32 files showed that, all of them
  law-filling defs. A def that fills a law of another file is named `<Alias>.<name>` in the text and `<namespace>:<name>` by Bend; the test accepts
  that pair when the alias is one of the file's own.
- The test is not vacuous: breaking one rule of the term skipper (`>` glued never ends a term) makes 372 declarations disagree.
- Parameter and return types are **skipped, not read**, in step 3 (`stopgap.rs`, deleted in step 4): `Bind.ty`, `DefDecl.ret`, `TypeDecl.kind`, a clause's
  type and a body are `Span`s for now. Step 4 turns each into the term it covers; the corpus numbers above are the proof that the extent of each was right.

## grammar

S-BD2: every form `bend.ts` reads that the BNF comment (lines 8–160) does not name, or names differently. Read 2026-11-20 from `parse_book` (2593), `parse_def`
(2535), `parse_tele` (2487), `parse_term_base` (1818), `parse_term_ops` (2101), `parse_term_all` (2241), `parse_term_tup` (2260), `parse_term_num` (2278),
`parse_term_do_stmt` (2329), `parse_body` (2376), `parse_block` (2466), `book_load` (968). Each row becomes a fixture in step 4 of BD1 (named in the last column;
`tele`, `skip`, `lexeme`, `name`, imports and the declaration forms are covered by step 3's fixtures).

Two facts change the plan's picture before the list:

- **The parser is not purely syntactic.** Three decisions read the book's tables while parsing: `+Name` is a quantified datatype (`+D<A>`) if `Name` is a declared
  `type`, else a reusable variable (`parse_term_base` `case "+"`); `f(~g, a)` takes `~` arguments only as many as the callee def's template count
  (`parse_term_ops`, `hd.x`); and `parse_patt` asks whether a name is a constructor. Our tree keeps both readings open (`+` applies to a term; `~` is accepted
  on any call argument) and resolution decides later (BD3/BD4).
- **A name's characters are letters, digits, `_` and `.`, ASCII only** (`char_is_name` 503, `char_is_head` 498): not `-`. A `-` is a quantity mark or an operator
  (the plan's `01 §2.3` and `02 §2.3` list `-` among a name's characters: wrong).

| # | Form | Read at | The BNF says | Fixture (step 4) |
|---|---|---|---|---|
| G1 | `f!(a)`: `!` before the call parens marks an offload; only on a named def | `parse_term_ops` | nothing | `call_bang` |
| G2 | `~` on call arguments: `t(~g, a)`, each `~` before the leading template arguments, or none | `parse_term_ops` | prose only (lines 136–138) | `call_template` |
| G3 | `+x` as a term: a reusable variable (`Var` with the "many" mark), not only `+D<A>` | `parse_term_base` `"+"` | `Plus ::= "+" D (<…>)?` only | `plus_var` |
| G4 | `(e : T)` inside any parens, not only around an operator: the `: T` names the operator namespace for the operators inside and is dropped | `parse_term_tup` | `Grp ::= "(" Body ")"`, `Ops` row | `paren_annotation` |
| G5 | `\{ K: a; L: b; tail }`: any number of arms (`Name ":" Term`, `;` optional), then an optional tail term; `\{}` is the empty match, and `\{ K: a }` has an implicit empty tail | `parse_term_base` `"\\"` | `Mat` shows one arm and a tail | `lam_match` |
| G6 | `K{…}` needs the `{` **glued** to the name; `K {` is a name then a stray `{`. `D<…>` and `f(…)` may be spaced | `parse_term_base` (`parse_take("{")`, no skip) | nothing | `ctr_glued` |
| G7 | `@ unsafe def` may have space after `@` (`parse_word` skips); `def f?(…)`: the `?` must follow the name with no space | `parse_book`, `parse_def` | `("@unsafe")? … Name "?"?` | `def_unsafe_spacing` |
| G8 | a def's foreign body: `import "path"` repeated; the path is read raw to the next `"` (no escapes) and must end `.c` or `.js`; a template cannot be foreign | `parse_def` | `["import" STRING]+` | `def_foreign` |
| G9 | a `def` with no `->` fills a law (its parameters are bare names); with `->` it types itself | `parse_def` | `("->" Term)?` | `def_fills_law` |
| G10 | file-level `import` lines are read by a **regex on trimmed lines**, not by `parse_*`: `import Base`, or `import <path> as <Alias>`; a trailing `# comment` is allowed; alias is `[A-Za-z_]\w*` (no dots); only blank and `#` lines may precede; the first other line ends the import section; the import lines are blanked before `parse_book` | `book_load` 973–1000 | `Import ::= "import" "Base" \| "import" Path "as" Name` | `imports_*` (step 3) |
| G11 | a declared name may not repeat an earlier one, an alias's name, or Base's bare name: errors `bend` raises while parsing, not syntax | `parse_fresh` 2525 | prose | none: we never refuse for it |
| G12 | constructor list of a `type` has no terminator: it continues while the next character is a name head and the word is not `def`/`type`/`law`; a ctor's fields use `tele("}")`: no `~`, a duplicate field name is an error | `parse_book` `"type"` | `[Ctr]` | `type_ctrs` (step 3) |
| G13 | `tele`: `~` only on leading parameters of a `)` list; a bare name `x` is `-x: Quant`; a field/param separator `,` is optional | `parse_tele` | `Bind ::= Quant Name ":" Term \| "~" Name ":" Term \| Name` | `tele_*` (step 3) |
| G14 | `@Bind -> T` reads only `Quant Name ":" Term` (domain at precedence 1: stops at the first bare `->`): not `~`, not a bare name | `parse_term_all` | `All ::= "@" Bind "->" Term` | `all_binder` |
| G15 | `[ ]`: `[a, b]` commas optional (`[a b]`); `[v : T * n]` needs `T` at precedence 12; `*` takes a power of two, `^` a depth | `parse_term_base` `"["` | `Array` row | `array_forms` |
| G16 | index: `x[i]` / `x[i] <- v` where the value is parsed at precedence 2; the statement form `a[i] <- v` + a following statement is the let `a = a[i] <- v` | `parse_term_ops`, `term_write` | `Index` row, prose | `index_write` |
| G17 | numbers: `(\d+)(n\|\.\d+([eE][+-]?\d+)?)?`; **no** `1e5`, hex or `_`; a literal glued to a name character is an error; `3n+t` takes a term after a glued `+` | `parse_term_num` | `Nat`/`U32`/`F32` rows | `numbers` |
| G18 | char and string escapes: `\n \t \r \0 \\ \' \"` and `\u{hex}`; a string or char may hold a raw newline | `parse_char` 1660, `ESCAPES` 1208 | `Chr`/`Str` rows | `chars_strings` |
| G19 | the operator table and its precedence (`INFIX` 2049): `->` 0 · `&` `\|` 1 (right) · `\|\|` 2 · `&&` 3 · `< <= > >=` 4 · `<>` `++` `<&>` 5 (right) · `.\|.` 6 · `.^.` 7 · `.&.` 8 · `<<` `>>` 9 · `+ -` 10 · `* / %` 11; `%` needs whitespace after it (it also heads a rewrite); `-`/`+` glued to `>` or a name head is no operator; a glued `>`-headed operator never fires | `parse_term_ops` | prose + `Ops` row | `ops_precedence`, `ops_glued` |
| G20 | `<` after a name: argument at precedence above comparison, then `>` or `,` decides type arguments (`D<A, B>`) or less-than; a compound first argument needs parens (`F<(A & B)>`); `>` closers stack (`List<List<A>>`) | `parse_term_ops` | prose (lines 115–126) | `type_args`, `less_than` |
| G21 | layout: a newline before `(` or `[` ends a spine; `parse_body(col)` — a `match`'s `case` rows sit at the first row's column, which must be deeper than the body's; a do-block statement is a step when a `;` or a line at the block's first column follows | `parse_body`, `parse_more`, `parse_term_ops` | prose | `layout_*` |
| G22 | a body is zero or more lets, then a reply: `-x = v` (erased), `x : T = v` (typed; a `: T` with no `=` rewinds), `x y z = a b c` (parallel), destructuring patterns (a term turned into a pattern: `K{a, b}`, literals, `+x`) | `parse_body` | `Local ::= (Quant Name \| Term)+ "=" Term+ ";"? Body` | `let_*` |
| G23 | `match a b:` / `match a, b:` (commas optional); `case p q:` needs as many patterns as scrutinees; `match` heads a body only, never a term (parens hold a body: `(match …)`) | `parse_terms`, `parse_body` | `Match ::= "match" [Term] ":" [Case]` | `match_multi`, `match_in_parens` |
| G24 | a lambda binder is parsed as a **term** first, then turned into one name (`x => b`, `+x => b`, `_ => b`); only at precedence 0; the body is a `parse_block` (lets and matches allowed) | `parse_term_ops` | `Lam ::= Name "=>" Body` | `lambda_forms` |
| G25 | rewrite `%e : P; body` / `%e@E : P; body` (`@` binds the equation name; `;` optional), the tail is a body | `parse_term_base` `"%"` | `Rwt` | `rewrite` |
| G26 | `do M<ts>:` heads a **term**; its statements: `x : T = v`, `t <- v`, `x : A <- v`, `t` as a step, `return t`, a final value; `do_end` rejects a stray `=` ("write x : T = …") | `parse_term_do_stmt` 2329 | `DoBlock`, `DoStmt` | `do_*` |
| G27 | law clauses: `for [-\|+\|~]x: T [where W]` and `exs x: T [where W]` (no quantity on `exs`); `~` only on leading clauses; the claim after the clauses is a body | `parse_book` `"law"` | `Clause`, `Law` | `law_*` (step 3 reads names; step 4 terms) |
| G28 | `?Name` hole (`?TODO` counts as incomplete: semantic); `Type`, `Data`, `Quant`, `Kind(t)` are term heads; `&0 &1 &2` are quantities; `match`/`case`/`return`/any keyword at term start is an error | `parse_term_base` | `Hol`, `Typ`, `Qnt`, `Qua` | `hole_kinds` |
| G29 | `#` starts a comment anywhere whitespace may be (`parse_skip`), including inside a term; there is no block comment; `#` inside a string or char is not one | `parse_skip` 1595 | nothing | `comments` |
| G30 | a dotted name: every `.` must be followed by a letter or `_` (`a.1` and `a.` are errors); one name, the dot means nothing | `parse_lexeme` 1636 | prose | `name_dots` |
