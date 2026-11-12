# 02.04 The defs table

```
the defs table
├── 1. the problem, from a real run
├── 2. the cause
├── 3. the design
├── 4. the invariants
├── 5. what changes, file by file
├── 6. the steps, each with a proof
├── 7. what Python must do (PY5, PY6)
└── 8. what does not change, and what was rejected
```

A change to the graph type of [`01-graph-and-tree.md`](01-graph-and-tree.md) §2, decided on 2026-11-11 (D13).

## 1. The problem, from a real run

A program with two marked classes (now `scenarios/construct-marked-class/`), run through `pylspt-dev` at commit `670ae79a15`:

```
pylspt's own tree                 pylspt's graph, drawn by analyzer_core::render
─────────────────                 ───────────────────────────────────────────────
main                              main.main
  Report                            <class>
    prepare                           main.prepare
  Plain                             <class>
```

pylspt's graph has three contexts (`main`, `Report.__init__`, `prepare`). The classes appear only on the edges, as
`Construct { class: Node("1111…"), … }`: an ID, with no name. pylspt never noticed, because its renderer does not read the graph. It
holds the engine handle (`Definition<'db>`) and asks `Sem::qname` while drawing. The core renderer has no engine to ask.

No pylspt test shows this: none of the fixture programs constructs a class that has a marker.

## 2. The cause

`Context` holds two unrelated things:

| Fields | What they describe | How many per function |
|---|---|---|
| `id` (function + digest), `status` | one **analysis** of a body | one per key |
| `qname`, `kind`, `file`, `range` | the **definition** | one |

So the graph can only describe a definition whose body was walked. A class is never walked, so it has no description. The core solver
works around that by inventing a context for each constructed class. That workaround is removed by this change, because:

- it breaks rule 2 (a context is a function plus a key): the class "context" has no body, no key, and no edges;
- `stats.contexts` and `contexts.len()` stop agreeing;
- every language has to remember to do it, and forgetting is silent (`<class>` in the tree);
- a function with several contexts repeats its description in each one.

## 3. The design

**Definitions get a table of their own. A context is only an analysis.**

```rust
// core/analyzer-core/src/graph.rs

/// What a definition is. One row per definition the graph mentions, function or class.
#[non_exhaustive]
pub struct DefInfo {
    pub id:    StableDef,
    pub qname: String,       // the language's qualified name, for display
    pub kind:  DefKind,
    pub file:  String,       // path relative to the project root, with `/`
    pub range: Span,         // the whole definition, bytes
}

#[non_exhaustive]
pub struct Context {
    pub id:     ContextId,   // id.func is the key into `defs`
    pub status: Status,
}

#[non_exhaustive]
pub struct Graph {
    pub entries:  Vec<ContextId>,
    pub defs:     Vec<DefInfo>,     // sorted by id; one row per id
    pub contexts: Vec<Context>,     // sorted by id
    pub edges:    Vec<Edge>,        // sorted by (from, site, to)
    pub stats:    Stats,
}

impl Graph {
    pub fn new(entries: Vec<ContextId>, defs: Vec<DefInfo>, contexts: Vec<Context>, edges: Vec<Edge>, stats: Stats) -> Self;
    pub fn def(&self, id: &StableDef) -> Option<&DefInfo>;       // binary search, like `context`
}
```

- `DefInfo::new(id, qname, kind, file, range)` and `Context::new(id)` (status `Finished`) are the constructors.
- `Graph::new` sorts `defs` by `(id, file, range.start)` and keeps the first row of each id. So two definitions that carry the same marker
  (a copied function) give the same graph on every run. That case is not new and is not solved here: their contexts were already merged.
- **`solve::FuncInfo` is deleted.** It has the same five fields. `Lang::describe` returns `DefInfo` (the field `stable` is now `id`).
- `EdgeTo` does not change. `Construct { class: StableDef, .. }` is now a key into `defs`, the same way `ContextId::func` is.
- `External` stays on the edge. An external is never entered, is never a `from`, and has no file in the project; `{ package, name }` is
  already its whole description.
- **`Construct` is for a class in project code only.** A class from a library is an `External` edge, as in pylspt. A language tests
  "project code" before it tests "is a class", so every row of `defs` has a file inside the project.
- To show a call site in an editor: `graph.def(&edge.from.func)` gives the function's `range.start`; add `site.start`.

> **🦀 Rust note: a table and a key, not a pointer.** In a language with a garbage collector, a context would hold a reference to its
> definition object. In Rust a struct that points into another field of the same owner (`Graph`) needs lifetimes or `Rc`, and neither
> serializes simply. The usual answer is the one a database uses: put the rows in a sorted `Vec`, and store the key (`StableDef`) where a
> pointer would be. `binary_search_by` finds a row in `O(log n)`, the `Vec` serializes in a stable order (rule 11: never iterate a hash
> map to produce output), and the borrow checker has nothing to say because no reference is stored.

## 4. The invariants

`analyzer_testkit::graph::violations` checks them, so a language that forgets a row fails `check_invariants`, which the scenario runner
calls on every graph. Added to the existing checks:

| # | Invariant | The sentence `violations` returns contains |
|---|---|---|
| 1 | `defs` is sorted by id and no id appears twice | `defs are not sorted` |
| 2 | the function of every context has a row | `context … has no row in defs` |
| 3 | the class of every `Construct` edge has a row | `class … has no row in defs` |
| 4 | every row is used by a context or by a `Construct` edge | `def … is used by nothing` |

Invariant 4 keeps the graph "everything reachable from the entries" and nothing else: a leftover row would come from a cache or from the
order work was done in, and two runs could differ.

**The renderer never fails on a broken graph** (rule 6). A definition without a row is drawn with the name `<unknown>` and sorts first at
its site. This one fallback replaces three special cases: `<class>`, the `Path` qname of an undescribed class, and the silent skip of an
edge to a context the graph does not hold (which dropped a call from the tree).

## 5. What changes, file by file

| File | Change |
|---|---|
| `core/analyzer-core/src/graph.rs` | `DefInfo`; `Graph::defs`, `Graph::def`; `Context` loses four fields; `Graph::new` takes `defs`; the doc comment on `Site` points at the def's range |
| `core/analyzer-core/src/solve.rs` | `FuncInfo` deleted; `describe` returns `DefInfo`; the solver collects a row for every context it lists and every class it sees constructed; `Run::classes` and the invented class context are deleted |
| `core/analyzer-core/src/render.rs` | names and sibling order come from `graph.def`; `code` no longer needs the callee's context to exist; `class` no longer looks for a context; the `<unknown>` fallback |
| `core/analyzer-testkit/src/graph.rs` | the four invariants and their tests |
| `core/analyzer-testkit/src/toy.rs` | `describe` returns `DefInfo` |
| `docs/` | step 6 |

No language crate changes: none uses the core yet. The `Lang` trait keeps its three functions.

The solver, in words: where it pushes a `Context` it also pushes that function's `DefInfo`; where it builds a `Construct` edge it pushes
the class's `DefInfo`. Both come from the `infos` cache it already has (cloned: the cache keeps its own copy). `Graph::new` drops the
repeats. Do **not** write the cache out by iterating it: it is a hash map (rule 11), and it may hold a function that was described but
never listed.

## 6. The steps

Tests are written first in each step. In Rust a test against a type that does not exist yet fails by not compiling, which is a fine
"red". Steps 1–3 are one crate, so the crate only compiles again at the end of step 3.

| # | Do | Proof |
|---|---|---|
| 1 | `graph.rs`. Tests first: `def_lookup` (found, not found); `new_sorts_and_dedups` also checks `defs` order and one row per id; `two_rows_for_one_id_keep_the_first_by_position`. Test helpers: `def(name)` next to `ctx(name)`; `ctx` is now `Context::new(cid(name))`. Then the types of §3 | compiles in step 3 |
| 2 | `solve.rs`. Tests first: in the construct test, `g.contexts.len() == 2` and `g.stats.contexts == 2`, and `g.def(&marked("Foo"))` is a row with the class's kind; new `a_function_with_two_contexts_has_one_def_row` (the `u8`-keyed toy); `no_dangling` also requires a row for every context and every class. Then the solver change of §5 | compiles in step 3 |
| 3 | `render.rs`. Tests first: the helper `graph(spec)` builds rows; the construct tests give the classes a row of kind `Class` and **no** context; `rule_5_a_construct_without_init_is_still_a_leaf_node` expects `main\n  Bar\n`; new `a_def_the_graph_does_not_describe_is_drawn_as_unknown` (a class, and a called function); new `an_edge_to_a_context_the_graph_does_not_hold_is_still_drawn`. Then the renderer change | `cd core && cargo test -p analyzer-core`: every test passes. `grep -n "<class>\|FuncInfo" core -r --include=*.rs` prints nothing |
| 4 | `analyzer-testkit`. Tests first, replacing the two `a_construct_whose_class_…` tests: one test per row of §4 (each builds a graph that breaks exactly that invariant and finds the sentence), and `a_good_graph_has_no_violations` gains a `Construct` edge whose class has a row. Then `violations`, then `toy.rs` | `cd core && cargo test`: every test passes, the toy scenarios included |
| 5 | `cargo fmt`, then `cargo clippy --all-targets -- -D warnings` in `core/` | no output |
| 6 | Docs. [`01-graph-and-tree.md`](01-graph-and-tree.md): §2 shows the types of §3 here, the sentence "add `Context::range.start`" names the def's range, §5 gains the row "`ContextOut` held the name → `DefInfo`, one row per definition". [`03-the-seam.md`](03-the-seam.md) §2 and [`../06-roadmap.md`](../06-roadmap.md) §2 (the toy): `FuncInfo` → `DefInfo`. (The TypeScript and Rust instructions are already written: [`../03-typescript/06-ts4-calls.md`](../03-typescript/06-ts4-calls.md) §4 and [`../04-rust/04-default-resolver/05-rs4-calls.md`](../04-rust/04-default-resolver/05-rs4-calls.md) §5.) [`../05-python-migration/02-what-moves-where.md`](../05-python-migration/02-what-moves-where.md) §4, the `graph()` row: "fills `defs`" in place of "fills `file` and `range` on each context". The three rows of §7 below go into [`../05-python-migration/01-what-changes.md`](../05-python-migration/01-what-changes.md) §4. Then delete the sentence under this file's title that says this file wins | `scripts/check-all.sh --static` passes; `grep -rn "FuncInfo" docs` prints only this file |
| 7 | Everything | `scripts/check-all.sh` passes |

## 7. What Python must do

Python keeps its own solver, so nothing in §5 reaches it. Its `graph()` function (PY5) builds the `Graph`, and must fill `defs` itself:

- one row for the function of every reached context, and one for the class of every `Construct` target;
- from the calls the old renderer makes today: `Sem::qname`, `Sem::def_position`, `Sem::stable_def`, and `Sem::is_class` for the kind.

The proof is the scenario `construct-marked-class` at `Level::Precise`, and `check_invariants` on the graph of every fixture.

Three more differences the same real run showed. They are PY6 work, not part of this change:

| Difference | pylspt today | core | Do |
|---|---|---|---|
| no fixture constructs a marked class | the 36 fixture programs pass on both renderers while the tree is wrong | — | add one fixture (the scenario's program) before the parity test is trusted |
| the name printed | `to_text` drops the `module.` prefix: `read_json` | prints `DefInfo::qname` as given | Python writes the qualname **without** the module, as the outline does, so a tree node joins to an outline entry by `(file, qualname)` |
| the marks | `[recursive]`, `[cut]`, `[incomplete]` | `[Recursive]`, `[Cut]`, `[Incomplete]` | rewrite the `# expect:` blocks of the fixtures once, in PY6 |

## 8. What does not change, and what was rejected

Does not change: the `Lang` trait's shape, `Edge`, `EdgeTo`, `ContextId`, `StableDef`, `Stats`, `Node`, `RenderOptions`, the twelve rules,
the scenario files, the outline types.

What changes for a consumer: the serialized graph gains `defs`, and a context is `{ id, status }`. Nothing reads it yet. For PY7's golden
comparison this is one more documented difference.

| Rejected | Why |
|---|---|
| keep the invented class context, and document it | the four costs of §2 stay; it is a rule each language must remember instead of a shape the type gives |
| put the name on the edge: `Construct { class, qname, file, range, .. }` | repeats the description at every construction site, and names functions and classes by two different mechanisms |
| a row in `defs` for externals too | an external has no file or range in the project, and `{ package, name }` on the edge is what the driver contract asks for |
| write `defs` from the solver's cache | iterates a hash map, and lists functions that were described but not reached |

Next: [`../03-typescript/README.md`](../03-typescript/README.md)
