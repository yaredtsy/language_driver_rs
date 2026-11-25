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

**Decided 2026-11-21 (D54): an anonymous function's calls are drawn under the function that calls it.** So the precise tree will change: the
second `each` will draw `work`, and `main` will no longer list it. The file here is the tree of Bend's first layer (the anonymous function known,
its calls with its writer) and is replaced in the step that builds the rule (`docs/08-bend/07-value-layer-design.md` §12, step V4).

**TypeScript** (`typescript/`, added at its context layer's TC2 step 5e, 2026-11-25) gives **both** files as they are: with the layer on, the
first `each` is the context whose `f` is `work`, and the arrow is held as "an anonymous function" (`AnonAny`), whose call is
`unresolved anonymous_function_called` and whose own call of `work` stays with `main`. TypeScript builds D54 at TC7, which is not scheduled
(D58): if this file is replaced for D54, TypeScript keeps today's tree as a file of its own until then. **Rust** gives the default tree today;
D54 answers its Q-RC1 too, so the shared precise tree is its to match when its layer reaches it.

**Python** evaluates a `lambda` to unknown (it follows nested `def`s, which have a node of their own), so its precise tree for this program will
be a file of its own when its folder is added.
