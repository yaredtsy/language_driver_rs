# 08.03 Phase 2: defs and calls

```
phase 2
├── 1. BD2: the outline
│   ├── 1.1 what is a def, and its qualname
│   ├── 1.2 range, name range, params, parent
│   └── 1.3 markers and the header
├── 2. BD3: the book
├── 3. BD4: the call graph
│   ├── 3.1 the Lang impl
│   ├── 3.2 the walk: what is a site
│   ├── 3.3 the policy table
│   └── 3.4 the wire and the CLI
├── 4. BD5: what the driver needs
└── 5. steps and proofs
```

Phase 2 answers the user's second question: **what are the things that can be called, and the call tree between them.** Everything here sits
on BD1's tree and on core as it is today (`outline`, `edit`, `graph`, `solve`, `render`, `wire`). Python is the reference for the shape of
each module, TypeScript for the policy table's form ([`../03-typescript/06-ts4-calls.md`](../03-typescript/06-ts4-calls.md)).

## 1. BD2: the outline

`bend_syntax::outline(text: &str, style: &MarkerStyle) -> Outline`, in `bend-syntax`, over `sem::parse` (which calls `bend_parser::parse` and
converts to owned outline data; here "owned" costs nothing since the tree is already ours, but the module boundary stays).

### 1.1 What is a def, and its qualname

**The qualname is the declared name as written, dots included.** Bend has no nesting (no def inside a def, no inline module), so there is no
joiner and no `<locals>`. `def Nat.add(…)` is `Nat.add`; `def M.name(x)` filling an alias's law is `M.name` (the alias is file-local, and the
qualname is a name in **this file**: rule 10 is about what the user sees in this file, and moving the import's alias renames the def, as
renaming a Python class renames its methods). The file path is never in the qualname (as in Rust: the file is the stored def's module part).

| # | Syntax | Kind | Qualname | Fixture |
|---|---|---|---|---|
| D1 | `def f(…) -> T:` | `Function` | `f` | `def_plain.bend` |
| D2 | `@unsafe def f(…)`, `def f?(…)` | `Function` | `f` (the `?` is not part of the name) | `def_unsafe.bend` |
| D3 | `def f(…) -> T:` with a body of `import "…"` lines | `Function` | `f` | `def_foreign.bend` |
| D4 | `def t(~f: …, x: …) -> T:` | `Function` | `t` | `def_template.bend` |
| D5 | `law f:` | `Other("law")` | `f` | `law_and_proof.bend` |
| D6 | `def f(x):` (no `->`: fills the law) | `Function` | `f` | `law_and_proof.bend` |
| D7 | `type D<…> is K:` | `Enum` | `D` | `type_plain.bend` |
| D8 | a constructor `K{…}` under D7 | **not a def** | | `type_plain.bend` |
| D9 | `import ./m.bend as M` | not a def | | `imports.bend` |
| D10 | `x => body`, a `let`, a `match`, a `do` block | not defs | | `nested_not_defs.bend` |

D5/D6 give **two defs with one qualname** in a file that holds both the law and its proof. That is allowed (D15: a qualname is not unique in a
file; `occurrence` tells them apart, and the marker tells them apart for good). The alternative, a suffix on one of them, is Q-BD2.

D7 is `Enum`, not `Type` or `Struct`: a Bend `type` is a sum of constructors, and D45's wire rule gives `class_prefix` to `Class | Struct |
Enum | Union`, so a `Construct` edge to it lands where V-NOC looks for things that are built. The kind the wire shows stays `enum`. Row BD-D3.

D8 follows the reasoning the owner gave in D43 and D45: a constructor has no body, so a def for it would be an empty node; `K{a, b}` is a
`Construct { class: D, init: None }` of the type instead, which is what a Rust tuple-struct literal would be. Row BD-D5.

### 1.2 Range, name range, params, parent

- **range**: from the first line of the `#` comment block **directly above** the declaration (no blank line between) or the `@unsafe`, whichever
  is first, to the last byte of the body. Bend's own convention (the demos, `base.bend`) is a comment block above a def as its doc, and the
  marker will live there (§1.3), so the range must cover it, as a Rust `///` run or a JSDoc is covered. A comment block followed by a blank
  line is not the def's. **[check]** the demos: is a blank line between doc comment and def ever used? If so, the rule changes to "the block
  that ends on the line above", blank or not.
- **name_range**: the name token only, without `@unsafe`, `def`, `?`, `<…>` or `(`.
- **params**: the names as written with their quantity and template marks, without types: `def replicate(-A: Data, n: Nat, +x: A)` gives
  `["-A", "n", "+x"]`; `def twice(~f: U32 -> U32, x: U32)` gives `["~f", "x"]`; a law's `for x: Nat` clauses give `["x"]`. A `type`'s
  parameters are its `params` too (`["a", "-A"]`).
- **parent**: always `None`. Nothing nests.
- **syntax_error**: `true` on a declaration BD1's recovery marked.

### 1.3 Markers and the header

Bend has one comment form, `#` to end of line, and no docstring. So (Q-BD1, needs your yes):

- **a def's marker** is a line `# ID: <text>` in the comment block directly above the declaration (before `@unsafe` if present), read and
  planned by core's `comment_marker::read/plan` with prefix `#`, under a new `Placement::Above`. The plan inserts the line **immediately above
  the declaration**, below any existing comment block, copying the declaration's indentation (always zero) and the file's line ending. If a
  block is there already, the marker is its last line; `bend` ignores comments, so nothing changes for the compiler or the proof.
- **the file header** is a `# ID: <text>` line at the top of the file, before the imports; `book_load` skips blank and `#` lines before the
  first `import`, so this is legal Bend (read from the code, 980–983). The slot: offset 0, or after a shebang-like first comment if the owner
  wants one kept first (**[check]** whether any corpus file starts with something other than a comment or an import).
- the testkit's `marker_round_trip` and `cut_every_50_bytes` run unchanged.

`Placement::Above` is a core change: a new enum variant (the enum is `#[non_exhaustive]`) and a `plan` that puts the line above instead of in
a docstring. Rule 5: every language's tests stay green; nothing else uses the variant.

**Not** the `#|` lines: a test's trailing `#| expected output` lines are comments to Bend and to us, never a marker, and never a def's doc
block (they end the file; they are above nothing).

The rule goes to `languages/bend/docs/qualnames.md` and `markers.md` at the end of BD2, and freezes at `bend-v0.1.0`.

## 2. BD3: the book

`bend_analysis::Book::open(root: &Path, entry: &Path) -> Option<Book>`, in `bend-analysis`'s `book.rs` over `sem/`. It is what `Project::open`
is elsewhere, with Bend's own shape: there is no manifest, no `tsconfig`, no toolchain; **a project is the file you asked about and what it
imports**, and the namespace rule of [`01-what-bend-is.md`](01-what-bend-is.md) §3 is relative to the **entry file's directory**.

| Step | Rule | Comes from |
|---|---|---|
| read the entry file; parse; read its imports | relative paths resolve against the importing file's directory; `import Base` loads nothing and marks Base as present | `book_load` 973–1035 |
| follow imports transitively | a cycle is recorded (`Stats.language["import_cycle"]`) and not followed twice; a missing file is recorded (`import_missing`) and its alias becomes an external module | the same |
| a hub import (`0x…/`, `name@ver/`) | **never fetched, never read from `~/.bend/lib`**: an external module; every `P.x` is `External` named `P.x` | row BD-D7 |
| build the tables | `tlds: ns:name → (file, decl)`, `ctrs: ns:Name → (file, type)`, `aliases per file`; Base's names from the generated list (Q-BD3) | `parse_fresh`, `parse_qual` |
| resolve a name at a site | `parse_reso`'s rule, over the whole book (order of declaration ignored: never fail) | 1724–1735 |
| the root (D26) | the folder asked for; files outside it are still read when imported (a `../lib.bend`), and their defs are `External`? No: they are project defs with a path outside the root, as Rust's path dependencies are. **[check]** what the V-NOC contract wants for a def outside the root; Python's answer (D26) is the model | [`../05-python-migration/07-the-root.md`](../05-python-migration/07-the-root.md) |

What `Book` holds is owned: file texts, trees, tables. No salsa, no database, no memory question: a book of the whole Bend corpus is a few
megabytes. **[measure]** in BD3: open the largest demo and the whole of `tests/` as one book each; time and bytes.

`LAWS.bend` / `PROOF.bend`: a `PROOF.bend` imports `LAWS.bend` which imports the code; a proof def `Laws.sorted` fills the law `sorted` in
`LAWS.bend`'s namespace. The book sees three files; the outline of each is its own; a call from `main` to `Laws.sorted(xs)` resolves through the
alias to the proof def in `PROOF.bend`. This is the scenario `law-across-files`, new for Bend ([`05-decisions-and-questions.md`](05-decisions-and-questions.md) §2).

## 3. BD4: the call graph

### 3.1 The `Lang` impl

Core's flat solver, one context per def, as TypeScript and Rust ([`../02-tree-rules/03-the-seam.md`](../02-tree-rules/03-the-seam.md)):

```rust
impl Lang for BendLang<'_> {
    const BASIS: Basis = Basis::Name;   // core has it: "the code's own names fixed it" (graph.rs). Every Bend edge is that kind
    type Func = DeclId;                 // (file index, declaration index): Copy + Eq + Hash
    type Key = ();                      // phase 3 fills it
    fn describe(&self, f) -> DefInfo    // qualname, kind, file, range from the outline
    fn digest(&self, _: &()) -> [u8; 16] { [0; 16] }
    fn walk(&mut self, f, _: &(), cancel) -> Summary<Self>   // §3.2 over the body of ONE def
}
```

Modules: `sem/` (the only one that names `bend_parser` types), `book.rs`, `walk.rs` (one body → its sites), `calls.rs` (`Lang` and the public
`call_graph`), `key.rs` (`type Key = ();` with the seam's comment), `env.rs` (empty, the seam). The same five names as the other two tracks, on
purpose.

### 3.2 The walk: what is a site

`walk` visits one def's `Body`, every nested `Lam` body included (phase 1 rule: a lambda's calls belong to the enclosing def), and does **not**
visit: the def's parameter types and return type, any `Term` in a type position (the right side of `:` in an annotation, a `let`'s or do-bind's
type, type arguments `D<…>`, a law's clause types, a rewrite's motive `P`), or a pattern. It counts what it skips: `Stats.language["type_position"]`.

At each `Term::Call { callee, args }`:

| `callee` | Target |
|---|---|
| a `Name` not bound in scope that resolves to a project def (own file, or alias) | `Call(def, ())` |
| a `Name` that resolves to a Base def, or to a hub alias | `External { name }` |
| a `Name` bound by a parameter, a `let`, a `case` pattern or a lambda (checked **first**, as `parse_var` does) | `Unresolved("function_typed_value")` |
| a `Name` that resolves to nothing (typo, a def `bend` would refuse too) | `Unresolved("unknown_name")` |
| any other term (`f(a)(b)`: the callee is a call; `(x => …)(a)`) | `Unresolved("function_typed_value")`; the inner call is still walked |

Each `args` entry whose term is a bare `Name` resolving to a project def, or a `~`-marked one, is a **passed** site (D22) to that def; a Base
def passed is nothing (an External is not walked). The same for a `Name` inside a `Ctr`'s arguments (`App{view, tick}`: `tick` passed).

`Term::Ctr { name }`: resolves in the `ctrs` table; a project type's constructor is `Construct { class: type, init: None }`; Base's is nothing
(row BD-D5). `Term::Op`, `Index`, `ArrayNew`, `Do`: the sugar's def by the table of [`01-what-bend-is.md`](01-what-bend-is.md) §2; a project
type's `T.add` / `Array` is never a project type / `M.bind` and `M.pure` are a `Call`; Base's are nothing (row BD-D8). `Term::Rewrite { eq }`:
`eq` is walked (a lemma call is a call). Everything else is walked for nested terms and is no site.

### 3.3 The policy table

One fixture per row under `bend-analysis/tests/fixtures/policy/`, the table copied to `languages/bend/docs/policy.md` after BD4, as the other
two languages do (and Rust's `policy.md` is a test: the file is read by a test that checks every row has its fixture; do the same).

| Row | Bend | Edge | Counter |
|---|---|---|---|
| direct | `f(a)` to a def above | `Call` | |
| gpu | `f!(a)` | `Call` | |
| recursion | `f(p)` inside `f` | `Call` to self (the renderer marks it) | |
| below | `g()` where `g` is declared below (only legal in `@unsafe`) | `Call` | |
| alias | `M.f(a)` | `Call` to the def in the other file | |
| no-shadow | `def show(x)` in a file where Base has `show`: `bend` refuses the declaration (`parse_fresh`), we outline it and resolve `show(x)` to **this file's** def (never fail) | `Call` to this file's | `duplicate_of_base` |
| binder-wins | `f(x)` where `f` is a parameter and a def `f` exists | `Unresolved` (the binder shadows the def: `parse_var` looks binders up first) | `function_typed_value` |
| base | `IO.print(s)`, `U32.show(a)`, `List.map(…)` | `External("IO.print")` | |
| hub | `P.f(a)` with `import 0x…/main.bend as P` | `External("P.f")` | `hub_import` once per import |
| foreign | a call to a D3 def | `Call` to the leaf (it has a declaration here; its body is a `.c`) | |
| variable | `f(x)` with `f` a parameter, `let`, pattern or lambda binder | `Unresolved` | `function_typed_value` |
| closure-call | `adder(2)(1)`, `add2(1)` after `add2 = adder(2)` | inner `Call adder`; outer `Unresolved` | `function_typed_value` |
| template-arg | `twice(~(x => f(x)), 40)`, `twice(~g, 40)` | `Call twice`; `f` is a `Call` from the enclosing def (phase 1 rule); `g` is a **passed** edge | |
| passed | `App.run(~U32, ~App{view, tick}, …)`, `h(g)` | `view`, `tick`, `g`: passed edges; `App.run` External | |
| construct | `Circle{r}` of a project type | `Construct Shape` | |
| construct-base | `Some{x}`, `Con{h, t}`, `[1, 2]`, `"s"`, `3n+p`, `(a, b)` | nothing | |
| op-project | `(a + b : Vec2)` with `def Vec2.add` in the project | `Call Vec2.add` | |
| op-base | `(a + b : U32)`, `x < y` inside `(… : U32)`, `a && b`, `s ++ t`, `h <> t` | nothing | `sugar_base` |
| index | `a[i]`, `a[i] <- v` | nothing (Base's `Array`) | `sugar_base` |
| do-project | `do Parser<…>:` with `def Parser.bind`, `def Parser.pure` in the project | `Call Parser.bind`, `Call Parser.pure`, one site per statement | |
| do-base | `do IO<Unit>:`, `do Maybe<…>:` | nothing | `sugar_base` |
| rewrite | `%add_zero(p) : P` | `Call add_zero` | |
| type-position | `-> Or(Spec.LE(x, h), …)`, `{e : IsEven(n)}`, `List<T>` | nothing | `type_position` |
| law-call | `main` calls `add_zero(2n)` where both `law add_zero` and `def add_zero(x)` exist | `Call` to the **proof def** (the one with code); the law gets no edge | |
| law-open | `main` calls `f(…)` where only `law f` exists (no proof yet) | `Call` to the law def (a leaf: it is the only declaration) | `open_law` |
| unknown | `nope(1)` | `Unresolved` | `unknown_name` |
| lambda-inline | `(x => g(x))(1)` | `Unresolved`; `g` a `Call` from the enclosing def | `function_typed_value` |

Rows the owner may want to reopen after seeing real trees: `op-base`, `index`, `do-base` (should `U32.add` show as an External like
`IO.print` does? the plan says no, because then every arithmetic expression is an edge; Q-BD4), and `type-position` (a proof's type-level
calls; Q-BD5).

### 3.4 The wire and the CLI

The wire is core's (`graph → render → Node → wire`, merged on request); `WireStyle` prefixes are the driver's. Nothing Bend-specific but the
`target_kind` strings `function`, `law`, `enum`. `bend-cli outline <file>`, `bend-cli calls <dir> <file> --entry <name> [--graph | --wire
[--merged]]`, about a hundred lines as the other two.

## 4. BD5: what the driver needs

The same four as TS5 / RS5, smaller because a book is cheap to rebuild: `Session::open(root, file)`, `refresh()` (stamps on every file the book
read, D41's way: a changed stamp reparses that file and rebuilds the tables; no watcher), `graph_within(budget)` and `Cancel` from core (TS5
steps 1–2, already built), and `lost` when the entry file is gone (D46's rule). No `set_text` (D42). The whole phase is small: there is no
engine to keep warm and no database to patch; rebuilding a book of a few hundred files is milliseconds (**[measure]** in BD3).

## 5. Steps and proofs

| # | Phase | Do | Proof |
|---|---|---|---|
| 1 | BD2 | `sem::parse`, `outline` with rows D1–D10, `languages/bend/docs/qualnames.md` | fixture snapshots; `cargo tree -p bend-syntax` shows `bend-parser` and `analyzer-core` only |
| 2 | BD2 | core: `Placement::Above` + `plan` (after Q-BD1); `bend-syntax`'s `marker`, `plan_marker`, `header_slot`; `markers.md` | `marker_round_trip`, `cut_every_50_bytes`; every other language's tests green |
| | | **tag `bend-v0.1.0`** (the owner's) | |
| 3 | BD3 | `Book::open`, the tables, `base_names.rs` + its script | the 16 demos open; `tests/import/` opens with its cycles and misses counted; the measurement |
| 4 | BD4 | `walk`, `calls`, `Lang`, `call_graph`, the wire view | the policy rows' fixtures; `scenarios/*/bend/` for the six scenarios + `law-across-files`; `expected.default.tree` matches |
| 5 | BD4 | `bend-cli`, `policy.md` as a test, a sweep over the demos reporting counters | the sweep's counters in `ENGINE.md` |
| | | **tag `bend-v0.2.0`** | |
| 6 | BD5 | `Session`, refresh, budget, cancel | the tier-3 conformance checks the driver runs |
| | | **tag `bend-v0.3.0`** | |

Next: [`04-phase-3-context-layer.md`](04-phase-3-context-layer.md)
