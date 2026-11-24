# 03.12.07 TC3: variables and captures

```
TC3
├── 1. what the user sees
├── 2. `sem`: every variable, and everything assigned to it
├── 3. the env: a value per variable, on demand
├── 4. captures: what a nested def reads from the functions around it
├── 5. module variables
├── 6. `bind`
├── 7. fixtures and tests
└── 8. steps
```

Depends on TC2. No core change. This phase fills `env.rs`, the file that has been one empty struct since TS4.

## 1. What the user sees

Patterns A7–A10 of [02](02-patterns.md); line c of the tour, and the half of line e that is not about props.

| Code | Before | TC3 |
|---|---|---|
| `const h = handler; run(h)` | `run` empty | `run` → `handler` |
| `const h = flag ? a : b; h()` | `a` alone (see below) | `a` and `b` |
| `function start(onDone) { const finish = () => { onDone(); }; timer(finish); }` · `start(save)` | `finish` `[Passed]`, with `onDone()` unresolved inside | `finish` `[Passed]` → `save` |
| two nested handlers, one calling the other, the second reading a parameter | unresolved in the second | resolved: the first carries what the second needs |
| `const onSave = pick ? a : b;` at the top of a file, `onSave()` in a function | `a` alone (see below) | `a` and `b` |
| `run(handler.bind(null))` | counted, no edge | `run` → `handler` |
| `let h = a; h(); h = b; h();` | `a` twice (see below) | both calls show `a` and `b` ([03](03-design.md) §11, "assigned twice") |


**Checked 2026-11-24** (with the analyzer of `9530a7d`, `a` and `b` both `() => void`): the three rows marked "see below" are **not
unresolved today: the default graph shows `a` alone**, with nothing that says `b` is missing. For a callee that is a name, the resolver
takes the declaration of the checker's resolved signature (`sem/resolve.rs`, `is_named_callee`), and the type of `h` is the type of `a`:
`const h = flag ? a : b` reduces to one type (it is not a union, so `is_union_callee` does not see it) and `let h = a` keeps `a`'s type
after `h = b`. `const h = a; run(h)` is as the table says (`run` empty, `a` `[Passed]`). A written type (`const h: () => void = a; h()`) makes
the call `function_typed_value`, which is the tour's line c now. **Whether TC3 may add `b` where the engine named `a` through a variable
is Q-TC8** (`docs/07-decisions-and-questions.md`); §7's "where the layer may speak" says only "by name", and this is not a call the code
fixes by name. The default graph keeps the engine's answer either way ("follow the engine").
## 2. `sem`: every variable, and everything assigned to it

```rust
pub(crate) struct Var {
    pub id: VarId,                  // a number: the declaring def and which declaration in it
    pub kind: VarKind,              // Local | Param(i) | Enclosing | Module
    pub assigned: Box<[Expr]>,      // every expression assigned to it, in source order (for a stable join)
    pub open: bool,                 // assigned somewhere in a way the table does not model
}
```

`flow.vars` lists every variable the body **declares** (with all its assignments) and every variable it **reads** from outside
(`Enclosing`, `Module`: no assignments here, they are in the table of the scope that declares it).

| Source | In the table |
|---|---|
| `const x = e`, `let x = e`, `var x = e` | `assigned += e` |
| `x = e`, `x \|\|= e`, `x ??= e`, `x &&= e` | `assigned += e` |
| `let x;` never assigned | nothing: the value is empty, and a site that calls it gets the engine's answer |
| `x += e`, `x++`, `for (x of …)`, `for (x in …)`, `catch (x)`, an array pattern | `open` |
| an object pattern (`const { k } = e`) | `open` in TC3; `Member(e, k)` from TC4 |
| a function or class **declaration** | not a variable: `Expr::Def`. If its name is ever assigned to, **[check]** how often (N4); then it is a variable that is open |
| an assignment **inside a nested function**, def or anonymous, to a variable of this scope | counted here, in the declaring scope's table |

The last row is what makes "complete" checkable ([03](03-design.md) §5): the reader walks the whole text of the declaring function,
its nested functions included, looking for writes to the function's own variables. (S-TC2 was to measure that pass; TC0 moved the measure
into TC1 step 2, `ENGINE.md` "S-TC2, S-TC3, N6".)

**The reader does not trust its own list.** For every reference of the variable in that text it asks `ast::is_write_access`. A write that is
none of the rows above makes the variable `open` and is counted `variable_write_unmodelled`. The references are found by a text search of the
variable's scope and the node at each hit, the helper TC0 built and deleted (`names_in`, at `25793b3` in `src/sem/resolve.rs`; the
engine's own scoped search is not reachable: `ENGINE.md` "N4, N7: reading the writes of a name"); it must take a private name (`#x`) too.
TC0 counted the writes it would not read: 0, 29 and 148 names on the three projects (N4). The count is expected to fall to 0 as rows are
added; while it is not 0, a form nobody thought of costs precision and never an edge ([03](03-design.md) §5).

An identifier that names a variable becomes `Expr::Var(id)`. `Param(i)` of TC2 stays for the parameter as passed; a parameter the body
assigns to is `Var { kind: Param(i), assigned: […] }`, and its value is the key's slot joined with the assignments. That replaces
TC2's "assigned means open".

## 3. The env

```rust
pub(crate) struct Env<'a, 'p> {
    flow: Arc<Flow>,
    key: KeyId,
    lang: &'a TypeScript<'p>,                              // for module values and other defs' tables
    values: RefCell<FxHashMap<VarId, Slot>>,               // Slot = InProgress | Done(Value)
}
```

`value_of` is [03](03-design.md) §5, with the memo and the in-progress mark of the Rust note there. Two variables defined through each
other (`let g = h; let h = g;`) are unknown and counted `variable_in_cycle`.

A `HashMap` is safe here because nothing is **listed** from it: values are looked up by variable, never iterated to produce output
(rule 9 of the repo).

## 4. Captures

```
needs(g) =  the variables g's body reads (its anonymous functions included)
            that are declared in a function around g                       not in g, not at the top of a file
            and whose type is capable                                      the filter of TC2 §2
          ∪ needs(h) minus the variables g itself declares                 for every nested def h that g's body names
```

`needs` is the smallest solution: two sibling handlers that name each other each get the union. `sem` computes it once for an
outermost function and all the defs nested in it, and stores it in each one's `Flow`.

**The value of a nested def carries them** ([03](03-design.md) §7): evaluating `Expr::Def(g)` for a nested `g` reads each variable of
`needs(g)` from the env of that place. `One::Fn` becomes `Fn(func, captures)`, with `captures` empty for a def that is not nested. In
`key_of`, `Key.captured` is those captures. In the walk of `g`, an `Enclosing` variable's value is the key's entry for it, or unknown
when the key has none (the open key: `g` asked for as an entry).

Why this is sound without Python's activations: a captured variable is read when the nested function **runs**, and by then any
assignment in its scope may have happened. The join of all of them is exactly what can be there.

Depth: a nested def can capture a variable that holds another nested def with captures of its own. The captures inside a value count
toward `KeyPolicy::max_depth`; past it they are unknown (`value_widened_depth`). That bounds pattern R3 for functions.

The canonical form: a variable is written as (the `StableDef` of the function that declares it, its name, which declaration of that
name it is). `Fn` and `CanonKey` get a `captured` list that is **skipped when empty**, so every digest of TC2 is unchanged.

## 5. Module variables

A variable declared at the top of a file is not captured: every function of the file sees the same one, so it does not belong in a key.

```
module_value(v)  =  value_of(v) in the env of the module def's own table, under the open key      once per request, kept in `TypeScript`
```

- Only a `const`. A top-level `let` or `var` is unknown: the reader would have to find its writes in every def of the file, and N4
  says whether that is worth doing.
- An import resolves to the declaring file's variable (`get_aliased_symbol`, as imports of functions already do), so
  `import { onSave } from "./handlers"` is the same variable.
- Rule 1 holds: the value is a function of the code alone, never of a caller.
- A `const` whose initializer is a call is unknown until TC6.

## 6. `bind`

`f.bind(x, …)` in any expression is `f`'s value (extra arguments are dropped: the bound parameters are unknown in the key, and the
positions shift by their number). `this` is not followed. The counter `uncovered_bind` stops counting the places that now have an
edge. `call` and `apply` stay counted.

## 7. Fixtures and tests

`local-alias` · `local-either` · `local-assigned-twice` · `local-open` (a `for … of` variable: the engine's answer) · `local-cycle` ·
`local-never-assigned` · `local-write-unmodelled` (a write in a form the table has no row for: open, counted) · `param-reassigned` (the handler **and** `noop`, complete) · `capture-handler` · `capture-sibling` ·
`capture-two-levels` · `capture-through-parameter` (the nested def is called three calls away, with its captures) ·
`capture-written-in-nested` (a nested def assigns the variable: both values) · `capture-entry` (asked at the nested def: today's tree) ·
`capture-anonymous` (an arrow inside the nested def reads the variable) · `module-const` · `module-const-imported` · `module-let`
(unknown) · `bind`.

| Test | |
|---|---|
| the three properties on every fixture | |
| `needs` printed in the table snapshots | reviewed against the source |
| every TC2 digest | unchanged (the skipped-when-empty rule) |
| the capability bits `LOCALS` and `CAPTURES` off, one at a time | TC2's graphs; then TC2's plus locals |
| `eval` on hand-written tables: join order, cycles, the in-progress mark | no engine |
| the four projects | the per-site measure; `variable_in_cycle`, `value_widened_depth` |

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `sem/flow.rs`: `Var`, assignments, `Expr::Var`, writes from nested functions | table snapshots; the tour's line c |
| 2 | `env.rs`: `value_of`, the memo; parameters joined with their assignments | `local-*`, `param-reassigned` |
| 3 | `needs`; `Fn` with captures; `Key.captured`; the canonical form | `capture-*`; TC2's digests unchanged |
| 4 | module variables | `module-*` |
| 5 | `bind` | `bind`; the counter drops where an edge appears |
| 6 | the four projects | `ENGINE.md` |
| 7 | (mine) `policy.md`, `ENGINE.md` | reviewed |

Next: [`08-tc4-objects-and-props.md`](08-tc4-objects-and-props.md)
