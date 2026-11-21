# callback-argument

Pins **rule 3's anonymous-function row** and its named twin: a function handed to another function that calls it.

`each(f, x)` calls `f(x)`. `main` hands it a def by name, `each(work, 1)`, and then a function written at the call, `each(x => work(x), a)`.

- **Default** (`expected.default.tree`): `each` is a leaf both times, because the call through `f` has no target a name fixes
  (`Unresolved(function_typed_value)`, not drawn). The named `work` is a passed edge under `main` (`work [Passed]`, D22). The anonymous
  function's call of `work` belongs to the def it is written in, `main`.
- **Precise** (`expected.precise.tree`): the first `each` knows its `f` is `work` and draws it; the passed edge stays, because `work` is still
  handed over there. The second `each` knows its `f` is that anonymous function, and draws nothing: an anonymous function's calls stay with the
  def that writes it, so `work` is under `main`, as before, and each call appears exactly once.

**Bend** (`bend/`, the context layer's step 5) wrote the folder first: `docs/08-bend/06-context-layer-design.md` §11, and §6 for why the
lambda's calls stay with their writer (BC-D6).

**TypeScript** and **Rust** give the same two trees by their own designs (`docs/03-typescript/12-context-layer/03-design.md` §2, lines a
and i; `docs/04-rust/05-context-layer/01-design.md` §8): the shared trees are theirs to match when their layers are built. If Q-TC1 and
Q-RC1 are later answered the other way (an anonymous function's calls move under the function that calls it), the precise tree changes for
every language at once.

**Python** already follows the other rule (its solver draws a lambda's calls where the lambda is called), so its precise tree will be a file of
its own, `expected.precise.python.tree`, when its folder is added.
