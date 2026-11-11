# Python qualnames

This is the naming rule of `python-syntax`. **It is frozen at the first tag (`python-v0.1.0`).** A qualname is stored in users' files: adding a
kind of def is safe, changing how an existing def is named is not. Every line below is pinned by a snapshot in
`crates/python-syntax/tests/snapshots/`; when this file and a snapshot disagree, the snapshot is what the code does.

The rule is the one the V-NOC Python driver and pylspt already use, moved here unchanged (pylspt commit `670ae79a15`, the driver's
`outline.rs`). Nothing about it is new; what is new is that it is written down and every row has a fixture.

## What a qualname is

```
qualname  =  the names of the enclosing defs, joined, then the def's own name

joiner    "."            between a class and what is in it        Outer.Inner · Outer.method
          ".<locals>."   between a function and what is inside    top.<locals>.inner · Outer.method.<locals>.helper
```

This is Python's own `__qualname__`, **without the module**. The qualname never contains the file or module name: a def is found by
`(file, qualname)`. A **function** (`def`, `async def`, a method) opens a `.<locals>.` scope. A **class** joins with `.`. A block (`if`, `else`,
`try`, `except`, `finally`, `for`, `while`, `with`, `match`, `case`) adds nothing: a def inside one is named as if the block were not there.

A name is **Python's identifier**, which is NFKC-normalized: `def 𝐱()` (mathematical bold x) is named `x`, as `__qualname__` would say. So
`qualname` can be shorter than the bytes in `name_range`. Never compute one from the other. Names are otherwise kept as written, accents and CJK
included, and private names are **not** mangled (`Dunders.__mangled`).

## What is a def

| Syntax | Kind | Qualname | Fixture |
|---|---|---|---|
| `def f():`, `async def f():`, decorated or not | Function | `f` | `functions.py` |
| a one-line body: `def f(): ...` | Function | `f` | `functions.py` |
| `class C:`, decorated or not, with bases or without | Class | `C` | `classes.py` |
| a `def` or `async def` directly in a class body, also through an `if` or `try` in that body | **Method** | `C.m` | `classes.py`, `blocks.py` |
| `@staticmethod`, `@classmethod`, `@property` members | Method | `C.m` | `classes.py` |
| a class in a class | Class | `A.B` | `classes.py` |
| a def or class in a function body | Function / Class | `f.<locals>.g`, `f.<locals>.C` | `locals.py` |
| a method of a class in a function | Method | `f.<locals>.C.m` | `locals.py` |
| a def or class in a method | Function / Class | `C.m.<locals>.g` | `locals.py` |
| a def in `if` / `else` / `try` / `except` / `finally` / `for` / `while` / `with` / `match` | Function | `g` | `blocks.py` |
| a def or class with type parameters (`def f[T]`, `class C[T]`, `def m[U]` in a class) | as above | `f`, `C`, `C.m`: the type parameters are not in the name | `generics.py` |
| non-ASCII names: `café`, `数据`, `数据.方法` | as above | the name | `names.py` |

**Two defs with one name are two defs.** Nothing is merged: a property getter and setter are `Property.value` twice, `@overload` twins and the
implementation are `twin` three times, and `if`/`else` branches that both define `conditional` are `conditional` twice (`classes.py`,
`duplicates.py`). Telling them apart is the stable identity's job (`StableDef::Path.nth`, in source order), not the qualname's. This differs from
TypeScript, where overload signatures are folded into one def.

The **kind** is `Method` for a def whose enclosing def is a class, and `Function` for every other def, including a def nested in a method.
`Class` is any `class`. (pylspt and the driver say `function` for both functions and methods and carry the parent's kind next to it; a
consumer that wants that shape maps `Method` to `Function` and reads the kind of the parent.)

## What is not a def

| Syntax | Why | Fixture |
|---|---|---|
| `lambda`, even assigned to a name: `square = lambda x: x * x` | no statement, no docstring to hold a marker | `not_defs.py` |
| a comprehension or generator expression | an expression | `not_defs.py` |
| `alias = some_function` | a name for an existing def, not a def | `not_defs.py` |
| `type Alias = int` | a type alias | `not_defs.py` |
| `x: int = 1`, any variable | | `not_defs.py` |

Only a `def` or `class` **statement** is a def, because only those can hold a docstring. The walk never enters an expression.

## What a def holds

| Field | Rule | Fixture |
|---|---|---|
| `range` | the whole statement **in bytes**, the first decorator included (`@one`, not `async def`), to the end of the last statement of the body | `functions.py`, `unicode_offsets.py` |
| `name_range` | the identifier only | every fixture |
| `params` | the names of positional-only, then regular, then keyword-only parameters, **in that order, not source order**; then `*args`, then `**kwargs`, each with its stars. A bare `*` and a `/` are not parameters. A class has none | `params.py` |
| `parent` | the enclosing def or class, whatever blocks sit between | `locals.py`, `blocks.py` |
| `marker` | the value of `<label>: <value>` read from the docstring (see below) | `markers.py` |
| `syntax_error` | a parse error **touches** the def's range, including an empty error at its end. It errs toward flagging a neighbour, never toward missing the broken one | `syntax_error_*.py` |

Offsets are bytes of the text as given: a UTF-8 BOM counts (a def after it starts at byte 3, `bom.py`), `\r\n` is two bytes (`crlf.py`), a tab is one
(`tabs.py`), an emoji is four (`unicode_offsets.py`). Converting to lines and UTF-16 columns is the driver's job.

## Syntax errors

The text is parsed for Python 3.13 and a syntax error never stops the outline: it is listed in `syntax_errors`, in the order the parser found them
(not source order), and the defs the parser could recover are listed too. In a broken file the names follow the parser's recovery: in
`syntax_error_indent.py` a `def` with a wrong indent is read as inside the def above it, so it is named `C.a.<locals>.b` and not `C.b`. Such a
name is not stable; the `syntax_error` flag is what says so, and a marker is never written into a def that carries it.

## Markers

A def's marker is a line `ID: <value>` in its docstring (D6). The label is the caller's, from `MarkerStyle`. The reader is deliberately loose, and
is the one pylspt used: it looks at every `:`, takes the word before it and the word after it, ignores a pair where either holds another `:`, and
the last pair that fits wins. So prose that mentions the label counts: `"The text says ID: nothing-real, and ..."` is marked `nothing-real,`. Tightening
it would stop reading the markers already in users' files, so it needs a migration, not an edit.

### Writing a marker

`plan_marker` returns one insertion (it never rewrites text: the old text is a subsequence of the new one) in a docstring, by these rules, which are
pylspt's, unchanged (`marker_forms.py`, `marker_crlf.py`, `marker_tabs.py`):

| The body | The insertion |
|---|---|
| no docstring | a new statement `"""ID: x"""` before the first statement, with that statement's indentation |
| no docstring, one-line body (`def f(): ...`) | a line break, the def's indentation plus four spaces, the new statement, and the same indentation before the old statement |
| a triple-quoted docstring whose closing quotes start their own line | a blank line and `ID: x` above the closing quotes |
| a triple-quoted docstring whose closing quotes follow text | the closing quotes move to their own line, after a blank line and `ID: x` |
| a one-line docstring in a one-line def | ` ID: x` appended inside the literal |
| a single-quoted docstring | ` ID: x` appended inside the literal (a line break would not parse) |

Line breaks are the file's own (`\r\n` stays `\r\n`), and indentation is copied byte for byte, tabs included.

It writes nothing, and says why, when: a parse error touches the def (`SyntaxError`); the def is already marked, prose that mentions the label included
(`AlreadyMarked`); the body has no statement or its docstring never closes (`UnsafeDocComment`); the marker text could not be read back (`BadMarkerText`:
empty, or whitespace, a colon, a quote, a backslash or a control character, which would also be able to end the docstring early); the text is not the
one the def came from (`DefNotFound`); or the style asks for a placement that is not built (`PlacementNotSupported`).

### The file marker

The file's own marker is not part of the outline. It is a comment in the header, with its own label, and `header_marker` reads it. `Outline::file_marker`
stays `None` from `python-syntax`. `plan_header_marker` puts `# <label>: <text>` after the leading run of comment lines (a shebang, an encoding line, a
licence block) and before the first statement, the module docstring included; a blank line ends the run, so the marker lands at the blank line and not
below it. Reading looks further, at the whole header, blank lines included. It is `comment_marker` from core with the prefix `#`; the only case it does
not cover is a header that ends the file with no line break, where the new line gets a break in front of it.
