# 04.04.05 RS4: the call graph

```
RS4
├── 1. the public function
├── 2. the pipeline
├── 3. step A: the sites of a body   (what is a call site)
├── 4. step B: the default resolver  (what a site reaches, and whether the answer is open)
├── 5. the bridge: from the engine's def to the outline's
├── 6. the policy table
├── 7. the crate, file by file
├── 8. tests
└── 9. steps, and the tag
```

Read [`../02-architecture.md`](../02-architecture.md) first: §5 and §6 there are why this phase has the shape it has.

## 1. The public function

```rust
// rust-analysis/src/lib.rs — the same shape as typescript-analysis
pub fn call_graph(project: &Project, file: &str, entry: &Entry<'_>, opts: &Options, cancel: &Cancel) -> Option<Graph>;

#[non_exhaustive]
pub struct Options { pub marker: MarkerStyle<'static>, pub limits: Limits }

pub fn breakdown(graph: &Graph) -> Breakdown;        // why edges are unresolved, and what is open; `Display` prints it
```

`None`: no such file, no def with that (qualname, occurrence), or a def that cannot be an entry (a trait, a module, a type alias, a
macro). A struct or an enum **can** be an entry: its root is `Root::Construct` with no `init`, a childless node, drawn exactly as when it
is a callee (D20 point 4). The tree and the wire are core's views of the graph (rule 7), re-exported.

## 2. The pipeline

```
entry (file, qualname, occurrence)
   │ sem::entry                         the outline's def → the engine's def (the bridge of §5, backwards)
   ▼
Solver<Rust>::graph(&[(func, Key)])     core's flat solver. `Key` is `()`
   │ for each context:  Lang::walk
   ▼
walk::body(f, &Env)
   │ A  sem.sites(f)                    the list of RawSite, cached
   │ B  resolve(f, site, &env)          for each, in order → Vec<Target>
   ▼
Graph ── calls::finish ──▶ Stats.language: reasons, open_* counters, the load's `skipped`
   │
   ├── render → Node → to_text          the tree
   └── wire(graph, root, sources, opts) the output that is used (D17); merged on request
```

## 3. Step A: the sites of a body

A def's body is: a function's block, a const's or static's initializer. A trait method without a body and a foreign function have none:
their list is empty and they are leaves.

`sem.sites(f)` walks the body's syntax **in source order**. With macros it walks the expansion in place of the macro call, so the order
is the order the code runs in.

| Syntax | Site? | `SiteKind` | Its span |
|---|---|---|---|
| `f(a)`, `path::f(a)`, `(expr)(a)` | yes | `Call` | the callee expression |
| `x.m(a)` | yes | `MethodCall` | the method's name |
| `S { a, b }`, `S { ..base }`, `Enum::V { a }`, `Self { .. }` | yes | `Record` | the path |
| `S(a, b)`, `Enum::V(a)`, `Self(a)` | yes | `Call` (the resolver finds it is a construction) | the callee expression |
| a unit struct or unit variant named as a value (`let u = Unit;`, `None`) | **no** | | there is no call in the text or in the compiler |
| a path to a function in a **value** position (`map(parse)`, `spawn(worker)`, `let f = Self::helper;`) | yes | `Passed` | the path. D22: handed over, not called |
| a call written in a macro's **arguments** (`println!("{}", f())`, `vec![g()]`, `assert!(check(x))`) | yes | as above | where it is written in the file |
| a call that comes from the **body of a project macro** (`macro_rules! log { () => { crate::emit() } }`, used as `log!()`) | yes | `MacroBody` | the macro call's path (`log`). Several such calls of one macro call share the span: they are several targets at one site |
| a call that comes from the body of an **external** macro (`println!` → `std::io::_print(…)`) | **no** | | the inside of external code is never entered (rule 5). The macro itself is not a node |
| a macro call the engine cannot expand | no site; counted `macro_not_expanded` | | what was in its arguments is lost, and the count says so (rule 8) |
| operators (`a + b`, `-a`, `a[i]`, `*p`), `?`, `for` (`IntoIterator`), a value going out of scope (`Drop`), auto-deref | **no** | | they are calls to the compiler, but nobody reads `a + b` as one, and showing them would bury the tree. A later option, per kind |
| `.await` | no; the awaited call is its own site | | transparent |
| inside a closure or an `async` block | **yes: sites of the enclosing def**, at their own positions (rule 3, M13). Each carries `closure: Some(n)`: the number of the closure, in source order within the def | | layer 1 ignores `n`. RC2 reads it |
| inside a nested item (a `fn`, `impl`, `const`, `static`, `mod` written in the body) | **no** | | they are defs of their own (RS1), walked when something calls them. (The engine's `outgoing_calls` takes every descendant and so counts them in the outer function; ours does not, and the oracle test of §8 allows exactly this difference) |
| inside `#[cfg]`-disabled code | no | | the engine has no body for it |

How "written in the arguments" and "from the macro's body" are told apart: the callee's name token is mapped up from the expansion; if it
maps to a token of the file inside the macro call's argument list, the call was written there. S-RS2 measures what `original_range_opt`
gives; the rule is fixed in `ENGINE.md` before this step is built.

The span is relative to the def's `range.start`, as core's `Site` wants. Two sites never share (start, end) and kind except `MacroBody`.

## 4. Step B: the default resolver

```rust
// walk.rs — final form from the first day (the seam, 02-tree-rules/03 §3)
fn resolve<'p>(lang: &Rust<'p>, f: Func<'p>, site: &RawSite, env: &Env) -> Vec<Target<Rust<'p>>> {
    resolve_from_env(lang, f, site, env).unwrap_or_else(|| {
        let callee = lang.sem.callee(f, site);          // THE default resolver
        lang.note_open(callee.open);
        vec![target(lang, callee.found, site, env)]     // Found::Def(g) → Target::Call(g, callee_key(lang, g, site, env))
    })
}
fn resolve_from_env<'p>(_: &Rust<'p>, _: Func<'p>, _: &RawSite, _: &Env) -> Option<Vec<Target<Rust<'p>>>> { None }
fn callee_key<'p>(_: &Rust<'p>, _callee: Func<'p>, _: &RawSite, _: &Env) -> Key { Key::default() }
```

Both empty functions are the seam: `resolve_from_env` is where a later layer answers an open site, `callee_key` is where it says under
which key the callee is walked ([`../02-architecture.md`](../02-architecture.md) §6). No `Target::Call` is built any other way.

`sem.callee`, by kind of site (D7: follow the engine):

| Site | Ask the engine | `Found` | `open` |
|---|---|---|---|
| `MethodCall` | `resolve_method_call_fallback` → a function | `Def` if project code, else `External` | `Some(..)` when the function is a **trait's own item** and the receiver's type (after auto-deref) is a type parameter → `TypeParam`; `Self` in a trait → `SelfInTrait`; an `impl Trait` parameter → `ImplTrait`; an opaque return type → `Opaque`; `dyn` → `Dyn`. Else `None` |
| | → a field, or nothing | `Unknown(unknown_method)` | `None` |
| `Call` | the callee's type → `as_callable()` → `Function` | `Def` / `External` | as above, on the path's qualifier (`W::new()`) |
| | → `TupleStruct`, `TupleEnumVariant` | `Construct(struct or enum)` / `External` | `None` |
| | → `Closure` | `Unknown(function_typed_value)` | `Closure` |
| | → `FnPtr` | `Unknown(function_typed_value)` | `FnPointer` |
| | → `FnImpl` (a value whose type is a parameter bounded by `Fn*`, or `impl Fn`) | `Unknown(function_typed_value)` | `FnParam` |
| | → nothing (the type is unknown or an error) | `Unknown(unknown_callee)` | `None` |
| `Record` | the path → a struct, a union, an enum variant | `Construct` / `External` | `None` |
| `Passed` | the path → a function | `Def` → a **passed** site (`SiteCalls::passed`). `External` → no edge, counted `external_passed`. A tuple struct or variant → no edge, counted `constructor_passed` (only a call into project code can be passed: D22) | as for `Call` |
| `MacroBody` | as `Call` / `MethodCall` | the same | the same |

Three rules about this table:

1. **`open` with a trait item whose cause we did not classify is `Open::Other`**, counted `open_other`. It must be near zero on real
   code; if it is not, the classification is missing a case, and the number says so before RC0 relies on the counters.
2. **`sem` calls the engine's substitution-returning functions and drops the substitution** (`resolve_method_call_fallback`,
   `resolve_path_with_subst`). RC1 keeps it. So layer 1's edges are, by construction, layer 2's edges under an empty key.
3. **The engine's answer is used as it is.** A method on a concrete type goes to the impl's method because `lookup_impl_method` found it,
   not because we looked. Where the engine's list has an order we do not control, `sem` sorts by (file, offset) (rule 9).

`Why` (the reason of an `Unresolved` edge, and its counter): `function_typed_value` (D16), `unknown_callee`, `unknown_method`,
`def_without_node`, `def_not_in_outline` (§5). The `open_*` counters and `macro_not_expanded`, `external_passed`,
`constructor_passed`, `macro_generated_target` are **notes**: counted in `Stats.language`, printed by `Breakdown`, never an edge's reason.

## 5. The bridge

The graph names a def the way the outline does (D20). So every engine def that becomes a `Func` goes through one function:

```
hir def ──HasSource──▶ (file, name range)
   │  the file is a macro expansion           → not a def of the text: External { package: <its crate>, name: <its path> }, counted `macro_generated_target`
   │  not project code (RS3 §4)               → External
   ▼
sem.outline(file)   rust-syntax's outline of that file, computed once per request and cached
   │  the def whose `name_range` is that range
   │  none (the two parses disagree)          → Unknown(def_not_in_outline): a bug that must show on the graph, not hide
   ▼
DefInfo { id: StableDef (the marker, or the path form), qualname, occurrence, kind, file, range }
```

`Lang::describe(f)` is this, memoized. It answers for functions **and** for structs and enums: the solver calls it for the `class` of
every construction and writes the row of `Graph::defs` (D13). For `Enum::Variant(a)` the class is the **enum** (a variant is not a def).
A `Func` is only ever made by the bridge, so a `Func` always has a row: `check_invariants` cannot find a missing one.

`sem::entry(file, qualname, occurrence)` is the same bridge backwards: the outline's def → its name range → the engine's def at that
range (`Semantics::to_def` on the node there).

A function that a macro generated is `External`, named, not entered: there is no place in the text to attach a doc or a marker to
([`../01`](../01-what-rust-analyzer-gives.md) §5b). What it calls is not in the tree; `macro_generated_target` counts how often that
happens, which is the number that would justify changing it.

## 6. The policy table

This becomes `languages/rust/docs/policy.md`, with the name of each row's fixture. The "layer 2" column is filled by the RC phases.

| Feature | Layer-1 policy | Changed by |
|---|---|---|
| a method on a concrete type | the method in that type's `impl`, inherent or trait | |
| a method through a type parameter, `impl Trait`, or `Self` in a default method | the **trait's** declaration. A leaf if it has no default body. Counted `open_type_param` / `open_impl_trait` / `open_self_in_trait` | RC1 |
| a method through `dyn Trait` | the trait's declaration. Counted `open_dyn` | RC3–RC5 |
| a method on the result of `-> impl Trait` | the trait's declaration. Counted `open_opaque` | RC4 |
| `Foo::new()` | a call to the function. Rust has no constructors |  |
| `Foo { a, b }`, `Foo(a, b)`, `Enum::Variant(a)`, `Self { .. }` | `Construct { class, init: None }`; the class of a variant is its enum | |
| `Some(x)`, `Ok(x)`, `String::from(..)` | `External` (`core`, `alloc`, `std`) | |
| a closure | not a def. Its calls are the enclosing def's, where they are written | RC2 (in part) |
| calling a closure value, a `fn` pointer, a parameter bounded by `Fn*` | `Unresolved(function_typed_value)`; counted `open_closure` / `open_fn_pointer` / `open_fn_param` | RC2, RC3–RC5 |
| a function named as a value | a **passed** edge to it, at the name (D22) | |
| operators, `?`, `Deref`, `Drop`, `for` | not sites | a later option |
| `.await` | transparent | |
| a call written in a macro's arguments | a site, where it is written | |
| a call from the body of a project macro | a site at the macro call | |
| the body of an external macro | not entered; the macro is not a node | |
| a call to a function a macro generated | `External`, named; counted | |
| derives and attribute macros | not expanded by default (RS3 §3); counted | `LoadOptions` |
| `#[test]` functions, `#[cfg(test)]` modules | ordinary defs when the engine has them. Hiding them is the driver's choice | |
| generics | one context for the generic function | RC1 |
| branches, loops, `match`, `if let`, `let else` | every call in the body is a site, once | |
| recursion | shown, not entered (rule 8 of the tree) | |
| aliases: `use a::f as g`, `pub use`, a type alias in a path (`type A = S; A::new()`) | followed to the real def. The edge names the def, never the alias | |
| project code | RS3 §4 | |
| external | `External { package: <crate name>, name: <path from the crate root> }` | |

## 7. The crate, file by file

```
rust-analysis/src/
├── lib.rs       Project, call_graph, Options, Breakdown, re-exports. Names only
├── sem/         the engine (or sem.rs: Q-RS3)
│   ├── mod.rs       Sem, Func, RawSite, SiteKind, Callee, Found, Open, Why: the whole surface the rest of the crate sees
│   ├── load.rs      RS3
│   ├── bridge.rs    §5: describe, entry, project code, the outline cache
│   ├── sites.rs     §3: step A
│   └── resolve.rs   §4: step B
├── walk.rs      body(): the loop; resolve(); resolve_from_env() (returns None); target()
├── env.rs       pub(crate) struct Env;
├── key.rs       pub(crate) type Key = ();
└── calls.rs     struct Rust<'p>; impl Lang (BASIS = Type, Func, Key, describe, digest → zeros, walk); call_graph; finish; Breakdown
```

`walk.rs`, `env.rs`, `key.rs`, `calls.rs` import nothing from `ra_ap_*`. `scripts/check-all.sh` checks it.

## 8. Tests

| Test | What |
|---|---|
| **the policy workspace** | `tests/fixtures/policy/`: one Cargo package, one module per row of §3, §4 and §6, loaded **once**. For each row: an entry, and its tree and `Breakdown` as a reviewed snapshot file |
| **the context workspace** | `tests/fixtures/context/`: the programs layer 2 is about (a generic called from two callers, a callback, a `dyn` in a local, a `dyn` parameter, a factory, a struct holding a `Box<dyn …>`, a recursive generic), written **now**, with their layer-1 trees and their `open_*` counts as snapshots. Each RC phase changes some of these snapshots on purpose; none writes a new program under pressure |
| **scenarios** | a `rust/` Cargo package in every folder of `scenarios/`; `analyzer_testkit::scenarios::run(&RUST, Level::Default)`. Rust's names differ from the shared trees (`Counter::bump`, not `Counter.bump`), so the testkit needs a per-language tree file where names differ (**Q-RS4**) |
| **the oracle** | for every function of both workspaces: every range `ide::Analysis::outgoing_calls` reports is a site of ours with the same target, except ranges inside nested items (§3). The engine's call hierarchy is the definition of "follow the engine" (D7); this test is that sentence, executable |
| invariants and determinism | `analyzer_testkit::graph::check_invariants` on every graph; `assert_deterministic` on three |
| the join | every def the outline lists in both workspaces is found by `sem::entry`, and `describe` of it gives the same (file, qualname, occurrence) back. The TypeScript join test (48 files, 334 defs) is the model |
| a real project | this repo's `core/`: every def an entry; no panic; the `Breakdown` of the whole, written into `ENGINE.md` with the date. These are the first real `open_*` numbers |
| the wire | one policy entry through `wire` unmerged and merged, as a snapshot; `WIRE_FORMAT_VERSION` asserted |

## 9. Steps

| # | Do | Proof |
|---|---|---|
| 1 | the bridge: `sem::entry`, `describe`, the outline cache | the join test |
| 2 | step A for plain calls, method calls, records; nested items skipped; closures tagged | a snapshot of the **site list** per policy module (no targets yet) |
| 3 | step B for those; `Callee`, `Open`, `Why`; `walk.rs`, `env.rs`, `key.rs` in final form | `sem`-level snapshots: site → `Found` + `open` |
| 4 | `calls.rs`: `impl Lang`, `call_graph`, `finish`, `Breakdown`; `rust-cli calls` | the first trees; invariants; determinism |
| 5 | constructions and `describe` for types; a struct as an entry | the construction fixtures, with a marked struct, tuple struct and enum shown by name |
| 6 | passed sites (D22) | fixtures: named, as a method path, a constructor (counted, no edge) |
| 7 | macros: arguments, project macro bodies, external bodies, unexpanded, generated targets | fixtures; the counters |
| 8 | the wire and `rust-cli calls --wire [--merged]` | the wire snapshot |
| 9 | scenarios (after Q-RS4), the oracle, the real project | all of §8 |
| 10 | `languages/rust/docs/policy.md`, one fixture named per row; `ENGINE.md` | reviewed |
| | **tag `rust-v0.2.0`** | |

Step 3 is where the design of this plan is tested for the first time: if `Open` cannot be computed from what the engine returns
(the receiver's type is not inspectable the way [`../01`](../01-what-rust-analyzer-gives.md) §7 assumes), stop and fix the plan before
step 4, because layer 2's order of work rests on those counters.

Next: [`06-rs5-driver-needs.md`](06-rs5-driver-needs.md)
