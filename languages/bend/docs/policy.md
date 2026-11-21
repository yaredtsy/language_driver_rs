# Bend: the policy table

What the analyzer draws, and what it does not, for each feature of the language: the default resolver, layer 1 (BD4). **Every row names a
fixture** and the entries built from it. Changing a row is a policy change: the fixture and its snapshot change in the same commit, and the
version takes a minor step (`docs/01-monorepo/06-versions-and-vnoc.md`).

The plan is `docs/08-bend/03-phase-2-defs-and-calls.md` (§3.2 the walk, §3.3 this table) and the draft rows BD-D5, BD-D8, BD-D9, BD-D12 of
`docs/08-bend/05-decisions-and-questions.md`. This file says what was built (BD4 step 4, 0adfc30, 8059bd9, 8e77a0c, reviewed 2026-11-20).
Written from the 27 snapshots, which are what the code does when this file and they disagree.

**Where a row is checked.** The last cell of a row is read by a test (`crates/bend-analysis/tests/policy.rs`, BD4 step 5; Rust's
`rust-analysis/tests/policy.rs` is the model), so it holds nothing but its words in backticks, in this grammar (the test reads the rows of
this first table only, from its `| Row |` header to the first line that is not a row; the counters table at the end is not read):

- **groups** separated by `; `;
- a group is a **fixture folder** under `crates/bend-analysis/tests/fixtures/policy/`, which is also the root the book is opened at, then
  **one file** of it (relative to that root), then **the entries** in that file (qualnames; `call_graph` takes a qualname, and when a law
  and its proof share one the proof is the entry, so no `#n` form is needed or read);
- a group that begins with a file keeps the folder of the group before it.

The test fails on a row with no fixture, a folder or file that is not there, a file with no entry, a row without its snapshot
`crates/bend-analysis/tests/snapshots/policy/<row>.txt` and a snapshot of no row. A snapshot is, for each entry, a heading
`== <file> <entry>`, the tree (the project's defs; `[Passed]` and `[Recursive]` are the renderer's marks), then every edge of the graph as
`from | callee text | what it is` (sorted by the `from` def's name, byte order; by (start, end) of the site within one def; the callee text
is the site's text: an operator's site is the operator token, `+`; an index's the whole `a[i]`; a do statement's the statement, `x : U32 <- item()`), then `counters: key=n, …` above 0 (`Stats.language`,
the book's counters merged in, so `type_position` shows on every row: every walked def has a return type). Write the expected text by hand
before running; bless only to write a snapshot you then review.

## How a callee is found

Four steps, Bend's own (`parse_var`, `parse_reso`; `docs/08-bend/01-what-bend-is.md` §3), over the whole book with declaration order ignored
(BD-D12: a forward reference `bend` would refuse still gets its edge):

1. **a binder in scope wins**: a parameter (template or not), a `let` pattern's name, a lambda's binder, a `case` pattern's variable, a `do`
   bind or let name, a rewrite's named equation, an `@`/`&` binder. A dotted name is never a binder. The call is `Unresolved(function_typed_value)`.
2. **an alias**: `M.f` is `f` of the file imported `as M`.
3. **this file's own** declaration.
4. **Base**: `External`, named as written.

A name none of the four knows is `Unresolved(unknown_name)`. Every resolved edge rests on `Basis::Name`: the code's own names fixed it.

## The rows

| Row | Bend | Edge | Counter | Where it is checked |
|---|---|---|---|---|
| `direct` | `f(1)` to a def declared above | `Call` | | `direct` `main.bend` `main` |
| `gpu` | `f!(1)` | `Call` (the `!` changes where it runs, not what) | | `gpu` `main.bend` `main` |
| `recursion` | `count(n)` inside `count` | `Call` to itself; the renderer marks it `[Recursive]` | | `recursion` `main.bend` `count` |
| `below` | `g()` in an `@unsafe def` where `g` is declared below | `Call` (order ignored, BD-D12) | | `below` `main.bend` `main` |
| `alias` | `M.f(1)` with `import ./lib.bend as M` | `Call` to `f` of `lib.bend` | | `alias` `main.bend` `main` |
| `no-shadow` | `def Nat.add` in a file where Base has it: `bend` refuses the declaration (`parse_fresh`); we outline it and `Nat.add(1n, 2n)` resolves to **this file's** def | `Call` to this file's | `duplicate_declaration` (the book's, once per declaration) | `no-shadow` `main.bend` `main` |
| `binder-wins` | `f(1)` where `f` is a parameter and a def `f` exists | `Unresolved` (step 1 before step 3) | `function_typed_value` | `binder-wins` `main.bend` `main` |
| `base` | `IO.print(U32.show(1))` | `External("IO.print")`, `External("U32.show")`, module `Base` | | `base` `main.bend` `main` |
| `hub` | `P.f(1)` with `import 0x…/main.bend as P`: nothing fetched, nothing read from disk | `External("P.f")`, module `P` | `hub_import`, once per import line | `hub` `main.bend` `main` |
| `foreign` | a call to a def whose body is `import "./leaf.c"` | `Call` to the leaf: it is declared here, its code is not ours to read | | `foreign` `main.bend` `main` |
| `variable` | `f(…)` with `f` a parameter, `g(…)` with `g` a `let`, `k(…)` with `k` a `case` variable | `Unresolved` each | `function_typed_value` | `variable` `main.bend` `main` |
| `closure-call` | `adder(2)(add2(1))` after `add2 = adder(2)` | the two `adder(2)` are `Call adder`; the outer call (its callee is a call) and `add2(…)` are `Unresolved` | `function_typed_value` | `closure-call` `main.bend` `main` |
| `lambda-inline` | `(x => g(x))(1)` | `Unresolved` for the call of the lambda; `g` is a `Call` **from the enclosing def** (a lambda's calls are its def's, phase 1) | `function_typed_value` | `lambda-inline` `main.bend` `main` |
| `template-arg` | `twice(~(x => f(x)), 40)`, `twice(~g, a)`; inside `twice`, `h(h(x))` with `~h` | `Call twice` twice; `f` a `Call` from `main`; `g` a **passed** edge (D22); `twice`'s two `h(…)` are `Unresolved` (BC1 resolves them per caller) | `function_typed_value` | `template-arg` `main.bend` `main` |
| `passed` | `h(g)` with `g` a def; `App.run(~U32, ~App{view, tick})` | `g`, `view`, `tick`: passed edges (a bare def name as a direct argument, or as a direct field of a constructor argument); `App.run` an `External`; `h`'s own `f(1)` is `Unresolved`. A def name deeper in an argument is **not** drawn | `function_typed_value`; `passed_deep` for the deeper ones; `type_passed` for a project type handed over (a Base type such as `~U32` is quiet: the row's snapshot has none) | `passed` `main.bend` `main` |
| `construct` | `Circle{5}` of a project `type Shape` | `Construct Shape`, no init (a type has no constructor function: a leaf) | | `construct` `main.bend` `main` |
| `construct-base` | `Some{x}`, `Con{h, t}`, `[1, 2]`, `"s"`, `3n+p`, `(a, b)` | nothing: data, constructors of Base types underneath | | `construct-base` `main.bend` `main` |
| `op-project` | `(a + b : Vec2)` with `def Vec2.add` in the project | `Call Vec2.add`; the site is the operator token `+`, where Bend's own `Ref` sits (step 5.1: 11,399 operator Refs of the demos at exactly our bytes) | | `op-project` `main.bend` `main` |
| `op-base` | `(a + b : U32)`, `(a < b : U32)`, `x && y`, `s ++ t`, `h <> l` | nothing: Base's arithmetic and the fixed operators are the language's, not sites (BD-D8); a Base call written out (`U32.add(a, b)`) is still an `External` | `sugar_base`, one per operator | `op-base` `main.bend` `main` |
| `index` | `a[i]`, `a[i] <- v` | nothing (Base's `Array.get` / `Array.set`); the site is the whole `a[i]`, as Bend's `Ref` is (46 of 46 demo indexes equal) | `sugar_base` | `index` `main.bend` `main` |
| `do-project` | `do Parser<U32>:` with `def Parser.bind`, `def Parser.pure` in the project | `Call Parser.bind` per bind, exec or step (the site is the statement), `Call Parser.pure` per `return`; the statement's own call (`item()`) is drawn too, before its bind (sites sort by start) | | `do-project` `main.bend` `main` |
| `passed-do` | `x : String <- name`, `Unit <- pop`, a step `tick`, `return done`, in a `do` of a project monad, with `name`, `pop`, `tick`, `done` defs | `Call Parser.bind` for each statement as in `do-project`, and `name`, `pop`, `tick`: passed edges (D22) at the name's span, a bare def name being a direct argument of the sugar's `M.bind`; a step's statement and its name share a span, the bind first | | `passed-do` `main.bend` `main` |
| `do-base` | `do IO<Unit>:`, `do Maybe<U32>:` | nothing for the monad; a Base call inside a step (`IO.print("a")`) is still its `External` | `sugar_base`, one per statement | `do-base` `main.bend` `main` `maybe` |
| `rewrite` | `%add_zero(p) : Nat` | `Call add_zero`: proof code calls proof code | | `rewrite` `main.bend` `main` |
| `type-position` | `-> Or(IsEven(n), Empty)`, `{Unit{} : IsEven(n)}`, `List<Nat>`: calls and constructors inside a type | nothing, in layer 1 (BD-D9): parameter and return types, annotations' right sides, type arguments, a `do`'s monad arguments, bind and let types, `@` domains, a rewrite's motive, a law's claim | `type_position`, one per skipped term | `type-position` `main.bend` `main` |
| `law-call` | `add_zero(2n)` where both `law add_zero` and its proof `def add_zero(x)` exist | `Call` to the **proof** (the def with code, `add_zero#1`); the law gets no edge. An entry named `add_zero` is the proof | | `law-call` `main.bend` `main` |
| `law-open` | `f(2n)` where only `law f` exists | `Call` to the law: a leaf, the only declaration | `open_law` | `law-open` `main.bend` `main` |
| `unknown` | `nope(1)` | `Unresolved` | `unknown_name` | `unknown` `main.bend` `main` |

Rows the owner may reopen after seeing real trees: `op-base`, `index`, `do-base` (Q-BD4: show Base's sugar as `External` edges?) and
`type-position` (Q-BD5: a proof's type-level calls). Either flip changes a row and its snapshot, not a qualname or the wire.

## What is not a site

| Form | Why |
|---|---|
| a `let`, a `match`, a `case` pattern, a lambda's binder | binders; patterns bind, they do not call |
| `Nil{}`, a literal, a list, a tuple, `3n+p` | data (row `construct-base`) |
| `&&` `\|\|` `++` `<>` `&` `\|` `<&>` `->` | fixed operators: the language's (counted `sugar_base` with the rest of the sugar) |
| an operator with no `: T` | `bend` refuses it; we draw `Unresolved(unknown_name)` |
| a project type handed over (`f(Shape)`) | no edge, `type_passed`. A Base type (`~U32`) is quiet, as every Base name handed over is |
| a def outside the root handed over | no edge, `external_passed` (D33) |
| a def's parameter and return types; a law's claim | type positions |

## The counters

`Stats.language`, all of them, and the row that shows each:

| Key | What | Row |
|---|---|---|
| `function_typed_value` | a call through a binder, a call result or a lambda written in place | `variable`, `closure-call`, `lambda-inline`, `template-arg`, `binder-wins` |
| `unknown_name` | a name none of the four steps knows; an operator with no `: T` | `unknown` |
| `unknown_constructor` | `K{…}` of no known type | (none in the table: a fixture when it is first seen) |
| `type_position` | a term inside a type, not walked | every row |
| `sugar_base` | sugar that names a Base def | `op-base`, `index`, `do-base` |
| `passed_deep` | a def name deeper than a direct argument or a direct constructor field | `passed` |
| `type_passed` | a project type handed over | the precise row `limit-bound-type` |
| `external_passed` | a def outside the root handed over | (none in the table) |
| `open_law` | a call to a law with no proof | `law-open` |
| `outside_root` | a call into a file the root does not hold (D33) | (the book fixture `outside_root`) |
| `hub_import`, `import_missing`, `import_cycle`, `duplicate_declaration`, `import_no_alias` | the book's (BD3) | `hub`, `no-shadow` |

## Scenarios

The wire's `target_kind` is the outline's `DefKind` as core's wire writes it (`"Function"`, `"Enum"`, `{"Other":"law"}`); a marked type gets
`class_prefix` as an entry and as a construction (D45). `bend-cli outline` prints line:column and the marker text, as the other CLIs do.

`scenarios/*/bend/`, run by `tests/scenarios.rs` at `Level::Default`: `straight-line`, `recursion-direct`, `external-call`, `method-on-class`
and `law-across-files` give the shared tree; `construct-marked-class` and `two-callers` have a Bend tree of their own (`expected.bend.tree`,
`expected.default.bend.tree`) because Bend has no constructor function and no receiver to dispatch on; each scenario's README says why.

## The precise rows (the context layer)

**Designed 2026-11-21, not built yet** (`docs/08-bend/06-context-layer-design.md`, BC1 + BC2 as one step, D51 and D52). The rows above are the
default level and do not change. These are what `Options::precision = Precise` draws: a def's parameter that a caller named (a `~` template or
a plain function-typed one) is called as that def, per caller, and the key flows down through a caller's own parameter passed along. The
last cell is read by `crates/bend-analysis/tests/context.rs` in the grammar of "Where a row is checked", with the fixtures under
`crates/bend-analysis/tests/fixtures/context/` and one snapshot per row, `crates/bend-analysis/tests/snapshots/context/<row>.txt`, which holds a
`default` and a `precise` section for each entry. `def@{p ↦ v}` below is a context of `def` whose key has `v` at parameter `p`. The `limit-*`
rows are the known limits of the layer: both sections are equal, but for the `key_slot_*` counter that says why (only `Precise` counts it).

| Precise row | Bend | Default | Precise | Counter | Where it is checked |
|---|---|---|---|---|---|
| `template-def` | `twice(~g, a)`; inside `twice`, `h(h(x))` with `~h` | `twice`'s two `h` are `Unresolved`; `g [Passed]` | two `Call g` from `twice@{h ↦ g}`; `g [Passed]` stays | | `template-def` `main.bend` `main` |
| `template-lambda` | `twice(~(x => f(x)), 40)` | `f` is a `Call` from `main`; `twice`'s `h` are `Unresolved` | the same tree; `twice@{h ↦ lambda 0 of main}`'s two `h` are `Unresolved(anonymous_function_called)` | `anonymous_function_called` | `template-lambda` `main.bend` `main` |
| `template-flow` | `outer(~h, x)` calls `middle(~h, x)` calls `twice(~h, x)`; `main` calls `outer(~g, 1)` | `g [Passed]` under `main`; the chain's `twice` is a leaf | `main` ▸ `outer` ▸ `middle` ▸ `twice` ▸ `g` · `g`; each context's key is `{h ↦ g}` (the demos' 15 of 16) | | `template-flow` `main.bend` `main` |
| `template-type` | `blk(~S: Data, ~at: S -> U32, s: S)` called `blk(~U32, ~sky, 1)`; inside, `at(s)` | `at(s)` `Unresolved` | `blk@{at ↦ sky}` calls `sky`; `S` is in no key, so the digest is the same with any type. `U32` is Base's and quiet; a project type there counts `type_passed` as in layer 1 (the plain test `a_type_is_in_no_key`) | | `template-type` `main.bend` `main` |
| `param-def` | `each(f: U32 -> U32, x)` calls `f(x)`; `main` calls `each(work, 1)` | `each` a leaf; `work [Passed]` | `each@{f ↦ work}` ▸ `work`; `work [Passed]` stays | | `param-def` `main.bend` `main` |
| `param-flow` | `relay(f, x)` calls `each(f, x)`; `main` calls `relay(work, 1)` | `relay` ▸ `each` (a leaf); `work [Passed]` under `main` | `relay@{f ↦ work}` ▸ `each@{f ↦ work}` ▸ `work`; no passed edge inside `relay` (BC-D4) | | `param-flow` `main.bend` `main` |
| `param-lambda-captures` | `outer(~h, x)` calls `each(y => h(y), x)`; `main` calls `outer(~g, 1)` | `outer`'s `h(y)` `Unresolved`; `each` a leaf | `outer@{h ↦ g}` ▸ `each` · `g`: the lambda's call is `outer`'s, resolved by `outer`'s key; `each@{f ↦ lambda 0 of outer}` says `anonymous_function_called` | `anonymous_function_called` | `param-lambda-captures` `main.bend` `main` |
| `two-keys` | `each(work, 1)`, `each(other, 2)`, `each(work, 3)` | one `each`, three times | two contexts of `each` with two digests, reached by three edges: the third call reaches the first context again; each `each` node shows its own callee | | `two-keys` `main.bend` `main` |
| `param-through-param` | `apply(h, f, x)` is `h(f, x)`; `main` calls `apply(each, work, 1)` | `apply` a leaf, its `h(f, x)` `Unresolved`; `each [Passed]`, `work [Passed]` | `apply@{h ↦ each, f ↦ work}` ▸ `each@{f ↦ work}` ▸ `work`: `f` is relevant in `apply` as an argument of a call through `apply`'s own parameter (§3.2 c); the two passed edges stay, `each` under the open key a leaf | | `param-through-param` `main.bend` `main` |
| `not-relevant` | `log(f: U32 -> U32, x)` never uses `f`; `log(work, 1)`, `log(other, 2)` | one `log` context | **one** `log` context, digest zeros: the slot is not relevant (§3.2) | | `not-relevant` `main.bend` `main` |
| `entry-open` | the entry is `twice` itself | its `h` are `Unresolved` | the same: the entry is the open key (BC-D7) | `function_typed_value` | `entry-open` `main.bend` `twice` |
| `recursion-keyed` | `f(~g, n)` is `g(f(~g, n))`; `r(~g, n)` is `r(~(y => g(y)), n)`; `main` calls both with `~work` | `f`, `r` `[Recursive]`; their `g` `Unresolved` | `f` ▸ `work` · `f [Recursive]`; `r` ▸ `r [Recursive]` · `work`. The graph has three contexts of the two: `f@{g ↦ work}`, and `r@{g ↦ work}` → `r@{g ↦ lambda 0 of r}` → itself (the lambda key reaches itself: finite, rule 3), where `g(y)` is `anonymous_function_called`. The tree marks the second `r` `[Recursive]` because the renderer marks recursion by function, not by context (tree rule 8) | `anonymous_function_called` | `recursion-keyed` `main.bend` `main` |
| `passed-open` | `h(g)` with `g` a def that has a function-typed parameter; and `each(work, 1)` | `g [Passed]`, `work [Passed]` | `g` is walked under the open key (its own parameter site stays `Unresolved`); `work` has **one** context, reached as passed and as called | `function_typed_value` | `passed-open` `main.bend` `main` |
| `limit-partial-application` | `drop(n, k, s)` calls `k(…)`; a `do P<Unit>:` of a project monad has `Unit <- drop(2n)` | `drop`'s `k` `Unresolved` | the same: `k` is never passed (10 of the demos' 32 parameter sites, `pure_hvm5_mini`) | `key_slot_missing` | `limit-partial-application` `main.bend` `main` |
| `limit-handed-to-bind` | `peek(k, s)` calls `k(…)`; `h : Hc <- peek` in a `do P<…>:` | `peek [Passed]`; its `k` `Unresolved` | the same: the sugar's `P.bind` gets the open key (BC-D5), and `P.bind` applies `peek` to a lambda's binder | `function_typed_value` | `limit-handed-to-bind` `main.bend` `main` |
| `limit-lambda-binder` | `pure(x)` is `k => s => k(x, s)` | `k(x, s)` `Unresolved` | the same: `k` is a lambda's own binder, its value comes from whoever calls the lambda | `function_typed_value` | `limit-lambda-binder` `main.bend` `main` |
| `limit-bound-type` | `g(-T: Data, a: T, b: T)` is `(a + b : T)`; `g(Vec2, p, q)` | `Sugar::Bound`: `Unresolved` | the same: a type is not a Known (0 sites in the corpus) | `function_typed_value`, `type_passed` | `limit-bound-type` `main.bend` `main` |
| `limit-constructor-of-defs` | `run(~app: App)` matches `case App{v, t}: t(s)`; `main` calls `run(~App{view, tick}, 0)` | `view`, `tick` `[Passed]`; `t(s)` `Unresolved` | the same: a constructor is not a Known, and `app` is not called, so nothing is counted for it (BC5: 3 demo sites) | `function_typed_value` | `limit-constructor-of-defs` `main.bend` `main` |
| `limit-base-def` | `each(U32.show, 1)` | `each` a leaf | the same: a Base def is not a Known | `key_slot_value` | `limit-base-def` `main.bend` `main` |
| `limit-local` | `g = work` then `each(g, 1)` | `each` a leaf | the same: a `let` is BC3's | `function_typed_value`, `key_slot_local`, and layer 1's `passed_deep` for the `work` in the `let` | `limit-local` `main.bend` `main` |
| `limit-open-parameter` | `relay(f, x)` is `each(f, x)`, and the entry is `relay` | `each` a leaf, its `f(x)` `Unresolved` | the same: `f` is `relay`'s own parameter and nobody named it (the entry is the open key, BC-D7) | `function_typed_value`, `key_slot_open` | `limit-open-parameter` `main.bend` `relay` |

The counters the layer adds to `Stats.language`: `anonymous_function_called` (the reason of a call through a parameter known to be a lambda: its
calls are drawn where the lambda is written), and, per context walked, for a parameter **the callee calls** that the caller did not name: `key_slot_open` (the argument
is the caller's own parameter and the caller's key does not know it), `key_slot_local` (a `let`, a `case` variable, a lambda's binder),
`key_slot_value` (a call's result, a constructor, a literal, a Base def), `key_slot_missing` (no argument at that position).

## Known limits, each a fixture

- a bare project def as the value of a `do` bind, exec, step or `return` is a passed edge since 85aee89 and 55368ce (row `passed-do`; the
  demos' `passed_deep` fell from 28 to 15, the rest being type names in value position, which owe nothing under BD-D9).
- the sites the context layer would resolve, measured (BC0, `ENGINE.md`): 55 of the demos' 4,323 call sites; what it would not: partial
  application in a CPS monad and a lambda's own binder (`docs/08-bend/04` §6).
- a closure called after being stored (`add2(1)`) and a closure returned by a call (`adder(2)(…)`) are unresolved: BC3 (values through lets
  and returns).
- a template parameter called inside its def is unresolved for every caller: BC1 resolves it per call site, exactly.
