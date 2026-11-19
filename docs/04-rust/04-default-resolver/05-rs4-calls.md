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

**Checked against RS3 as built (2026-11-17, reviewer, at `98770af`).** This file was written on 2026-11-12, before RS0 and RS3. What changed:
the `sem/` layout is the built one (§7: `origin.rs` holds `Owner` and is extended, not replaced by `bridge.rs`); a call by path asks
`resolve_path` first and the callable type second (§4, ENGINE.md "Rung 3" point 3); a record asks `resolve_variant`; the bridge has three
ways a def of a local crate is still `External`, each counted (§5); a def's id follows D34; the external name is settled (§5, §6); every
`warning_<code>` and the load's `skipped()` go into `Stats.language` (§2); what rungs 3 and 4 (no `std`) do to the tree (§6, §8);
the solver is called with `Start` and a `Budget`, as TypeScript's is (§1, §2); built-in derives expand without the proc-macro server (§6);
a call from an external macro's body that a project macro uses is still not entered (§3).

## 1. The public function

```rust
// rust-analysis/src/calls.rs, re-exported by name from lib.rs — the same shape as typescript-analysis
pub fn call_graph(project: &Project, file: &str, entry: &Entry<'_>, opts: &Options, cancel: &Cancel) -> Option<Graph>;
pub(crate) fn graph_within(project: &Project, file: &str, entry: &Entry<'_>, opts: &Options, budget: &Budget, cancel: &Cancel) -> Option<Graph>;

#[non_exhaustive]
pub struct Options { pub marker: MarkerStyle<'static>, pub limits: Limits }   // + `Options::new(marker)`, as TypeScript

pub fn breakdown(graph: &Graph) -> Breakdown;        // why edges are unresolved, and what is open; `Display` prints it
```

`call_graph` is `graph_within` with `Budget::default()`, as in TypeScript (`typescript-analysis/src/calls.rs`), so RS5's session cannot
drift from it. `file` is relative to the root, as `Project::files()` lists it. `Entry` is core's enum (D32, `133c6c2`): `Entry::Def { qualname, occurrence }`
goes to `sem::entry`; **`Entry::File` is always `None` in Rust**, by the language: nothing runs at the top of a Rust file (a `const` or
`static` initializer is evaluated at compile time, and `fn main` is a def). `Project::defs_named(file, qualname)` (D15, as TypeScript's,
`None` unless the file is project code as D33) comes with `call_graph` in step 4: the list of (occurrence, kind, range) a tool shows
when one qualname has several defs.

`None`: no such file, a file that is not in `Project::files()` (in no crate's module tree, RS3 §2), no def with that (qualname, occurrence),
a def the engine has no def for (in `#[cfg]`-disabled code), or a def that cannot be an entry (a trait, a module, a type alias, a macro, an
impl). A struct, a union or an enum **can** be an entry: the solver starts from `Start::Construct { class, init: None }` (core's
`solve::Start`, as TypeScript's `start_of`), and the graph's root is `Root::Construct` with no `init`, a childless node, drawn exactly as
when it is a callee (D20 point 4, D25 point 1). Rust has no constructor function, so `init` is always `None`. The tree and the wire are
core's views of the graph (rule 7), re-exported.

## 2. The pipeline

```
entry (file, qualname, occurrence)
   │ sem::entry                         the outline's def → the engine's def (the bridge of §5, backwards)
   ▼
sem::attached(&project, |sem| …)        one `hir::attach_db` around the whole request: every engine call, and the `Semantics`, live inside it
   ▼
Solver<Rust>::graph_within(&[Start], budget, cancel)   core's flat solver. `Key` is `()`. `Start::Call(func, ())` or `Start::Construct`
   │ for each context:  Lang::walk(f, key, cancel)
   ▼
walk::body(f, &Env)
   │ A  sem.sites(f)                    the list of RawSite, cached
   │ B  resolve(f, site, &env)          for each, in order → Vec<Target>
   ▼
Graph ── calls::finish ──▶ Stats.language
   │
   ├── render → Node → to_text          the tree
   └── wire(graph, root, sources, opts) the output that is used (D17); merged on request
```

`hir::attach_db` is not optional (S-RS2: a `hir` call outside it panics), and a `Semantics` made outside it is a bug. `origin.rs` attaches
per call today (nesting is allowed: its tests call it inside their own `attach_db`); RS4 makes one `attach_db` per request and builds the
`Sem` inside it, so the solver runs inside one closure. (A closure-scoped context, not a value you can store: this is why `Sem` is made and
dropped inside `call_graph`, never kept in `Project`.)

What `calls::finish` writes into `Stats.language` (a `BTreeMap<String, u64>`, so counts only, never a message):

| Key | From |
|---|---|
| each `Unresolved` reason | the edges, as TypeScript's `finish` |
| `open_*`, `macro_not_expanded`, `external_passed`, `constructor_passed`, `macro_generated_target`, `outside_root`, `duplicate_marker` | the notes of the walk and the bridge (§3–§5), absent when 0 |
| `build_scripts_skipped`, `proc_macros_skipped`, `layout_skipped` | `Project::skipped()`, as it is (RS3 §3, §6) |
| `warning_<code>: 1` | each of `Project::warnings()` (D37): `warning_no_toolchain`, `warning_dependencies_missing`, … The message stays on the `Warning` |

`Project::environment_inputs()` is strings, so it is **not** copied (RS3 §3 said it would be; corrected there). `Breakdown` reads the
`warning_*` keys back and prints them first: a tree read at rung 3 says so at its top.

## 3. Step A: the sites of a body

A def's body is: a function's block, a const's or static's initializer. A trait method without a body and a foreign function have none:
their list is empty and they are leaves. Since D44 no call reaches a trait method without a body (its site is `Unresolved`, §6), so
such a def is a node only as an entry of its own; a foreign function is still reached.

`sem.sites(f)` walks the body's syntax **in source order**. With macros it walks the expansion in place of the macro call, so the order
is the order the code runs in.

Step A is not "syntax only" (as `../02-architecture.md` §5 first said): it expands macros and resolves the names of value paths, both
engine questions. What it may ask is what **does not depend on the key**: the syntax, a macro's expansion, and name resolution of a path
(`resolve_path`). Types are step B's. So a `Passed` site is made only when name resolution says the path is a function or a tuple
constructor; a path to a local, a const, a static, a unit struct or variant is no site and no count (a variable read is not a call). A
path through a type parameter (`.map(W::make)`) is a `Passed` site whose answer step B opens.

| Syntax | Site? | `SiteKind` | Its span |
|---|---|---|---|
| `f(a)`, `path::f(a)`, `(expr)(a)` | yes | `Call` | the callee expression |
| `x.m(a)` | yes | `MethodCall` | the method's name |
| `S { a, b }`, `S { ..base }`, `Enum::V { a }`, `Self { .. }` | yes | `Record` | the path |
| `S(a, b)`, `Enum::V(a)`, `Self(a)` | yes | `Call` (the resolver finds it is a construction) | the callee expression |
| a unit struct or unit variant named as a value (`let u = Unit;`, `None`) | **no** | | there is no call in the text or in the compiler |
| a path to a function in a **value** position (`map(parse)`, `spawn(worker)`, `let f = Self::helper;`) | yes | `Passed` | the path. D22: handed over, not called |
| a call written in a macro's **arguments** (`println!("{}", f())`, `vec![g()]`, `assert!(check(x))`) | yes | as above | where it is written in the file |
| a call that comes from the **body of a project macro** (`macro_rules! log { () => { crate::emit() } }`, used as `log!()`) | yes (D30) | `MacroBody` | the macro call's path (`log`). Several such calls of one macro call share the span: they are several targets at one site |
| a call that comes from the body of an **external** macro (`println!` → `std::io::_print(…)`) | **no** (D30) | | the inside of external code is never entered (rule 5). The macro itself is not a node |
| a macro call the engine cannot expand | no site; counted `macro_not_expanded` | | what was in its arguments is lost, and the count says so (rule 8) |
| operators (`a + b`, `-a`, `a[i]`, `*p`), `?`, `for` (`IntoIterator`), a value going out of scope (`Drop`), auto-deref | **no** | | they are calls to the compiler, but nobody reads `a + b` as one, and showing them would bury the tree. A later option, per kind |
| `.await` | no; the awaited call is its own site | | transparent |
| inside a closure or an `async` block | **yes: sites of the enclosing def**, at their own positions (rule 3, M13). Each carries `closure: Some(n)`: the number of the closure, in source order within the def | | layer 1 ignores `n`. RC2 reads it |
| inside a nested item (a `fn`, `impl`, `const`, `static`, `mod` written in the body) | **no** | | they are defs of their own (RS1), walked when something calls them. (The engine's `outgoing_calls` takes every descendant and so counts them in the outer function; ours does not, and the oracle test of §8 allows exactly this difference) |
| inside `#[cfg]`-disabled code | no | | the engine has no body for it. **Also a disabled statement or expression inside an active body** (`#[cfg(any())] helper();`, a `#[cfg]` on a block or a `match` arm): name resolution still answers there (ENGINE.md "RS4: the bridge"), so step A takes only call expressions the engine **lowered into the body** (the body's source map has them), never what the file's text holds. **[check]** in step 2, with a fixture |

How "written in the arguments" and "from the macro's body" are told apart (**answered by S-RS2, 2026-11-16, `ENGINE.md`**): walk the macro call's expansion
(`expand_macro_call`, recursively) and ask `original_range_opt` of the callee expression (a call) or of the name (a method call). **`Some(range)` is a call written in the
file** (an argument of the macro; the range is exactly what was written). **`None` is a call from the macro's own body** (`println!`'s `_print`, `vec!`'s helpers, the
`inner_call()` of a project `macro_rules!`): its site is the macro call's range, kind `MacroBody`. A call whose callee resolves through a function a `macro_rules!`
generated has a callee `HasSource` in a macro file (External-like: no outline entry of ours).

**Whose body a call comes from is the innermost macro call that made it.** The walk recurses one macro call at a time, so at each level it
knows the macro (`Semantics::resolve_macro_call`) and its crate. A call with no original range is a `MacroBody` site only when the macro
that expanded to it is a **project** macro (RS3 §4: its crate is `Local`, its `macro_rules!` written under the root). So in
`macro_rules! log { ($e:expr) => { println!("{}", $e) } }` used as `log!(f())`, `f()` is a site where it is written, and `println!`'s own
`_print` is not a site, although it sits inside a project macro's expansion (D30 point 1: an external macro's body is never entered).
Fixture in step 7.

At rungs 3 and 4 (no `std`, RS3 §6) `println!`, `vec!`, `format!`, `assert!` have no definition, so they do not expand: each is
`macro_not_expanded`, and the calls written in their arguments are lost, counted, and explained by `warning_no_toolchain` (or
`warning_cargo_failed`, `warning_no_manifest`, `warning_manifest_unreadable`). **Answered in step 7** (8355422): an unexpanded macro call's
arguments give no site (they are a token tree) and no `unknown_*` reason; an unknown macro inside an unknown one is never met (one count);
one in the arguments of a macro that expands is met in the expansion, once. **One count per written place** (decided in the step 7
review, built in ff8f48b: the place is the macro call's own path range, or its original range inside an expansion): a macro that repeats its argument (`both!(unknown!(f()))`) meets the unknown call twice in its expansion, and it is counted once
when the unexpanded call has a place in the file (the same rule as a repeated argument's site, below); one with no place in the file (in a
project macro's body) is counted at each meeting, since each macro call that holds it is its own loss.

The span is relative to the def's `range.start`, as core's `Site` wants. Two sites never share (start, end) and kind except `MacroBody`.

**As built (RS4 step 2, 7edd537, reviewed 2026-11-17).** `sem/sites.rs`, `Sem::sites(func) -> Rc<Sites>` (list + `macro_not_expanded`), cached
per request; the list is sorted by the span (start, end), so a callee name orders the sites (`twice(r.get()) + helper()`: `twice`, `get`,
`helper`), and the `MacroBody` sites of one macro call keep the walk's order.

- **"Lowered" is `Semantics::type_of_expr(expr).is_some()`**: a lowered expression has a type, even an unknown one; one in a disabled
  statement, block, `let`, match arm, closure or record has none. The [check] of the cfg row above is answered with `fixtures/policy/src/cfg_off.rs`.
  A **macro call** is never asked (the answer is `None` for a statement-position macro and for one that cannot expand, lowered or not): the
  calls of its expansion are asked one by one.
- **A macro that repeats an argument gives one site** (`both!(f())` → `(f(), f())`: one `f`), the first: this table's rule (one place, one
  kind), the same as a call in a loop that runs twice. Not counted. `MacroBody` sites at one macro call stay several (several targets).
- **`macro_not_expanded` is not counted under a `#[cfg]` of the body.** The engine gives no expansion both for a macro it does not know and
  for one in a disabled statement (there it is the inner `format_args_nl!` of a disabled `println!` that fails), and does not say which.
  Accepted cost: a macro that truly cannot expand inside an **enabled** `#[cfg]` statement is not counted. Over-counting every
  `#[cfg(windows)] println!` on a Mac would be the worse lie.
- **Whose text a call is (step 7, 8355422; was the known gap).** A call written in a project macro's body **as an argument of an external
  macro** (`macro_rules! wrap { () => { vec![g()] } }`) has no file range, and its innermost macro is `vec!`. The walk keeps the macro calls
  it is inside (`Level`: the expansion's id, its root, project or not, its `at`); `body_owner` maps the call's last token up one expansion
  at a time (`InMacroFile<SyntaxToken>::upmap_once`, `Sem::upmap_once`) while it came from the **arguments** of the macro call that holds
  it, and the macro whose **body** it is in decides: a `MacroBody` site at `wrap!`; at `nest_wrap!` through two project levels (the
  outermost call written in the file); none for an external body (`println!`'s `_print`, where the mapping finds nothing). Only below the
  outermost level: for a call written in a file the engine maps a body token to the macro's **definition**, and `original_range` has
  already said "no place in the file". Probed in the review: `println!`, `assert!`, `format!` in a project macro's body, a project macro
  written in `println!`'s arguments, `vec![vec![g()]]`: all give the call.
- **Two calls of one target in one project macro's body are one edge** (`format!("{}{}", g(), g())` in a project macro: one `g`): they
  share the `MacroBody` span, and a site has a target once. Accepted, as a repeated argument's one site.
- **A file range is compared with its file** (fix asked in the review): `original_range_opt` returns a `FileRange`; the site is in the file
  only when that file is the def's own. A range in another file (a project macro defined in another module) is "no place in this body",
  never a span by offsets alone.

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
| `Call`, callee a path | **first** `resolve_path_with_subst` on the path → `Function` | `Def` / `External` | as above, on the path's qualifier (`W::new()`) |
| | → a struct (`Adt`), an enum variant (`Variant`) | `Construct(struct or enum)` / `External` | `None` |
| | → `SelfType` (`Self(1, 2)`), a local, or nothing: go on to the callable type, below | | |
| `Call`, any callee | `type_of_expr(callee)` → `as_callable()` → `Function` | `Def` / `External` | as above |
| | → `TupleStruct`, `TupleEnumVariant` | `Construct(struct or enum)` / `External` | `None` |
| | → `Closure` | `Unknown(function_typed_value)` | `Closure` |
| | → `FnPtr` | `Unknown(function_typed_value)` | `FnPointer` |
| | → `FnImpl` (a value whose type is a parameter bounded by `Fn*`, `impl Fn`, `&dyn Fn`) | `Unknown(function_typed_value)` | `FnParam` |
| | → nothing (the type is unknown or an error) | `Unknown(unknown_callee)` | `None` |
| `Record` | `resolve_variant(RecordExpr)` → a struct, a union, an enum variant (`hir::Variant` covers all three, and answers `Self { .. }`) | `Construct` / `External` | `None` |
| `Passed` | the path → a function | `Def` → a **passed** site (`SiteCalls::passed`). `External` → no edge, counted `external_passed`, **whatever its cause** (a dependency's, a project macro's, a built-in derive's, one outside the root: one counter for "handed over, no edge"; `macro_generated_target` and `outside_root` count calls only). A tuple struct or variant, the project's or not → no edge, counted `constructor_passed` (only a call into project code can be passed: D22). `Unknown` (only `def_not_in_outline`, a bug flag) → an ordinary `Unresolved` edge: core marks passed only an edge into a project context (`solve.rs`), and a bug must show | as for `Call` |
| `MacroBody` | as `Call` / `MethodCall` | the same | the same |

**A local that holds a function item is the function** (decided in the step 6 review, 2026-11-17): `let f = Self::helper; f(1)` is
`Call "f"` → `Def(Parser::helper)`, beside the `Passed` site of `Self::helper`. In Rust a function item has a type of its own that names
exactly one function (`fn(u8) -> u8 {Parser::helper}`), so `as_callable` answers `Function`: the engine's answer (D7), and not D16, whose
values have a type shared by many functions. A closure, a `fn` pointer (`helper as fn()`, a `fn()` parameter) and an `Fn*` bound stay
`function_typed_value`. TypeScript gives the same tree for `const f = helper; f(1)` (`helper [Passed]`, then `helper`; probed).

**Why the path first** (S-RS2's rule, measured in RS3 step 7, ENGINE.md "Rung 3" point 3): `resolve_expr_as_callable` fails when the
function's signature names a type the load does not know (`Option`, `String` without `std`), while name resolution does not need the
signature. On rung 1 the path first lifts `core/`'s in-project edges from 826 to 881; on rung 3, `languages/rust/` loses 5% of its
in-project edges path first and 22% callable first. It is also the more specific answer: `<JsonWriter as Make>::make()` is the impl's
`make` by path and the trait's by type. The callable type is the fallback for what has no path (`(h.cb)(1)`) or whose path names a value.

**As built (RS4 step 3, a7af483, reviewed 2026-11-17).** `sem/resolve.rs`, `Sem::callee(f, site) -> Callee { found, open }`; `Found` is
`Def`, `Construct`, `External { package, name, cause, construct }` or `Unknown(Reason)`. A path that names a function, a struct or a variant
is final; `Self(1)`, a local, a function-typed value go on to the callable type. `open` is asked only when the answer is a **trait's own
item**: of the receiver's type through `Type::autoderef` (the first step that is a parameter, a `dyn`, an opaque type or unknown decides),
or of the path's qualifier. `Self` in a trait is the engine's implicit type parameter too, so it is asked before `is_implicit` (else it
reads `impl_trait`). Snapshots: `tests/snapshots/callees/`, one per policy module.

Two changes, decided in the review:

- **A projection is `Open::Projection`** (`open_projection`), not `Other`: `items.next().unwrap().write()` with `I::Item: Writer` is
  answered by types, as `TypeParam` is (RC1 substitutes the key, then the projection normalizes). `Other` stays for what nothing names
  (an unknown type), so its number keeps meaning "the list is missing a case".
- **A trait-qualified path is asked of its `Self`**: `Writer::write(w)`, `Iterator::next(&mut it)`, `let x: W = Default::default()`. The
  qualifier is a trait, which says nothing about the type, so today these read `open_other`. The type is the substitution's `Self`:
  `resolve_path_with_subst` returns it (`GenericSubstitution::types`, the entry named `Self`; **[check]** that it is listed for a trait's
  item), and `dispatch_of` reads it as a receiver. Using the substitution to classify is not using it to resolve: rule 2 below still holds.
  Fixture rows: those three, through a type parameter (`type_param`) and on a concrete type (no `open`).

Three rules about this table:

1. **`open` with a trait item whose cause we did not classify is `Open::Other`**, counted `open_other`. It must be near zero on real
   code; if it is not, the classification is missing a case, and the number says so before RC0 relies on the counters.
2. **`sem` calls the engine's substitution-returning functions and drops the substitution** (`resolve_method_call_fallback`,
   `resolve_path_with_subst`). RC1 keeps it. So layer 1's edges are, by construction, layer 2's edges under an empty key.
3. **The engine's answer is used as it is.** A method on a concrete type goes to the impl's method because `lookup_impl_method` found it,
   not because we looked. Where the engine's list has an order we do not control, `sem` sorts by (file, offset) (rule 9).

`Reason` (the reason of an `Unresolved` edge, and its counter; named so as not to clash with `warning::Why`): `function_typed_value` (D16),
`unknown_callee`, `unknown_method`, `def_not_in_outline` (§5; a project def with no name lands here too, so there is no `def_without_node`). The `open_*` counters and `macro_not_expanded`, `external_passed`,
`constructor_passed`, `macro_generated_target`, `outside_root`, `duplicate_marker` are **notes**: counted in `Stats.language`, printed by
`Breakdown`, never an edge's reason.

## 5. The bridge

The graph names a def the way the outline does (D20). So every engine def that becomes a `Func` goes through one function. Its first
half is built: `sem/origin.rs`, `Loaded::owner_of_function → Owner::{Project { file }, External { package, name }}` (RS3 step 3, RS3 §4).
RS4 extends it, in that file, to the other kinds a `Func` or a construction names (a `Const`, a `Static`, an `Adt`, a `Variant`, whose
owner is its enum's) and to the name range; `bridge.rs` is the second half.

```
hir def ──origin.rs──▶ Owner
   │  External                                → External { package, name } (the name format below)
   │  Project { file, name range }
   ▼
sem.outline(file)   rust-syntax's outline of that file, computed once per request and cached
   │  the def whose `name_range` is that range
   │  none (the two parses disagree)          → Unknown(def_not_in_outline): a bug that must show on the graph, not hide
   ▼
DefInfo { id: StableDef (the marker, or the path form; D34), qualname, occurrence, kind, file, range }
```

**A def of a local crate can still be `External`, three ways**, and each is counted, because each is a body we could have walked and did
not (rule 8). **A construction is never counted** (decided in the step 5 review, 2026-11-17): a struct, union or enum has no body,
and Rust has no constructor function, so building one that a macro made (`Made(1)`) or one outside the root loses nothing. It is
`External`, named, `construct: true`, and only a **call** adds to `macro_generated_target` or `outside_root`. TypeScript counts `new Tool()` of
a class outside the root (`tests/outside_root.rs`: `helper`, `Tool`, `Tool.run` = 3) because building a class runs code: its constructor,
its field initializers, a base class's constructor. It decides by file (`is_outside_root_source`: a source file, never a `.d.ts`), not by
whether this class declares a constructor. Python walks `__init__` under the class node when the MRO has one. Rust's `init` is always
`None`: the same rule, count only where code was there to run. `Owner` today folds them into `External`; RS4 gives the bridge a way to tell them apart (a field or a variant: the coding
session's choice, crate-private):

| The def's source | Example | Edge | Counted |
|---|---|---|---|
| in a macro file (`file_id()` is `None`) | `make!(generated)` of a project `macro_rules!`; an `include!` of build-script output | `External { package: <its crate> }` | `macro_generated_target` |
| in a file of **another** crate | a built-in derive: `#[derive(Default)]`'s `default` has the trait's declaration in `core` as its source (S-RS2) | `External { package: <the type's crate>, name: <the type's path>::default }` (answered in step 1: the method's module is the type's; ENGINE.md "RS4: the bridge") | `macro_generated_target` |
| in a file of its own crate, outside the root | a path dependency or workspace member above or beside the root (RS3 §2, as D33) | `External { package: <its crate> }` | `outside_root`, as TypeScript's |

**The external name** (provisional in RS3 §4, settled here): `package` is the crate's display name, and `name` is the path **from the
crate's root, without the crate's own name**: `External { package: "alloc", name: "string::String::len" }`. The segment before the function's
name (amended in the RS4 step 3 review: without it, impls on a builtin type and the several trait impls of one type share a name, and the
merged wire counts them as one target):

| The function is | Its segment | Example |
|---|---|---|
| a trait's own item | the trait | `iter::traits::iterator::Iterator::next` |
| in an inherent impl of a struct, enum or union | the type's name, no generic arguments (as before) | `string::String::len`, `vec::Vec::new` |
| in an inherent impl of any other type | the self type as the engine displays it | `num::u8::checked_add`, `slice::[T]::len` |
| in a trait impl | `<self type as trait>`, both as the engine displays them, generic arguments included | `default::<u8 as Default>::default`, `string::<String as From<&str>>::from`, a built-in derive's `shapes::<Derived as Default>::default` |
| free | none | `mem::swap` |

The engine's display (`HirDisplay`), not RS1's printer: an external name is never stored, so its spelling only has to be unique and the
same on every run (rule 9). Today `origin.rs` puts the
crate in both (`name: "alloc::string::String::len"`), and core's `render` draws an external as `"{package}.{name}"`, so the tree and the
wire would show `alloc.alloc::string::String::len`. Without the repeat it is `alloc.string::String::len`, the same split as Python
(`package` the module, `name` the qualname) and TypeScript (`react` · `useState`). The crate is the **defining** one, as the engine says
(`alloc`, not `std`, for `String`: D7). The name is not stored in users' files, so it may still change; the joiner `.` is core's and is
not changed for one language. **An external variant keeps its name** (`core` · `option::Option::Some`, `result::Result::Err`): a project
variant is built as its enum because the outline has no variant def, but an external has no outline row, and naming `Ok(x)` and
`Err(e)` both `result::Result` would make the merged wire count them as one. Built in step 5 (b4aed14): `owner_of_variant` names an
external variant `<the enum's name>::<variant>`, and every construction of a variant (a path, a tuple variant, a record) goes through
`Sem::bridge_variant`.

`Lang::describe(f)` is this, memoized. It answers for functions **and** for structs, unions and enums: the solver calls it for the `class`
of every construction and writes the row of `Graph::defs` (D13). For `Enum::Variant(a)` the class is the **enum** (a variant is not a def).
A `Func` is only ever made by the bridge, so a `Func` always has a row: `check_invariants` cannot find a missing one.

**The id follows D34**, as TypeScript's `describe` does (TS5 D1, `c30e969`): the marker is read with `Options::marker` (RS2's reader);
when an earlier def **of the same file** has the same marker, this def's id is the path form and it is counted `duplicate_marker`. The
outline of the file is cached anyway, so the check costs nothing.

`sem::entry(file, qualname, occurrence)` is the same bridge backwards: the outline's def → its name range → the engine's def at that
range (`Semantics::to_def` on the node there). `None` when the file is not in `files()` or the def is in code a `cfg` turns off (the
engine has no def there).

A function that a macro generated is `External`, named, not entered: there is no place in the text to attach a doc or a marker to
([`../01`](../01-what-rust-analyzer-gives.md) §5b). What it calls is not in the tree; `macro_generated_target` counts how often that
happens, which is the number that would justify changing it.

## 6. The policy table

This becomes `languages/rust/docs/policy.md`, with the name of each row's fixture. The "layer 2" column is filled by the RC phases.

| Feature | Layer-1 policy | Changed by |
|---|---|---|
| a method on a concrete type | the method in that type's `impl`, inherent or trait | |
| a method through a type parameter, `impl Trait`, or `Self` in a default method | the trait's **default body**, when the method has one. A **required** method (no body): `Unresolved(function_typed_value)` (D44). Either way counted `open_type_param` / `open_impl_trait` / `open_self_in_trait` | RC1 |
| a method through `dyn Trait` | as the row above: the default body, or `Unresolved(function_typed_value)` for a required method (D44). Counted `open_dyn` | RC3–RC5 |
| a method on the result of `-> impl Trait` | **the engine's answer, the impl's method**: it sees through a return-position `impl Trait` to the hidden type (S-RS2, `ENGINE.md`), so `make().write()` is `<Json as Writer>::write` in layer 1 already (D7). Only where the engine still answers the trait's declaration (expected: a hidden type that depends on the function's own type parameters, `fn make<W: Writer + Default>() -> impl Writer`; **[check]** in step 5) is it the declaration, counted `open_opaque` | RC4 §2, only for what `open_opaque` still counts |
| `Foo::new()` | a call to the function. Rust has no constructors |  |
| `Foo { a, b }`, `Foo(a, b)`, `Enum::Variant(a)`, `Self { .. }` | `Construct { class, init: None }`; the class of a variant is its enum | |
| `Some(x)`, `Ok(x)`, `String::from(..)` | `External` (`core`, `alloc`, `std`). D30: kept as edges; their share of all edges is **[measure]**d in step 9 on a rung-1 load, and a large share reopens the question with the number | |
| no `std` (rungs 3 and 4; `NoStdSource` on rung 1) | `Some(x)`, `String::new()` have no def: `Unresolved(unknown_callee)`; a method on a `std`-typed receiver `Unresolved(unknown_method)`; `std`'s macros `macro_not_expanded` (§3). No new reason: `warning_<code>` in the same `Stats` says why. Calls by path and methods on project types still resolve (ENGINE.md "Rung 3") | |
| a closure | not a def. Its calls are the enclosing def's, where they are written | RC2 (in part) |
| calling a closure value, a `fn` pointer, a parameter bounded by `Fn*` | `Unresolved(function_typed_value)`; counted `open_closure` / `open_fn_pointer` / `open_fn_param` | RC2, RC3–RC5 |
| a function named as a value | a **passed** edge to it, at the name (D22) | |
| operators, `?`, `Deref`, `Drop`, `for` | not sites | a later option |
| `.await` | transparent | |
| a call written in a macro's arguments | a site, where it is written | |
| a call from the body of a project macro | a site at the macro call | |
| the body of an external macro | not entered; the macro is not a node | |
| a call to a function a macro generated | `External`, named; counted `macro_generated_target` (§5) | |
| built-in derives (`Clone`, `Debug`, `Default`, `PartialEq`, …) | expanded by the engine itself, with no proc-macro server: `x.clone()` on a `#[derive(Clone)]` project type resolves to a method with no source of its own, so `External`, named by the type's crate, counted `macro_generated_target` (§5) | |
| derives and attribute macros from a proc-macro crate | not expanded by default (RS3 §3); counted `proc_macros_skipped` | `LoadOptions` |
| a call into a local crate outside the root | `External`, named by its crate; counted `outside_root` (§5, RS3 §2) | |
| `#[test]` functions, `#[cfg(test)]` modules | ordinary defs when the engine has them. Hiding them is the driver's choice | |
| generics | one context for the generic function | RC1 |
| branches, loops, `match`, `if let`, `let else` | every call in the body is a site, once | |
| recursion | shown, not entered (rule 8 of the tree) | |
| aliases: `use a::f as g`, `pub use`, a type alias in a path (`type A = S; A::new()`) | followed to the real def. The edge names the def, never the alias | |
| project code | RS3 §4 | |
| external | `External { package: <crate name>, name: <path from the crate root, the crate left out> }`: `alloc` · `string::String::len` (§5) | |

## 7. The crate, file by file

```
rust-analysis/src/
├── lib.rs       module list and re-exports by name only (as built: Project, LoadOptions, OpenError, Warning, Why; RS4 adds call_graph, Options, Breakdown, breakdown)
├── project.rs   RS3: Project, LoadOptions, OpenError                      (built)
├── warning.rs   RS3: Warning, Why, collect                                 (built)
├── paths.rs     RS3: engine-text paths, root-relative                      (built)
├── manifest.rs  RS3: the Cargo.toml reader of rung 3                       (built)
├── toolchain.rs RS3: cargo --version, rustup, substitution                 (built)
├── sem/         the engine, the only module that names it (D35)
│   ├── mod.rs       exports by name: Loaded, Rung, load (built); RS4 adds Sem, Func, RawSite, SiteKind, Callee, Found, Open, Why, attached
│   ├── load.rs      RS3                                                    (built)
│   ├── origin.rs    Owner, owner_of_function (built); RS4 extends it (§5)
│   ├── bridge.rs    §5: describe, entry, the outline cache, D34
│   ├── sites.rs     §3: step A
│   └── resolve.rs   §4: step B
├── walk.rs      body(): the loop; resolve(); resolve_from_env() (returns None); callee_key(); target()
├── env.rs       pub(crate) struct Env;
├── key.rs       pub(crate) type Key = ();
└── calls.rs     struct Rust<'p>; impl Lang (BASIS = Type, Func, Key, describe, digest → zeros, walk); call_graph; graph_within; start_of; finish; Breakdown
```

`Why` in `sem/mod.rs` is the unresolved reason; the crate already exports a `Why` (`warning::Why`, why a toolchain was substituted). The
two never meet outside the crate, but one of them is renamed when RS4 adds the first (the sem one, say `Unknown`): two `Why` in one
crate is a trap for a reader. Everything outside `sem/` imports nothing from `ra_ap_*`: rule 4, which `scripts/check-all.sh` checks for
the whole crate.

## 8. Tests

| Test | What |
|---|---|
| **the policy workspace** | `tests/fixtures/policy/`: one Cargo package, one module per row of §3, §4 and §6, loaded **once**. For each row: an entry, and its tree and `Breakdown` as a reviewed snapshot file |
| **the context workspace** | `tests/fixtures/context/`: the programs layer 2 is about (a generic called from two callers, a callback, a `dyn` in a local, a `dyn` parameter, a factory, a struct holding a `Box<dyn …>`, a recursive generic), written **now**, with their layer-1 trees and their `open_*` counts as snapshots. Each RC phase changes some of these snapshots on purpose; none writes a new program under pressure |
| **scenarios** | a `rust/` Cargo package in every folder of `scenarios/`; `analyzer_testkit::scenarios::run(&RUST, Level::Default)`. Rust's names differ from the shared trees (`Counter::bump`, not `Counter.bump`), so the testkit looks for a per-language tree file (`expected.default.rust.tree`) before the shared one, only where names differ (D30). **Loaded once, as one workspace** (ENGINE.md, S-RS2 "Scenario cost": 3 s for all against 3 s each): the harness copies the `rust/` folders into a temporary folder with a generated `[workspace]` manifest, under the test's `target/` copy helper (`tests/common`) |
| **the oracle** | for every function of both workspaces: every range `ide::Analysis::outgoing_calls` reports is a site of ours with the same target, except ranges inside nested items (§3). The engine's call hierarchy is the definition of "follow the engine" (D7); this test is that sentence, executable. Needs `ra_ap_ide` as a dev-dependency (not yet in `Cargo.toml`; about 2.5 min of cold build, ENGINE.md S-RS1), and a `cargo tree` check that it stays dev-only |
| invariants and determinism | `analyzer_testkit::graph::check_invariants` on every graph; `assert_deterministic` on three |
| the join | every def the outline lists in both workspaces is found by `sem::entry`, and `describe` of it gives the same (file, qualname, occurrence) back. The TypeScript join test (48 files, 334 defs) is the model. Stated exceptions, each listed in the test: kinds that cannot be an entry (§1), and defs in `#[cfg]`-disabled code (the fixture has one, `#[cfg(any())]`) |
| **no `std`** | the policy workspace's plain-call and project-method rows at rung 3 (a worker with no `cargo` on `PATH`, as RS3's `worker_without_cargo`): **the site list of every policy def is the one of rung 1 except inside `std`'s macros** (so "lowered" does not depend on `std`), the project edges are the same as at rung 1, `std` calls are `Unresolved`, `warning_no_toolchain: 1` and `macro_not_expanded` are in `Stats.language`. A lone file (rung 4): one entry, `warning_no_manifest: 1` |
| a real project | this repo's `core/`: every def an entry; no panic; the `Breakdown` of the whole, written into `ENGINE.md` with the date. These are the first real `open_*` numbers |
| the wire | one policy entry through `wire` unmerged and merged, as a snapshot; `WIRE_FORMAT_VERSION` asserted |

**As built (step 9, reviewed 2026-11-17).**

- **Scenarios** (52e98e7): six `scenarios/*/rust/` programs, one load (a generated `[workspace]` under `target/`, `LOADS == 1` asserted).
  The testkit looks for `expected.<level>.<folder>.tree`, then `expected.<folder>.tree`, then the shared files (a core test pins the
  order). Rust's own trees: `method-on-class` and `two-callers` (`::` names), `construct-marked-class` (no constructor function, so
  `Report` is a leaf). The other three give the shared tree.
- **No `std`** (0d7bad2, `sem/nostd.rs`, a worker): 35 of 40 trees are rung 1's; the 5 others are the `std`-macro rows, a prefix of rung
  1's tree with `macro_not_expanded`. Two site lists differ for another reason, named in the test: a **hand-over of a `std` path**
  (`.map(Some)`, `.map(String::from)`) names nothing without `std`, so §3 gives no site, and `constructor_passed` / `external_passed` are
  not counted. **Decided in the review: accepted, not counted.** A value-position path that resolves to nothing may be a constant, a
  static or a unit variant (`u8::MAX`, `PI`, `None`) as well as a function, so counting it as a lost hand-over would count reads too;
  at rung 1 a project that compiles has none, and at rung 3 `warning_no_toolchain` already says every `std` answer is missing.
- **`core/` as the real project** (b41610b, `sem/realproject.rs`): 13 files, 455 entries, no panic, `check_invariants` on every graph;
  every one of the 3333 sites counted once, as exactly one category (asserted); no `unknown_*`, `def_not_in_outline`, `open_other` or
  `macro_not_expanded` (asserted, so a change in `core/` that adds one fails Rust's test: the classification to extend or the number to
  accept). The numbers are in `ENGINE.md` "RS4: core/ as a real project". Exact counts are not pinned: `core/` changes.
- **The oracle** (33f7779, `sem/oracle.rs`): for every function, const and static, every range `outgoing_calls` reports is a site of ours
  with the same target, or one of **four classes, each recognised by a rule and counted**: a **nested item** (another outline def inside
  this one holds the range, §3); a **tuple variant** (the oracle's target is a variant lying inside our target, its enum, §5); a
  **generated target** (ours is `External` with cause `MacroFile` / `OtherCrate`, the oracle's is in the project, §5); a **trait method**
  (ours is an impl's method of the oracle's trait function, same name, `trait_()` equal: the oracle takes the callable type, we the path
  first and the engine's name resolution answers the impl, §4 rule 3). Any other difference fails with its range. **Policy workspace: 124
  the same, 3 nested, 2 tuple variants, 6 generated, 2 trait methods; context workspace: all 28 the same**, all pinned. (The review's
  "3 + 6 + 2 = 11" had left out the two tuple variants; the spike's "10" was the last three classes: 13 exceptions in all.) `ra_ap_ide`
  is a dev-dependency only, and `scripts/check-all.sh --static` checks the normal `cargo tree` of `rust-analysis` and `rust-cli` has
  `ra_ap_ide_db` and no `ra_ap_ide` (it fails when the dependency is moved). The oracle reports calls and method calls only, so a record,
  a hand-over and a macro's arguments are ours alone.
- **The context workspace** (eab0ce0, `tests/fixtures/context/`, `tests/context.rs`): a generic called from two callers, a callback (a
  `fn` pointer, an `Fn` parameter, a closure, a def handed over), a `dyn` in a local, a `dyn` parameter, a factory (`Box<dyn Writer>`,
  and `impl Writer`, which the engine sees through), a struct holding a `Box<dyn Writer>`, a recursive generic. 14 trees with their
  `Breakdown` in `tests/snapshots/trees/context.txt` (read in the review: `emit` and `countdown` one node with `type_param` 1; `apply` /
  `apply_generic` `fn_pointer` / `fn_param`; the `dyn` cases `Writer::write` with `dyn` 1; `make_impl().write()` already
  `<Json as Writer>::write`). The join finds and names its 26 entries. Each RC phase changes these snapshots on purpose.

## 9. Steps

| # | Do | Proof |
|---|---|---|
| 1 ✅ | **done** (e9e03ed, reviewed 2026-11-17). The bridge: `origin.rs` extended (const, static, ADT, variant; the name range; the three `External` causes of §5; the name without the crate), `bridge.rs` (`sem::entry`, `describe` with D34, the outline cache), one `attach_db` per request | the join test; `origin.rs`'s owner tests updated to the new name |
| 2 ✅ | **done** (7edd537, reviewed 2026-11-17; macros built here too, as far as §3 "As built" says). Step A for plain calls, method calls, records; nested items skipped; closures tagged | a snapshot of the **site list** per policy module (no targets yet) |
| 3 ✅ | **done** (a7af483 + the step-2 file fix 41bf33f, reviewed 2026-11-17; two changes asked, §4 "As built", and the external-name table of §5). Step B for those, path first (§4); `Callee`, `Open`, `Reason`; `walk.rs`, `env.rs`, `key.rs` in final form | `sem`-level snapshots: site → `Found` + `open` |
| 4 ✅ | **done** (f5bd90c step-3 changes, 9dc4a52, reviewed 2026-11-17: 21 entries in `tests/snapshots/trees/policy.txt`; probed on `core/`. Core's default render hides externals, unresolved and open answers: the `Breakdown` carries them, as in TypeScript. `rust-cli calls <dir> <file> --entry <qualname> [--id | --occurrence] [--graph] [--timing]`, `--entry` required, several matches listed and none picked, exit 2; warnings on stderr first). `calls.rs`: `impl Lang`, `call_graph` over `graph_within` (`Entry::File` → `None`), `Project::defs_named`, `finish` (the `Stats.language` table of §2, `skipped()` and `warning_*` included), `Breakdown`; `rust-cli calls` | the first trees; invariants; determinism; a graph of a load with a warning has its `warning_<code>` |
| 5 ✅ | **done** (b4aed14, reviewed 2026-11-17: `tests/fixtures/policy/src/constructions.rs`, six trees in `tests/snapshots/trees/policy.txt`; probed: a union, a type alias of a struct and of an enum, `..base`, `..Default::default()`, a glob-imported variant, `Option::<u8>::Some`, and `core/`'s `RenderOptions` / `Mark` as entries; `core/`'s edge counts unchanged, 22 / 11 / 108 / 0). A struct, union or enum entry is `Root::Construct` with no `init`, a childless tree; a construction is not counted as an unwalked body (§5) | the construction fixtures, with a marked struct, tuple struct and enum shown by name |
| 6 ✅ | **done** (719b1ad, reviewed 2026-11-17: `tests/fixtures/policy/src/passed.rs`, seven trees; step A takes a path in a value position that `resolve_path` says is a function, a tuple struct or variant, or `Self` of a tuple struct, never the callee of a call (parentheses looked through: `(f)(1)`), only from lowered code; `RawSite::is_passed` tells a `MacroBody` hand-over from a call. Probed: a hand-over in a project macro's body, `&helper`, `helper as fn()`, a struct field, an array, a return value; a macro-made and two derive-made functions handed over, counted `external_passed`; `core/` unchanged) | fixtures: named, as a method path, a constructor (counted, no edge), a local (no site) |
| 7 ✅ | **done** (5cee903 `external_passed` label + `generated_handed_over` row, 8355422, reviewed 2026-11-17: `body_owner` + `upmap_once` close the `wrap!` gap; fixtures in `macros.rs`; ten trees; the §3 [check] answered; `macro_generated_target` counts calls to a project macro's function and a derive's methods once per call, `core/`'s 8 are all derives; `core/` unchanged, 22 / 11 / 108 / 0). Fix asked: one `macro_not_expanded` per written place (§3). Was: macros: what step 2 left (§3 "As built": a project macro's own call written as an external macro's argument, `upmap_once`), generated targets, built-in derives; the policy rows and counters for what step 2 built | fixtures; the counters; the two [check]s of §3 and §5 |
| 8 ✅ | **done** (ff8f48b `macro_not_expanded` once per written place; 97d1311 the wire, reviewed 2026-11-17: `lib.rs` re-exports core's wire items by name; `impl Sources for Project` gives the engine's loaded text (`sem::text_of`), byte-equal to the file; `rust-cli calls … --wire [--merged] [--function-prefix P] [--class-prefix P]`, JSON alone on stdout, flag rules as `typescript-cli`; `tests/wire.rs`: four entries unmerged and merged, `WIRE_FORMAT_VERSION` 2. Probed on `core/`'s `render`: 59 nodes unmerged, 28 merged with `call_count` summing to 31 (28 + 31 = 59). Two findings for core, both outside this step: (1) a **byte-order mark is counted as a column** on line 1 (`Positions::at`): `(1,19)` where an editor says 18; TypeScript's `text_of` keeps the mark too, so it is not Rust's alone; fix asked in core, (2) a marked struct gets `function_prefix`: Q-RS7) | the wire snapshot |
| 9 ✅ | scenarios (per-language trees, D30, one shared workspace), the oracle, no `std`, the real project. **Done** (52e98e7, 0d7bad2, b41610b, 33f7779, eab0ce0; core 3edf5bb BOM column; reviewed 2026-11-17, §8 "As built") | all of §8 |
| 9b | **D44 (owner, 2026-11-19; not built).** A site whose answer is a **project** trait's own function with **no body** (a dependency's or `std`'s stays `External`) is `Found::Unknown(FunctionTypedValue)`, its `open` kept. The oracle gains a fifth class, **a required trait method** (ours `Unresolved(function_typed_value)` with an open kind, the oracle's target the trait's function with no body), recognised by that rule and counted. The context workspace's `dyn` and generic trees lose their `Writer::write` leaves. **Decided at the stop (2026-11-19):** the scenario `two-callers/expected.default.rust.tree` loses its two `Writer::write` leaves (blessed; its README says why); a hand-over of a required method draws no edge and is counted `signature_passed` (D44); the step-2 stitching test keeps its assertion and gets an entry deep enough to have a frontier at depth 3 | the policy and context snapshots: every change is a required-method leaf that became an `Unresolved` edge, and nothing else moves; the `open_*` counts of every snapshot are unchanged; the oracle's counts pinned again; a default method reached through `dyn` stays a `Call` |
| 10 | `languages/rust/docs/policy.md`, one fixture named per row; `ENGINE.md` | reviewed |
| | **tag `rust-v0.2.0`** | |

Step 3 is where the design of this plan is tested for the first time: if `Open` cannot be computed from what the engine returns
(the receiver's type is not inspectable the way [`../01`](../01-what-rust-analyzer-gives.md) §7 assumes), stop and fix the plan before
step 4, because layer 2's order of work rests on those counters.

Next: [`06-rs5-driver-needs.md`](06-rs5-driver-needs.md)
