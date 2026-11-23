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
├── 9. anonymous functions: decided (D54), built in TC7
├── 10. the session, the driver, the counters  (Q-TC3)
├── 11. the precision policy  (the table every fixture follows)
├── 12. limits: what widens, and what cuts
├── 13. which file changes in which phase
├── 14. how it is tested
└── 15. built to grow: numbers, capabilities, versions, neutral files, crates
```

This file holds what the phases share. Each phase has a file of its own with its steps.

**Revised 2026-11-23** after a review against Python's `interp/` and tsrs at the pin: §3 (data, wide, exact), §4 and §6 (numbers, an
interned key), §5 (who says a value is complete), §7 (binding through the resolved signature), §8 rule 8, §9 (D54), §10, §12, §14, and
§15, which is new. The README lists every change.

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
| i | `each` (nothing under it); `join` under `main` | **TC2**, then TC7 | TC2: the same tree; `f()` in `each` has the reason `anonymous_function_called`. TC7 (D54): `each` → `join` |

This program is `tests/fixtures/context/tour/src/tour.tsx` from TC1 on, with the first column as its snapshot. Writing that snapshot
is how every "today" cell of this plan gets checked.

## 3. Values

```rust
// value.rs: plain data. It imports nothing from `sem` (§15.4).
/// What can be at a place, as far as the layer knows.
pub(crate) enum Value {
    /// These can be there: sorted, no duplicates, at most `KeyPolicy::max_set`.
    /// `open`: and so can something the layer does not know. Then the engine's answer stands beside the known ones.
    Set { known: SmallVec<[One; 2]>, open: bool },
    /// Too much for a cap (§12). No member, open, and a join with it is it.
    Wide,
}

pub(crate) enum One {
    Fn(DefId),                             // TC2  a def with a body: a function, a method, a `const` arrow, a property arrow that is a def (D21)
                                           //      (TC3 adds what a nested def captured: the variables of the functions around it, as they are where it is named)
    Anon(DefId, u32),                      // TC2  an anonymous function: the def it is written in and its number there
    AnonAny,                               // TC2  an anonymous function, as a key writes it: which one changes no edge before TC7 (§6)
    Undefined,                             // TC2  nothing was passed: a parameter with this value starts from its default
    Class(DefId),                          // TC4  a class, as a value
    Object(Object),                        // TC4  an object literal or a set of JSX attributes: members by name, and whether the list is complete
    Instance { class: DefId, exact: bool },// TC5  an object of this class. `exact`: the layer followed a `new`; not exact: the checker's static class
}
```

Three values have a name:

| | Members | Means | Made by |
|---|---|---|---|
| `Value::unknown()` | none, open | the layer knows nothing: ask the engine. Everything starts as this | an expression the table does not model; a parameter of an entry |
| `Value::data()` | none, closed | **nothing callable is there**, and the layer knows it | a number, a string, a boolean, `null`, a regular expression, a template, arithmetic, a comparison, `typeof`, a JSX element used as a value |
| `Value::Wide` | none, open | too much to list; **it absorbs every join** | a cap (§12) |

`data` is not `unknown`. `const h = ok && handler; h()` is `{handler}` joined with data, which is `{handler}`: one edge. If `false` were unknown,
the value would be open and an `Unresolved` edge would stay beside `handler` for ever. Python keeps the same two apart (`AbstractValue::Ty` and
`AbstractValue::Unknown`; `Fp::Data` and `Fp::Unknown`).

`Wide` is not `unknown` either. If a cap answered `unknown`, the next join would add a member again, and which members a value ends with would
depend on the order of the joins. A value that absorbs keeps `join` a real "least upper bound": the same whatever the order. (Python has no such
value: there, too many values is a cost cut, marked. Both are sound; this plan takes the value, because the context stays reusable.)

`join(a, b)`: `Wide` if either is; else the union of the known members in the stable order (a def by file and offset), open if either is; `Wide`
if the union has more than `max_set` members (counted `value_widened_set`). `join` with data changes nothing. These are laws, and a test checks
them on generated values (§14).

> **🦀 Rust note: `SmallVec<[One; 2]>` instead of `Vec<One>`.** A `Vec` always puts its items on the heap, even one item. A `SmallVec<[T; 2]>`
> keeps up to two items inside itself and only goes to the heap for a third. Almost every value here has one member, so most values never
> allocate. It reads and compares like a `Vec`. The crate is already in this workspace's lockfile (tsrs uses it), and Python's layer holds its
> values the same way (`SmallVec<[AbstractValue; 2]>`).

Four properties, each a difference from Python's `AbstractValue`:

1. **No identity.** `Instance { class: Json, .. }` is the same value wherever it appears. Two objects of one class are one value.
2. **A set says whether it is complete.** `{encode}` claims only `encode` can be there. `{encode, open}` claims `encode` can, and
   maybe more. TypeScript already draws this shape for a union receiver: the members that have a body are targets, and one
   `Unresolved` marks the rest (`Why::UnionReceiver`). The layer reuses that picture, it does not invent one.
3. **Closed.** A value mentions no variable of the function it was computed in. So it can sit in a key and mean the same thing in the
   callee.
4. **An instance says how it is known.** `exact: true` is an object the layer followed from its `new`. `exact: false` is the checker's static
   class of an expression: a subclass may be behind it. Python keeps the same two apart (`OriginKey::is_built_here`, the one thing a
   `Basis::Path` edge may rest on). They are different facts, so they are different values and different keys from the first day (TC5).

## 4. The body as data: `Flow`

Today a site leaves `sem` as a span and an index, and `sem` answers one question about it: what does the engine say it calls. The
layer has more questions: what is the callee *made of*, what does each argument hold, what was assigned to this variable. Asking them
one by one would put a checker call behind every step of the evaluator, and every tsrs pointer one mistake away from leaving `sem`.

Instead `sem` reads a body **once** and hands back a table of plain data:

```rust
// flow.rs: plain data. No tsrs type, no lifetime. `sem/flow.rs` fills it.
pub(crate) struct Flow {
    pub params: Box<[Param]>,          // in the order of the def's own signature (no `this` parameter): capable, rest, default
    pub vars: Box<[Var]>,              // every variable the body declares or reads, and everything assigned to it
    pub sites: Box<[SiteFlow]>,        // index for index with `Sem::call_sites`: the callee and the arguments, as expressions
    pub handed: Box<[Handed]>,         // today's hand-overs (D22)
    pub anons: Box<[AnonFlow]>,        // the anonymous functions written in the body, in source order
    pub returns: Box<[Expr]>,          // TC6: what the body returns
    pub needs: Box<[VarId]>,           // TC3: the variables of enclosing functions this body reads
    pub facts: Facts,                  // what is true of the code alone: relevant parameters (TC2), `touches` (TC4)
}

pub(crate) enum Expr {
    Unknown,                           // anything not listed below
    Data,                              // a literal and the other forms of §3's table: nothing callable
    Param(u32),                        // parameter i, as passed
    Var(VarId),                        // a variable: a local, a name bound by a pattern, a captured or module variable
    This,
    Def(DefId),                        // a name the checker resolves to a def with a body, or to a class
    Anon(u32),                         // the i-th anonymous function of this body
    Member(Box<Expr>, NameId),         // `a.b`, `a["b"]`, and a step of a destructuring pattern
    Object(Box<[Prop]>),               // `{ a: e, b, ...c }`, and the attributes of a JSX element
    Omit(Box<Expr>, Box<[NameId]>),    // `const { a, ...rest } = e`: `rest` is `e` without `a`
    OneOf(Box<[Expr]>),                // `c ? a : b`, `a || b`, `a ?? b`
    New(u32),                          // TC5: what the construction at site i built
    Call(u32),                         // TC6: what the call at site i returned
}
```

**The handles are numbers.** `DefId` is a file's number and a def's number in it; `VarId` is a `DefId` and the number of a declaration in it;
`NameId` is the number of a property name in a table `Sem` keeps. `sem` turns a `Func<'p>` into a `DefId` and back at its door. A number is
good for as long as the program is loaded, and means nothing after a reload: whatever holds one is dropped with the program (§8 rule 2).

> **🦀 Rust note: why numbers and not `Func<'p>`.** `Func<'p>` borrows the `Sem`: the `'p` says "I am valid only while that `Sem` is". The
> table of a body is kept *inside* the `Sem`, beside `Sem::bodies`, so that it is read once per program and not once per request. But a struct
> cannot hold a value that borrows from itself: there is no way to write the lifetime of "me". (`session.rs` says the same of the solver: it is
> built fresh per request "because a struct cannot borrow from itself".) A plain number borrows nothing, so it can be stored anywhere. The
> price is that the compiler no longer checks that a number belongs to *this* program; the rule "dropped with the program" replaces the
> check, and a test loads a second program to prove a stale number is never read.

> **🦀 Rust note: owned data against arena pointers.** A tsrs node is a `P<Node>`: a pointer into memory the compiler does not track
> (an *arena*), valid only while the program is loaded and a region is entered. Rust's borrow checker cannot see when such a pointer
> dies, so one stored in the wrong place is a crash waiting for a reload. An `Expr` is *owned*: it holds its own bytes
> (`Box`, numbers), so the borrow checker sees all of it and nothing in it can dangle. Converting at the door of `sem` costs a
> copy of a small tree once per body, and it is the reason rule 4 of the repo ("no engine type leaves `sem`") can be kept while the
> layer grows.

What this buys beyond safety:

- **The evaluator needs no engine.** `eval.rs` works on `Flow` and on a small trait for the questions about classes (§15.4), so most of the
  layer is tested with hand-written `Flow`s.
- **A body's table can be snapshotted.** "What did `sem` understand of this function" becomes a text file a review can read.
- **The layer survives the engine.** Only `sem/flow.rs` knows tsrs. If the engine is bumped, or one day replaced, the files above it do not
  change.
- **It is the piece Rust's RC3 also plans** (`sem.flow(f)`: "the body as an owned list of events"). Two languages with the same table
  is what the rule of two waits for; nothing moves to core before that.

`Sem::flow(def) -> Option<Arc<Flow>>` is memoized per def, beside `Sem::bodies`, for as long as the program is loaded. It is a function of the
code alone, and so is everything in `Flow::facts`: they are worked out once per program, never once per request (Python does the same for
its relevant parameters, through its engine's cache).

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

**Who says that every assignment was read: the engine, not our list of forms.** The table models a few ways of writing a variable. If the
reader missed one (a destructuring assignment inside a nested arrow), the value would be called complete, the `Unresolved` edge would be
dropped, and no counter would say so. So the reader does not trust its own list: for every reference of the variable in its scope it asks
`ast::is_write_access`, and **a write that is not one of the modelled assignments makes the variable open**. A form nobody thought of then
costs precision, never an edge.

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
/// A key: a number. Two keys are equal exactly when their numbers are. `KeyId::OPEN` is 0.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct KeyId(u32);

/// What a key holds, in its normal form (below).
pub(crate) struct KeyData {
    /// The receiver, for a method.                                                   TC5
    this: Value,
    /// One per parameter, in order. Unknown when the parameter is not relevant (§12). TC2
    params: Box<[Value]>,
    /// The variables of enclosing functions the def reads, sorted.                    TC3
    captured: Box<[(VarId, Value)]>,
}

/// Every key of one request. `TypeScript` holds it.
pub(crate) struct Keys {
    table: IndexSet<KeyData>,      // a key is looked up by its normal form; its place in the set is its number
    digests: Vec<[u8; 16]>,        // index for index with `table`: made once, when the key is first met
}
```

`type Key = KeyId` for the solver. This is Python's shape (`Interner<Fp, FpId>`, `Interner<Context, ContextId>`, and the digest made once per
context in `canon.rs`).

> **🦀 Rust note: interning.** To *intern* a value is to store it once in a table and hand out its position. Two things follow. Comparing two
> keys is comparing two `u32`s, however large the keys are, and the solver compares and hashes a key at every edge. And there is exactly one
> place where a key is built, so there is exactly one place where it is put in its normal form. `indexmap::IndexSet` is a hash set that also
> remembers the order of insertion: `insert_full(x)` returns the position of `x`, new or old. That is an interner in one call, and the crate
> is already in this workspace's lockfile.

- **The normal form**, made by the one function that interns: unknown slots at the end are dropped (so the open key of a function with three
  parameters equals that of one with none); a set is sorted and has no duplicates; an anonymous function is written `AnonAny`. Without one
  normal form, two keys could differ in memory and have the same digest: the solver would walk two contexts and `Graph::new` would merge
  them into one row, silently. A test asserts that a request has as many keys as digests.
- **The open key** is everything unknown. Its number is 0 and its digest is **zeros**. So a context under the open key has, in every phase,
  the id it has today; and the entry of a request is always the open key.
- **Keys are closed** (§3). What a caller does not know, it passes as unknown.
- **Sets stay sets.** `run(flag ? a : b)` is one context of `run` whose slot holds `{a, b}`, and one site with two targets. It is not
  two contexts. (Python's `Fp::Union` does the same.)
- **No anonymous function is told from another in a key, before TC7.** Whichever arrow was passed, the call of it is
  `Unresolved(anonymous_function_called)`, so two keys that differ only there have equal summaries. Keeping them apart would give a helper
  like `forEach(xs, cb)` one context per call site and nothing for it. Python's layer has no lambda in a key at all (it evaluates one to
  `Unknown`). TC7 gives an arrow that moves its identity and its captures.
- **The digest** is Python's scheme (`interp/canon.rs`): the key written in a canonical form that holds only stable ids (a def by its
  `StableDef`, a variable by its declaring def and its name, a property by its text), serialized, hashed with `xxh3_128`. Never a
  `#[derive(Hash)]`, an engine id, a `DefId` or a map order (rule 9). `xxhash-rust` and `serde_json` are already in the lockfile.
- **The canonical form only grows.** A later phase adds a variant, or a field that is skipped when empty, so a key that does not use the new
  thing keeps its digest (Python does this for its `alias` field, so that "every id in every wire, and the golden, stay as they were"). Each
  variant has a test that pins its bytes. When a digest of an existing key must change all the same, `CONTEXT_ID_VERSION` (a constant the
  crate exports, like core's `WIRE_FORMAT_VERSION`) goes up, and the tag notes say so.

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
    params[i]  = admit(g, i, eval(argument i))         argument i goes to parameter i of the resolved signature (below). A spread: that slot and every later one unknown.
                                                       A rest parameter: unknown. No argument: {Undefined}.
    this       = eval(the receiver)                    for a method call                                                (TC5)
    captured   = what the value of g carries                                                                            (TC3)
```

`admit` applies §12: a slot that is not relevant, or too large, is unknown. A JSX element passes one argument, the object of its
attributes.

**Binding goes through the signature the checker resolved, not through the syntax.** `get_resolved_signature_exported(call)` gives the
signature the call resolves to (the overload the checker picked), and `Signature::parameters()` lists its parameters **without `this`**
(`this_parameter()` is apart). `Flow.params` of a def is in the order of that def's own signature. So for `function f(this: Ctx, cb)`,
`f(handler)` binds `handler` to `cb`: binding by the position in the syntax would be off by one. `has_rest_parameter()` and
`min_argument_count()` say the rest. All of these are public at the pin ([01](01-where-we-start.md) §4). Only the argument list of a JSX
element, a tagged template and a decorator is private (`get_effective_call_arguments`): a JSX element's one argument is built from its
attributes here, and the other two pass nothing until a fixture asks.

**A tripwire for a wrong binding.** Where the layer says a site calls exactly these functions, each of them must be assignable to the
callee's static type (`is_type_assignable_to`, public). One that is not is a bug in the binding or in the table, not a fact about the
program: it is counted `value_type_mismatch`, the engine's answer is kept beside it, and the count must be 0 on the four projects.

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
2. **Nothing outlives the program.** A key, a value and a table hold numbers (`DefId`, `VarId`, `NameId`) that mean something only while the program is loaded, and they are dropped with it. What leaves the analyzer is a
   digest made of stable ids.
3. **Precision is decided by the key and the code; a cost limit only cuts.** Every widening is a `KeyPolicy` rule and happens the same
   way on every run. A cut marks the context `Incomplete` and is counted.
4. **A site the code fixes by name is never overridden** (§7).
5. **Unknown means the engine's answer.** A phase can replace an unresolved answer by a better one. It cannot lose an edge.
6. **A set is complete, or it says it is open.** One untracked way in makes the value open. The known members are still drawn, with
   the engine's answer beside them.
7. **Off is identical.** With `Precision::Default` no line of the layer runs after the option is read, and every graph, tree, wire and
   counter is what it is today.
8. **The answer does not depend on where a request starts.** A context reached from two entries has the same edges and returns the same
   value. Return values around a cycle are where this breaks if "still being walked" is answered with unknown; TC6 takes Python's rule
   instead ([10](10-tc6-return-values.md) §3).

Rules 5 and 7 are why the layer can be released one phase at a time. Each has a test (§14).

## 9. Anonymous functions: decided (D54), built in TC7

An anonymous function is not a def. Today its calls belong to the def it is written in. When the layer learns that the `f` in
`each(f)` **is** that arrow, where do the arrow's calls show? That was Q-TC1, the same question as Rust's Q-RC1.

**D54 (2026-11-21) answers it: under the function that calls it.** It was decided while Bend's layer was built, so the README asks you to
confirm it for TypeScript. Until TC7 builds it, nothing changes: an arrow's calls stay with the def that writes it.

| | Through TC6: with the function it is written in | TC7: under the function that calls it |
|---|---|---|
| `each(() => join())` in `main` | `main` → `each`, `join`. `f()` in `each` is `Unresolved(anonymous_function_called)`: known, drawn elsewhere | `main` → `each` → `join` |
| every call appears | exactly once, always | once, **if** some project function calls the arrow. An arrow handed to a library (`xs.map(x => f(x))`, `useEffect(() => …)`, `onClick` of a `<button>`) is called by no body we walk, so it stays with its writer |
| the arrow's own parameters | the checker's types | filled from what the caller passes: `each(x => x.run())` can resolve `x.run()` |
| a variable the arrow reads | resolved already: the arrow's sites are its writer's sites, in its writer's env (line h of §2) | carried in the value: the arrow gets its captured values |

Two things TC7's design must settle before it is built ([11](11-later.md) §1):

1. **When an arrow moves.** Only when the function it is handed to really calls that parameter, itself or through what it passes it to. If
   it only stores the arrow, the calls must stay with the writer, or they vanish from every tree.
2. **The tree marks recursion by function, not by context** ([`02-tree-rules/02-the-rules.md`](../../02-tree-rules/02-the-rules.md), rule 8).
   An arrow moved under `each` whose body leads back to `each` (a continuation, a visitor) makes the second `each` a `[Recursive]` leaf, and
   what the arrow calls after it is in the graph and **not in the tree**. The wire tree is the output that is used (D17), so this is a
   question about the tree rules for every language, and it is yours.

What Python does: it follows a nested `def` (a function with a name and a row in the outline) to where it is called, and evaluates a
`lambda` to unknown (`interp/walk.rs`). So TC7 goes further than the reference, and has no built example to copy.

Named functions do not have this question. `each(encode)` shows `encode` under `each`, where it is called, **and** keeps `encode`
`[Passed]` under `main`, where it is handed over. D22 already says a def can have both, and they are two nodes.

## 10. The session, the driver, the counters

**Starting from a context (Q-TC3).** A node the budget stopped at is `Status::Frontier`, drawn `Cut`, and continued by asking for its
def as a new entry. A def has no key, so the continuation would start from the open key, and the same subtree would have two answers
depending on where the budget fell. The fix:

```
Session                     keeps: digest → the key in canonical form, for the contexts a request listed and did not finish
                            (a frontier, a cut). Those are the ones a driver continues. Dropped at a reload.
Session::graph_from(context: &ContextId, budget, cancel)
                            the key is found by its digest, turned back into a `KeyData` and interned in the new request.
                            Not found (after a reload, another session): the open key, and `Stats.language["context_not_found"]`.
```

The canonical form is the one the digest is made from, so nothing new is defined: it holds no number of a loaded program, and that is why
a `Session` can keep it across requests. Core needs nothing: `Start::Call(func, key)` already takes any key. This changes what the driver
sends (a context id where it sends a def), so it is a `vnoc`-side item and needs your yes. V-NOC has no TypeScript driver yet (only
Python's), so the driver can be written to this contract from its first version. It is built in TC2, because TC2 is the first phase whose
trees would otherwise depend on the budget.

**Counters per def.** A counter of what a body *has* (`uncovered_*`, `signature_passed`, `engine_kind_unhandled`, the hand-over notes)
is added by each walk. Two contexts of one function would count it twice. From TC2 these are kept by def and added once.

**The measure.** `Stats.unresolved` counts edges, so it grows with contexts and cannot say whether a phase helped. The layer's measure
is per **site**: of the sites the default resolver leaves open, how many are answered in every context that reaches them, in some, in
none. TC0 builds it, and every phase reports it.

**What one request costs.** A request builds its solver fresh, so no summary is kept from one request to the next. That is right today
(a warm request for `TaskDetailPanel` is about 1 ms). It is the limit to watch as the layer grows: §15.1.

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
| a number, a string, a boolean, `null`, a regular expression, a template; arithmetic, a comparison, `typeof`; a JSX element as a value | **data**: nothing callable is there. Not unknown: `ok && handler` is `handler` alone | TC2 |
| `f?.()` | as `f()` | TC2 |
| an anonymous function, as a value | known by where it is written; called where known: `anonymous_function_called` | TC2 (its calls move in TC7, D54) |
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
| **a second name for an object** (`const o = opts`; `const { actions } = props`, where `actions` is an object) | allowed only when the second variable passes the same test; otherwise the object's members are open | TC4 |
| **an object held by a `const` at the top of a file** | every file can see it, so the test covers the declaring file and the object must not be exported; otherwise its members are open, counted `module_object_shared` | TC4 |
| an object put in an array literal (`[props]`, a hook's dependency list) | stored: its members are open. TC0 counts how often (N5) before this row is kept or relaxed | TC4 |
| a getter or a setter of a tracked object | unknown | later |
| `new C(…)`, by name | an instance of `C` | TC5 |
| an argument or a receiver with no value, whose static type is a class | an instance of that class (D27's idea, from Python) | TC5 |
| a method call on an instance | the method of that class, or the nearest base class that has it with a body | TC5 |
| `this`, in a method | the receiver's value, from the key | TC5 |
| the result of a call | unknown before TC6; the callee's return value after | TC6 |
| `await e` | `e`'s value | TC6 |
| a recursive call's result | the value of the cycle, worked out by rounds; `Wide` past the cap of rounds, counted | TC6 |
| `this.x`, `obj.x` on an instance | unknown before TC8 | TC8 |
| a `Map`, a `Set`, an array, a `Promise` holding a tracked value | unknown | TC9 |
| `eval`, `with`, `Proxy`, `Reflect`, a name computed at run time | unknown | never |

## 12. Limits

| | Kind (rule 10 of the tree rules) | Effect | Counted |
|---|---|---|---|
| a parameter whose **declared type cannot hold** a function or an object (`string`, `number`, a literal union, an array of those) | precision | its slot is always unknown. The cheapest filter there is, and Python has no equal | no: it changes no edge |
| **relevant parameters** (Python's `relevant.rs`): the value cannot reach a callee, a receiver, an argument, a return or a nested def | precision | its slot is unknown, so callers that differ only there share one context | no |
| `KeyPolicy::max_set` (8) | precision | a larger set becomes **wide**: no member, open, and it absorbs every later join (§3) | `value_widened_set` |
| `KeyPolicy::max_depth` (2): an object inside an object inside a slot | precision | the deeper member is unknown. This is what ends pattern R3 | `value_widened_depth` |
| `KeyPolicy::max_members` (64) | precision | a larger object is unknown | `value_widened_members` |
| `Limits::max_contexts`, `max_depth`, the deadline, cancel | cost | cut, marked, as today | core's `Stats.cuts` |
| contexts **per function** (64, Python's number) | cost | later keys of that function are cut, marked `Incomplete` | a new `Cut` in core (TC2) |

A precision limit is a function of the key and the code, so two runs agree. A per-function cap must **cut** and never widen to the
open key: which callers arrive first depends on the order of the walk (Rust's design, §7, says the same). The numbers are Python's
defaults where it has one; TC0 measures whether they fit TypeScript.

**A cut can make the precise tree smaller than the default tree.** A context that is cut has no edges, and the default resolver, with one
context per function, would have walked it. No fixture reaches a cap, so the fixtures cannot see this. Every phase's sweep therefore
counts, on the four projects, the sites the default graph draws that the precise graph does not; the number must be 0, or the phase is
not released. Python's cap behaves the same way (`solver.rs` `cost_limit`: a cut, and whoever read it is cut too).

## 13. Which file changes in which phase

| | `sem/` | `flow.rs` | `value.rs` | `env.rs` | `key.rs` | `eval.rs` | `walk.rs` | `calls.rs`, `session.rs` | core |
|---|---|---|---|---|---|---|---|---|---|
| TC1 | `sem/flow.rs`: sites, arguments, names | new: the types | new: `Value::unknown` only | a struct with a `Flow` | `KeyId`, the interner; `KeyId::OPEN` | new: returns unknown | the five `()` | `Options::precision`; `digest` | — |
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
| **the laws**, on generated values (`proptest`): `join` gives the same result in any order and twice; `Wide` absorbs; data changes nothing; two keys are equal exactly when their digests are; a key survives its canonical form and back | the value and the key are what §3 and §6 say, for cases nobody wrote by hand |
| **the previous phase from this one**: with the phase's new capability bit off, every graph equals the previous phase's (§15.2) | a phase adds and does not disturb |
| **two ways to the same tree** (Python's `Mode::Inline` against `Mode::Summary`): a small evaluator that enters every callee's table in place, with a recursion guard, against the solver's graph, on every fixture without a cycle | the keys and the solver agree with the plain reading of the code |
| the sweep: sites drawn at `Default` and missing at `Precise`; `value_type_mismatch` | 0 and 0 (§12, §7) |

## 15. Built to grow

The layer is built in ten phases over a long time, on an engine that changes every day. These five things make each step an addition and
not a rewrite. Each is cheap in TC1 and expensive to add later.

### 15.1 Numbers inside the layer

`Flow`, `Value` and `KeyData` hold numbers (`DefId`, `VarId`, `NameId`), never a `Func<'p>` (§4). Three things follow:

- the table of a body can be kept in `Sem`, once per program;
- keys can be interned (§6);
- **a memo can one day outlive a request.** Today a request builds its solver fresh, because a solver that holds `Func<'p>` borrows the
  project, and a `Session` cannot hold both. With numbers in the keys, summaries could be kept in the `Session` beside the project and
  dropped at a reload. It is **not built**: for the default resolver it would save a millisecond. The trigger is a number TC6 must report:
  the time of one "expand this node" request once return values exist, because such a request may walk deep for a value, and a tree opened
  level by level is hundreds of requests.

### 15.2 A capability set behind `Precision`

```rust
bitflags! {
    /// What the layer may know. `Precision::Precise` is every bit that is built; a test may take some away.
    pub(crate) struct Caps: u8 {
        const PARAMS = 1; const LOCALS = 2; const CAPTURES = 4; const OBJECTS = 8; const INSTANCES = 16; const RETURNS = 32;
    }
}
```

| What it gives | How |
|---|---|
| a stronger proof per phase | with the phase's new bit off, every graph is byte for byte the previous phase's. "Off is identical" (rule 7) is the case of no bit |
| a tag that carries a phase without turning it on | the public `Precision` maps to a set of bits; which set is one line |
| a regression on a real project, found | run it again with one bit less, until the graph is right |

It stays private: users see `Precision::Default` and `Precision::Precise`. (`bitflags` is in the lockfile.)

### 15.3 Versions

| What | Rule |
|---|---|
| a context id | `CONTEXT_ID_VERSION`; the canonical form only grows; bytes pinned per variant (§6) |
| the table of a body | not stored outside a process, so it has no version: its snapshots are its contract |
| a counter in `Stats.language` | a name is never reused for another meaning; a retired name is listed in the tag notes |
| the wire | core's `WIRE_FORMAT_VERSION`, unchanged by this plan |

### 15.4 The neutral files are written as if they were core's

| File | Imports from `sem` | Why it matters |
|---|---|---|
| `value.rs` (the set, `join`, the caps) | nothing | the same laws hold for any language; Rust's RC3 needs them |
| `key.rs` (the interner, the normal form, the digest) | nothing but `StableDef` | Python and Bend each have a copy of the digest today: the rule of two is already met for it (Q-TC6) |
| `eval.rs` | nothing: it asks its questions about classes through a trait | hand-written tests implement the trait in ten lines |
| the three test properties (off is identical, never fewer edges, a context is a function of its key) | nothing | they belong in `analyzer-testkit` when a second language runs them |

```rust
/// What the evaluator asks that is not in a `Flow`. `Sem` answers it; a test answers it by hand.
pub(crate) trait World {
    fn flow(&self, def: DefId) -> Option<Arc<Flow>>;
    fn member(&self, class: DefId, name: NameId) -> Member;      // TC5: a def with a body, a signature, external, nothing
}
```

Moving a file to core is then a move, decided when the second language needs it, and never a redesign.

### 15.5 Crates before our own code

| Need | Crate | In the lockfile today | Instead of |
|---|---|---|---|
| the key table; any map whose order reaches output | `indexmap` | yes | a hand-written interner; sorting a `HashMap` before printing |
| sets of one or two members | `smallvec` | yes | `Box<[One]>` |
| maps inside one request | `rustc-hash` | yes | the standard hasher, which is slow on purpose |
| sets of marks and capabilities | `bitflags` | yes | hand-written bit tests |
| the digest | `xxhash-rust`, `serde_json` | yes | — |
| the laws of `join`, of the normal form, of the canonical round trip | `proptest` (tests only) | no: one download | hand-picked cases, which is how an order-dependent join gets through |
| the groups of a cycle (TC6) | `petgraph` or `ena`, chosen in TC6's note | no | Python's hand-written union-find (`solver.rs` `Heads`) |

"In the lockfile" means tsrs already depends on it: no download and no new build (the disk is short).

Looked at and not taken:

| | Why not |
|---|---|
| `salsa` (memo with cycles; Python's engine is built on it) | core's rule is "no salsa", the solver is core's, and tsrs gives it nothing to track: a save rebuilds the whole program |
| a Datalog engine (`ascent`, `datafrog`) | the layer's math has that shape, but those engines work a whole program bottom-up; a request starts at one entry, under a budget, a deadline and a cancel |
| `rpds` (Python's env snapshots) | one value per variable has no snapshots: the design does not need what the crate gives |
| a fork of tsrs | upstream moves by many commits a day (61 between the pin and 2026-11-23); what the layer needs is public at the pin ([01](01-where-we-start.md) §4) |

Next: [`04-tc0-counts-and-spikes.md`](04-tc0-counts-and-spikes.md)
