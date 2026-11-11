# construct-marked-class

Pins **rule 5** (a construction is a node for the class; its children come from the constructor function) for a class
that carries a **marker**.

`main` builds two classes, both marked:

- `Report` has a constructor function that calls `prepare`, so `prepare` is its child.
- `Plain` has none, so it is a leaf.

Why the marker matters: a marked class is identified in the graph by its marker alone, which holds no name. The tree
is drawn from the graph with no engine to ask, so the graph must hold a context for the class (its name, file and
range). A language that leaves it out gets `<class>` in place of `Report` and `Plain`.

Both levels give the same tree, so there is one `expected.tree`.
