# 08. Bend

```
bend
├── what Bend is ─────────────────── 01-what-bend-is.md            the language as read from its source · no engine · the numbers
├── PHASE 1: READING ─────────────── 02-phase-1-parsing.md         BD0 spikes · BD1 the parser crate and its tree
├── PHASE 2: DEFS AND CALLS ──────── 03-phase-2-defs-and-calls.md  BD2 outline + markers · BD3 the book · BD4 call graph · BD5 driver needs
├── PHASE 3: THE CONTEXT LAYER ───── 04-phase-3-context-layer.md   BC0 measure · BC1 templates · BC2 function-typed parameters · BC3–BC5
│   ├── the design ───────────────── 06-context-layer-design.md    BC1+BC2 as one step: Known · the key · flow-down · the rows · the proof · the steps
│   └── the value layer ──────────── 07-value-layer-design.md      on by default · lambdas under their caller · variables · returns · factories · constructor fields
└── decisions and questions ──────── 05-decisions-and-questions.md draft rows BD-D1… · what needs your yes · what I did not check
```

Written 2026-11-20, after reading `../language_clone/bend` at commit `0e2f38f` (Bend 2.0.40 and after; the clone was made for this plan,
beside `tsrs` and `rust-analyzer`): `bend2/bend.ts` (the grammar comment at its head, the parser at lines 1540–2700, `book_load` and the
name resolution at 918–1045 and 1720–1745), `guide/GUIDE.md`, `AGENTS.md`, `CHANGELOG.md`, `bend2/base.bend` and the `demos/`. **Nothing here
was built or run.** Every `[check]` and `[measure]` is a claim to verify; the list is in [`05-decisions-and-questions.md`](05-decisions-and-questions.md) §4.

## The verdict in one paragraph

**Yes, and it is the smallest language in the repo.** Bend2 has no Rust implementation and no parsing library we could sit on: the language is
one TypeScript file, `bend2/bend.ts`, of which the parser is about 1,150 lines and the grammar comment 150. So Bend is the one track where we
write the reader ourselves. That is affordable because (a) the grammar is small and written down in one place, (b) **every call target is
decided by name alone**: Bend does almost no inference, a top-level def is reached by its spelled name, operators are sugar for `T.add` where
`T` is written beside them, and the only thing a name cannot settle is a call through a variable, which is exactly what the context layer is
for in every language (D16); and (c) the owner's constraints that made the other tracks heavy (an engine's memory, its salsa version, its
project model) do not exist. The cost is in one place: the parser must stay in step with a language that releases often (2.0.40 on
2026-11-05, commits daily), so the "engine pin" here is a **grammar pin**, a Bend commit, and the proof of every phase includes parsing the
whole Bend test corpus at that commit (1,982 `.bend` files).

Three things in this plan go against what the repo's rules say today, and they are said here, not hidden (CLAUDE.md: "if code and plan
disagree, say so"):

1. **"We never write parsers."** Bend has no engine to take one from. The parser is ours, in a crate of its own that plays the engine's part
   (`bend-parser`), so rule 4 still holds literally: only `sem` imports it. Row BD-D1.
2. **D6: the marker lives in a docstring.** Bend has no docstring and no doc comment; it has `#` line comments. The marker is a `# ID: …` line
   above the def: the first use of the `Placement` the enum kept for this day. Needs your yes (Q-BD1), and a small core change.
3. **Resolving a name needs Base's names**, and Base is a 3,525-line `.bend` file shipped inside the `bend` binary, not a crate. We carry a
   generated list of its names, pinned with the grammar (Q-BD3).

## The phases at a glance

| Phase | Builds | Crate | Depends on | Proof it is done | Size |
|---|---|---|---|---|---|
| **BD0** | three throwaway experiments | `languages/bend/spikes/` | B0 | answers in `languages/bend/ENGINE.md` | S |
| **BD1** | `parse(text) -> Tree` with recovery; the tree | `bend-parser` | BD0 | every file of the corpus that `bend` accepts parses; a snapshot per fixture; never panics | L |
| **BD2** | `outline(text, style)`, `marker`, `plan_marker`, the header | `bend-syntax` | BD1 | fixture snapshots; round trip; never panics | M |
| | **tag `bend-v0.1.0`** | | | qualname rule frozen (rule 10) | |
| **BD3** | `Book::open(root, file)`: imports followed, names resolved | `bend-analysis` | BD1 | the demos and this repo's scenarios load; a hub import is a named external, nothing is fetched | M |
| **BD4** | `call_graph`, the wire, `bend-cli` | `bend-analysis`, `bend-cli` | BD3 | every scenario at `Level::Default`; a fixture per policy row | M |
| | **tag `bend-v0.2.0`** | | | | |
| **BD5** | `Session`: refresh, budget, cancel | `bend-analysis` | BD4; TS5 steps 1–2 in core | tier-3 conformance | S |
| | **tag `bend-v0.3.0`** | | | | |
| **BC0** | one measurement | `languages/bend/spikes/` | BD4 | counts in `ENGINE.md`; go / no-go for BC1 | S |
| **BC1** | templates: `~f` resolves per call | `bend-analysis` | BC0 | `callback-argument` (template form) matches `expected.precise.tree` | M |
| **BC2** | function-typed parameters | `bend-analysis` | BC1 | `two-callers`, `callback-argument` (closure form) match | M |
| **BC3–BC5** | values through lets and returns, constructor fields, the fixpoint | `bend-analysis` | BC2 | the fixtures of each | L in all |

S, M, L are relative. **[measure]** after BD1: the parser is the one piece of work this repo has not done before, so its size is the least
known number in the table.

**State (2026-11-20): BD0 and BD1 complete** (aeef3fc, c8742de, f1ed5f6, e67f328, 379b909), every proof reproduced at review; `bend-parser` is
2,030 code lines, 85 fixtures; `languages/bend/ENGINE.md` has the corpus numbers, the term-node counts and the grammar list G1–G36. **BD2 complete** (08bdce7, 01f622e, 692dfa0): `qualnames.md` and `markers.md` are the contract.
**Tag `bend-v0.1.0` is the owner's, after Q-BD2 (law and proof: one qualname or a suffix).** **BD3 complete** (03de29f, 66695e6): the book, Base's names as a generated list (Q-BD3 answered as recommended), the Ref oracle at zero mismatches. **BD4 and BD5 complete** (0adfc30 … 80f3390, 2026-11-21): **layer 1 of Bend is built**, every proof reproduced at review. The three tags
(`bend-v0.1.0`, `bend-v0.2.0`, `bend-v0.3.0`) are the owner's, Q-BD2 before the first. BC0 done (6f4cff0): 55 unresolved sites in 4,323 on the demos. **D51, D52: the context layer is built, BC1+BC2 as one step** ([`06-context-layer-design.md`](06-context-layer-design.md); 89a07ba … 2c03f3b, 2026-11-21, every step's proof reproduced at review), **off by default**. On the demos it draws 16 of the 55 unresolved sites and knows 12 more to be a lambda (`languages/bend/ENGINE.md` §BC1+BC2). **D53, D54, D55 (owner, 2026-11-21): the layer goes on by default, a lambda's calls are drawn under the function that calls it, and the layer follows values as Python's does** (variables, returns, factories, partial application, constructor fields): [`07-value-layer-design.md`](07-value-layer-design.md), four phases V0 to V9. **V0 to V2 built and reviewed (0e1667d, 4375dfc, acdeba3): the layer is on by default, and under it a body is evaluated from its own tree, with the same answers as before, item by item.** V3 built and reviewed (2f5766d): a variable has the value of its binding. V4 (a lambda moves under its caller) is held for the owner: 07 §6.1; V6 to V8 (returns, partial application, `do` as a call, constructor fields) go first, their rows are 07 §12.2 and §12.3. Every other Q-BD is answered (05 §3).

The user asked for three phases. They map so: **phase 1 (reading)** is BD0–BD1, the parser and its tree; **phase 2 (what can be called, and
the call tree)** is BD2–BD5, the outline, the book and the call graph on core's flat solver; **phase 3 (the context layer)** is BC0–BC5.

## What V-NOC gets, and when

```
bend-v0.1.0   .bend files appear in the tree and on the canvas · docs attach · docs survive renames
bend-v0.2.0   call trees, computed whole; a Bend tree is nearly exact already (01-what-bend-is.md §4)
bend-v0.3.0   call trees that open one level at a time, can be cancelled, are fast the second time
BC1, BC2      (minor versions) a template or a higher-order def shows what it calls for THIS caller
```

## The rule for every phase

The TypeScript track's rule, unchanged ([`../03-typescript/README.md`](../03-typescript/README.md)), with one word changed: `ENGINE.md` lists
the **grammar pin** (the Bend commit the parser was written against), the corpus numbers at that pin, and how to bump: re-run the corpus
script, read `bend.ts`'s diff since the pin, fix the parser, move the pin. A bump is a change of its own (only the pin, `bend-parser`,
`ENGINE.md`), as an engine bump is elsewhere.

Next: [`01-what-bend-is.md`](01-what-bend-is.md)
