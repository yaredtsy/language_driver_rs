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
| 10 | (found at step 2b) | an object is interned by its members, so a variable whose object is built from a variable that settles over `solve`'s rounds keeps one object per round (the round join keeps them): `y = {a: x}` with `x = f; x = g` holds `{a: data}` and `{a: {f, g}}`. Sound, but two keys can hold equal values with different digests | step 4 makes the projects' sweep count values that hold two or more objects; if step 8 shows they split contexts, a value drops an object whose every member is contained in another object of the same value, at the end of `solve`. Not built before the number |
| 11 | (found at step 3b's design) step 3: "`eval`'s `Member`, `Omit` and a spread read a variable's or a parameter's object as open when it is not trusted" | a variable's value also goes, unopened, into a key (`g(x)`), a capture list, a literal (`{ f: x }`) and a second name (`const o = x`): `function f(x) { x.a = other; g(x); }` would hand `g` the key's `{a: save}`, and `g`'s `p.a()` would be answered `save` | **the read of the variable opens it**, not the read through it: `eval` of an untrusted `Var` or named `Param` gives its value with **each object opened** (every member open, not complete), and nothing else changed, so a function is never opened by the rule. A read through it is then open by `member`'s own rule (an open member reads open). A pattern parameter is never opened (read at the entry) |
| 12 | §5: "a function asked for while being computed: true (a cycle)" | a depth-first memo that answers "true" on a cycle makes every recursive function that passes its object down untrusted (`walk(node, ctx) { walk(child, ctx) }`, common in the compiler) | trust is solved **whole**, as `needs` is: the variables reached from the one asked (through second names and the parameters it is passed to), then "not trusted" spreads back from every variable that has a use the rule does not allow. A cycle with no such use is trusted. It is the least answer, so it does not depend on which variable is asked first (rule 9 with a memo per program) |

**The steps, as they will be built** (re-ordered again at step 2's design, 2026-11-26: a destructured prop, `const { onClose } = props`,
reads through the object as soon as `OBJECTS` is on, so the trust rule must exist **before** the bit is turned on, not only before member
calls are answered):

| # | Do | Proof |
|---|---|---|
| 1 | `sem/flow.rs`: `Object`, `Member`, `Omit`, `Default`, patterns, JSX attributes as an `Object` argument; every reader as `Unknown` | table snapshots; no graph moves (**built**, `9e64de5`) |
| 2 | values, **off**: `One::Object` in the request's table of nested values (rows 1–3), `eval` of the four forms behind `Caps::OBJECTS`, the depth and member caps (row 4), the digest; `OBJECTS` not in `Caps::BUILT` | unit tests on hand-written tables; a pinned digest; no graph moves |
| 3 | the trust rule and `touches`, **off** (a fact per function and parameter, kept in `Sem` as relevance is): 3a the table's uses, 3b the solve and `eval`'s reading, 3c the counters | unit tests; table snapshots that print it |
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

**Step 3, as decided for the build (2026-11-26).** The trust rule of §5, in two halves, both off.

- **3a, the table records the uses** (`sem/flow.rs`, plain data). For every capable row, a `Uses` read from every reference of the
  variable's symbol in the declaring def's **whole** node, nested defs included (the scope §5 asks for): `escapes` (a use the rule does not
  allow), `passed` (argument `i` of a call whose callee the engine names as project functions, `(def, i)` for each; a JSX attribute's
  value given to a component named so, `(component, 0)`), `names` (second names: `const o = x`, `y = x`, and the variables of
  `const { a } = x`), `receiver` (a method is called on it: Q-TC9) and `exported` (a module row). A pattern parameter's row in `params`
  gets the variables of its pattern as its `names`. Each reference is placed by its parent, after the nodes that only wrap it
  (parentheses, `as`, `!`, `satisfies`):
  - the object of an access chain (`x.a`, `x.a.b`, `x["a"]`): allowed, unless an access of the chain is written (`=`, `+=`, `++`,
    `delete`): escapes; a chain that ends as the callee of a call: allowed, and `receiver`. **Fixed at 3a's review (3a-fix):** a chain
    whose top can hold a function or an object (its type is not `holds_only_data`) is then placed again, as if the top were the
    reference: `const b = x.b; b.c = f` makes `b` a second name of `x` (so the write through `b` reaches `x`), `return x.b` escapes,
    `g(x.b)` is `passed (g, 0)`. A top that is data (`x.label`) stays allowed. Without it a write through a member read into a variable
    was not seen, and `x.b.c()` would be answered from a key the code had changed;
  - the callee of a call (`x()`), a condition (`if`, `while`, `for`, `do`, `?:`'s test), the operand of `!` or `typeof`, either side of
    a comparison (`===`, `!==`, `==`, `!=`, `instanceof`, `in`), an expression statement: allowed. `&&`, `||`, `??`: allowed only when
    the binary is itself an expression statement or a condition, else escapes;
  - `{...x}`, `<T {...x} />`: allowed;
  - argument `i` of a call or a `new` (not spread): `passed` when the engine names the callee as project functions only
    (`functions_named`, no class), else escapes (a library, an unknown callee: D66);
  - `<T a={x} />`: `passed (T, 0)` when the tag names project functions only, else escapes;
  - the initializer of a declaration, or the right side of `y = x` where `y` is a variable with a row: `names` (every variable of an
    object pattern; an array pattern escapes);
  - anything else (a return, a store `o.f = x`, `{ f: x }`, `[x]`, a spread argument `f(...x)`, a template, an unknown place): escapes.
- **3b, the rule is solved** (`trust.rs`, neutral, kept per program in `Sem` as relevance is): `trusted(var) = !escapes && !exported
  && every (g, i) in passed: !touches(g, i) && every second name trusted`; `touches(g, i) = !trusted(parameter i of g)` (its row, or for a
  pattern parameter its `names`), `true` for a def with no table; a cycle is `true`. **A pattern parameter's own elements are never
  opened in its body**: they are read at the entry, before any code of the body runs, so no write can come between; its uses matter only
  to `touches`, for a caller that reads the object it passed after the call (a JSX element's attributes are never read again). `eval`'s `Member`, `Omit` and a spread read a
  variable or a parameter's object as open when it is not trusted (only with `OBJECTS`). Counted, once per body: `module_object_shared`
  (an exported module row read through) and, for Q-TC9 (a), `this_written_outside_class` (a def that is not a class member whose body
  writes through `this`, by the same placement) and `member_call_on_trusted` (a trusted variable with `receiver`). **Split at 3b's design:**
  3b is the solve and `eval`'s reading (below); **3c** is the three counters, with the table's `this` placement they need.

**Step 3b, as decided for the build (2026-11-26).** Rows 11 and 12 correct the bullet above; this is what is built. Off: `OBJECTS` is not
in `Caps::BUILT`, so no graph moves.

- **`trust.rs`** (neutral, as `needs.rs` and `relevance.rs`: imports nothing from `sem`, asks only `World`).
  `trust_of_component(world, var) -> Vec<(VarId, bool)>`: `var` first, then every variable reached from it, each with its answer. A
  variable's **node**: its row (`world.flow(var.def)`, `var_index(var.at)`). No table or no row: not trusted. A row that is not capable:
  trusted, and nothing follows (data has no member to write). Otherwise it is **bad** when `uses.escapes || uses.exported`, and it
  follows each of `uses.names` and, for each `(g, i)` of `uses.passed`, parameter `i` of `g`: a named parameter's row (`param_var(i)`),
  a pattern parameter's `uses.names`; **bad** instead when `g` has no table, `i` is past the last parameter (`arguments`), parameter `i`
  is a rest parameter (its elements are not followed), a pattern parameter has `uses.escapes`, or a named one has no row. A variable the
  world already knows (`World::trusted_known`, not the one asked) is a fixed node: its answer, nothing followed, not returned. Then "not
  trusted" spreads back from every bad node to every node that reaches it; the rest are trusted. `Uses::receiver` is not read (Q-TC9).
  `touches(g, i)` is not a function of its own: it is the answer of the variables parameter `i` of `g` stands for.
- **The memo**: `Flows::trust` in `Sem` (`trust_memo`, `remember_trust`), a fact of the program, as `needs` and `relevance`.
  `World::trusted(var)` and `World::trusted_known(var)`; `TypeScript` answers through the memo and keeps a whole component; `Quiet`
  passes both through.
- **`eval`** (row 11): `as_read(world, var, value)` at the end of `variable` (a row of this body, a captured entry, a module's value) and
  of `param` for a **named** parameter (`var` is its row's `VarId`; no row: not trusted). Only when the world knows `OBJECTS` and the
  value holds an object (trust is not asked otherwise). Not trusted: `objects_opened`, each `One::Object` made again by `made_object` with
  every member joined with unknown and `complete: false`, every other member of the value as it was. Opening is idempotent (an object is
  interned by its members). `Member`, `Omit` and a spread are unchanged: they read what the variable gave.
- **Not decided here, written for step 8**: 3a's "a chain whose top can hold a function or an object is placed again" treats a
  function-valued member as capable, so `<button onClick={props.onClose}>` makes `props` (and the object a caller passed) untrusted, though
  handing a function on cannot write the object's members. Step 8 counts how often a variable is untrusted only through such a chain
  before the rule narrows to "a top that can hold an object other than a function".

Proof: unit tests on hand-written tables (`trust.rs`, `eval.rs`); the trust of every capable row of `trust.tsx` in a reviewed snapshot
(`trust.trust.txt`), the fixture extended with a cycle, a rest parameter and pattern parameters; the same answers asked in the other order
on a fresh `Sem`; every other snapshot unchanged.

**Step 3b built as decided, `68af644`** ([`engine/11-tc4.md`](../../../languages/typescript/engine/11-tc4.md)): 544 passed, 13
ignored; only `trust.flow.txt` (new entries) and the new `trust.trust.txt` moved.

**Step 3c, as decided for the build (2026-11-26).** Three counters, each a fact of a body (`note_body`: said at a def's first walk of the
request), only when the world knows `OBJECTS`, so nothing moves while the bit is off. They are numbers for two open questions, upper
bounds by design:

- **`member_call_on_trusted`** (Q-TC9 (a)): the rows of the body's own table that are capable, have `uses.receiver` and are trusted. Each
  is a variable whose object a method may change if that method writes `this`.
- **`this_written_outside_class`** (Q-TC9 (a)): 1 for a body whose def's `this` is not trusted. The table gets **`Flow::this_uses:
  Option<Uses>`**: `Some` for a function declaration, a function expression, and a method or accessor of an object literal; `None` for a
  class member, a constructor, an arrow (its `this` is the enclosing def's), a class and a file's code. Each `ThisKeyword` in the def's node
  is placed by `Reader::place`, as a reference of a variable is; a nested arrow is entered, a nested non-arrow function or class is not.
  "Not trusted" is `trust::uses_trusted`: the same rule as a row's (bad uses, then every name and passed parameter trusted).
- **`module_object_shared`** (§5's table, N5's question): the distinct rows of a file's own code that the body's table reads (an
  `Expr::Var` anywhere in it, inside `Member`, `Object`, `Omit` and `Default` too) that are capable and `exported`. Such a variable is
  never trusted, so its objects are read open.

`trust::body_counts(world, def, flow)` computes the three; `TypeScript::walk` notes them beside `variable_write_unmodelled`. Policy
rows for them wait for step 4 (the bit on).

**Step 3c built as decided, `b92d671`** ([`engine/11-tc4.md`](../../../languages/typescript/engine/11-tc4.md)): 547 passed, 13
ignored; only `trust.flow.txt` and `trust.trust.txt` moved. `this_written_outside_class` counts any `this` that is not trusted, so a
`return this.n` counts as a write: read the number as an upper bound. A receiver that is not capable is not counted, and no test shows it
yet (owed in step 4).

**Step 4, as decided for the build (2026-11-27).** The bit on. Member calls are still not answered (step 5), so what moves is what is
read **out of** an object into a variable: a pattern (`const { onDone } = opts`, a pattern parameter), a member read into a name
(`const done = opts.onDone`), a nested def that reads through a captured object. Two commits.

- **4a, before any code changes**: the fixtures of the twenty policy rows `object-*` and `props-*` (`policy.md`, "With the context
  layer"), their snapshots with **both halves as the code gives today** (the bit is off in both), and the flow-fixture lines below with their
  table snapshots as today; the test `the_objects_bit_off_is_tc3` writing `snapshots/context/objects-off.txt` with `PARAMS | LOCALS |
  CAPTURES | MODULES | RELEVANCE` named bit by bit; and `the_relevance_bit_off_is_step_5` given its bits by name (`PARAMS | LOCALS |
  CAPTURES | MODULES`) in place of `BUILT - RELEVANCE`, which would take `OBJECTS` in with 4b. No other change: 4b's diff is then
  exactly what the bit changes.
- **4b, the bit**: `OBJECTS` in `Caps::BUILT`.
  - **The readers read the new forms, only with the bit** (row 6): `needs::each_expr` takes `objects: bool` and, when it is set, enters
    `Member(x, _)` and `Omit(x, _)` (`x`), `Object` (each `Named` and `Spread` value) and `Default` (both); when it is not, it enters none
    of them, as today. `relevance::Marks::atoms` reads the same, under the same flag. `needs` and `relevance` pass
    `world.knows(Caps::OBJECTS)`; `trust::body_counts` passes `true` (it runs only with the bit) and **`each_var` goes**: `each_expr`
    now visits every expression inside. Without the flag a function read through a member (`cb.call()`) would become relevant with the
    bit off and split contexts for nothing, and the off snapshots would move.
  - **The memos are kept per bit**: `Flows::needs` and `Flows::relevance` are keyed by `(DefId, bool)`, the bool being whether the world
    knows `OBJECTS`; `Sem::{needs_memo, remember_needs, relevance_memo, remember_relevance}` take it. One `Sem` serves requests with the bit
    on and off (the tests do), and a memo filled with one must not answer the other. The trust memo is not keyed: trust is asked only with
    the bit.
  - **A pattern parameter is capable** when it is an object binding pattern, not rest, and its type can hold a function or an object (the
    `holds_only_data` test of a named parameter, on the type of the parameter's node). An array pattern and a rest parameter's pattern stay
    not capable (arrays are not modelled; a rest parameter is not a slot).
  - **A JSX child the engine marks as only white space is no child** (`JsxText` with `containsOnlyTriviaWhiteSpaces`: white space with a
    line break; `<a> </a>` keeps its child, as React does), nor is an expression container with no expression (`{/* note */}`).
  - **`key_slot_objects_two_or_more`** (row 10's number for step 8): `intern_key` notes, per walk, each slot of the key (a parameter or a
    captured entry) whose value holds two or more objects at its top.
  - The test owed by 3c: a receiver row that is not capable is not counted by `member_call_on_trusted`.
- **What moves in 4b**: the "on" halves of the twenty new rows and of `tour` (`main` and `lineE`: `back` calls `join`); the table
  snapshots where an object pattern parameter gains `capable` and where a white-space child goes; the counters of keyed tests that run
  `Caps::BUILT`. `locals-off.txt`, `captures-off.txt`, `modules-off.txt`, `relevance-off.txt` and `objects-off.txt` do not move, nor any
  "off" half. Every other row's change is a finding to be read.

**Step 4 corrected at 4b (2026-11-27).** 4a was built as decided (`1ec66e4`: the twenty fixtures, `objects-off.txt`, 548 passed). With
the bit on, the coder stopped on four differences from the rows; each is decided here.

1. **Member calls are answered at step 4.** `walk::resolve_from_env` evaluates the callee of a site whose engine answer is open, and
   a `Member` callee now has a value: `opts.onDone()` calls `save`, the tour's line d calls `encode`. Accepted, not gated: the trust rule
   is what makes an answer safe, and it is in place (row 5); a gate would be code removed at step 5. Step 5 keeps the rest of its row: a
   literal's method as a target, and what the layer says beside the engine at a member call the engine names (`walk::holds_name`, D59).
2. **A body's rows are solved in the order of their reads.** `solve` started every row at the empty value, which is also data, and a
   member of data is unknown; the round join kept that, so `const o = opts; const { onDone } = o` read `onDone` open although every name is
   trusted. A member of data stays unknown (a string's `trim` is a function: reading it as nothing would drop a call). Instead the rows
   are split into strongly connected components of "row reads row" (a `Var` of this body anywhere in what a row is assigned), solved
   dependencies first: a row in no cycle is evaluated once, from settled rows; a cycle iterates as before (from data, with the round
   join, at most 32 rounds; a cycle that does not settle opens every row of the body, `variable_rounds_capped`). For values of functions
   only, the result is the same least answer (the off snapshots must not move); only a read through a member depends on the order.
3. **A pattern or a second name declared in a nested def is that def's row.** The placement of a use (step 3a) named the variables of
   `const { a } = x` and `const o = x` through `var_of`, which accepts only a variable of this def or one around it, so a pattern in a nested
   def (a module object read in a function; `props` read in `back`) made the variable escape. The placement names a **declared** variable
   by its declaring def, wherever it is in this def's node; `var_of`'s scope rule stays for the table's `Expr::Var`.
4. **`dataPattern` (`{ id }: { id: string }`) is capable**, as a named parameter of that type already is: `holds_only_data` takes any
   object type as able to hold an object. §7's narrowing (an object type whose members are all data) stays a [check], for every parameter
   at once, and would move TC2's snapshots: not in TC4 unless step 8 counts contexts it would save.

**Step 4 built as decided, `1ec66e4` and `aaf5f53`** ([`engine/11-tc4.md`](../../../languages/typescript/engine/11-tc4.md)): every
row as written, no off snapshot moved; a cycle through a member read stays open (sound). One CLI test from TC1 assumed `--precise`
prints nothing new on the scenarios; it now prints a counter, and 4b-fix narrows the test.

**Step 4c, found at the samples (reviewer, 2026-11-27): a key keeps only what can reach a call.** With the bit on, the private
codebase's sample went from 6,104 to 8,062 contexts and the cap of 64 per function cut 349, all at three functions called from many
places with object literals whose members are all data (`logEvent(name, { duration: 12 })`; `engine/11-tc4.md`, "TC4 after step 4").
Data cannot reach a call, so it is not a reason for a context, the rule relevance already has for a whole parameter. In a key (each
parameter slot and each captured entry), every object is written **reduced**: a member whose value cannot reach a call is dropped, and an
object that lost a member is not complete. A value can reach a call when it is open, or holds a function (`Fn`, `Closure`, `Anon`,
`AnonAny`), `Undefined` (it selects a default), or an object whose reduced form keeps a member. `{}` stays complete (nothing was
dropped: `open({})` still reads `Undefined` and takes the default); `{ a: 1 }` and `{ b: 2 }` are both `{}` not complete, one key. A
member read of a dropped member is unknown: sound, and a default beside it is shown with the engine's answer. Values inside a body are
not reduced. Pinned digests of TC2 and TC3 hold no object and do not move.

**Step 4c built as decided, `a5a345f` and `6362d7a`** ([`engine/11-tc4.md`](../../../languages/typescript/engine/11-tc4.md), "TC4
step 4c as built"): `eval::for_key` in `intern_key`; `object-data-members` has two contexts of `report`, `object-rest` moved its digest,
nothing else. On the samples the private codebase's cuts went 349 → 150, all at two functions whose callers' literals hold **unknown**
members (not data): the key still splits by member names. A further rule (a bare unknown member is dropped too) was measured: 0 cuts.

**Step 4c-fix, decided (owner, 2026-11-27): an unknown member is no reason for a context either.** In the callee, a member that is
unknown and a member that is missing from an object that is not complete read the same (unknown), so a key gains nothing by keeping it.
`reaches_a_call` no longer counts `open`: a value can reach a call when one of its **known** members can (`Fn`, `Closure`, `Anon`,
`AnonAny`, `Undefined`, an object that keeps a member). So `{ a: x }` with `x` unknown is `{}` not complete, and so is `{ a: x }` with
`x` unknown or `{ b: 1 }`; `{ a: x }` with `x` unknown or `save` keeps `a`. Only an object's members are reduced; a slot that is unknown
stays unknown. The cost, accepted: a name the literal leaves out reads unknown, not `undefined`, when the literal has an unknown member,
so a default there is shown beside the engine's answer instead of alone (sound). What the review measured was a looser form (an open value
with any known member kept); the samples are run again on this one. The unit test also gains the case the review's mutation found
untested: `{ inner: { u: undefined } }` is kept whole.

**Step 4c-fix built as decided, `fe9151e` and `812e533`** (`engine/11-tc4.md`): the private sample has 0 cuts and 6,904 contexts
(objects off: 6,104), V-NOC 733 (off: 693). Step 4c is closed; next is 4d (D68).

**Step 4d, as decided for the build (reviewer, 2026-11-27): D68, a method call can change its own object.** D68 opens a receiver's object
when a function member writes `this` or cannot be seen. Read literally (every member of the object), it would open almost every key's
object since step 4c: a reduced object is rarely complete, and a props object often holds an unknown member (`value={state.x}`). Only
the **methods that are called** can write, so the rule reads the names called:

- **The table** (`Uses`, step 3a's placement): a chain that ends as the callee of a call (`x.m()`, `x.a.m()`, `x?.m()`, `x["m"]()`) records
  its last name in `Uses::called` (sorted, no repeats); a last access with no literal name (`x[k]()`) sets `Uses::called_any`. `receiver`
  stays (the counter reads it). The same for a def's `this_uses` (`this.clear()` inside a method). `Flow::arrow`: the def's node is an arrow
  (a named arrow that is a def, `const reset = () => {}`; **corrected at 4d-a**: a property arrow of an argument literal, `run({ reset: () => {} })`,
  is not a def but an anonymous function of the def it is written in, D21).
  `AnonFlow::arrow`: the anonymous function is an arrow (read where `collect.rs` lists them); `anons` is then read outside tests.
- **Values**: `One::AnonAny` is renamed **`One::AnonArrow`** (an anonymous arrow, which one not said), `Canon::AnonAny` likewise with
  `#[serde(rename = "AnonAny")]`, so its bytes and every digest that holds an arrow stay. **`One::AnonFunction`** and
  **`Canon::AnonFunction`** are added **last**: an anonymous function expression (`function () {}`), which one not said.
  `eval::anon_kinds(world, value)` writes each `Anon(def, i)` that is not an arrow (or whose table or anon is not found) as `AnonFunction`;
  `TypeScript::object`, `TypeScript::capture` and `intern_key` (first) apply it, and the normal forms (`nested.rs`, `KeyData::normal`) keep
  writing the remaining `Anon` (arrows) as `AnonArrow`. Only a key, a capture list or an object that holds a function expression moves
  its digest; two callers that pass an arrow and a function expression no longer share a context.
- **The program fact** (`trust.rs`): `Trust { trusted, calls }`, `Calls { any, names }`, one per variable of a component, kept in the same
  memo. A variable's calls are its own `called` and those of every variable it follows (second names and the parameters it is passed to:
  a pattern parameter's variables too), spread to a fixpoint as "not trusted" is. Names are matched at **any depth** of the value (a name
  does not say which object of the value it was called on): sound, and simpler than paths through patterns and member reads.
  `trust::uses_calls(world, uses)` gives the same for a method's `this_uses`.
- **The read** (`eval::as_read`, a trusted variable with calls): the value's objects are **opened** (as for an untrusted one) when, in
  any object of the value at any depth, a called name (every member when `any`) is a member that is open, or is missing from an object
  that is not complete, or holds a function that may write `this`: a def with no table, a def that is not an arrow and has no `this_uses`
  (a class member), a def whose `this_uses` is not trusted, a non-arrow `Anon`, or an `AnonFunction` (an anonymous function has no table,
  so it cannot be seen). A def whose `this_uses` is trusted adds its own calls to the names (`this.clear()` makes `clear` called), to a
  fixpoint. An arrow, `AnonArrow`, `Undefined`, data and an object cannot write `this`. A member that is itself unknown needs no opening
  of its own: what it holds is read unknown already. Counted: **`object_opened_by_method`**, at each read that opens by this rule.
- **The rows** (written by hand; policy rows `method-*`): `method-writes-this` (`reset() { this.f = other }`, `run` calls `o.reset();
  o.f()`: `o.f` is `save` and the engine's answer beside), `method-reads-this` (`if (this.f) {}`: `save` alone), `method-arrow-member`
  (`reset: () => {}`: `save` alone), `method-uncalled` (`run` calls only `o.f()`: `save` alone), `method-through-parameter` (`poke(o)`
  calls `t.reset()`, then `main` calls `o.f()`: opened), `method-this-calls-this` (`reset() { this.clear() }`, `clear() { this.f =
  other }`: opened), `method-unknown-member` (`reset: makeReset()`, dropped from the key by 4c-fix: opened), `method-anon-member` (JSX:
  `reset={() => {}}` and `reset={function () {}}`: two contexts of `Btn`, only the second opened).
- **Split in two prompts**: **4d-a** (the fixtures with today's code; then the table and the values, which move only table snapshots,
  digests, and the contexts of a row that passes an arrow and a function expression to one function) and **4d-b** (the fact, the read,
  the counter; the rows' "on" halves move as written). The cost is measured on the samples after 4d-b.

**Step 4d-a built, `40461ff` and `e8037f9`** ([`engine/11-tc4.md`](../../../languages/typescript/engine/11-tc4.md)): as decided, with
the correction above. No rule applied; only `method-anon-member` split into two contexts of `Btn`. Two guards were untested (`anon_kinds`
in `intern_key` and in `capture`: each mutation passes every test); 4d-b's first commit adds the row `anon-kinds-in-keys`, and the row
`method-arrow-def` for an arrow that is a def (no row read `Flow::arrow` in a graph).

**Step 4d-b built as decided, `afa9e3f` and `0fc9249`** (`engine/11-tc4.md`): the rows moved as written; on the private sample 123
reads opened, 27 edges added beside (21,182 → 21,209 unresolved), the same contexts, no time measurable. **4d-fix (found at 4d-b):**
`walk::members` drew an `anonymous_function_called` target for **each** anonymous member of a value (the engine's mark `anon_already`
is not set by the loop), so a value of two arrows, or since 4d-a of an arrow and a function expression, said one edge twice (the graph
drops it; the walk-alone test and the counters do not). It draws one per site; the coder's `dedup()` in the walk-alone test is taken out,
since that test is the one guard that sees a doubled edge.

**4d-fix built, `2be3cc5`**: as decided; three off snapshots moved one number each (`Stats.unresolved`, the double that was
counted), no edge. **Step 4d is closed**; next is 4e (D69).

**Step 4e, as decided for the build (reviewer, 2026-11-27): D69, a value before a type.** Today a parameter or a variable whose declared
type holds only data (`capable` false) is data in every context: its key slot is unknown (`key::admits`), `eval::passed` and
`eval::row_value` answer data, and the table records no assignment of such a row. Probed at `b1a174d` (§1.3's program): `liar` draws
`noop` alone and removes the engine's mark, though `handler` is what is called. D69: where a value is known it is used; the type says only
that the **unknown** part is data.

- **What a data-typed place keeps of a value** (`eval::as_data_typed`): its functions (`Fn`, `Closure`, `Anon`, `AnonArrow`,
  `AnonFunction`) and its objects **opened** (`objects_opened`: the trust rule never followed a data-typed row, so a write through it is
  not seen), closed (the rest is data, by the type); with none of these, data. `Undefined` is dropped (a data-typed default is data).
- **4e-a, parameters.** `key::bind`: a slot that is not capable (and not rest) is relevant as any other; its value is the argument's
  functions and objects, closed, and **unknown when it has none**, as today, so a caller that passes data (nearly all) keeps its key and
  its digest, and a key splits only where a function or an object reaches a data-typed parameter (a cast). A missing argument is unknown,
  not `Undefined`, for such a slot. `eval::passed` for a parameter that is not capable: `as_data_typed` of its slot (data for an open key
  or an unknown slot); its default is not read. Row `param-data-holds-function` (§1.3's program, entries `liar` and `honest`).
- **4e-b, variables.** The table records `assigned` and `open` for **every** row, not only capable ones (`Uses` stays capable-only, and
  `needs` too: a captured read of a data-typed variable stays unknown in the nested def, sound). `eval::row_value` for a row that is not
  capable: `as_data_typed` of what it is assigned (a parameter's row starts from `passed`). Row `var-data-holds-function`
  (`const s: string = handler as any; (s as any)()`). Its cost is measured: every data-typed row is now evaluated (time and peak on the
  samples, the table's size).
- **What must not move**: a key or a value where no function or object reaches a data-typed place: `local-data` (both calls one context),
  `callback-data`, every digest of the other rows.
- **No bit**: D69 replaces TC2 step 5f's and TC3's rule, it is not a layer of its own, so it is on with every set of bits; the five
  `*-off.txt` snapshots move in the two new rows' entries only.

**Step 4e-a built as decided, `fd997eb` and `c9d1bf7`** (`engine/11-tc4.md`): only `liar`'s context of `lied` moved; the samples did
not move at all. **4e-b, as decided for the build**: in `Reader::vars` a row that is not capable records `assigned` (its initializer
and its modelled writes) as a capable row does; `open` stays `capable && open` and `unmodelled` is counted for capable rows only (a
data-typed row's value is closed by `as_data_typed`, so neither changes it); `uses` stays capable-only, and so do `needs` and
`variable_assigned_more_than_once`; the table printer is unchanged (**corrected at 4e-b**: it prints `= …` for every row
with assignments, capable or not, so three table snapshots show the data-typed rows' assignments, nine lines; kept, not hidden: a table
snapshot shows what the table holds). `eval::row_value` of a row that is not capable: what a parameter's
row is passed (`passed`) joined with every assigned value, then `as_data_typed`. Evaluated only with `LOCALS`, so `locals-off.txt` does
not move. Relevance now reads a data-typed row's assignments too, so a parameter assigned into one and called through it may become
relevant: each such row is reported.

**Two steps added after 4b (owner's decisions, 2026-11-27).** **4d**, D68 (Q-TC9): a method call can change its own object. A
variable that is a receiver (in its scope, or through a parameter it is passed to that is a receiver) has its object opened at the read
when one of the object's function members writes through `this` or cannot be seen (an open member, an object that is not complete, an
anonymous function that may not be an arrow); a key tells an arrow from another anonymous function. **4e**, D69 (§1.3 of the review
before TC3): a value before a type: a parameter or a variable declared as data holds the value it is given where one is known; the type
says only that what is unknown is data. Each is designed when its prompt is written, with its cost measured.

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
