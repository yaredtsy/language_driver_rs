# 08.06 The context layer: the design (BC1 + BC2, one step)

```
the design
├── 1. one program, before and after
├── 2. Known: what a slot can hold                        (BC-D1)
├── 3. the key: positions, relevance, the open key, the digest  (BC-D2)
├── 4. flow-down: how a key is built at a call site       (BC-D3, BC-D4, BC-D5)
├── 5. answering a site
├── 6. a lambda as Known stays with the def that writes it   (BC-D6; Q-TC1, Q-RC1)
├── 7. the rules
├── 8. what changes, and what does not
├── 9. the precise rows (the table `tests/context.rs` reads)
├── 10. the limits, each a fixture
├── 11. scenarios: the two precise trees
├── 12. the proof
├── 13. steps for the coder, one commit each
└── 14. the decision rows
```

Written 2026-11-21 from BC0's numbers (`languages/bend/ENGINE.md` §BC0), under D51: the owner delegated Bend's decisions to the reviewer, and
this file **replaces §3 and §4 of [`04-phase-3-context-layer.md`](04-phase-3-context-layer.md)**, which were written before anything was
measured. It is aligned with TypeScript's design ([`../03-typescript/12-context-layer/03-design.md`](../03-typescript/12-context-layer/03-design.md))
and Rust's ([`../04-rust/05-context-layer/01-design.md`](../04-rust/05-context-layer/01-design.md)) in the shapes that must agree across
languages (the open key is zeros, keys are closed, a lambda's calls stay with its writer, precision is an option, the digest is Python's
scheme), and smaller than both everywhere else, because Bend has no mutation, no fields that change, no receivers, no dispatch. **Nothing
here was built when it was written**; §12 is the proof and §13 the order.

The size of the prize, from BC0, so nobody expects more: 55 `function_typed_value` sites in the demos' 4,323 call sites; this step names the
function for **28** of them (16 template parameters, exact by the compiler's own rule; 12 plain parameters), 0.65 points of all call sites.
The other 27 are listed in §10, one fixture each.

## 1. One program, before and after

The layer-1 fixture `template-arg` (`languages/bend/docs/policy.md`), as it is drawn today and as this step draws it:

```bend
def twice(~h: U32 -> U32, x: U32) -> U32:
  h(h(x))

def f(x: U32) -> U32:
  x

def g(x: U32) -> U32:
  x

def main() -> U32:
  a = twice(~(x => f(x)), 40)
  twice(~g, a)
```

| Layer 1 (`Precision::Default`, today) | This step (`Precision::Precise`) |
|---|---|
| `main` ▸ `twice` · `f` · `twice` · `g [Passed]` | `main` ▸ `twice` · `f` · `twice` ▸ `g` · `g` · `g [Passed]` |
| one context of `twice`; its two `h(…)` are `Unresolved(function_typed_value)` | two contexts of `twice`: under `{h ↦ g}` both `h(…)` are `Call g`; under `{h ↦ lambda 0 of main}` both are `Unresolved(anonymous_function_called)`: known, drawn where the lambda is written (§6) |
| `f` is a call **from `main`** (the lambda's calls are its writer's) | the same, unchanged |
| `g [Passed]` | the same: a def handed over is still handed over (D22) |

Nothing a reader saw in layer 1 is gone: every edge of the left column is in the right one, and the only sites that changed had no target.

## 2. Known: what a slot can hold

```rust
// key.rs
/// What a caller is known to have put in a function-shaped parameter. Closed: it names no variable of any def.
pub(crate) enum Known {
    /// A def of the book with code (a function, or a law's proof; a law alone is a leaf, as `project()` makes it).
    Def(DeclId),
    /// A lambda (`x => …`, `\{…}`) written in `writer`'s body: the `nth` one the walk enters there, in source order.
    Lambda { writer: DeclId, nth: u32 },
}
```

**BC-D1: a Known is a def or a lambda, and nothing else.** BC0 found template arguments that are neither: a nullary def *called* (`~World()`,
`~Walkers.game()`) and a constructor of defs (`~App{Fly.view, Fly.tick}`). They are not admitted, on purpose:

- `World()` is a **type** (`def World() -> Data`); a type never decides a call target in this step (`Sugar::Bound`, the one place it could, occurs
  0 times in 1,744 files). Types are kept out of the key the same way whether written `~U32`, `~S` or `~World()`.
- `Walkers.game()` and `App{…}` are **values that hold functions**. Nothing in this step reads a field or a call's result (BC3, BC5: not built, 3 + 3
  demo sites). A slot holding one would make a context whose edges equal the open key's: a context for nothing. When BC5 is built, `Known` gains a
  variant; the enum is crate-private and the digest's canonical form skips what is empty, so nothing moves.
- Base's defs (`each(U32.show, 1)`), a hub's, a def outside the root: not Known. Calling them inside the callee would be an `External`; it is
  correct but unmeasured, so it is a listed limit (§10), not a guess.

So the plan's §1 sentence "`Known` must hold a closed term (a def, a lambda, a call, a constructor)" is **corrected** here: a Known is one of two
things; the other closed terms are counted (`key_slot_value`, §4) where they land in a relevant slot, so the count says when BC5 pays.

## 3. The key

```rust
// key.rs: today `pub(crate) type Key = ();`
/// What a context holds besides the def: the parameters a caller named, by position. Sorted by position; only relevant positions (§3.2);
/// never an unknown slot. Empty is the open key.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    slots: Box<[(u32, Known)]>,
}
```

> **🦀 Rust note: a sorted slice instead of a `BTreeMap`.** A key is built once, then compared, hashed (the solver's memo is a
> `HashMap<(Func, Key), Summary>`) and cloned. A `Box<[(u32, Known)]>` is one pointer and one length, `#[derive(Hash, Eq)]` works on it,
> and two keys with the same slots in the same order are equal: sorting by position at construction is what makes the order canonical.
> A `BTreeMap` would give the same guarantees for more words and an allocation per entry.

### 3.1 Positions

A parameter is named by its **position in the def's written parameter list**: the `~` templates first (Bend's grammar puts them first, G13),
then the rest, erased `-A: Type` ones included. Erased parameters are passed explicitly at a call site (`P.pure(Unit, Unit{})` for
`def P.pure(-A: Type, x: A)`), so **argument `i` is parameter `i`** with no mapping. A call with fewer arguments than parameters leaves the rest
missing (partial application, §10); extra arguments bind nothing.

### 3.2 Relevant positions

**BC-D2.** Position `i` of def `g` is **relevant** when, in `g`'s own body (its walk items alone, so it is a fact of the code, computed once per
def per request and memoized in `Bend`), parameter `i` is

- **(a)** the callee of a site (`What::Opaque { param: Some(i) }`: `f(x)`, `f!(x)`, `f(a)(b)`'s inner call), or
- **(b)** a direct argument (`~` or plain, through parentheses) of a call whose callee name the book resolves to a project def: passed along
  (the flow-down of §4).

Not relevant: a parameter handed into a constructor field, into a `do` statement, into a sugar's operand, used as a type, or never used. A slot
that is not relevant is never in a key, so `def log(f: U32 -> U32, x: U32) -> U32: x` has one context however many callers name an `f`, and its
context id is layer 1's. (b) over-approximates Rust's fixpoint (RC1 §5): a parameter passed into an irrelevant slot of the callee still counts.
That costs a context with the same edges, never an edge, and needs no fixpoint. Measured on the demos (§12) before anything finer is considered.

### 3.3 The open key, the digest, closed keys

- **The open key** is the empty `slots`. Its digest is **zeros**, so every context of layer 1 keeps its id until a caller names something, and the
  entry of a request is always the open key (BC-D7, §5).
- **Keys are closed.** A `Known` names a def or a lambda by identity and nothing of the caller. What a caller does not know, it does not pass
  (the slot is absent), so a key means the same thing wherever the solver meets it (rule 1, §7).
- **The digest** is Python's scheme (`interp/canon.rs`), the one TypeScript's and Rust's designs name too: the key written in a canonical form that
  holds only **stable ids**, serialized, hashed with `xxh3_128`, the 16 bytes little-endian.

```rust
#[derive(Serialize)]
enum Canon { Def(StableDef), Lambda(StableDef, u32) }          // StableDef: the id `describe` gives the def (marker, or path + qualname + nth)
#[derive(Serialize)]
struct CanonKey { slots: Vec<(u32, Canon)> }                   // in position order; a later variant adds a field with `skip_serializing_if`
```

Never `#[derive(Hash)]`, a `DeclId` (an index into this request's book) or a map order (rule 9). `xxhash-rust` (feature `xxh3`, Python's
pin) becomes a dependency of `bend-analysis`, and `serde` (derive) and `serde_json` move from its dev-dependencies to its dependencies; both
are workspace crates already.

## 4. Flow-down: how a key is built at a call site

At every site whose target is a project def `g` (a name resolved by the book, or a parameter the env knows, §5), the walk of `f` under key `K`
builds `g`'s key from `g`'s relevant positions and the arguments as `sem` read them:

| The argument at a relevant position `i` is | The slot | Counted (per context, `Stats.language`) |
|---|---|---|
| a bare unbound name the book resolves to a project def with code, in the root | `Known::Def(d)` | |
| a lambda (`x => …`, `\{…}`), through parentheses | `Known::Lambda { writer: f, nth }` | |
| a bare name bound to **`f`'s own parameter `j`**, not shadowed | **`K[j]`** if `K` has it; else absent | `key_slot_open` when absent |
| a bare unbound name that is a `type`, a Base def, a hub's, a def outside the root | absent (a type is already `type_passed`) | `key_slot_value` (not for a type) |
| a name bound to a `let`, a `case` pattern, a lambda's binder, a `do` name, an `@` binder | absent | `key_slot_local` |
| a call, a constructor, a literal, an operator, anything else | absent | `key_slot_value` |
| no argument at that position | absent | `key_slot_missing` |

**BC-D3: the key flows down from the first commit.** The third row is the whole point: 330 of the demos' 387 `~` arguments are the caller's own
`~` parameter passed along (`Grid.go(~F, ~cam, ~unit, ~shade, …)` → `Grid.strip(~F, ~cam, ~unit, ~shade, …)` → `Mesh.vert(…)` → `Surf.at(~unit, …)`
→ `Unit.go(~unit, …)` in `app_slash_boss_3d`), and 15 of the 16 template sites sit at the bottom of such a chain. A key made only where a def name is
written resolves 1 of 16. The rule costs one lookup: the argument is `f`'s parameter `j`, so the value is whatever `f`'s own key says for `j`. A
chain of any length needs no special code: each def evaluates its arguments where its own parameters already have values.

**BC-D4: a passed edge's target is walked under the open key.** `h(g)` draws `g [Passed]` from the def that names `g` (D22, unchanged), and the solver
walks `g` as `(g, open)`: nobody knows what the receiver will pass it. A parameter passed along (`each(f, x)` inside `relay(f, x)`) draws **no**
passed edge: the text names no def there, and the value is drawn where it is named (`work [Passed]` under `main`) and where it is called
(`work` under `each`). TypeScript's design §2 line b draws `relay` the same way.

**BC-D5: a sugar's target gets the open key.** `(a + b : Vec2)` → `Vec2.add`, `a[i]`, `do Parser<…>:` → `Parser.bind` / `Parser.pure`: the compiler
places the operands and the continuation; mapping them onto `Vec2.add`'s or `Parser.bind`'s positions is its business, and no demo site needs it
(class e is 0 in 1,744 files; the `P.bind` sites are §10's limit). The `passed-do` edges stay as they are.

A `Construct` has no key (a type has no code). An `External` and an `Unresolved` have none.

## 5. Answering a site

`walk::resolve` keeps the form it has had since the first day: ask the env first, the book otherwise. Today's `resolve_from_env` returns `None`;
this step makes it answer **one** case:

```
resolve_from_env(lang, f, item, env):
    lang.precision is Default                         → None                       (rule 7: off is identical)
    item is not Opaque { param: Some(i), args }       → None                       (a name the book fixes is never overridden: rule 4)
    env.known(i) is
        None                                          → None                       (the book says Unresolved(function_typed_value), as today)
        Some(Known::Def(d))                           → Site { Call(d, key_of(d, args, env)) }    the callee's own key, from the same arguments
        Some(Known::Lambda { .. })                    → Site { Unresolved(anonymous_function_called) }   §6
```

`env` is the context's key seen as a map: `Env<'k> { key: &'k Key }`, `fn known(&self, position: u32) -> Option<&Known>`. Bend has no `let` of a
function to track inside one body in this step (BC3), so the env **is** the key; `env.rs` stays the one place a later phase would add locals.

`resolve_from_book` is unchanged except that every `Target::Call(id, ())` for a project def becomes `Target::Call(id, key)`: `key_of(id, args, env)`
for a `What::Call`, the open key for a `What::Passed` (BC-D4) and a `Sugar` (BC-D5). With `Precision::Default` every key is the open one and the
graph is layer 1's, byte for byte.

**BC-D7: the entry is the open key.** `twice` asked for by name shows its template sites unresolved, as layer 1 does: nobody told us what was
passed. A request that **continues a cut node** (tier 3's `Budget`, BD5) today names a def, which would start from the open key and give the
continued subtree a different answer from the whole tree. TypeScript's answer (Q-TC3, 03-design §10) is taken as it stands: `Session` keeps, for every
context a request listed, its digest and its canonical key (dropped at a `reload`); `Session::graph_from(&ContextId, budget, cancel)` turns the
canonical key back into a `Key` (each `StableDef` is looked up as an entry is: by marker, or by path, qualname and `nth`) and starts the solver
there; a digest it does not know, or a def that is gone, starts from the open key and counts `context_not_found`. What the driver sends is a
`vnoc`-side change that waits for Q-TC3; until then a node continued the old way is correct and less precise.

## 6. A lambda as Known stays with the def that writes it

**BC-D6.** The plan's §3 left this open ("decide with TC, not alone"). TypeScript's Q-TC1 and Rust's Q-RC1 ask the same question, and both designs
recommend **A**: an anonymous function's calls stay under the def it is written in; moving them under the function that calls it is a later phase
(TC7) designed once for every language in `02-tree-rules/`. Bend takes A, for the reasons that are stronger here than there:

| | A: stays with the writer (this step) | B: moves under the callee (later, TC7, all languages) |
|---|---|---|
| `twice(~(x => f(x)), 40)` in `main` | `main` ▸ `twice` · `f`; `twice`'s `h(…)` under that key are `anonymous_function_called` | `main` ▸ `twice` ▸ `f` |
| every call appears | exactly once, always | once, **if** a walked body calls the lambda. A lambda handed to Base (`List.map(~(x => …), xs)`: Base runs it) must stay with its writer anyway |
| what the lambda's own calls need | nothing new: a `~h` the lambda captures is **the writer's parameter**, and the writer's key already resolves it (`param-lambda-captures`, §9) | the captured values travel in the Known; `Lambda` gains an env |
| the tree's order | where the code is written | where it runs |
| cost | zero beyond the enum variant | a second kind of summary, and a pass that removes what moved |

Why a `Known::Lambda` exists at all under A: the context `(twice, {h ↦ lambda 0 of main})` is distinct from the open one, its sites say **why** they
have no target (`anonymous_function_called`: "known, drawn elsewhere", TypeScript's name for the same reason, kept so the three languages count the
same thing), and B adds to the variant later without a new one. The lambda's number is its order among the lambdas the walk enters in the writer's
body, as TypeScript numbers its arrows; a lambda written inside a type is not walked and not numbered.

## 7. The rules

The first three are pylspt's, as the two sibling designs restate them; the rest are this layer's. Each has a test (§12).

1. **A summary is a function of its key and the code.** Relevance, the key, the digest: none reads the caller or the order contexts were met in.
2. **Nothing request-local leaves a summary.** A `DeclId` lives in the key; what leaves the analyzer is a digest of stable ids.
3. **Precision is decided by the key and the code; a cost limit only cuts.** There is no widening in this step: a Known has no depth (no captures, no
   nesting), so a key cannot grow through recursion (`def r(~g, n): r(~(y => g(y)), n)` gives `{g ↦ lambda 0 of r}` and then the same key again:
   `[Recursive]`), and the number of contexts of a def is bounded by the Knowns of the book to the power of its relevant slots. Core's
   `Limits::max_contexts` is the backstop and cuts as it does today; no per-function cap is needed, and if TypeScript adds one to core, Bend takes it.
4. **A site the code fixes by name is never overridden.** `resolve_from_env` speaks only at `Opaque { param: Some(_) }`.
5. **Unknown means the book's answer, never fewer edges.** An absent slot gives exactly layer 1's target. A phase can replace `Unresolved` by a
   better answer; it cannot lose an edge.
6. **A set is complete, or it is absent.** A slot holds one Known: Bend passes one term per argument, there is no `c ? a : b` in a value and no
   reassignment, so the join of TypeScript's §5 has nothing to join. If BC3 ever needs a set, `Known` becomes one then.
7. **Off is identical.** With `Precision::Default` no line of the layer runs after the option is read; every graph, tree, wire, counter and snapshot
   is layer 1's.

## 8. What changes, and what does not

| Piece | Changes |
|---|---|
| `sem/walk.rs` | the scope stack records **which binder** a name is (`Some(i)`: the def's parameter `i`; `None`: a local), innermost wins; `What::Opaque { param: Option<u32>, args }`; `What::Call { name, args }` with `args: Vec<ArgShape>` (`Name(String)` an unbound bare name · `Param(u32)` · `Local` · `Lambda(u32)` · `Other`, each read through parentheses as `passable` does); lambdas numbered in the order the walk enters them. Owned data, as everything that leaves `sem` |
| `key.rs` | `Known`, `Key`, `Key::open`, `Canon`, `digest`, `relevant(lang, def)`, `key_of(lang, callee, args, env)` |
| `env.rs` | `Env<'k> { key: &'k Key }`, `known(position)` |
| `walk.rs` | `resolve_from_env` answers (§5); every project `Call` target carries a key; the `key_slot_*` counts |
| `calls.rs` | `Options::precision: Precision` (`#[non_exhaustive]`: `Default`, `Precise`; `Default` is the default value, BC-D8); `type Key = key::Key`; `digest`; `walk` builds the `Env` from the key; the start is `Key::open()`; `walked()` matches `Target::Call(callee, _)`; **the layer-1 notes (`type_position`, `sugar_base`, `passed_deep`, `type_passed`, `external_passed`, `open_law`, `outside_root`) are added once per def**, the first time it is walked, not once per context (TypeScript's §10); the `key_slot_*` counts are per context, since they are about keys; the unresolved reasons stay what the edges say |
| `session.rs` | keeps digest → canonical key for the contexts it listed; `graph_from` (BC-D7) |
| `lib.rs` | exports `Precision`; nothing else |
| `bend-cli` | `calls … --entry … --precise`; refused with `--all` (the sweep enters every def under the open key, so it would show nothing) |
| `tests/` | `tests/context.rs`, `tests/fixtures/context/<row>/`, `tests/snapshots/context/<row>.txt`; the scenarios test gains a precise subject; `tests/corpus.rs` gains the demos measure |
| **unchanged** | `bend-parser`, `bend-syntax`, the outline, qualnames, markers, `book.rs` (one lookup may be added for `graph_from`: a `StableDef` back to a `DeclId`), the renderer, the wire and its merge, the policy rows and snapshots of layer 1, the scenarios' default trees, **`core/`** (no trait change, no new `Cut`, no testkit change: the scenarios already pick a language's own tree per level) |

A snapshot of a context row (`tests/snapshots/context/<row>.txt`) holds, for each entry, **two sections**, `== <file> <entry> default` and
`== <file> <entry> precise`, each in the policy snapshot's grammar (tree, edges, counters), the `from` of an edge written `def@xxxxxxxx` (the first
eight hex digits of the digest) when its context is not the open one, and after the edges a `contexts:` block, one line per context, `def` and the
full digest in hex or `open`, sorted. The digest in a reviewed file is the cross-process determinism test: a run that produced different bytes
would fail it.

## 9. The precise rows

The same table stands in `languages/bend/docs/policy.md` under `## The precise rows (the context layer)` (written with this file, 2026-11-21),
and **that copy is the one `tests/context.rs` reads**, with the policy test's grammar (the table from its `| Precise row |` header to the first
non-row line; every fixture under `tests/fixtures/context/`, its snapshot under `tests/snapshots/context/`). The limits of §10 are rows of the
same table. A row that proves wrong while building is reported, and the reviewer changes both copies.

| Precise row | Bend | Default | Precise | Counter | Where it is checked |
|---|---|---|---|---|---|
| `template-def` | `twice(~g, a)`; inside `twice`, `h(h(x))` with `~h` | `twice`'s two `h` are `Unresolved`; `g [Passed]` | two `Call g` from `twice@{h ↦ g}`; `g [Passed]` stays | | `template-def` `main.bend` `main` |
| `template-lambda` | `twice(~(x => f(x)), 40)` | `f` is a `Call` from `main`; `twice`'s `h` are `Unresolved` | the same tree; `twice@{h ↦ lambda 0 of main}`'s two `h` are `Unresolved(anonymous_function_called)` | `anonymous_function_called` | `template-lambda` `main.bend` `main` |
| `template-flow` | `outer(~h, x)` calls `middle(~h, x)` calls `twice(~h, x)`; `main` calls `outer(~g, 1)` | `g [Passed]` under `main`; the chain's `twice` is a leaf | `main` ▸ `outer` ▸ `middle` ▸ `twice` ▸ `g` · `g`; each context's key is `{h ↦ g}` (the demos' 15 of 16) | | `template-flow` `main.bend` `main` |
| `template-type` | `blk(~S: Data, ~at: S -> U32, s: S)` called `blk(~U32, ~sky, 1)`; inside, `at(s)` | `at(s)` `Unresolved`; `type_passed` | `blk@{at ↦ sky}` calls `sky`; `S` is in no key, so the digest is the same with any type (`type_passed`, as today) | `type_passed` | `template-type` `main.bend` `main` |
| `param-def` | `each(f: U32 -> U32, x)` calls `f(x)`; `main` calls `each(work, 1)` | `each` a leaf; `work [Passed]` | `each@{f ↦ work}` ▸ `work`; `work [Passed]` stays | | `param-def` `main.bend` `main` |
| `param-flow` | `relay(f, x)` calls `each(f, x)`; `main` calls `relay(work, 1)` | `relay` ▸ `each` (a leaf); `work [Passed]` under `main` | `relay@{f ↦ work}` ▸ `each@{f ↦ work}` ▸ `work`; no passed edge inside `relay` (BC-D4) | | `param-flow` `main.bend` `main` |
| `param-lambda-captures` | `outer(~h, x)` calls `each(y => h(y), x)`; `main` calls `outer(~g, 1)` | `outer`'s `h(y)` `Unresolved`; `each` a leaf | `outer@{h ↦ g}` ▸ `each` · `g`: the lambda's call is `outer`'s, resolved by `outer`'s key; `each@{f ↦ lambda 0 of outer}` says `anonymous_function_called` | `anonymous_function_called` | `param-lambda-captures` `main.bend` `main` |
| `two-keys` | `each(work, 1)`, `each(other, 2)`, `each(work, 3)` | one `each`, three times | two contexts of `each` with two digests; the third call reuses the first (`reuse_hits`); each `each` node shows its own callee | | `two-keys` `main.bend` `main` |
| `not-relevant` | `log(f: U32 -> U32, x)` never uses `f`; `log(work, 1)`, `log(other, 2)` | one `log` context | **one** `log` context, digest zeros: the slot is not relevant (§3.2) | | `not-relevant` `main.bend` `main` |
| `entry-open` | the entry is `twice` itself | its `h` are `Unresolved` | the same: the entry is the open key (BC-D7) | `function_typed_value` | `entry-open` `main.bend` `twice` |
| `recursion-keyed` | `f(~g, n)` is `g(f(~g, n))`; `r(~g, n)` is `r(~(y => g(y)), n)`; `main` calls both with `~work` | `f`, `r` `[Recursive]`; their `g` `Unresolved` | `f@{g ↦ work}` ▸ `work` · `f [Recursive]`; `r@{g ↦ work}` ▸ `r@{g ↦ lambda 0 of r}` ▸ `r [Recursive]`, then `work` (the lambda's `g(y)` is `r`'s site, resolved by `r`'s key; under the lambda key it is `anonymous_function_called`, and the lambda key reaches itself: finite, rule 3) | `anonymous_function_called` | `recursion-keyed` `main.bend` `main` |
| `passed-open` | `h(g)` with `g` a def that has a function-typed parameter; and `each(work, 1)` | `g [Passed]`, `work [Passed]` | `g` is walked under the open key (its own parameter site stays `Unresolved`); `work` has **one** context, reached as passed and as called | `function_typed_value` | `passed-open` `main.bend` `main` |
| `limit-partial-application` | `drop(n, k, s)` calls `k(…)`; a `do P<Unit>:` of a project monad has `Unit <- drop(2n)` | `drop`'s `k` `Unresolved` | the same: `k` is never passed (10 of the demos' 32 parameter sites, `pure_hvm5_mini`) | `key_slot_missing` | `limit-partial-application` `main.bend` `main` |
| `limit-handed-to-bind` | `peek(k, s)` calls `k(…)`; `h : Hc <- peek` in a `do P<…>:` | `peek [Passed]`; its `k` `Unresolved` | the same: the sugar's `P.bind` gets the open key (BC-D5), and `P.bind` applies `peek` to a lambda's binder | `function_typed_value` | `limit-handed-to-bind` `main.bend` `main` |
| `limit-lambda-binder` | `pure(x)` is `k => s => k(x, s)` | `k(x, s)` `Unresolved` | the same: `k` is a lambda's own binder, its value comes from whoever calls the lambda | `function_typed_value` | `limit-lambda-binder` `main.bend` `main` |
| `limit-bound-type` | `g(-T: Data, a: T, b: T)` is `(a + b : T)`; `g(Vec2, p, q)` | `Sugar::Bound`: `Unresolved` | the same: a type is not a Known (0 sites in the corpus) | `function_typed_value` | `limit-bound-type` `main.bend` `main` |
| `limit-constructor-of-defs` | `run(~app: App)` matches `case App{v, t}: t(s)`; `main` calls `run(~App{view, tick}, 0)` | `view`, `tick` `[Passed]`; `t(s)` `Unresolved` | the same: a constructor is not a Known (BC5: 3 demo sites) | `key_slot_value` | `limit-constructor-of-defs` `main.bend` `main` |
| `limit-base-def` | `each(U32.show, 1)` | `each` a leaf | the same: a Base def is not a Known | `key_slot_value` | `limit-base-def` `main.bend` `main` |
| `limit-local` | `g = work` then `each(g, 1)` | `each` a leaf | the same: a `let` is BC3's | `key_slot_local` | `limit-local` `main.bend` `main` |

The layer-1 row `closure-call` (`adder(2)(1)`, `add2 = adder(2)`) is the limit for a call's result (class c, 3 demo sites): its default and
precise trees are equal, and it is not repeated here.

## 10. The limits, each a fixture

What the 27 demo sites this step does not answer are, by BC0's classes, and the row that pins each:

| Sites | What | Row |
|---|---|---|
| 10 | partial application in `pure_hvm5_mini`'s CPS parser: `Unit <- drop(2n)`, the continuation arrives through `P.bind` | `limit-partial-application` |
| 4 | a primitive handed to the monad's bind: `h : Hc <- peek`, `eof`, `pop` | `limit-handed-to-bind` |
| 3 | a caller passes a lambda's own binder | `limit-lambda-binder` |
| 3 | no caller in the book but the sugar (`P.bind`, `name.go`) | `limit-handed-to-bind` (the sugar's open key) |
| 3 | a call's result called, `f(a)(k)` | `closure-call` (layer 1) |
| 3 | a variable from a constructor field | `limit-constructor-of-defs` |
| 1 | a lambda's own binder called | `limit-lambda-binder` |

And two limits BC0 could not count, pinned anyway: a Base def in a slot (`limit-base-def`) and a type as the operator namespace
(`limit-bound-type`). Tracking an unfinished call through `M.bind` is a different mechanism from a key, for one program; it stays a limit.

## 11. Scenarios: the two precise trees

**`two-callers`** gets `expected.precise.bend.tree` (Bend's own, as its default tree is: no receiver, a template instead):

```
main
  read_json
    emit
      json_write
    json_write [Passed]
  read_csv
    emit
      csv_write
    csv_write [Passed]
```

**`callback-argument`** is new, the scenario the seam's list names for rule 3's anonymous-function row ([`../02-tree-rules/03-the-seam.md`](../02-tree-rules/03-the-seam.md)
§5). Bend writes the folder first: `README.md`, `bend/main.bend`, and the **shared** `expected.default.tree` and `expected.precise.tree`, which are the
trees TypeScript's design gives for the same program (03-design §2, lines a and i); Python's rule draws the lambda's call under `each` (B), so
its tree will be its own file when its folder is added, and the README says so.

```bend
import Base

# `each` calls whatever `f` its caller hands it: a def by name, or a lambda written at the call.
def work(x: U32) -> U32:
  x

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def main() -> U32:
  a = each(work, 1)
  each(x => work(x), a)
```

```
expected.default.tree          expected.precise.tree
main                           main
  each                           each
  work [Passed]                    work
  each                           work [Passed]
  work                           each
                                 work
```

The second `each` is the context `{f ↦ lambda 0 of main}`: its one site is `anonymous_function_called`, drawn nowhere; the lambda's `work` is
`main`'s, as in the default tree.

`tests/scenarios.rs` runs the eight folders twice: `BEND` with default options at `Level::Default` (8) and `BEND_PRECISE` with `Precision::Precise`
at `Level::Precise` (8). The testkit needs nothing: a language's own tree for a level is looked for first, and the six scenarios where both levels
agree have one `expected.tree`.

## 12. The proof

| Test | Proves |
|---|---|
| **off is identical**: the whole existing suite (policy, book, wire, refresh, session, scenarios at `Default`) runs unchanged, no snapshot touched | rule 7 |
| **never fewer edges** (`tests/context.rs`, over every policy fixture, context fixture and scenario): for each entry, the default graph's edges mapped to `(from def, site, to def-or-kind)` are a subset of the precise graph's; no site that had a target has none; the only edges the precise graph lacks are `Unresolved(function_typed_value)` ones replaced at the same site | rules 4, 5 |
| **the snapshots' digests**: reviewed hex in a file, compared on every run | rule 1 in its cross-process form; rule 9 |
| `assert_deterministic` on every precise graph | rule 9 |
| the 19 rows of §9 flip from their default section to their precise section exactly as the table says (the 7 limit rows: both sections equal) | the design |
| `two-callers` and `callback-argument` at `Level::Precise` | the shared definition of done |
| the cut-and-continue test: a tree cut at `Budget { max_depth: Some(1) }` and continued with `graph_from` at each frontier node equals the tree asked for whole | BC-D7 |
| `every_ref_the_oracle_prints_is_accounted_for` and `every_ref_the_oracle_prints_resolves` (ignored, `--test corpus`) rerun after step 1: **0 in none, 0 differ**, as before | the walk's places did not move while `sem` learned the binders and the arguments |
| **the demos** (ignored, `--test corpus`, `context_layer_on_the_demos`): the 16 books of the sweep, each entered at its `main` with `Precision::Precise`, whole graph. For every `function_typed_value` site of layer 1 (the 55, from `walked()` under `Default`), the contexts of its def that the graph has, and whether the site is answered in all of them, in some, in none, or the def is not reached. Printed as a table and as a list, one line per site: `file def line callee-text → class, target(s)` | **28 answered in every context (16 at a `~` parameter, 12 at a plain one), 0 in some, 27 in none**, each of the 27 in a §10 row; the 28 named, so the next measurement can see one move |
| the same run: contexts, walks, wall time, `Default` against `Precise`, per book | what the layer costs; written in `ENGINE.md` |

The numbers go into `languages/bend/ENGINE.md` under `## BC1+BC2` (the reviewer writes it from the coder's report), and the plan's §2 of
[`04`](04-phase-3-context-layer.md) gets the line that says what was reproduced.

## 13. Steps for the coder, one commit each

| # | Do | Proof |
|---|---|---|
| 1, 1b | `sem/walk.rs`: the scope knows which binder is a def parameter (innermost wins); `What::Opaque { param, args }`, `What::Call { name, args }`, `ArgShape`; lambdas numbered as entered; unit tests in `sem` for each shape (a parameter shadowed by a `let` is `Local`; a `~` lambda through parentheses is `Lambda(n)`; `f(a)(k)`'s inner call carries `[a]`) | every snapshot unchanged (`walk.rs` ignores the new fields); the two oracle tests at 0 |
| 2 | `key.rs` (`Known`, `Key`, `Canon`, `digest`, `relevant`, `key_of`), `env.rs`, `calls.rs` (`Precision`, `type Key`, `digest`, the open start, `walked()`); `xxhash-rust` added to the workspace; every project `Call` target carries a key under `Precise` | under `Default` byte-identical; under `Precise`, `two-keys` has two contexts of `each` with two digests and **no new edge yet**; `not-relevant` has one context with digest zeros; the digest of one key is equal from two entries |
| 3 | `resolve_from_env` answers; the `key_slot_*` counts; the layer-1 notes once per def; `tests/context.rs` reading the precise table of `policy.md` (it is there already); the 12 rows' fixtures and two-section snapshots; the never-fewer-edges test; `assert_deterministic` | the 12 rows flip as §9 says; a def with one `type_position`, reached under two keys, counts 1 |
| 4 | the 7 limit rows: fixtures, snapshots (default section equals precise section) | their counters as §9 says |
| 5 | `two-callers`: `expected.precise.bend.tree`; the `callback-argument` folder (README, `bend/`, both shared trees); `BEND_PRECISE` at `Level::Precise`; `bend-cli calls --precise` (refused with `--all`) | 8 + 8; the CLI prints the precise tree of `two-callers` |
| 6 | `Session` keeps digest → canonical key; `graph_from`; the lookup from a `StableDef` to a `DeclId`; `context_not_found` | the cut-and-continue test |
| 7 | `context_layer_on_the_demos` (ignored) and its run; the timings; the oracle tests rerun | the §12 table, in the report, with the 28 sites named |
| 8 | (the reviewer) `ENGINE.md` §BC1+BC2, `policy.md`'s known-limits list, `04` §2, the README's state line, `07`'s row | reviewed |

**State.** Step 1 built (89a07ba), reviewed 2026-11-21: the unit tests, the whole suite with no snapshot touched, clippy, the static checks and
both oracle tests (32,805 refs, 0 in none, 0 differ) reproduced. **One thing moved, and step 1b puts it back:** the item of a call is now pushed
after its arguments are walked, and two places *can* share a span when the callee is itself a sugar: in `a[i](1)` the call of the element and the
`Array.get` it stands on are both at `a[i]`, in `(a & b)(1)` both at `a & b`. Layer 1 listed the call first and step 1 lists the sugar first
(seen with `walked()` on both commits; no fixture has the form, so no snapshot noticed). The rule, now written down: **two places with one span
keep the order layer 1 had, the outer one first**, as a `do` step's bind comes before the name it hands over (`policy.md`, row `passed-do`).

Two notes for the later steps, found at this review. Whether a parameter is a `~` template is already in the outline (`Def.params[i]` is written
`~f`, `languages/bend/docs/qualnames.md`), so step 7's split of the answered sites into template and plain needs nothing more from `sem`. An
argument written `(x => x : T)` or `{f : T}` is `ArgShape::Other` (an annotation is not looked through, as `passable` does not): its slot is
absent and counted `key_slot_value`.

Rules for every step, as for every phase of this track: `cargo fmt`, `cargo clippy -p bend-analysis -- -D warnings` (and `-p bend-cli` where
touched), `cargo test -p bend-analysis` with the usual caps; no product words, no engine type out of `sem` (there is none to leak here, but
`ArgShape` and `What` are the owned shapes and the only ones `walk.rs` reads); never iterate a hash map to produce output; a commit per step with
the step's number in its first line; a report per step that says what passed and what did not, with the output.

After step 7 the layer can be tagged for the first time, **off by default** (BC-D8). Flipping the default is a one-line change and a minor version
step of its own, made after the demos' precise trees have been read.

## 14. The decision rows

| # | Row | Where |
|---|---|---|
| BC-D1 | **A Known is a project def with code or a lambda (writer, number); a type, a call's result, a constructor of defs, a Base or outside def is not one** and is counted where it lands in a relevant slot | §2 |
| BC-D2 | **The key is the relevant positions of the callee that the caller named, sorted; empty is the open key, digest zeros; the digest is `xxh3_128` of the canonical form over stable ids.** A position is relevant when the def calls that parameter or passes it as a direct argument to a project def | §3 |
| BC-D3 | **The key flows down from the first commit**: a caller's own parameter passed along resolves through the caller's key | §4 |
| BC-D4 | **A passed edge's target is walked under the open key; a parameter passed along draws no passed edge** | §4 |
| BC-D5 | **A sugar's target gets the open key** (operators, indexes, `do` statements) | §4 |
| BC-D6 | **A lambda as Known stays with the def that writes it**; the callee's site says `anonymous_function_called`. The same as Q-TC1 = A and Q-RC1 = A; moving (TC7) is designed once for every language | §6 |
| BC-D7 | **The entry is the open key**; `Session::graph_from(context)` continues a cut node under its key (Q-TC3's shape); the driver's half waits for Q-TC3 | §5 |
| BC-D8 | **Precision is an option, `Default` by default**, selectable for good; the flip to `Precise` by default is a later minor step | §8, §13 |
| BC-D9 | **The limits are the rows of §10, one fixture each**; BC3–BC5 stay unbuilt (D51) | §10 |

Recorded in the repo's register as D52 ([`../07-decisions-and-questions.md`](../07-decisions-and-questions.md)).

Next: the coder prompt (given with this file), then [`05-decisions-and-questions.md`](05-decisions-and-questions.md) for the record.
