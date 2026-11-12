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

## Update: a third value, `Basis::Name` (owner's decision, 2026-11-12)

`Type` could not tell "the code names this function" from "a type's method was chosen". On Jarvis (Python) it was 101 of 127 project edges, a fifth of
them certain and the rest possible. `Basis` has three values now:

| Basis | Meaning | Python |
|---|---|---|
| `Name` | the callee expression names a def, a class or a closure (project or library), with no receiver's class in between | ty types the callee expression as the function or the class itself, or a union of only those |
| `Path` | followed along a path the analyzer tracked | a method on an object built in this walk; a closure called through a variable |
| `Type` | the engine's answer through a type | a method through a receiver, a def that arrived as data (a parameter), a decorated def, `cls()`, `type[X]` |

The variant is in `core/analyzer-core/src/graph.rs`. **Nothing changes for TypeScript until it opts in:** every TypeScript edge is still `Type`,
`const BASIS = Basis::Type` is still required, and `#[non_exhaustive]` keeps a `_` arm valid.

### If you want `Name` at TS4: the exact change (your call)

I picked a **defaulted trait method**, not a field on `Target`. A field on `Target::{Call, Construct, External}` breaks every constructor of those variants
(three arms in `walk.rs::target`, the core's own test languages, the testkit's toy). The method breaks nothing: the solver already has the caller
(`node.0`) and the site where it stamps `L::BASIS`, so it asks the language, and the default answers `BASIS`.

Core, `core/analyzer-core/src/solve.rs`. This diff was applied to a copy of core and compiled: core's tests pass (78, one new) and clippy is clean.
The new test is in the diff.

```diff
@@ -69,6 +69,13 @@
     /// What this language's resolved edges rest on. An answer from the engine is `Basis::Type`. Required, not defaulted: a language
     /// says it, the core does not assume it.
     const BASIS: Basis;
+    /// What the edge to `target`, found at `site` of the function `caller`, rests on. `Self::BASIS` unless the language can say more at
+    /// this site: `Basis::Name` for a call whose callee expression names a function or a class. Defaulted, so a language that does not
+    /// override it is unchanged.
+    fn basis(&self, caller: Self::Func, site: Site, target: &Target<Self>) -> Basis {
+        let _ = (caller, site, target);
+        Self::BASIS
+    }
     /// The engine's handle for a function or class. Only valid during one request.
     type Func: Copy + Eq + Hash;
     /// What a context holds besides the function. `()` for a default resolver.
@@ -250,7 +257,10 @@
                         Target::Call(f, k) => {
                             let callee = (*f, k.clone());
                             run.discover(&callee, depth + 1);
-                            EdgeTo::Context(id_of(&self.lang, &mut run.infos, &callee), L::BASIS)
+                            EdgeTo::Context(
+                                id_of(&self.lang, &mut run.infos, &callee),
+                                self.lang.basis(node.0, calls.site, target),
+                            )
                         }
                         Target::Construct { class, init } => {
                             // A class has no body to walk, so it gets a row in `defs` and no context.
@@ -265,10 +275,12 @@
                             EdgeTo::Construct {
                                 class,
                                 init,
-                                basis: L::BASIS,
+                                basis: self.lang.basis(node.0, calls.site, target),
                             }
                         }
-                        Target::External(e) => EdgeTo::External(e.clone(), L::BASIS),
+                        Target::External(e) => {
+                            EdgeTo::External(e.clone(), self.lang.basis(node.0, calls.site, target))
+                        }
                         Target::Unresolved(why) => {
                             stats.unresolved += 1;
                             EdgeTo::Unresolved(why.clone())
@@ -509,6 +521,43 @@
     }
 
     #[test]
+    fn a_language_may_say_more_than_its_default_basis_at_one_site() {
+        struct Named;
+        impl Lang for Named {
+            const BASIS: Basis = Basis::Type;
+            type Func = u8;
+            type Key = ();
+            fn describe(&self, f: u8) -> DefInfo {
+                toy_info(if f == 0 { "a" } else { "b" })
+            }
+            fn digest(&self, _: &()) -> [u8; 16] {
+                [0; 16]
+            }
+            fn walk(&mut self, f: u8, _: &(), _: &Cancel) -> Summary<Self> {
+                let at = |n| SiteCalls {
+                    site: Site::new(n, n + 1),
+                    targets: vec![Target::Call(1, ())],
+                };
+                let sites = if f == 0 { vec![at(1), at(2)] } else { vec![] };
+                Summary {
+                    sites,
+                    complete: true,
+                }
+            }
+            fn basis(&self, _: u8, site: Site, _: &Target<Self>) -> Basis {
+                if site.start == 1 {
+                    Basis::Name
+                } else {
+                    Basis::Type
+                }
+            }
+        }
+        let g = Solver::new(Named, Limits::default()).graph(&[(0, ())], &Cancel::new());
+        let basis: Vec<_> = g.edges.iter().map(|e| e.to.basis()).collect();
+        assert_eq!(basis, [Some(Basis::Name), Some(Basis::Type)]);
+    }
+
+    #[test]
     fn two_sites_to_one_callee_are_two_edges() {
         let g = graph_of(&[("a", &["b", "b"]), ("b", &[])], "a");
         assert_eq!(calls(&g, "a"), ["b", "b"]);
```

TypeScript, `languages/typescript/crates/typescript-analysis`. Not applied; I did not touch your files.

1. `src/calls.rs`, in `impl Lang for TypeScript`, next to `const BASIS`:

```rust
fn basis(&self, caller: Func<'p>, site: Site, _target: &Target<Self>) -> Basis {
    if self.sem.callee_is_named(caller, site) { Basis::Name } else { Basis::Type }
}
```

2. `src/sem.rs`: `Sem::callee_is_named(func, site) -> bool`. The rule to copy from Python: the callee expression is an identifier (or `new` of one), or a
   member of a namespace or module import, whose declaration is a function or a class: not a property read on a value, not `this.m`, not a call through a
   union with an unknown member. Python asks ty "is the type of the callee expression the function/class itself?"; ask the checker the same. Estimate:
   15 to 25 lines, plus a `named` flag if you would rather carry it on `Resolved::Def` and `External` than re-ask at the stamp.
3. `tests/calls.rs`, `every_resolved_edge_rests_on_the_type_and_every_unresolved_edge_says_why` (the two asserts that change, lines 67 and 70 at this
   commit): they say every edge is `Type`. With the override they become

```rust
EdgeTo::Context(_, basis) | EdgeTo::External(_, basis) => {
    assert!(matches!(basis, Basis::Name | Basis::Type));   // was: assert_eq!(*basis, Basis::Type)
    resolved += 1;
}
EdgeTo::Construct { basis, .. } => assert!(matches!(basis, Basis::Name | Basis::Type)),   // was: assert_eq!(*basis, Basis::Type)
```

   and a new test pins one identifier call as `Name` and one method call on a value as `Type` in the `resolve` fixture. If you do not override
   `basis`, neither assert changes.
