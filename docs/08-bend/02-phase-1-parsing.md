# 08.02 Phase 1: reading

```
phase 1
├── 1. BD0: three spikes
├── 2. BD1: the parser crate
│   ├── 2.1 the crate and why it is separate
│   ├── 2.2 the tree
│   ├── 2.3 the reader, function by function
│   └── 2.4 what it does not do
├── 3. error recovery: where `bend` stops and we go on
├── 4. tests and the proof
└── 5. steps
```

## 1. BD0: four spikes

Throwaway code in `languages/bend/spikes/`, its own tiny workspace, deleted once the answers are in `languages/bend/ENGINE.md`.

| # | Question | How | Answer goes to |
|---|---|---|---|
| S-BD1 | **Which files of the corpus does `bend` accept?** The negatives (a test whose trailing `#|` lines are an error message) must be excluded from "must parse"; `tests/parse/` is positives | a script over `../language_clone/bend`: run the installed `bend` (or `bun bend2/main.ts`) with a check-only flag on every `.bend`; record accept / refuse per file, with `--check-only` (`bend2/main.ts` 43: "check the file and its imports; run nothing"). Neither `bend` nor `bun` is installed on the owner's machine (checked 2026-11-20): the owner installs `bend` first (`curl -fsSL https://bend-lang.com/install.sh \| sh`), and the pinned clone is the source of truth for the grammar even if the installed `bend` is newer. Without it, the fallback is the `#|` convention: a file whose expected lines are an error is a negative, every other file is a positive | `ENGINE.md` §corpus: the accepted list's count, and the script kept as `scripts/bend-corpus.sh` |
| S-BD2 | **Is the grammar comment complete?** Walk `parse_term`, `parse_body`, `parse_tele`, the do-block and the law parser and list every form the code reads that the BNF at lines 8–160 does not name | reading, not running | `ENGINE.md` §grammar: the list; each item becomes a fixture in BD1 |
| S-BD4 | **The Ref oracle.** `bend.ts` keeps a `Span` on every `Ref` node it builds, and `parse_book` is an exported function. A Bun script (`scripts/bend-refs.ts`, run from the clone, never committed into a crate) imports `bend2/bend.ts`, loads a book, and prints one line per def: `(def, callee name, span)`. Needs `bun` (not installed here; the owner installs it). Its output is the oracle BD1 lacks for declarations and BD4 lacks for calls: our sites, by span, must be a superset of its `Ref`s to project defs, and any `Ref` we lack is a bug or a policy row. Re-run against upstream head by hand at every bump | `ENGINE.md` §oracle: how to run it, and the first diff's numbers |
| S-BD3 | **Base's names.** Parse `bend2/base.bend` with the BD1 parser once it exists, or with a regex now (`^(def\|type\|law\|@unsafe def) Name`, and constructor lines under a `type`), and emit the name list | a 30-line script | the generated `base_names.rs` and the script that writes it, `scripts/bend-base-names.sh` (Q-BD3) |

No spike measures memory or time: there is nothing to measure yet. The parser's speed is measured in BD1 §4.

## 2. BD1: the parser crate

### 2.1 The crate and why it is separate

```
languages/bend/
├── Cargo.toml                 members = ["crates/*"]; no engine pin: ENGINE.md carries the grammar pin instead
├── ENGINE.md                  the grammar pin (a bend commit) · the corpus numbers · the grammar list · how to bump
├── crates/
│   ├── bend-parser/           THE ENGINE WE WRITE. parse(text) -> Tree. Depends on nothing but analyzer-core (for Span).
│   ├── bend-syntax/           sem.rs is the only module that imports bend_parser. outline · marker · header
│   ├── bend-analysis/         sem/ is the only module that imports bend_parser. book · walk · calls
│   └── bend-cli/
└── docs/                      qualnames.md after BD2, markers.md after BD2, policy.md after BD4
```

Why a fourth crate instead of a `parser.rs` inside `bend-syntax`: so that rule 4 ("only `sem` imports the engine, and no engine type leaves
it") keeps its literal form, and the rest of the two crates is written against `sem`'s owned data exactly as in TypeScript and Rust. If a
Rust implementation of Bend appears one day, or Bend ships a tree-sitter grammar, it replaces `bend-parser` behind two `sem` modules. It also
keeps `bend-syntax` fast to compile for the same reason the other `*-syntax` crates are: the parser has no dependencies. Row BD-D1.

`bend-parser`'s public API is small and is the one place a tree type is public:

```rust
pub fn parse(text: &str) -> Tree;                 // never fails, never panics: errors are in the tree
pub struct Tree { pub imports: Vec<Import>, pub decls: Vec<Decl>, pub errors: Vec<ParseError> }
```

### 2.2 The tree

An owned tree at the **surface** level: what the user wrote, with sugar kept, every node with a `Span` (core's `text::Span`, byte offsets).
Bend's own parser desugars while parsing (`parse_call` builds `App(App(Ref f, a), b)`; a list literal becomes `Con{…}`); we do not, because
the outline and the markers need the source shape, and the call walk needs to know that `f(a, b)` was written as one call with two arguments.

```rust
pub enum Decl { Type(TypeDecl), Def(DefDecl), Law(LawDecl) }

pub struct DefDecl { pub name: Name, pub unsafe_: bool, pub params: Vec<Bind>, pub ret: Option<Term>, pub body: DefBody, pub span: Span, pub doc: Option<Span> }
pub enum DefBody { Code(Body), Foreign(Vec<StrLit>) }             // `import "./e.c"` lines
pub struct TypeDecl { pub name: Name, pub params: Vec<Bind>, pub kind: Term, pub ctrs: Vec<CtrDecl>, pub span: Span, pub doc: Option<Span> }
pub struct LawDecl  { pub name: Name, pub clauses: Vec<Clause>, pub claim: Body, pub span: Span, pub doc: Option<Span> }
pub struct Bind { pub quant: Quant, pub template: bool, pub name: Name, pub ty: Option<Term> }   // `+x: A`, `~f: A -> B`, bare `a`
pub struct Name { pub text: String, pub span: Span }               // dots included: "Nat.add" is one Name

pub enum Body { Match(Match), Let(Let), Do(DoBlock), Term(Term) }
pub enum Term {
    Name(Name),                                                   // Var or Ref: which one is resolution's job, not the parser's
    Call { callee: Box<Term>, gpu: bool, args: Vec<Arg> },        // f(a, b) · f!(a) · t(~g, a); Arg = { template: bool, term: Term }
    Ctr  { name: Name, args: Vec<Term> },                         // K{a, b}
    TypeApp { name: Name, args: Vec<Term>, reusable: bool },      // D<A, B> · +D<A>
    Lam { binds: Vec<Bind>, body: Box<Body> },                    // x => body
    Op { lhs: Box<Term>, op: Op, rhs: Box<Term> },                // a + b, inside (.. : T); also && || ++ <> & |
    Annot { term: Box<Term>, ty: Box<Term> },                     // (e : T)  {e : T}
    Index { array: Box<Term>, index: Box<Term>, write: Option<Box<Term>> },
    Rewrite { eq: Box<Term>, named: Option<Name>, motive: Box<Term>, rest: Box<Body> },   // %e : P; body
    Lit(Lit), List(Vec<Term>), Tuple(Vec<Term>), ArrayNew { .. }, Arrow { .. }, Pair { .. }, Eq { .. }, Refl, Hole(Name),
    Kind(..), Quant(..), LamMatch(..), Absurd, Group(Box<Body>),
    Error(Span),                                                  // recovery: an unreadable term, see §3
}
```

This is a sketch, not the final list: S-BD2 completes it from the code. Two rules for the final version: every variant has a `Span`, and no
variant stores a desugared form (the sugar table lives in `bend-analysis`'s `calls`, where it is a policy, not a parse).

> **🦀 Rust note: `Box` in a recursive enum.** `Term` contains `Term`, so the compiler cannot know its size unless the inner one is behind a
> pointer: `Box<Term>` is that pointer, one heap allocation per node. `Vec<Term>` is already a pointer, so lists of children need no `Box`.
> This is the plain shape for an owned AST; rust-analyzer's `rowan` (a green/red tree shared by offset) is the other shape, and it is overkill
> here: we never edit the tree, and files are small.

### 2.3 The reader, function by function

The Rust mirrors `bend.ts`, one function per `parse_*` function, same names minus the prefix, so that a diff of `bend.ts` between two pins
says which Rust functions to look at. The ones that carry the rules of §2 in [`01-what-bend-is.md`](01-what-bend-is.md):

| `bend.ts` | Ours | Carries |
|---|---|---|
| `parse_skip` (1595) | `skip` | whitespace and `#` comments; **we also record each comment's span** (markers and docs need them; `bend` throws them away) |
| `parse_lexeme`, `parse_name` (1636, 1652) | `lexeme`, `name` | a name's characters (`char_is_head`, `char_is_name`: ASCII letters, digits, `_`, `.`; not `-`); the reserved words |
| `parse_term` (1810, about 560 lines) | `term(lvl)` | the term grammar with precedence levels, the `<` rule, glued `-`/`+`, juxtaposition, the operator-needs-spaces rule |
| `parse_body` (2376) | `body(col)` | statements at a block's column; `match`, lets, parallel lets, `a[i] <- v` re-binding, the do-block |
| `parse_tele` (2487) | `tele(close)` | a parameter list: quantities, `~`, bare quant names, the `:` type |
| `parse_book` (2583) | `book` | the top level: `@unsafe`, `def`, `type`, `law`; this is where recovery lives (§3) |
| `book_load`'s import loop (973–1000) | `imports` | the `import` lines, read by a **regex per trimmed line** (not by `parse_*`), their alias; **no file is read here**: the parser hands back paths, `bend-analysis` opens them |

Reading `bend.ts` once more before writing each function is the method; the plan does not copy the parser into the doc.

### 2.4 What it does not do

No desugaring, no `term_higher` / `term_lower`, no `match_flatten`, no name resolution (a `Name` is a `Name`; whether it is a variable, a def
or Base's is decided in `bend-analysis` over the whole book), no spans into other files, no reading of files or of the network.

## 3. Error recovery: where `bend` stops and we go on

`bend` throws at the first error. An outline must come back for any text (the testkit cuts every fixture at every 50th byte). Two levels of
recovery, no more:

1. **Declaration level.** When a declaration fails, record a `ParseError` with the span from the failure to the resync point, mark the
   declaration `syntax_error: true` (its `span` runs to the resync point, its name is kept if it was read), and resync at the next line that
   starts at column 0 with `def`, `type`, `law`, `@unsafe` or `import`, or at the end of the text. Declarations after the broken one are read
   normally. This is what makes the outline of a half-typed file list every other def.
2. **Term level, one case.** An unreadable term inside a body becomes `Term::Error(span)` running to the end of the line, and the body goes
   on with the next statement. **[check]** in BD1 whether this second level pays for itself: it keeps the call sites after a typo in the same
   def; if it makes the parser hard to keep in step with `bend.ts`, drop it and let level 1 carry the def.

No recovery inside a term, a pattern or a parameter list beyond that. The outline is the product here; a broken def with a kept name and
range is the right answer for it, and `syntax_error: true` is how the driver already shows it.

## 4. Tests and the proof

| Test | Where | Checks |
|---|---|---|
| **fixtures** | `bend-parser/tests/fixtures/*.bend`, one file per grammar form (every row of the BNF, every sugar, every item S-BD2 adds, every "juxtaposition law" sentence) | the tree's debug print equals its reviewed snapshot |
| **the corpus** | `scripts/bend-corpus.sh`: every file S-BD1 says `bend` accepts | parses with **zero** `ParseError`s. This is the proof of BD1. The negatives must produce at least one error (we are not asked to refuse the same things, only not to accept silently what is garbage) |
| **recovery** | fixtures with a deliberate error, one per recovery rule in §3 | the snapshot shows the error, the resync and the defs after it |
| **never panics** | the corpus cut at every 50th byte (the testkit's `cut_every_50_bytes` wants an outline; BD1 runs the same loop on `parse` directly) | returns, on a character boundary |
| **time** | the corpus, release build **[measure]** | written into `ENGINE.md`; the expectation is well under a second for all 1,982 files |

What the proof is **not**: agreement with `bend`'s own tree. `bend` has no tree printer at the surface level (`term_show` prints desugared
core terms). What it does have is a `Span` on every `Ref` node, and S-BD4's script turns that into an oracle for **names and where they are**:
every declaration name and every reference `bend` sees, with its span. BD1's corpus test compares declarations against it; BD4 compares call
sites. The tree's shape itself is checked by reviewed snapshots against `GUIDE.md`'s syntax reference.

## 5. Steps

**State (2026-11-20):** BD0 done (c8742de), steps 1–3 done (aeef3fc, f1ed5f6) and accepted; the corpus proof reproduced by the reviewer (1,744
files, 176,229 declarations, 57 ms release). Step 3 reads types and bodies as spans through a throwaway `stopgap.rs` (313 lines) that step 4
deletes; the 30 grammar items the BNF does not name are `ENGINE.md` §grammar G1–G30, each a step 4 fixture. S-BD4 (the Ref oracle) was added to
the plan after the step 3 prompt went out and is not done: it moves to BD4, where its output (callee spans) is compared.

| # | Do | Proof |
|---|---|---|
| 1 | `scripts/new-language.sh bend`, then add `crates/bend-parser` to the workspace; `ENGINE.md` gets the grammar pin `0e2f38f` and an empty corpus section | the workspace builds, `cargo tree -p bend-parser` shows only `analyzer-core` |
| 2 | S-BD1 and `scripts/bend-corpus.sh` | the accepted count in `ENGINE.md` |
| 3 | `skip`, `lexeme`, `name`, `imports`, `book` with recovery, declarations with **bodies skipped** (a body is read as "lines indented past the declaration's column") | the outline-shaped half: every declaration of the corpus is found with its name and span; the recovery fixtures |
| 4 | `tele`, `term`, `body`, the do-block, laws' clauses; S-BD2 alongside | corpus at zero errors; one fixture per form |
| 5 | the time measurement, `ENGINE.md` complete | the table of §4 filled |

Step 3 before step 4 is deliberate: BD2 (the outline and the markers) needs only step 3, so the first tag can come while the term grammar is
still being finished. A declaration whose body we skip is a declaration whose calls we do not know yet, which is fine for an outline and wrong
for a call graph; BD4 depends on step 4.

Next: [`03-phase-2-defs-and-calls.md`](03-phase-2-defs-and-calls.md)
