# 03.12.08 TC4: objects and props

```
TC4
├── 0. before TC4: the cap per function, the allowance by site
├── 1. what the user sees
├── 2. `sem`: literals, members, patterns, JSX attributes
├── 3. values: `Object`, `Class`
├── 4. reading a member
├── 5. when a shape can be trusted
├── 6. JSX
├── 7. limits
├── 8. fixtures and tests
├── 9. steps (as first planned)
└── 10. checked against the code (2026-11-26): the steps to build from
```

Depends on TC3. With TC3 this is the phase React code shows: a parent's tree says what its children do with the
handlers it gave them. TC4 itself changes nothing in core; the cap of §0 does, and it comes first.

## 0. Before TC4: the cap per function and the allowance by site

Scheduled by the review before TC3 ([12](12-review-before-tc3.md) §7: §2.3 and §3.4). Two steps, each one prompt, before step 1.

**Step 0a, the cap on the contexts of one function** ([03](03-design.md) §12, the shape of [06](06-tc2-functions-through-parameters.md) §7;
decided 2026-11-26). TC2's N6 put it off (no function near 64 at the 95th percentile); TC4 adds objects to a key, TC5 receivers, and
each multiplies. As built:

- **core** (`solve.rs`): `Limits::max_contexts_per_function: Option<u32>`, `None` by default, so Rust, Bend and Python (which keeps its
  own `contexts_per_func`) do not change; `Limits::new` sets `None`. `Cut::MaxContextsPerFunction` (`"max_contexts_per_function"`).
  `graph_within` counts, per function, the contexts it **reads** in this request (walked, or taken from the memo: the same unit as
  `Budget::max_contexts`, so a warm memo cuts the same context, rule 11); a context past the cap is listed, `Incomplete`, counted in
  `Stats.cuts`, and not walked. A context on the frontier or cut by another limit is not counted. The checks run in this order:
  `max_contexts`, `max_depth`, the cap, the cancel.
- **TypeScript** (`calls.rs`): `Options::new` sets the cap to **64** (Python's number) for both precisions. With `Precision::Default`
  a function has one context, so it never fires; the in-crate measure (`measure_contexts`) uses the same limits, so it prints the cut.
- **Which context is cut** is the one the breadth-first walk reads 65th: it depends on the entry, as every cost limit does, and is
  marked. A cut can make a precise tree lose a subtree the default tree has ([03](03-design.md) §12): the sweep's comparison counts it.
- **Python's mapping** (`interp/core_graph.rs`, `ContextsPerFunc → MaxContexts`) is not changed here; it can name the new cut later, as
  a Python step of its own.

Proof: core's tests (a keyed toy: the third context of a function is cut at cap 2, the first two whole; `None` and a one-context
language never cut; the same graph twice on one solver); the row `cap-per-function` (under 64, every context) and a test of the same
fixture at cap 2 (`run`'s third context `Incomplete`, nothing calls `c`, which `third` still hands over, the properties hold); every language's tests green (Python's on
CI); the compiler's `createProgram`, whole graph: how many contexts are cut, and the comparison with the default graph.

**0a built as decided, `c6b88b2` (core) and `7021f7a`** ([`engine/11-tc4.md`](../../../languages/typescript/engine/11-tc4.md)): 500
passed, 13 ignored; Rust, Bend and core green. The four `*-off.txt` snapshots gained the row's section (they are written over every row).
On the compiler's `createProgram` (whole graph) the cap cuts **111** contexts, in 4 functions (`visitNode` 125 contexts listed); the
comparison with the default graph finds 0 problems.

**Step 0b, the allowance by site** (§3.4; decided 2026-11-26). `never_fewer_edges` allows exactly the sites the layer emptied (a callee
that is only `undefined`), not as many sites as `callee_undefined` counts: with a count, a bug that empties the wrong site passes whenever
another site was honestly counted (92 on the compiler's `createProgram`). As built:

- **The walk lists what it emptied.** In `walk::resolve_from_env`'s open-entry path, a site is emptied when `members` saw `Undefined`
  and no target is left (the only way the layer turns an engine answer into no edge). `resolve` says so beside its targets, and
  `body_of` records `(f, site)` on `TypeScript` (`emptied`). After the solve, the request turns it into a sorted, deduplicated
  `Vec<(StableDef, Site)>` (the def's id as the graph's edges name it). The counter `callee_undefined` and every output stay as they are.
- **Tests read it through a test-only feature**, `__testing` (as `__proof`: not API, a convention): `typescript_analysis::testing`
  with `call_graph_and_emptied` and `session_graph_and_emptied`, each the graph and the list. The crate's tests turn it on by listing
  the crate as its own dev-dependency with the feature (checked on 2026-11-26 in a scratch crate: Cargo accepts it, and a build without
  tests does not see the feature). The public call function still returns a `Graph` (rule 7).
- **`never_fewer_edges(default, precise, emptied)`**: a site of `default` with no edge in `precise` is allowed only when every edge it
  had was `Unresolved` **and** it is in `emptied`; and a listed site where `default` has an answer (an edge that is not `Unresolved`) is
  reported. Its callers (the context rows, `precise.rs`, the sweep's comparison) pass the list of the precise graph.

Proof: the row `callback-not-passed` (`main`) lists exactly `run`'s site `cb?.()`, asked at `run` nothing; the property's own test
(the right site allowed, no list or another site reported, a listed answered site reported); a walk that does not record fails the
rows' property test; every snapshot unchanged.

**0b built as decided, `d581079`** ([`engine/11-tc4.md`](../../../languages/typescript/engine/11-tc4.md)): 501 passed, 13 ignored, no
snapshot moved. Of the 97 context-row entries one lists a site (`callback-not-passed` `main`); on the compiler's `createProgram` the
list covers exactly the 3 sites the precise graph loses (the counter said 92). **Both steps of §0 are done; TC4 step 1 is next.**

## 1. What the user sees

Patterns B1–B8 of [02](02-patterns.md); lines d and e of the tour.

| Code | Before | TC4 |
|---|---|---|
| `open({ onDone: save })` · `function open(opts) { opts.onDone(); }` | `open` empty | `open` → `save` |
| `<Panel onClose={handleClose} />` · `function Panel(props) { props.onClose(); }` | `Panel` empty | `Panel` → `handleClose` |
| `function Panel({ onClose }) { const back = () => onClose(); … }` | `back` with nothing under it | `back` → `handleClose` (TC3 carries it into `back`) |
| `function Panel(props) { return <Inner {...props} />; }` | `Inner` knows nothing | `Inner` gets every member `Panel` got |
| `<Layout Icon={SaveIcon} />` · `<Icon />` in `Layout` | unresolved | `Layout` → `SaveIcon` |
| `register(JsonWriter)` · `new C()` in `register` | unresolved; `uncovered_class` | a construction of `JsonWriter` |
| `emit({ write() { encode(); } })` · `w.write()` in `emit` | unresolved | the literal's `write` → `encode` |

## 2. `sem`: literals, members, patterns, JSX attributes

| Source | `Expr` |
|---|---|
| `{ a: e, b, m() {}, ...c }` | `Object([Named(a, e), Named(b, Var b), Named(m, Def m), Spread(c)])`, in source order |
| `{ [k]: e }`, a getter or a setter in a literal | the object is **not complete**; that member is not listed |
| `x.k`, `x["k"]`, `x?.k` | `Member(x, k)` |
| `const { k, j: { i }, ...rest } = e` | `k` ← `Member(e, k)`; `i` ← `Member(Member(e, j), i)`; `rest` ← `Omit(e, [k, j])` |
| `function f({ k = d })` | `k` ← `Member(Param(0), k)` with the default `d`, as a parameter's default in TC2 |
| `<Tag a={e} b {...p}>…</Tag>` | one argument: `Object([Named(a, e), Named(b, Unknown), Spread(p), Named(children, Unknown)])` |
| a name the checker resolves to a class | `Def(class)`, which now evaluates to `One::Class` |

`sem/handed.rs` already walks binding patterns for the hand-over counters (`assigned_defs`, the `for … of` and destructuring rows of
the policy); the reader reuses those walks.

## 3. Values

```rust
pub(crate) struct Object {
    /// Sorted by name. A later write of the same name in the literal wins.
    members: Box<[(NameId, Value)]>,
    /// `false`: a spread of something unknown, a computed key or an accessor. A member that is not listed may still be there.
    complete: bool,
}
```

| Construct | Value |
|---|---|
| `{ a: e }` | `Object { a: eval(e) }`, complete |
| `{ ...s, a: e }` | `s`'s members, then `a`. If `s` is not one complete object: not complete, and only `a` (written **after** the spread) is listed |
| `{ a: e, ...s }` | if `s` is one complete object: `a` unless `s` has it. Otherwise `a` is **open** (the spread may replace it) |
| `Omit(o, names)` | `o` without those members |
| a class by name | `Class(class)`. `new p()` on it is a `Construct`, with `init` as for a class named there; `<P />` on it is the class-component construction |

A member's value is a `Value` like any other, so an object can hold a set, another object, a nested def with its captures.

## 4. Reading a member

```
member(v, k) = join over v.known:
    Object, has k                    its value
    Object, complete, no k           {Undefined}          so a default in a pattern applies, as for a missing argument
    Object, not complete, no k       unknown
    anything else                    unknown              (an instance: TC5)
  and open, if v is open
```

`opts.onDone()` is a **member call**: the callee is `Member(opts, onDone)`. If the member is a function it is called, and the engine's
answer (`function_typed_value`) is replaced. If the object holds a literal's method (`{ write() {} }`), that method is a def (D9) and
the target is its context, with the key built from the call's arguments.

## 5. When a shape can be trusted

A function value cannot change. An object can: `opts.onDone = other` after `open(opts)` was called makes the key's `{ onDone: save }`
a lie. Python answers this with effects in summaries; the layer answers it with a rule that needs none.

**In a body, an object held by a variable is trusted only if every use of that variable is one of:**

```
read a member                x.k    x?.k    const { k } = x
spread it                    { ...x }    <T {...x} />
pass it to a JSX element or to a project function that leaves it alone        (below)
compare it, test it          if (x)    x === y    typeof x
```

**Any other use makes its members open, in that body:** a write through it (`x.k = v`, `x[e] = v`, `delete x.k`, `x.k++`,
`Object.assign(x, …)`), passing it to code that may write through it, storing it (`y.f = x`, `[x]`, `{ f: x }`), returning it.
Because a variable has one value in a body (§5 of the design), "open in that body" covers the uses before the write as well as after,
and every key the body passes it in.

**Three cases the lists above must name** (added by the review of 2026-11-23):

| Case | Rule |
|---|---|
| a second name: `const o = opts`; `const { actions } = props`, where `actions` is an object | not a store: allowed, and the test then covers **both** variables. If either is written through, stored or returned, the object is open under both names. Two names defined through each other: open |
| a variable of an enclosing function, used in a nested def | "in a body" means in the variable's whole scope, the nested functions included, as for assignments ([03](03-design.md) §5) |
| a `const` at the top of a file | every def of the file can use it, and so can every file that imports it. Trusted only if the declaring file passes the test **and** the constant is not exported; otherwise its members are open, counted `module_object_shared`. N5 says whether a look across the project is worth building |

An array literal that holds the object (`[props]`, a hook's dependency list) is a store by the list above. N5 counts it before the row is kept.

**"Leaves it alone"** is a fact about a function and one of its parameters, from its table alone:

```
touches(g, i) =  parameter i of g has a use that is not in the first list above,
                 where "a project function that leaves it alone" is asked again for the function it is passed to
                 a function asked for while being computed: true          (a cycle)
                 code we cannot read (a package, a `.d.ts`): true
```

It does not depend on a key, so it is computed once per (function, parameter) and rule 1 holds.

The last line is the careful half. `console.log(opts)` and `JSON.stringify(opts)` make `opts` open, although they change nothing. N5
(TC0) says how often that happens. If it is often, the question for you is whether a library is assumed to leave an object's members
alone: that would be a second stated exception to "never guess" (D28 is the first), and I would rather see the number first.
A set of JSX attributes is not affected: the object is made at the element and handed straight to the component.

## 6. JSX

A JSX element is already a call site on its tag (D8). TC4 gives it an argument:

- **A function component** gets the attributes object as parameter 0, like any call.
- **A tag that is a value** (`<Icon />` with `Icon` a prop, a local, a member): the tag is evaluated like a callee. A function is
  called; a class is constructed.
- **A class component**: the attributes go to the constructor's parameter 0. `this.props` in its methods is a field: TC8.
- **`children`**: listed as unknown. A function passed as a child (`{x => …}`) is an anonymous function: TC7 (D54).
- **A component wrapped by a library** (`memo(Panel)`, `forwardRef(…)`): the tag's value is a call's result. Unknown here; TC10 has
  the row.

## 7. Limits

`KeyPolicy::max_depth` (2) counts objects inside objects: `{ actions: { save } }` is depth 2 and kept; a member one level deeper is
unknown (`value_widened_depth`). `KeyPolicy::max_members` (64): a larger literal is unknown as a whole (`value_widened_members`).
`capable` (TC2 §2) already keeps an object-typed parameter; a parameter of an object type **none of whose members can hold a function or
an object** (`{ id: string; n: number }`) is not capable **[check]** that the checker's property list makes this cheap.

**Relevance by member.** Python's relevance is per parameter, and for an object that is too coarse: `Dialog` reads `props.onClose` and
nothing else, so two `<Dialog>` elements that differ in a handler `Dialog` never reads would be two contexts. When a parameter is only ever
read through members or a pattern (never passed on whole, spread, stored or returned), its slot holds **only the members the body reads**;
`Flow::facts` lists them. A parameter that is used whole keeps the whole object. **[measure]** on V-NOC: contexts per component, with and
without.

## 8. Fixtures and tests

`object-options` · `object-shorthand` · `object-method` (the literal's own method) · `object-nested` · `object-too-deep` ·
`object-spread-before` · `object-spread-after` · `object-spread-unknown` · `object-rest` (`Omit`) · `object-pattern-param` ·
`object-pattern-local` · `object-pattern-default` · `object-computed-key` (not complete) · `object-written-through` (open) ·
`object-written-in-callee` (`touches`) · `object-to-external` (open, counted) · `object-stored` · `object-implements` (B7) ·
`props-function-component` · `props-destructured` · `props-nested-handler` (A8, whole) · `props-spread-through` ·
`props-component-as-prop` · `props-class-component` · `props-children` (unknown) · `class-as-value` · `class-as-value-jsx`.
Added by the review: `object-second-name` · `object-second-name-written` (open under both) · `object-module-const` ·
`object-module-const-exported` (open, counted) · `object-in-array` · `props-unread-member` (two elements that differ in a member the
component never reads: one context).

| Test | |
|---|---|
| the three properties on every fixture | |
| `touches` printed in the table snapshots | reviewed |
| `eval` on hand-written tables: spread order, `Omit`, defaults, the depth cap | no engine |
| the tour's lines d and e | their final column |
| V-NOC's `TaskDetailPanel` **from its parent**: of its 19 callback-prop calls, how many resolve | the number this phase is for |
| the four projects | the per-site measure; `object_escapes_external`, the `value_widened_*` counters |

## 9. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `sem/flow.rs`: `Object`, `Member`, patterns, `Omit` | table snapshots |
| 2 | `value.rs`: `Object`; `eval`: literals, spread, member, defaults in patterns | unit tests; `object-options`, `object-pattern-*` |
| 3 | member calls answered; a literal's method as a target | `object-method`, `object-implements` |
| 4 | the trust rule and `touches` | `object-written-*`, `object-to-external`, `object-stored` |
| 5 | JSX attributes as the argument; a tag that is a value | `props-*` |
| 6 | `Class` | `class-as-value*`; `uncovered_class` drops where an edge appears |
| 7 | the depth and member caps | `object-too-deep`; pattern R3 ends |
| 8 | the four projects; `TaskDetailPanel` from its parent | `ENGINE.md` |
| 9 | (mine) `policy.md`, `ENGINE.md`; the N5 question, if the number asks it | reviewed |

No tag at TC4 (D64: the first tag that carries the layer is TC5's, with the layer on by default). The steps above are as first planned;
§10 corrects their order and content against the code, and its table is the one to build from.

## 10. Checked against the code (reviewer, 2026-11-26, at `684ef7e`, after TC3 and §0)

Read against `flow.rs`, `value.rs`, `eval.rs`, `key.rs`, `captures.rs`, `relevance.rs`, `needs.rs`, `sem/flow.rs`, the tour's table
(`tour.flow.txt`) and Python's `interp/key.rs` and `relevant.rs`. What holds, and what changes.

**What holds.** `Caps::OBJECTS` (8) is already a bit: TC4 needs no wider `Caps`. A destructured parameter's names are variables of the
table already (the tour: `Panel`'s `onClose@13 capable open`, `#0 pattern`): TC4 gives them `Member(Param(0), onClose)` in place of
`open`, which is exactly V-NOC's 94 prop sites ([`engine/10-tc3.md`](../../../languages/typescript/engine/10-tc3.md), "V-NOC's open
sites"). `SiteFlow::args` of a JSX site is one `Unknown` today, the place §6 fills. D66 decided §5's library question: an object
handed to a library is open. N5 (TC0) counted the trust rule's cost: objects passed in are almost never written through (0.3–2%), local
literals often (26–36%), and "handed to a project function" is the large row (21–58%), so `touches` is what keeps TC4 useful.

**What changes** (each decided here; none is the owner's):

| # | The plan said | The code says | So |
|---|---|---|---|
| 1 | §3: `Object { members: Box<[(NameId, Value)]>, complete }` as a member of a value | `One` is `Copy` and small; a value that holds other values goes through a request table, as `One::Closure(DefId, Captures)` does (`captures::CaptureSets`, an `IndexSet`) | **`One::Object(Objects)`**, a number into a request table of objects (`objects.rs`, beside `captures.rs`), each in its normal form (members sorted by name, `Anon` written `AnonAny`, unknown members kept: an unknown member is not a missing one). Added **after** `Undefined` in `One`'s order, so no existing order or digest moves |
| 2 | §6 of the design: the digest is of the canonical key | `Keys::intern` writes a capture list through a `list` closure, so the digest never holds a request's own number | an object is written into the canonical key **by its members** (name, canonical value), never by its number; every pinned digest of TC2/TC3 unchanged |
| 3 | §7: `max_depth` (2) counts objects in objects; captures have their own `MAX_CAPTURE_DEPTH` (2) | Python has one depth (`fp_depth`, 2) for every step from a parameter: an attribute, a free value, a default | **one depth** for objects and capture lists together (2), counted `value_widened_depth`; a closure in a closure is cut where it is today |
| 4 | §9: the depth and member caps at step 7 | from step 2 a key can hold an object made from a key (R3: `walk(node) { walk({ parent: node }) }`), so without the cap the keys of a recursion never end | **the caps come with step 2**, with the row `object-too-deep` |
| 5 | §9: member calls answered (3) before the trust rule (4) | the layer replaces the engine's open answer where it knows the value; an object written through after it was passed would make that answer wrong | **the trust rule before any answer**: step 3 is the trust rule and `touches`, step 4 answers member calls |
| 6 | §2–§5 add `Expr` variants | `relevance.rs` matches `Expr` exhaustively (`relevance.rs:82`: a new variant does not compile until it is read); `needs::each_expr` (`needs.rs:167`) recurses only into `OneOf` with an `if let`, so a new variant would be skipped **silently** | step 1 makes `each_expr`'s recursion a `match` with no `_` arm, and gives each new variant its reading in both: `Member(x, _)` and `Omit(x, _)` read `x`, `Object` reads each member's expression. A parameter read only as `props.onClose()` is then relevant (today it is not: the callee is `Unknown`) |
| 7 | §7 "relevance by member", with a **[measure]**; §8's row `props-unread-member` expects one context | Python's relevance is per parameter (`relevant_params`), and its key holds every attribute of an object (`Fp::Instance`) | not built in this phase's first pass: the row is written at step 5 with **two** contexts (Python's answer), and step 7 measures the ceiling (contexts per component, and the same with each slot cut to the members the body reads, from the table, as N6 was). Built at step 7b only if it pays, then the row flips (D67's rule) |
| 8 | the README's point 8: an off snapshot per bit | none for `OBJECTS` yet | `objects-off.txt` blessed at step 2, before any code reads the bit |
| 9 | "After step 8 is where I would tag" | D64 | no tag (above) |

**The steps, as they will be built** (re-ordered again at step 2's design, 2026-11-26: a destructured prop, `const { onClose } = props`,
reads through the object as soon as `OBJECTS` is on, so the trust rule must exist **before** the bit is turned on, not only before member
calls are answered):

| # | Do | Proof |
|---|---|---|
| 1 | `sem/flow.rs`: `Object`, `Member`, `Omit`, `Default`, patterns, JSX attributes as an `Object` argument; every reader as `Unknown` | table snapshots; no graph moves (**built**, `9e64de5`) |
| 2 | values, **off**: `One::Object` in the request's table of nested values (rows 1–3), `eval` of the four forms behind `Caps::OBJECTS`, the depth and member caps (row 4), the digest; `OBJECTS` not in `Caps::BUILT` | unit tests on hand-written tables; a pinned digest; no graph moves |
| 3 | the trust rule and `touches`, **off** (a fact per function and parameter, kept in `Sem` as relevance is) | unit tests; table snapshots that print it |
| 4 | **on**: `OBJECTS` in `Caps::BUILT`; `objects-off.txt` first; `relevance` and `needs` read the new forms (row 6); a pattern parameter is capable; a whitespace-only JSX child is no child | `object-options`, `object-pattern-*`, `object-too-deep`, `object-written-*`, `object-to-external`, `object-stored`, `object-second-name*`, `object-module-const*`, `object-in-array`, `props-function-component`, `props-destructured`, `props-nested-handler` |
| 5 | member calls answered (`walk::holds_name` of a `Member`); a literal's method as a target | `object-method`, `object-implements`, `props-*` read through `props.x` |
| 6 | a JSX tag that is a value | `props-component-as-prop`, `props-spread-through`, `props-children`, `props-unread-member` (two contexts) |
| 7 | `Class` | `class-as-value*`, `props-class-component`; `uncovered_class` drops where an edge appears |
| 8 | the four projects; `TaskDetailPanel` from its parent; the ceiling of relevance by member | `engine/11-tc4.md` |
| 8b | relevance by member, only if step 8 says it pays | `props-unread-member` flips |
| 9 | (mine) `policy.md`, `ENGINE.md`; the README's phase rule | reviewed |

Each step is one prompt; step 1 or 2 may split when its prompt is written.

**Step 2, as decided for the build (2026-11-26).** Values for objects, behind a bit that stays off.

- **One table for values that hold values.** `captures.rs` becomes `nested.rs`, `CaptureSets` becomes `Nested`, and it holds the objects
  too, so one depth covers both (row 3): `MAX_DEPTH` (2, was `MAX_CAPTURE_DEPTH`). An object is `One::Object(ObjectId)` (after
  `Undefined`), its data `ObjectData { members: Box<[(Box<str>, Value)]>, complete: bool }`, members sorted by name. Interning: an
  `Anon` in a member is `AnonAny`; an inner closure too deep is its `Fn` (as today); an inner object too deep is dropped from its value,
  which becomes open; both counted `value_widened_depth`. More than `MAX_MEMBERS` (64) members: no object (the value is unknown),
  counted `value_widened_members`. The depth of a list or an object is 1 plus the deepest list or object among its values.
- **`eval`**, only when the world knows `OBJECTS` (else unknown, as in step 1):
  - `Object(parts)`, in order: `Named(k, e)` sets `k` (a later one wins); `Unnamed` makes the object not complete and every member set
    before it open; `Spread(e)`: `Undefined` adds nothing; a value that is exactly one object (not open) copies its members over, and if
    that object is not complete, the object is not complete and the members set before it are open; anything else (data, a function,
    several objects, open) makes the object not complete and the members set before it open.
  - `Member(x, k)`, a join over the members of `x`'s value: an object that has `k`, its value; a complete one without `k`,
    `{Undefined}`; one that is not complete, unknown; `Undefined`, `{Undefined}` (reading through nothing calls nothing); anything else
    (a function, data), unknown; and open if `x`'s value is open.
  - `Omit(x, names)`: one object (not open) without those members, complete as it was; anything else unknown.
  - `Default(x, d)`: where `x`'s value may be nothing (it holds `Undefined`, or is open), `d`'s value takes `Undefined`'s place, the rule
    `passed` already has for a parameter's default (one helper for both).
- **The digest**: `Canon::Object { members: Vec<(String, Canon)>, complete: bool }`, the last variant, an object written by its members,
  never by its number; `Keys::intern` is given the objects as it is given capture lists. A pinned test fixes its bytes; every pinned digest
  of TC2 and TC3 is unchanged.
- `World` gets `object` (intern) and `object_data` (read), as it has `capture` and `captured`.

**Step 2 built as decided, `e698cfe`** (`engine/11-tc4.md`): 521 passed, 13 ignored, no snapshot moved. Step 2b adds the test the
review found missing (an object's depth counts its closures) and the rounds of a variable that holds an object.

**Step 1, as decided for the build (2026-11-26).** The table reads the new forms; nothing evaluates them yet, so no graph moves.

- `Expr` gains four variants: `Member(Box<Expr>, Box<str>)` (`x.k`, `x?.k`, `x["k"]` with a string literal), `Object(Box<[Part]>)`
  (an object literal, a JSX element's attributes; parts in source order), `Omit(Box<Expr>, Box<[Box<str>]>)` (an object pattern's
  `...rest`: the source without the names listed before it) and `Default(Box<Expr>, Box<Expr>)` (a pattern element's `= d`). `Part` is
  `Named(Box<str>, Expr)` (`k: e`, shorthand `k`, a method `k() {}` = its def, a JSX attribute), `Spread(Expr)` or `Unnamed` (a
  computed key, a getter, a setter: the object is not complete). Normal form: a `Member` or `Omit` of `Unknown` is `Unknown`.
- A property access first asks `functions_named` as today (a function of a namespace, an import, stays a `Def`); only where that
  gives no function is it a `Member`. An element access with a numeric literal stays `Unknown` (arrays are not modelled).
- **Patterns**: a binding element of an object pattern gets its value from its path (`Member` of the source by its property name,
  `Default` for its `= d`, `Omit` for `...rest`); the source is the def's own parameter (`Param(i)`), a declaration's initializer, or the
  enclosing element. An array pattern, a parameter of a nested function, a declaration that is open (loop, catch, ambient, a script's
  top) and a computed key keep the variable open, as today. A pattern parameter's row (`named: false`, `capable: false`) is unchanged:
  step 2 makes it capable.
- **JSX**: the site's one argument is an `Object` of the attributes (an attribute with no value is `Data`, a string `Data`, `{e}`
  `e`, `{...e}` a `Spread`), with `Named("children", Unknown)` last when the element has children.
- Every reader of `Expr` gives the new variants the reading of `Unknown` for now: `eval` unknown, `relevance` reads nothing,
  `walk::holds_name` false, `walk::reads_many_sources` none, and `needs::each_expr` does not enter them, written as a `match` with no
  `_` arm so step 2 cannot forget one. Step 2 gives them their real reading (§10 row 6) together with `eval`.

**Step 1 built as decided, `9e64de5`** ([`engine/11-tc4.md`](../../../languages/typescript/engine/11-tc4.md)): 502 passed, 13 ignored;
only the table snapshots moved. A shorthand is read through its value symbol; a def read through a member is not a hand-over; a rest
parameter's pattern stays open.

Next: [`09-tc5-instances.md`](09-tc5-instances.md)
