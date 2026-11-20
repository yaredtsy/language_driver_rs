# law-across-files

Pins **Bend's law and proof across files** (`docs/08-bend/05-decisions-and-questions.md` §2): a `law` stated in one file, proven by a `def` of
the same name in another through an import alias, called from a third.

- `LAWS.bend` states `law sorted`.
- `PROOF.bend` imports `LAWS.bend as Laws` and proves it as `def Laws.sorted(xs)`: a def with no `->` fills the law its resolved name reaches.
- `main.bend` imports both and calls `Laws.sorted(xs)`.

The call resolves through the alias to the **proof** (the def with code); the law gets no edge, so the tree is `main ▸ Laws.sorted`. Without
the proof the same call would reach the law, a leaf, and count `open_law` (policy row `law-open`).

Two things to know. `main.bend` imports `PROOF.bend` so that the proof is in the book; in a real project the dependency runs the other way
(`PROOF.bend` imports the code), and the entry is `PROOF.bend` itself. And the proof's qualname in `PROOF.bend` is `Laws.sorted`, the name as
written (`languages/bend/docs/qualnames.md`), while Bend's own qualified name for it is the law's `LAWS:sorted`.

Only Bend has laws, so this scenario has one folder. Both levels give the same tree.
