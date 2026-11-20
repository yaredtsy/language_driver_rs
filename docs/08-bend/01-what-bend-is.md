# 08.01 What Bend is

```
what Bend is
├── 1. the language, as it matters to us
├── 2. the grammar: one page, one file
├── 3. names: how a call finds its def without a type checker
├── 4. what is a def, what is a call, what is neither
├── 5. no engine: what we write, what we do not
├── 6. the numbers
└── 7. risks
```

Everything here was read from `../language_clone/bend` at `0e2f38f`. Line numbers are into `bend2/bend.ts` at that commit.

## 1. The language, as it matters to us

Bend is a dependently typed, affine, pure functional language with Python-shaped syntax (indentation blocks, `def`, `match`/`case`) and
Haskell/Lean semantics. A file is a list of `import` lines, then top-level declarations of three kinds: `type` (a datatype with constructors),
`def` (a function, with its body), `law` (a claim, proven by a `def` of the same name). There are no classes, methods, traits, interfaces,
modules-as-blocks, statements at the top level, mutation or loops. Repetition is recursion, and **mutual recursion is forbidden**: a def may
call only defs declared above it, or itself (an `@unsafe def` may call below). Functions are values (closures), but a closure may be called at
most once and only top-level defs may be called freely.

What this means for a call-tree analyzer:

- **The call graph of a Bend book is a DAG plus self-loops** (plus whatever `@unsafe` adds). No cycles to cut except direct recursion, which
  the renderer already marks.
- **There is no dispatch.** No method is chosen by a receiver's type at runtime. The default resolver is not "the engine's answer, imprecise
  where it dispatches"; it is exact wherever a name is written, and unknown exactly where a *variable* is called (`f(x)` with `f` a parameter
  or a `let`). Those sites are the context layer's whole job (phase 3), and in Bend they come in two flavours, one of which (templates, `~f`)
  the compiler itself resolves per call site, so the layer can be exact there too.
- **Types are terms**, and a type-level def (`def IsEven(n: Nat) -> Type`) is called in types. Calls inside a type position are code to the
  checker and nothing to the runtime. The policy must say which they are to us (row BD-D9: not sites in layer 1, counted).

## 2. The grammar: one page, one file

`bend.ts` lines 8–160 are the grammar in BNF, kept by hand beside the parser. The parts that matter to us, in the file's own words:

```
TLD    ::= ADT | Law | Def
ADT    ::= "type" Name ("<" [Bind ","?] ">")? "is" Term ":" [Ctr]        Ctr ::= Name "{" [Bind ","?] "}"
Law    ::= "law" Name ":" [Clause] Body       Clause ::= ("for" (Quant | "~") | "exs") Name ":" Term ("where" Term)?
Def    ::= ("@unsafe")? "def" Name "?"? "(" [Bind ","?] ")" ("->" Term)? ":" (Body | ["import" STRING]+)
Import ::= "import" "Base" | "import" Path "as" Name
Book   ::= [Import] [TLD]

Body   ::= Match | Local | Reply
Match  ::= "match" [Term] ":" [Case]          Case ::= "case" [Term] ":" Body
Local  ::= (Quant Name | Term)+ "=" Term+ ";"? Body
Term   ::= Var | Ref | Ann | Typ | Qnt | Qua | Min | All | Lam | App | ADT | Ctr | Mat | Efq | Eql | Rfl | Rwt | Hol | Grp
DoBlock ::= "do" Name "<" [Term ","?] ">" ":" [DoStmt]
```

plus a table of sugars (lines 56–80): arrows, pairs, lists (`[a, b]`, `h <> t`), tuples, arrays (`[v : T*n]`, `a[i]`, `a[i] <- v`), numeric,
char and string literals, `D<A>` / `+D<A>` type fills, and **operators**: inside `(.. : T)`, `+ - * / % .&. .|. .^. << >> < <= > >=` are calls
to `T.add` … `T.is_ge`; `&& || ++ <> & |` are fixed. Then two pages of prose rules (lines 82–160) that a parser must honour: juxtaposition
("a call or index suffix may be spaced but a newline ends the spine"), how `<` after a term decides between type arguments and less-than,
that `-` or `+` glued to a name heads a binder or a literal and never an operator, that statements live in bodies and parens hold one body,
how `~` marks a template and its arguments, and the do-notation.

The parser (lines 1540–2700) is **scanner-less recursive descent**: no token list; it reads characters, skips `#` comments and whitespace
(`parse_skip`), tracks the column of a block's first statement (`parse_body(p, col)`), and fails at the first error (`parse_fail` throws).
Every word it dispatches on is reserved (`def type law match case do return for exs where is import Type Data Kind Quant`).

Every term carries a `Span { file, beg, end }` (byte offsets). Good: the shape of a `Site` is already there.

## 3. Names: how a call finds its def without a type checker

This is the part that makes the default layer exact, so it is worth being precise (lines 918–1045, `book_load`; 1720–1745, `parse_qual`,
`parse_reso`, `parse_own`).

- A **book** is one entry file plus everything it imports, transitively, plus Base. Imports come first in a file; a cycle is an error.
- A file's **namespace** is its real path, relative to the **entry file's directory**, without `.bend` (`demos/x/SPEC.bend` imported from
  `demos/x/main.bend` is `SPEC`). Base's namespace is `""`. A hub package (`import 0x<hash>/main.bend as P`, or `<name>@<version>/…`) is
  fetched from the network into `~/.bend/lib` and gets a namespace of its own. **We never fetch** (row BD-D7): a hub import is an external
  module, every call through its alias a named `External`.
- A declared name is **qualified** as `ns:name` (`parse_qual`), Base's names stay bare. A name is a lexeme that may contain dots: `Nat.add`
  is one name, and the dot means nothing (`GUIDE.md`: "dots inside a name are just characters: `U32.show` needs no module").
- `import ./m.bend as M` binds a **file-local alias**: a name `M.x` resolves to `ns(M):x` (`parse_reso`). Two aliases of one file agree.
- An unqualified name resolves to **this file's own** `ns:name` if the book has it, else to the bare (Base) spelling if the book has that
  (`q in tab || !(k in tab) ? q : k`). A file **cannot** declare a name Base already has: `parse_fresh` refuses `nm in tab` as a duplicate
  (2525–2533), so there is no shadowing of Base by project code. (Corrected 2026-11-20 after a second reading; the first draft said the
  shadow wins.)
- Before any of that, a **local binder wins**: `parse_var` (1708) looks the name up in the binders in scope first (`p.idx`), then calls
  `parse_reso`. So resolution is four steps: binder, alias, own file, Base.
- **Constructors** are a second table (`book.ctrs`), qualified the same way, and a constructor name is fresh across the whole book
  (`parse_fresh(…, "ctrs")`). `Nil{}`, `Con{h, t}`, `Some{x}`, `True{}` are Base's.
- A def written **below** a reference to it is an error unless the caller is `@unsafe`: so at a call site in a well-formed file, the callee is
  already in the table. We must not rely on that (never fail): we resolve against the whole book after parsing every file, so order does not
  matter to us, and a forward reference in a file `bend` would refuse still gets its edge.
- A `def f(x):` with no `->` **fills the law `f`** in scope (same qualified name, possibly through an alias: `def M.name(..)`): the law and the
  proof are one TLD to Bend. §4 says what they are to us.

A type checker is never needed for any of this. The one place Bend's parser looks beyond syntax is `parse_reso` consulting the tables of names
seen so far, and we have those tables after parsing the book.

## 4. What is a def, what is a call, what is neither

The full policy with its fixtures is in [`03-phase-2-defs-and-calls.md`](03-phase-2-defs-and-calls.md) §2 and §5. The short form:

| Bend | To us | Why |
|---|---|---|
| `def f(…) -> T:` and the body | a def, `Function` | has code |
| `@unsafe def f`, `def f?` | the same def | the flag is about proofs, not about what it calls |
| `def e(…) -> IO(B):` + `import "./e.c"` | a def, `Function`, a leaf | a foreign effect: code we cannot read |
| `def t(~f: …)` | a def, `Function` | a template; the context layer's best case (BC1) |
| `law f:` | a def, `Other("law")` | the claim users most want to document; no code |
| `def f(x):` after `law f` | a def, `Function` | the proof: code. Two defs, the law and its proof (row BD-D4) |
| `type D … is K:` | a def, `Enum` | a sum type with constructors; built like a class (D45) |
| `K{x: A}` under a type | **not a def**; `K{a}` is a `Construct` of `D` | no body (the reasoning of D43 and D45) |
| `import ./m.bend as M` | not a def | an alias |
| `x => body`, `+x => body` | not a def; its calls belong to the enclosing def | phase 1 rule, as in every language |
| `x = v`, `match`, `case`, `do` | not defs | statements |
| `f(a, b)`, `f!(a)`, `t(~g, a)`, `M.f(a)` | a `Call` when `f` names a def; `External` when Base's or a hub's | by name, §3 |
| `f(a)` with `f` a parameter or a `let` | `Unresolved(function_typed_value)` | D16; phase 3 resolves it |
| `~g` or a bare def name as an argument, or inside `K{…}` | a **passed** edge (D22) | handed over, not called |
| `(a + b : T)`, `a[i]`, `do M<…>:` | the sugar's def: `T.add`, `Array.get`, `M.bind` / `M.pure`: a `Call` when the type is the project's, otherwise nothing | row BD-D8: Base's arithmetic is the language's, not a site |
| `Nil{}`, `[1, 2]`, `"s"`, `3n+p`, `(a, b)` | nothing | data; literals are constructors of Base types underneath |
| calls inside a type (`-> Or(Spec.LE(x, h), …)`, `{x : T}`, `List<T>`) | nothing in layer 1, counted `type_position` | types are not code the runtime runs; row BD-D9 |
| `%lemma(p) : P` (a rewrite with a lemma) | a `Call` to `lemma` | proof code calls proof code; it is the user's code |

## 5. No engine: what we write, what we do not

| We write | We do not write |
|---|---|
| a parser for the grammar above, with error recovery at declaration boundaries | a type checker, a kind checker, an evaluator, termination checking |
| a tree with spans, kept at the **surface** level (sugar not expanded) | the desugaring to Bend's core terms (`term_higher`, `match_flatten`) |
| the name tables of §3 (`tlds`, `ctrs`, aliases) over a parsed book | anything that needs a type: there is nothing in layer 1 that does |
| a table from sugar to the def it calls (operators → `T.verb`) | fetching from the hub, reading `~/.bend` |
| a generated list of Base's top-level names and constructors (Q-BD3) | Base itself |

The parser is where the from-scratch cost sits: about 1,150 lines of TypeScript to port, written in a style that maps one to one onto Rust
functions (`parse_term` → `term`, `parse_body` → `body`, `parse_tele` → `tele`). Keeping those names is the cheapest way to track drift: a
diff of `bend.ts` between two pins names the Rust functions to touch.

> **🦀 Rust note: why no parser-generator or combinator crate.** `chumsky`, `winnow`, `pest`, `lalrpop` all want a grammar they can own; Bend's
> is scanner-less and indentation- and spacing-sensitive ("`-` glued to a name is a binder, spaced it is an operator"; "a newline ends the
> spine"; the `<` rule). Encoding those rules in a combinator library is possible and harder to read than the recursive descent Bend itself
> uses, and we want the Rust to mirror `bend.ts` function for function. `logos` (a lexer generator) is the one crate that would fit a
> tokenizer, but Bend has no token level: the same characters are read differently by position. So: hand-written, no dependencies beyond
> `analyzer-core`. This is also what the repo already does with ruff's parser in spirit: one tree, spans, recovery.

## 6. The numbers

| What | At `0e2f38f` |
|---|---|
| `bend2/bend.ts` | 4,021 lines: grammar 8–160, parser 1540–2700 (about 1,150), the rest theory and checker |
| `bend2/base.bend` | 3,525 lines, 541 top-level declarations (`def`, `type`, `law`, `@unsafe def`) |
| `.bend` files in the repo | 1,982 (772,414 lines, most of them generated benchmarks): `tests/` (24 namespaces; `parse/` holds 173 positive parse tests; a negative test is one whose trailing `#|` lines are an error message), `demos/` (16 programs with `LAWS.bend` / `PROOF.bend`), `bench/` |
| releases | 2.0.40 on 2026-11-05; the clone's last commit 2026-11-18; the `CHANGELOG` names parser changes under "Clearer errors" and "Fixes" in most releases |

**[measure]** in BD0: how many of the 1,982 files `bend` itself accepts (the parse-failure tests are negatives), and how long parsing them
takes in our parser. **[measure]** after BD1: lines of `bend-parser`.

## 7. Risks

| Risk | Why it is one | What the plan does |
|---|---|---|
| the grammar moves under us | a young language, releases every few weeks, each with parser fixes | a grammar pin in `ENGINE.md`; the corpus proof re-run at every bump; function names that mirror `bend.ts`; the Ref oracle (02 §4): a Bun script that dumps every `Ref` with its span from `bend.ts` itself, diffed against our call sites |
| a tree-sitter grammar looks like a shortcut | `HigherOrderCO/tree-sitter-bend` exists; its last push is from 2024, before the Bend2 syntax **[check]** | not used; noted so nobody picks it up |
| error recovery that `bend` does not have | `bend` fails at the first error; we must outline a broken file (never fail; the cut-every-50-bytes test) | recovery only at declaration boundaries: a line at column 0 starting with `def`, `type`, `law`, `@unsafe` or `import` resyncs; the broken def keeps its range and gets `syntax_error: true` (BD1 §3) |
| Base's names | needed to tell "unknown name is the file's own" from "it is Base's" | a generated list, pinned; a file's own def still wins (§3), so the list being stale costs only a few `External` vs `Call` mistakes on Base's *new* names, never a wrong edge to a project def |
| a marker placement core has not built | D6 kept `Placement` for it; nothing built | Q-BD1; the code is `comment_marker::read/plan` with `#`, which core has for the file header already |
| what to show in proofs | a proof is long, calls lemmas constantly, and half of it is type positions | row BD-D9 (type positions not sites) keeps the tree to the code that runs; the owner can reopen after seeing the demos' trees |

Next: [`02-phase-1-parsing.md`](02-phase-1-parsing.md)
