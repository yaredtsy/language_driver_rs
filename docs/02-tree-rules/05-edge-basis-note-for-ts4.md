# Note for the TS4 session: what an edge rests on (`Basis`) and why an edge is `Unresolved` (`Reason`)

Status: **applied in commit 2296fc8.** `Basis` and `Reason` are in `core/` (graph, solve, render, the testkit) and Python's `core_graph.rs`, `call.rs`, `summary.rs` and `wire.rs` are adapted, in one commit with
every language's tests green (rule 5). The owner approved the direction on 2026-11-12; the shape below is the one TypeScript step 4 is written against. (Written as a proposal first; the text below
is unchanged.)

## Why

Python's edges come from two sources: a path the analyzer tracked (an object built in the walk, its attributes) and the engine's static type. Both
look the same in the graph today, and the only trace of why an `Unresolved` edge exists is a counter. The graph should say both on the edge.
For TypeScript every resolved edge is `Type`, and `Unknown(Why)` is the reason of an `Unresolved` edge.

## The type diff (`core/analyzer-core`)

```rust
// graph.rs
+/// What a resolved edge rests on.
+#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
+#[non_exhaustive]
+pub enum Basis {
+    /// Followed along a path the analyzer tracked: an object built in this walk, its attributes. (Python only, today.)
+    Path,
+    /// The engine's static type answered.
+    Type,
+}
+
+/// Why an edge has no target. The same string as the `Stats.language` counter key, so a count is what the edges say.
+/// An owned string, not `&'static str`: a graph read back from disk could not be deserialized into a borrowed one.
+#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
+pub struct Reason(String);
+impl Reason {
+    pub fn new(key: &str) -> Self { Reason(key.to_owned()) }
+    pub fn as_str(&self) -> &str { &self.0 }
+}

 pub enum EdgeTo {
-    Context(ContextId),
+    Context(ContextId, Basis),
-    Construct { class: StableDef, init: Option<ContextId> },
+    Construct { class: StableDef, init: Option<ContextId>, basis: Basis },
-    External(External),
+    External(External, Basis),
-    Unresolved,
+    Unresolved(Reason),
 }
```

The basis sits inside the three variants that have a target, and the reason inside the one that has none, so there is no combination to forbid
(no `Unresolved` edge with a basis, no `Context` edge with a reason). `Edge` itself does not change.

```rust
// solve.rs
 pub enum Target<L: Lang + ?Sized> {
     Call(L::Func, L::Key),
     Construct { class: L::Func, init: Option<(L::Func, L::Key)> },
     External(External),
-    Unresolved,
+    Unresolved(Reason),
 }

 pub trait Lang {
+    /// What this language's resolved edges rest on. An answer from the engine is `Basis::Type`. Required, not defaulted: a language
+    /// states it, the core does not assume it.
+    const BASIS: Basis;
     ...
 }
```

The solver stamps `L::BASIS` on every `Context`, `Construct` and `External` edge it builds, and passes the reason of `Target::Unresolved(r)`
through (`stats.unresolved += 1` stays). A language that needs a basis per target (path for one, type for another at one site) would put it in
`Target`; no language of the core solver needs that yet, and Python builds its graph without `Lang`.

## What you do at step 4

1. `impl Lang for <your type> { const BASIS: Basis = Basis::Type; ... }`.
2. Map the step 3 result: `Resolved::Unknown(why)` becomes `Target::Unresolved(Reason::new(why.counter()))`. Keep counting
   `Stats.language[why.counter()]` as D16 and rule 8 say.
3. Nothing else. Your tests that name `EdgeTo::External(e)` would read `EdgeTo::External(e, _)`.

Today `languages/typescript` has no use of `EdgeTo` or `Target::Unresolved`: the places that change in the repo are `core/analyzer-core`
(`graph.rs`, `solve.rs`, `render.rs`), `core/analyzer-testkit` (`graph.rs`, `toy.rs`) and Python's `core_graph.rs`, `wire.rs` and one fixture.

## What does not change

- No new edge kind (D16). `Unresolved` stays one variant; it gains the reason.
- No digest changes: digests hash the canonical key bytes, not edges. Nothing in the repo serializes a `Graph`.
- The tree and the wire do not change by default. A `show_basis` render option (a `ByType` mark) is a later, separate decision.
