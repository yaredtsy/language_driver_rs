# Python markers

A marker is **a line of text with a label, attached to a def or to a file**. The analyzer reads it (`Def::marker`, `header_marker`) and plans where one
would go (`plan_marker`, `plan_header_marker`). It never chooses the text and never writes a file: the caller applies the returned `Edit`. The label
(`ID`) is a parameter: `MarkerStyle { label, placement }`. **How a marker is read is frozen at the first tag (`python-v0.1.0`)**, like the names:
markers are already in users' files, and a stricter reader would stop seeing them.

Every example below is a test: `src/edit.rs` and `src/header.rs` hold the exact before and after texts (pylspt's, moved unchanged), and the testkit's
`marker_round_trip` runs the planner on every def of every fixture (`tests/outline.rs`).

## Reading a def's marker

A def's marker is a line `ID: <value>` in its docstring (D6):

```python
def parse_order(raw):
    """Parses an order.

    ID: 0Q8V5W4T2F7KX9M3B6D1RZ4H7K
    """
```

The label is the caller's, from `MarkerStyle`. The reader is deliberately loose, and is the one pylspt used: it looks at every `:`, takes the word before it and the word after it, ignores a pair where either holds another `:`, and
the last pair that fits wins. So prose that mentions the label counts: `"The text says ID: nothing-real, and ..."` is marked `nothing-real,`. Tightening
it would stop reading the markers already in users' files, so it needs a migration, not an edit.

## Writing a marker

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
(`AlreadyMarked`); the body has no statement or its docstring never closes (`UnsafeDocComment`); the marker text could not be read back (`InvalidText`:
empty, or whitespace, a colon, a quote, a backslash or a control character, which would also be able to end the docstring early); the text is not the
one the def came from (`DefNotFound`); or the style asks for a placement that is not built (`PlacementNotSupported`).

## The file marker

The file's own marker is not part of the outline. It is a comment in the header, with its own label, and `header_marker` reads it. `Outline::file_marker`
stays `None` from `python-syntax`. `plan_header_marker` puts `# <label>: <text>` after the leading run of comment lines (a shebang, an encoding line, a
licence block) and before the first statement, the module docstring included; a blank line ends the run, so the marker lands at the blank line and not
below it. Reading looks further, at the whole header, blank lines included. It is `comment_marker` from core with the prefix `#`; the only case it does
not cover is a header that ends the file with no line break, where the new line gets a break in front of it.
