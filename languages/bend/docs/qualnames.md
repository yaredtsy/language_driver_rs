# Bend qualnames

This is the naming rule of `bend-syntax`. **It freezes at the first tag (`bend-v0.1.0`).** A qualname is stored in users' files: adding a kind
of def is safe, changing how an existing def is named is not (rule 10). Every row below is pinned by a snapshot in
`crates/bend-syntax/tests/snapshots/`; when this file and a snapshot disagree, the snapshot is what the code does. The plan it was written
from is `docs/08-bend/03-phase-2-defs-and-calls.md` §1 (rows BD-D3 to BD-D6). Written 2026-11-20 from the snapshots of BD2 step 1 (08bdce7).

## What a qualname is

```
qualname  =  the declared name, exactly as written, dots included
```

That is the whole rule. Bend has no nesting (no def inside a def, no inline module, no class), so there is no joiner, no `<locals>`, and
`parent` is always `None`. The dot inside a Bend name is a character, not a separator (`GUIDE.md`: "`U32.show` needs no module"), so
`def Nat.double` is the one def `Nat.double`, not a `double` under a `Nat`. The file path is never in the qualname: the file is the stored
def's module part, and moving a file renames nothing.

A def that fills a law of an **imported** file is written through the import's alias (`def M.name(x)`) and is named as written, `M.name`.
Bend resolves it to `<namespace>:name`; we do not, because the qualname is what the user sees in this file. Renaming the alias renames that
def, as renaming a Python class renames its methods.

**A qualname is not unique in a file** (D15): a `law f` and the `def f` that proves it share one (row D5, D6). `occurrences(&outline)`
(0-based, source order, D18) separates them, and the marker is what tells them apart for good. Q-BD2 (`docs/08-bend/05`) may give the law a
suffix before the tag; until it is answered, the two share the name.

## What is a def

| # | Syntax | Kind | Qualname | Params | Fixture |
|---|---|---|---|---|---|
| D1 | `def f(a: Nat, b: Nat) -> Nat:` | `Function` | `f` | `a, b` | `def_plain.bend` |
| D2 | `@unsafe def f(…)`, `@ unsafe` on its own line, `def h?(…)` | `Function` | `f`, `h` (the `?` is sugar, not part of the name) | | `def_unsafe.bend` |
| D3 | `def sin(x: F32) -> F32:` with a body of `import "./sin.c"` lines (a foreign effect) | `Function` | `sin` | `x` | `def_foreign.bend` |
| D4 | `def twice(~f: U32 -> U32, x: U32)` (a template) | `Function` | `twice` | `~f, x` | `def_template.bend` |
| D5 | `law add_zero:` | `Other("law")` | `add_zero` | the `for` / `exs` names: `n` | `law_and_proof.bend` |
| D6 | `def add_zero(n):` (no `->`: the proof of the law) | `Function` | `add_zero` (the same; occurrence 1) | `n` | `law_and_proof.bend` |
| D6b | `def M.name(x):` filling an aliased law | `Function` | `M.name` | `x` | `def_fills_aliased_law.bend` |
| D7 | `type List<a, -A: Kind(a)> is Kind(a):` | `Enum` | `List` | `a, -A` | `type_plain.bend` |
| D8 | a constructor `Con{head: A, tail: …}` under D7 | **not a def** | | | `type_plain.bend` |
| D9 | `import Base`, `import ./m.bend as M` | not a def | | | `imports.bend`, `imports_only.bend` |
| D10 | `x => …`, `x = v`, `match`, `case`, `do M<…>:` inside a body | not defs | | | `nested_not_defs.bend` |

D7 is `Enum`, not `Struct` or `Type`: a Bend `type` is a sum of constructors, and the wire gives `class_prefix` to `Class | Struct | Enum |
Union` (D45), so a construction edge to it lands where V-NOC looks for things that are built. The kind on the wire stays `enum`.

D8 follows the owner's reasoning in D43 and D45: a constructor has no body, so a def for it would be an empty node. `Con{h, t}` in a body is
a construction of `List` (BD-D5, built in BD4).

## Params

The parameter names **as written**, with their quantity and template marks and without their types: `-A: Data, n: Nat, +x: A` gives
`-A, n, +x`; `~f: U32 -> U32, x: U32` gives `~f, x`; a bare `a` gives `a`. A law's params are the names of its `for` and `exs` clauses with
their marks (`for ~k`, `for -A`, `for +x where P(x)`, `exs y` give `~k, -A, +x, y`). A type's params are its `<…>` binders the same way.

## Range and name range

- **range**: from the first byte of the `#` comment block that ends on the line **directly above** the declaration, or from the declaration's
  first token (`@`, `def`, `type`, `law`) when there is none, to the last byte of the declaration. A comment block followed by a blank line is
  not the def's (`def_plain.bend`: `main` starts at `def`, not at the comment two lines above). A doc above `@unsafe` is in the range
  (`def_unsafe.bend`: `g`). Measured on the corpus (`ENGINE.md` §BD2): 1,136 declarations have a block directly above, 103 have one a blank
  line away, and 61 of those are section banners (`# Array` / `# -----`), which is why the blank line is the rule.
- **name_range**: the name token only: not `@unsafe`, `def`, `?`, `<…>` or `(`.

## Errors

| Case | Outline | Fixture |
|---|---|---|
| a def that cannot be read to its end | kept, with its name, its doc, the parameters read so far and `syntax_error: true`; its range ends at the last code before the next declaration; the defs around it are untouched | `broken_between.bend` |
| a declaration whose name was never read (`@unsafe foo`) | no def; one `SyntaxError` | |
| text between declarations that is not one | no def; one `SyntaxError`; the neighbours stay unflagged | `stray_text.bend` |
| a non-ASCII name (`def naïve()`) | Bend names are ASCII: the def is `na`, flagged (the parser stops at `ï`, as Bend's does) | `unicode.bend` |
| an empty file, a file of comments, a file of imports | no defs, no errors | `empty.bend`, `only_comments.bend`, `imports_only.bend` |

Each `Outline.syntax_errors` entry is one `ParseError` of the parser, message `expected <what>, found <what>`.

## What is not here

The file-header marker and the def marker (`markers.md`, after BD2 step 2); what a def calls (`policy.md`, after BD4).
