# 03.12.03 The context layer: the design

```
the design
├── 1. what "context" means in TypeScript
├── 2. one program, through every phase
├── 3. values
├── 4. the body as data: `Flow`
├── 5. variables: one value each
├── 6. the key
├── 7. resolving a site
├── 8. the rules
├── 9. anonymous functions: one decision to make  (Q-TC1)
├── 10. the session, the driver, the counters  (Q-TC3)
├── 11. the precision policy  (the table every fixture follows)
├── 12. limits: what widens, and what cuts
├── 13. which file changes in which phase
└── 14. how it is tested
```

This file holds what the phases share. Each phase has a file of its own with its steps.

## 1. What "context" means in TypeScript

The default resolver leaves a site unresolved when the checker's answer is a **type with no body**: a function type (`cb: () => void`),
a method signature of an interface, `any`. The checker is right: at that place, any function of that type may arrive. Which one
arrives is decided by the **caller**.

```
context  =  (function, key)          key = what this caller passed: per parameter, a function, an object, an instance
```

| | Rust (RC1, RC2) | Python | TypeScript |
|---|---|---|---|
| what the key holds | type arguments | fingerprints of argument **values** | argument **values** |
| who knows it | the type checker, exactly | an interpreter that runs the body | an evaluator over a table of the body (§4) |
| identity of objects | none | yes (origins, aliasing) | none, until fields (TC8) |
| writes | the borrow checker bounds them | effects in summaries | one value per variable (§5); an object that is written through is unknown (§11) |

Rust's first two phases ask the engine for types, because Rust makes a copy of a generic function per type. TypeScript erases type
arguments, and tsrs keeps the inferred ones private ([01](01-where-we-start.md) §4). So TypeScript starts where Rust's RC3 starts:
with values.

A value here is a small thing. It is never a number or a string. It is one of: *this function*, *this class*, *an object of this
class*, *an object literal with these members*.

## 2. One program, through every phase

```tsx
interface Writer { write(): void }
class Json implements Writer { write() { encode(); } }
class Csv  implements Writer { write() { join(); } }
function encode() {}
function join() {}

function each(f: () => void) { f(); }                             // a callback parameter
function relay(f: () => void) { each(f); }                        // passed through
function open(opts: { onDone: () => void }) { opts.onDone(); }    // an options bag
function Panel({ onClose }: { onClose: () => void }) {            // props, and a nested handler that captures one
  const back = () => { onClose(); };
  return <button onClick={back} />;
}
function send(w: Writer) { w.write(); }                           // an interface parameter
function pick(): Writer { return new Csv(); }                     // a factory
function twice(f: () => void) { return () => { f(); f(); }; }     // returns a function

function main() {
  each(encode);                 // a
  relay(join);                  // b
  const h = encode; h();        // c
  open({ onDone: encode });     // d
  <Panel onClose={join} />;     // e
  send(new Json());             // f
  pick().write();               // g
  twice(encode)();              // h
  each(() => join());           // i
}
```

| Line | The default resolver shows | Changes in | Then shows |
|---|---|---|---|
| a | `each` (nothing under it); `encode` `[Passed]` | **TC2** | `each` → `encode`; the passed edge stays |
| b | `relay` → `each` (nothing under it) | **TC2** | `relay` → `each` → `join` |
| c | `h()` unresolved `[check]` | **TC3** | `encode`, called |
| d | `open` (nothing under it); `encode` `[Passed]` | **TC4** | `open` → `encode` |
| e | `Panel` → `back` `[Passed]` (nothing under it); `join` `[Passed]` | **TC4**, with TC3's captures | `Panel` → `back` `[Passed]` → `join` |
| f | `send` (nothing under it); `Json`, a construction | **TC5** | `send` → `Json.write` → `encode` |
| g | `pick`; `.write()` unresolved | **TC6** | `pick`, then `Csv.write` → `join` |
| h | `twice` (nothing under it); `encode` `[Passed]`; the second call unresolved | **TC2** | `twice` → `encode`, `encode`: the arrow is written in `twice`, so its calls are `twice`'s, and `twice` now knows `f`. TC6 only changes the reason of the second call |
| i | `each` (nothing under it); `join` under `main` | **TC2**, then TC7 | TC2: the same tree; `f()` in `each` has the reason `anonymous_function_called`. TC7 (if Q-TC1): `each` → `join` |

This program is `tests/fixtures/context/tour/src/tour.tsx` from TC1 on, with the first column as its snapshot. Writing that snapshot
is how every "today" cell of this plan gets checked.

## 3. Values

```rust
// value.rs
/// What can be at a place, as far as the layer knows.
pub(crate) struct Value<'p> {
    /// These can be there. Sorted, no duplicates, at most `KeyPolicy::max_set`.
    known: Box<[One<'p>]>,
    /// ...and so can something the layer does not know. Then the engine's answer stands beside the known ones.
    open: bool,
}

pub(crate) enum One<'p> {
    Fn(Func<'p>),          // TC2  a def with a body: a function, a method, a `const` arrow, a property arrow that is a def (D21)
                           //      (TC3 adds what a nested def captured: the variables of the functions around it, as they are where it is named)
    Anon(Anon<'p>),        // TC2  an anonymous function: the def it is written in and its number there   (TC7 adds what it captured)
    Undefined,             // TC2  nothing was passed: a parameter with this value starts from its default
    Class(Func<'p>),       // TC4  a class, as a value
    Object(Object<'p>),    // TC4  an object literal or a set of JSX attributes: members by name, and whether the list is complete
    Instance(Func<'p>),    // TC5  an object of this class                                                (TC8 adds fields)
}
```

`Value::unknown()` is no known member and `open: true`. It is what everything starts as, and it means: ask the engine.

> **🦀 Rust note: `Box<[T]>` instead of `Vec<T>`.** Both own a run of `T` on the heap. A `Vec` can grow, so it stores a pointer, a
> length and a capacity. A `Box<[T]>` is frozen: pointer and length, nothing else. A value in a key is built once and then only
> compared and hashed, so the frozen form says what we mean, is one word smaller, and `#[derive(PartialEq, Eq, Hash)]` works on it as
> on a `Vec`. You make one with `vec.into_boxed_slice()`.

Three properties, each a difference from Python's `AbstractValue`:

1. **No identity.** `Instance(Json)` is the same value wherever it appears. Two objects of one class are one value.
2. **A set says whether it is complete.** `{encode}` claims only `encode` can be there. `{encode, open}` claims `encode` can, and
   maybe more. TypeScript already draws this shape for a union receiver: the members that have a body are targets, and one
   `Unresolved` marks the rest (`Why::UnionReceiver`). The layer reuses that picture, it does not invent one.
3. **Closed.** A value mentions no variable of the function it was computed in. So it can sit in a key and mean the same thing in the
   callee.

## 4. The body as data: `Flow`

Today a site leaves `sem` as a span and an index, and `sem` answers one question about it: what does the engine say it calls. The
layer has more questions: what is the callee *made of*, what does each argument hold, what was assigned to this variable. Asking them
one by one would put a checker call behind every step of the evaluator, and every tsrs pointer one mistake away from leaving `sem`.

Instead `sem` reads a body **once** and hands back a table of plain data:

```rust
// flow.rs: plain data, no tsrs type. `sem/flow.rs` fills it.
pub(crate) struct Flow<'p> {
    pub params: Box<[Param]>,          // in order: can its type hold a value we track, is it a rest parameter
    pub vars: Box<[Var<'p>]>,          // every variable the body declares or reads, and everything assigned to it
    pub sites: Box<[SiteFlow<'p>]>,    // index for index with `Sem::call_sites`: the callee and the arguments, as expressions
    pub handed: Box<[Handed<'p>]>,     // today's hand-overs (D22)
    pub anons: Box<[AnonFlow]>,        // the anonymous functions written in the body, in source order
    pub returns: Box<[Expr<'p>]>,      // TC6: what the body returns
    pub needs: Box<[VarId<'p>]>,       // TC3: the variables of enclosing functions this body reads
}

pub(crate) enum Expr<'p> {
    Unknown,                           // anything not listed below
    Param(u32),                        // parameter i, as passed
    Var(VarId<'p>),                    // a variable: a local, a name bound by a pattern, a captured or module variable
    This,
    Def(Func<'p>),                     // a name the checker resolves to a def with a body, or to a class
    Anon(u32),                         // the i-th anonymous function of this body
    Member(Box<Expr<'p>>, Name),       // `a.b`, `a["b"]`, and a step of a destructuring pattern
    Object(Box<[Prop<'p>]>),           // `{ a: e, b, ...c }`, and the attributes of a JSX element
    Omit(Box<Expr<'p>>, Box<[Name]>),  // `const { a, ...rest } = e`: `rest` is `e` without `a`
    OneOf(Box<[Expr<'p>]>),            // `c ? a : b`, `a || b`, `a ?? b`
    New(u32),                          // TC5: what the construction at site i built
    Call(u32),                         // TC6: what the call at site i returned
}
```

`Func<'p>` and `VarId<'p>` are handles: a file and a number. They are how `sem` already lets a def be named outside it.

> **🦀 Rust note: owned data against arena pointers.** A tsrs node is a `P<Node>`: a pointer into memory the compiler does not track
> (an *arena*), valid only while the program is loaded and a region is entered. Rust's borrow checker cannot see when such a pointer
> dies, so one stored in the wrong place is a crash waiting for a reload. An `Expr` is *owned*: it holds its own bytes
> (`Box`, numbers, strings), so the borrow checker sees all of it and nothing in it can dangle. Converting at the door of `sem` costs a
> copy of a small tree once per body, and it is the reason rule 4 of the repo ("no engine type leaves `sem`") can be kept while the
> layer grows.

What this buys beyond safety:

- **The evaluator needs no engine.** `eval.rs` works on `Flow` alone, so most of the layer is tested with hand-written `Flow`s.
- **A body's table can be snapshotted.** "What did `sem` understand of this function" becomes a text file a review can read.
- **It is the piece Rust's RC3 also plans** (`sem.flow(f)`: "the body as an owned list of events"). Two languages with the same table
  is what the rule of two waits for; nothing moves to core before that.

`sem::flow` is memoized per def, beside `Sem::body`, for as long as the program is loaded. It is a function of the code alone.

## 5. Variables: one value each

```
value_of(variable) =
    a parameter's binding   the key's slot for that parameter, along the pattern's path;  joined with what the body assigns to it
    a captured variable     the key's entry for it                                         (TC3)
    a module variable       the value computed once from the module's own Flow             (TC3)
    a local                 the join of every expression assigned to it, anywhere in its scope
```

The **join** of two values is the union of their known members, and open if either is. There is no "before" and "after": a variable
has one value in a context. [01](01-where-we-start.md) §3.1 says why; the price is in §11 (the row "assigned twice").

A variable is `open` when the body assigns to it in a way the table does not model (`for (h of handlers)`, an array pattern, `h ||= x`).
So a value is complete only when every assignment was read. All assignments to a variable are inside its scope, and `sem::flow` reads
the whole scope, the nested functions included: completeness is checkable from the text.

Evaluation is **on demand**: a variable's value is computed the first time a site needs it, and kept for the rest of the walk.

> **🦀 Rust note: a memo with an "in progress" mark.** `const f = () => f();` defines `f` through itself. An evaluator that calls
> itself to evaluate `f` would never return. The usual cure is a map with three states per variable: absent (not asked yet),
> `InProgress`, and `Done(value)`. Asking for a variable that is `InProgress` returns `Value::unknown()` instead of recursing. In Rust
> the map lives in a `RefCell` inside the walk, so `eval(&self, …)` can stay a `&self` method: the `RefCell` moves the "one writer at a
> time" check from compile time to run time. The borrow must be released **before** recursing (`drop(map)` or a short block), or the
> inner call panics on a second `borrow_mut`.

## 6. The key

```rust
// key.rs: today `pub(crate) type Key = ();`
pub(crate) struct Key<'p> {
    /// The receiver, for a method.                                                   TC5
    this: Value<'p>,
    /// One per parameter, in order. Unknown when the parameter is not relevant (§12). TC2
    params: Box<[Value<'p>]>,
    /// The variables of enclosing functions the def reads, sorted.                    TC3
    captured: Box<[(VarId<'p>, Value<'p>)]>,
}
```

- **The open key** is everything unknown, and is written with nothing in it (unknown slots at the end are trimmed, so the open key of a
  function with three parameters equals that of one with none). Its digest is **zeros**. So a context under the open key has, in every
  phase, the id it has today; and the entry of a request is always the open key.
- **Keys are closed** (§3). What a caller does not know, it passes as unknown.
- **Sets stay sets.** `run(flag ? a : b)` is one context of `run` whose slot holds `{a, b}`, and one site with two targets. It is not
  two contexts. (Python's `Fp::Union` does the same.)
- **The digest** is Python's scheme (`interp/canon.rs`): the key written in a canonical form that holds only stable ids (a def by its
  `StableDef`, an anonymous function by its def and its number, a variable by its declaring def and its name), serialized, hashed with
  `xxh3_128`. Never a `#[derive(Hash)]`, an engine id or a map order (rule 9).

## 7. Resolving a site

```
resolve(site)                                      walk.rs, the function that exists today
    engine = sem.callee_with_facts(site)           the default resolver, unchanged
    if precision is Default           → engine
    if the layer may not speak here   → engine     (below)
    v = eval(site.callee)                          for `a.b()`: the member `b` of eval(a)
    targets = one per known member of v            Fn → Call(f, key_f);  Class → Construct;  Anon → Unresolved(anonymous_function_called)
            + engine's answer, if v is open        so unknown is exactly today's answer
```

**Where the layer may speak.** A call the code fixes by name is the engine's, always. The layer answers only where the engine's own
answer depends on a value:

| The engine says | The layer |
|---|---|
| `Unresolved`: `function_typed_value`, `any_or_untyped`, `no_declaration` when the callee is a call (`f()()`), the non-member part of `union_receiver` | may replace it |
| a method **reached through a receiver** (`w.write()`), project or external | may replace it with the method of the receiver's tracked class, when the value is complete (TC5) |
| a function, a `const` arrow, an import, a class, by name; `super(…)`; an external function by name | **never**. The layer still builds the callee's key |
| `computed_name`, `decorator_result`, `decl_not_in_outline`, `anonymous_function_called` | not before the phase that names it |

**The callee's key**, at every site whose target is project code, the closed ones included:

```
key_of(g, site) =
    params[i]  = admit(g, i, eval(argument i))         by position. A spread argument: that slot and every later one unknown.
                                                       A rest parameter: unknown. No argument: {Undefined}.
    this       = eval(the receiver)                    for a method call                                                (TC5)
    captured   = what the value of g carries                                                                            (TC3)
```

`admit` applies §12: a slot that is not relevant, or too large, is unknown. A JSX element passes one argument, the object of its
attributes.

**A nested def is a value with its captures.** Where the name of a nested def is evaluated (called, or handed over), the variables it
reads from the functions around it (`flow(g).needs`) are read from the env of that place and travel with the value. So the key is
right wherever the def ends up being called: by name in the function that declares it, by a sibling, or three calls away through a
parameter. A hand-over (`onClick={back}`) builds the key the same way with every parameter unknown: nobody knows what the receiver
will pass, but what `back` captured is known where it is handed over. That is pattern A8.

Arguments are evaluated in the caller's env, once. A chain `a → b → c` needs no special code: `b` evaluates its argument where its own
parameter already has a value from `a`.

## 8. The rules

The first three are pylspt's, as Rust's design restates them. The others are this layer's.

1. **A summary is a function of its key and the code.** A module variable's value is a function of the code alone.
2. **Nothing outlives the program.** A key holds handles (`Func<'p>`) that die with the loaded program. What leaves the analyzer is a
   digest made of stable ids.
3. **Precision is decided by the key and the code; a cost limit only cuts.** Every widening is a `KeyPolicy` rule and happens the same
   way on every run. A cut marks the context `Incomplete` and is counted.
4. **A site the code fixes by name is never overridden** (§7).
5. **Unknown means the engine's answer.** A phase can replace an unresolved answer by a better one. It cannot lose an edge.
6. **A set is complete, or it says it is open.** One untracked way in makes the value open. The known members are still drawn, with
   the engine's answer beside them.
7. **Off is identical.** With `Precision::Default` no line of the layer runs after the option is read, and every graph, tree, wire and
   counter is what it is today.

Rules 5 and 7 are why the layer can be released one phase at a time. Each has a test (§14).

## 9. Anonymous functions: one decision to make

An anonymous function is not a def. Today its calls belong to the def it is written in. When the layer learns that the `f` in
`each(f)` **is** that arrow, where do the arrow's calls show? **Q-TC1**, the same question as Rust's Q-RC1.

| | A: stay with the function it is written in (through TC6) | B: move under the function that calls it (TC7) |
|---|---|---|
| `each(() => join())` in `main` | `main` → `each`, `join`. `f()` in `each` is `Unresolved(anonymous_function_called)`: known, drawn elsewhere | `main` → `each` → `join` |
| every call appears | exactly once, always | once, **if** some project function calls the arrow. An arrow handed to a library (`xs.map(x => f(x))`, `useEffect(() => …)`, `onClick` of a `<button>`) is called by no body we walk, so it must stay with its writer |
| what decides where a call is drawn | the text | whether the arrow reached a project call: a fact about the graph, not about one summary. Rule 1 strains |
| the arrow's own parameters | the checker's types | filled from what the caller passes: `each(x => x.run())` can resolve `x.run()` |
| a variable the arrow reads | resolved already: the arrow's sites are its writer's sites, in its writer's env (line h of §2) | must be carried in the value: `Anon` gets its captured values |
| the cost | none | the arrow's sites are drawn at the calling site (one site, several targets), and removed from the writer when every hand-over of the arrow is followed |

I recommend A through TC6: nothing is lost, nothing is drawn twice, and today's rule stays true. Most arrows in React code go to
libraries, where B changes nothing. B is TC7 ([11](11-later.md) §1), designed once for both languages in `docs/02-tree-rules/`. A does
not close the door: `One::Anon` exists from TC2, and B adds to it.

Named functions do not have this question. `each(encode)` shows `encode` under `each`, where it is called, **and** keeps `encode`
`[Passed]` under `main`, where it is handed over. D22 already says a def can have both, and they are two nodes.

## 10. The session, the driver, the counters

**Starting from a context (Q-TC3).** A node the budget stopped at is `Status::Frontier`, drawn `Cut`, and continued by asking for its
def as a new entry. A def has no key, so the continuation would start from the open key, and the same subtree would have two answers
depending on where the budget fell. The fix:

```
Session                     keeps: digest → the key in canonical form, for every context a request listed. Dropped at a reload.
Session::graph_from(context: &ContextId, budget, cancel)
                            the key is found by its digest and turned back into a `Key<'p>`.
                            Not found (after a reload, another session): the open key, and `Stats.language["context_not_found"]`.
```

The canonical form is the one the digest is made from, so nothing new is defined: it holds no `'p` handle, and that is why a `Session`
(which cannot borrow from its own `Project`) can keep it. Core needs nothing: `Start::Call(func, key)` already takes any key. This
changes what the driver sends (a context id where it sends a def), so it is a `vnoc`-side item and needs your yes. It is built in TC2,
because TC2 is the first phase whose trees would otherwise depend on the budget.

**Counters per def.** A counter of what a body *has* (`uncovered_*`, `signature_passed`, `engine_kind_unhandled`, the hand-over notes)
is added by each walk. Two contexts of one function would count it twice. From TC2 these are kept by def and added once.

**The measure.** `Stats.unresolved` counts edges, so it grows with contexts and cannot say whether a phase helped. The layer's measure
is per **site**: of the sites the default resolver leaves open, how many are answered in every context that reaches them, in some, in
none. TC0 builds it, and every phase reports it.

## 11. The precision policy

Each row is a fixture in `tests/fixtures/context/` and a row of the precise table in `languages/typescript/docs/policy.md`.

| Feature | Policy | From |
|---|---|---|
| a function, a method, a class, by name | itself | TC2 |
| an argument | its value goes into the callee's key, by position | TC2 |
| a spread argument; a rest parameter | unknown, for that slot and later ones | TC2 (arrays: TC9) |
| a missing argument; a default (`cb = noop`) | `Undefined`; a parameter whose value has `Undefined` takes the default in its place | TC2 |
| `c ? a : b`, `a \|\| b`, `a ?? b`, `a && b` | either side. No condition is evaluated | TC2 |
| parentheses, `as`, `!`, `satisfies`, `<T>x` | looked through | TC2 |
| `f?.()` | as `f()` | TC2 |
| an anonymous function, as a value | known by where it is written; called where known: `anonymous_function_called` | TC2 (Q-TC1: TC7) |
| a `const`, `let` or `var` | **one value: the join of everything assigned to it** | TC3 |
| assigned twice (`let h = a; h(); h = b; h()`) | both calls show both. Stricter would need the order of execution | TC3; refined only if measured |
| a variable of an enclosing function, read in a nested def | the value it has there, carried in the nested def's key | TC3 |
| a `const` at the top of a file; an import of one | its value, computed once from the module. A top-level `let` or `var`: unknown | TC3 |
| `f.bind(x)` | `f`; `this` is not followed | TC3 |
| `f.call(x, a)`, `f.apply(x, [a])` | counted (`uncovered_call_apply`), as today | later, if measured |
| an object literal; the attributes of a JSX element | an object: its members by name | TC4 |
| `{ ...a, k: v }` | `k` is `v`. A member not written after the spread is `a`'s, if `a` is a complete object; else unknown | TC4 |
| `a.k`, `a["k"]`; `const { k } = a`; `function f({ k })` | the member `k` of `a`'s value | TC4 |
| an array pattern; `for … of`; an element of an array | unknown | TC9 |
| **an object that is written through** (`a.k = v`, `a[x] = v`, `delete a.k`, `Object.assign(a, …)`), in the body that holds it | its members are open, in that body and in every key it goes into | TC4 |
| **an object handed to code that may write through it** (a project function that does, or code we cannot read) | the same: its members are open in the body that handed it over | TC4; the library half is measured there |
| a getter or a setter of a tracked object | unknown | later |
| `new C(…)`, by name | an instance of `C` | TC5 |
| an argument or a receiver with no value, whose static type is a class | an instance of that class (D27's idea, from Python) | TC5 |
| a method call on an instance | the method of that class, or the nearest base class that has it with a body | TC5 |
| `this`, in a method | the receiver's value, from the key | TC5 |
| the result of a call | unknown before TC6; the callee's return value after | TC6 |
| `await e` | `e`'s value | TC6 |
| a recursive call's result | unknown, counted `return_in_cycle` | TC9 |
| `this.x`, `obj.x` on an instance | unknown before TC8 | TC8 |
| a `Map`, a `Set`, an array, a `Promise` holding a tracked value | unknown | TC9 |
| `eval`, `with`, `Proxy`, `Reflect`, a name computed at run time | unknown | never |

## 12. Limits

| | Kind (rule 10 of the tree rules) | Effect | Counted |
|---|---|---|---|
| a parameter whose **declared type cannot hold** a function or an object (`string`, `number`, a literal union, an array of those) | precision | its slot is always unknown. The cheapest filter there is, and Python has no equal | no: it changes no edge |
| **relevant parameters** (Python's `relevant.rs`): the value cannot reach a callee, a receiver, an argument, a return or a nested def | precision | its slot is unknown, so callers that differ only there share one context | no |
| `KeyPolicy::max_set` (8) | precision | a larger set becomes unknown | `value_widened_set` |
| `KeyPolicy::max_depth` (2): an object inside an object inside a slot | precision | the deeper member is unknown. This is what ends pattern R3 | `value_widened_depth` |
| `KeyPolicy::max_members` (64) | precision | a larger object is unknown | `value_widened_members` |
| `Limits::max_contexts`, `max_depth`, the deadline, cancel | cost | cut, marked, as today | core's `Stats.cuts` |
| contexts **per function** (64, Python's number) | cost | later keys of that function are cut, marked `Incomplete` | a new `Cut` in core (TC2) |

A precision limit is a function of the key and the code, so two runs agree. A per-function cap must **cut** and never widen to the
open key: which callers arrive first depends on the order of the walk (Rust's design, §7, says the same). The numbers are Python's
defaults where it has one; TC0 measures whether they fit TypeScript.

## 13. Which file changes in which phase

| | `sem/` | `flow.rs` | `value.rs` | `env.rs` | `key.rs` | `eval.rs` | `walk.rs` | `calls.rs`, `session.rs` | core |
|---|---|---|---|---|---|---|---|---|---|
| TC1 | `sem/flow.rs`: sites, arguments, names | new: the types | new: `Value::unknown` only | a struct with a `Flow` | a struct; `Key::open()` | new: returns unknown | the five `()` | `Options::precision`; `digest` | — |
| TC2 | which parameter a name is; capable types | `Param`, `Anon`, `OneOf` | `Fn`, `Anon`, `Undefined`; join | parameters from the key | `params`; the canonical form | `Param`, `Def`, `Anon`, `OneOf` | `resolve_from_env` answers; `key_of` | counters per def; `graph_from`; the per-site measure | scenario level per scenario; `Cut` per function |
| TC3 | every assignment; `needs`; module variables | `Var`, `needs` | | locals, captured, module | `captured` | `Var` | hand-over keys carry captures | the module memo | — |
| TC4 | literals, JSX attributes, patterns, writes-through, escapes | `Object`, `Member` | `Object`, `Class` | | objects in slots; the depth cap | `Object`, `Member` | JSX binds one object | | — |
| TC5 | the static class of an expression; a member of a class by name | `New`, `This` | `Instance` | `this` | `this` | `New`, `This`; member of an instance | receiver sites may be replaced | | — |
| TC6 | returns | `returns`, `Call` | | | | `Call` | the return value of a summary | | **`Lang::Ret`, the handle** (a note first) |

`lib.rs` gains `Precision` and `Session::graph_from` and nothing else. `typescript-syntax`, the renderer, the wire and the merge do not
change in any row.

## 14. How it is tested

| Test | Proves |
|---|---|
| **off is identical**: the whole existing suite runs with `Precision::Default` and no snapshot changes | rule 7 |
| **never fewer edges**: for every context fixture and scenario, each default edge from a site the layer may not speak at is in the precise graph; no site that had a target has none | rules 4 and 5 |
| **no-reuse mode** (Python's `Limits::reuse = false`): solve with the memo off and compare the edge sets | rule 1 |
| **determinism**: `assert_deterministic` on the precise graphs; digests equal across two processes | rule 9 of the repo; no handle leaked into a digest |
| **the evaluator alone**: `eval.rs` on hand-written `Flow`s, no `Sem` | the layer's logic, in milliseconds |
| **the table of a body**: a text snapshot of `sem::flow` per fixture def | what `sem` understood; reviewed, not just accepted |
| **the old code from the new**: `call_sites` and `handed_over` recomputed from `Flow`, equal to today's | TC1's proof that the table is faithful |
| the tour (§2) and one fixture per row of §11 and of [02](02-patterns.md): each snapshot holds the default tree and the precise tree | each phase's proof; a flip is a diff a review reads |
| the shared scenarios at `Level::Precise`, **per scenario** | the definition of done that Python wrote |
| the four projects, before and after: contexts, walks, time, the per-site measure | whether the phase was worth it; written in `ENGINE.md` |

Next: [`04-tc0-counts-and-spikes.md`](04-tc0-counts-and-spikes.md)
