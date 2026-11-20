# 08.05 Decisions and questions

```
decisions and questions
├── 1. draft rows (BD-D1…): decided here unless you say otherwise
├── 2. new scenarios
├── 3. questions that need your yes (Q-BD1…)
└── 4. things I did not check
```

Numbering: `BD-D` rows are Bend's; they move into [`../07-decisions-and-questions.md`](../07-decisions-and-questions.md) as `D49…` when you
accept them, as Rust's did. Nothing is decided until then.

## 1. Draft rows

| # | Row | Why |
|---|---|---|
| BD-D1 | **Bend's engine is a parser we write, `bend-parser`, a crate of its own; `sem` is still the only module that imports it.** The exception to "we never write parsers" is Bend alone, because Bend has no implementation in Rust and no parsing library; its grammar is one page | [`02-phase-1-parsing.md`](02-phase-1-parsing.md) §2.1 |
| BD-D2 | **The engine pin is a grammar pin**: a commit of `bendlang/bend` in `ENGINE.md`, with the corpus numbers at that pin. A bump re-runs the corpus and reads `bend.ts`'s diff | README |
| BD-D3 | **A Bend `type` is `DefKind::Enum`**; its constructors are not defs | 03 §1.1 D7, D8 |
| BD-D4 | **A law and its proof are two defs with one qualname** (`Other("law")` and `Function`); `occurrence` and the marker tell them apart (D15) | 03 §1.1 D5, D6; or Q-BD2 |
| BD-D5 | **A constructor application of a project type is a `Construct` of the type; of a Base type it is nothing** (data: lists, numbers and strings are constructors underneath) | 03 §3.2 |
| BD-D6 | **The qualname is the declared name as written, dots included, no path, no joiner**; `parent` is always `None` | 03 §1.1 |
| BD-D7 | **Nothing is fetched and `~/.bend` is never read.** A hub import is an external module; its calls are named `External`s; counted `hub_import` | 03 §2 |
| BD-D8 | **Sugar on a Base type is not a site** (`(a + b : U32)`, `a[i]`, `do IO<…>:`, `&&`, `++`, `<>`); **sugar on a project type is a `Call`** to the def it names (`(a + b : Vec2)` → `Vec2.add`; `do Parser<…>:` → `Parser.bind`, `Parser.pure`) | 03 §3.3 rows op-*, index, do-* |
| BD-D9 | **A call in a type position is not a site in layer 1**, counted `type_position`: parameter and return types, annotations' right sides, type arguments, law clauses, rewrite motives | 01 §4, 03 §3.2 |
| BD-D10 | **Recovery at declaration boundaries only** (a column-0 `def` / `type` / `law` / `@unsafe` / `import` line resyncs); a term-level `Error` node is tried and dropped if it costs drift | 02 §3 |
| BD-D11 | **The `Lang` impl's `BASIS` is `Basis::Name`** (core's "the code's own names fixed it"): every Bend edge is that kind, and the one a reader may take as certain | 03 §3.1 |
| BD-D12 | **Bend's call targets are resolved over the whole book, ignoring declaration order**, so a forward reference `bend` would refuse still gets its edge (never fail) | 01 §3 |

## 2. New scenarios

`scenarios/<name>/bend/main.bend` for the six existing folders, and one new folder:

| Scenario | Pins | Default tree | Precise tree |
|---|---|---|---|
| `law-across-files` | `LAWS.bend` states `law sorted`, `PROOF.bend` proves it as `def Laws.sorted`, `main.bend`'s `main` calls `Laws.sorted(xs)` through the alias | `main ▸ Laws.sorted` (the proof def) | same |

The six existing scenarios in Bend, with notes: `method-on-class` has no method; its Bend form is a def named `Point.len` called as
`Point.len(p)`, and the scenario's README must say the rule it pins (a dotted name is one name) rather than pretend there is a receiver.
`construct-marked-class` is `Shape` with a marker above `type Shape` and `Circle{5}` in `main`. **[check]** whether the renderer's rules
produce the same tree text for a `Construct` with `init: None` as the other languages' `expected.default.tree` shows; if the text differs, the
Bend tree is its own file and the scenario README says why.

## 3. Questions that need your yes

| # | Question | My recommendation |
|---|---|---|
| Q-BD1 | **The marker above the def.** Bend has no docstring. Build `Placement::Above` in core (a new `#[non_exhaustive]` variant and `plan`) and use it for Bend's defs; the header stays the plain comment it is everywhere. D6 said "build only `Docstring`"; this is the day it was kept for | yes. The core change is small and uses `comment_marker`, which exists for the header |
| Q-BD2 | **One qualname for a law and its proof** (BD-D4), or a suffix on the law (`add_zero<law>`, angle brackets cannot appear in a Bend name)? | one qualname, as BD-D4: a doc attached to "the law `add_zero`" and one attached to "the proof `add_zero`" are told apart by the marker, and the suffix would be the only qualname in the repo that is not a spelling the user wrote. If V-NOC stores qualname **without** kind and needs uniqueness per file, choose the suffix and tell me |
| Q-BD3 | **Base's names: a generated list committed in `bend-analysis` (pinned with the grammar), or read `base.bend` from the user's `bend` install at run time?** | the list. The install bundles `base.bend` inside the binary (`BEND_DIR` at `bend.ts` 910; **[check]** whether a copy sits on disk at all), and a run-time read breaks "text in, data out" and makes the answer depend on the machine. A stale list costs an `External` where a new Base def is called; never a wrong project edge |
| Q-BD4 | **Show Base's arithmetic and `do IO` as `External` edges?** (BD-D8 says no) | no, for the first tag. A tree where every `(a + b : U32)` is an edge is unreadable, and nothing is lost: a Base call written out (`U32.add(a, b)`) is still an `External`. Reopen after the demos' trees |
| Q-BD5 | **Calls in type positions** (BD-D9 says not sites, counted): is that what a Bend user writing proofs wants, or should a type-level def's call show? | not sites now; the counter tells us how many there are on the demos and the proofs, and the decision can flip to "a `Call` with a `type_level` mark" without changing a qualname or the wire |
| Q-BD6 | **The language id and the tag**: `bend`, `bend-v0.1.0`, files `.bend` only | yes |
| Q-BD7 | **Does this go into `CLAUDE.md`?** The line "We never write parsers or type checkers" becomes "… except Bend's parser (BD-D1)" | your edit, not mine: CLAUDE.md is yours |

## 4. Things I did not check

Checked while writing (2026-11-20): `bend <file> --check-only` exists (`bend2/main.ts` 43: "check the file and its imports; run nothing"), and core's `Basis::Name` is the variant Bend's edges rest on.

Checked by BD0 and BD1 step 3 (2026-11-20, `languages/bend/ENGINE.md`): every accepted file starts with comments, then imports, then declarations (127 start with code on their first non-blank line: the header slot is offset 0); parsing the accepted corpus takes 57 ms in release; a name has no `-`; `bend.ts` spans are UTF-16 units; the parser consults the book's tables in four places, not one. Open from BD1: `scripts/check-all.sh` runs clippy on `*-syntax`, `*-analysis` and `*-cli` only, so `bend-parser` needs a line (step 4's task).

| # | Claim | Where it is used | How to check |
|---|---|---|---|
| 13 | `HigherOrderCO/tree-sitter-bend` predates the Bend2 syntax (last push 2024) | 01 §7 | look at the repository |
| 14 | the demos' call-site counts (5,900 sites, 64 with a local head) | 04 §2 | BC0 |
| 2 | the grammar comment at `bend.ts` 8–160 names every form the parser reads | the fixture list of BD1 | S-BD2 |
| 5 | the demos never put a blank line between a doc comment block and its def | the range rule, 03 §1.2 | grep the demos |
| 6 | `base.bend` sits on disk in an install, or only inside the binary | Q-BD3 | install `bend`; look |
| 7 | the renderer prints a `Construct` with `init: None` as the other languages' scenario trees show | §2 | run the scenario in BD4 |
| 9 | a book of the whole `tests/` is a few megabytes and opens in milliseconds | BD3, BD5 | BD3 step 3 |
| 10 | TC's answer on lambdas as values (Q-TC) is one Bend can share | BC1 | read 12-context-layer when Q-TC1..5 are answered |
| 11 | the term-level `Error` node is worth its drift cost | BD-D10 | BD1 step 4 |
| 12 | how the V-NOC contract treats a def imported from outside the root (`../lib.bend`) | BD3 | D26's text and the Python driver |
