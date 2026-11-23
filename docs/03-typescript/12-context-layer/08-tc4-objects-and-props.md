# 03.12.08 TC4: objects and props

```
TC4
├── 1. what the user sees
├── 2. `sem`: literals, members, patterns, JSX attributes
├── 3. values: `Object`, `Class`
├── 4. reading a member
├── 5. when a shape can be trusted
├── 6. JSX
├── 7. limits
├── 8. fixtures and tests
└── 9. steps
```

Depends on TC3. No core change. With TC3 this is the phase React code shows: a parent's tree says what its children do with the
handlers it gave them.

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

After step 8 is where I would tag: the first release whose trees look different on V-NOC's own code.

Next: [`09-tc5-instances.md`](09-tc5-instances.md)
