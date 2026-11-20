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
| `const h = flag ? a : b; h()` | unresolved | `a` and `b` |
| `function start(onDone) { const finish = () => { onDone(); }; timer(finish); }` · `start(save)` | `finish` `[Passed]`, with `onDone()` unresolved inside | `finish` `[Passed]` → `save` |
| two nested handlers, one calling the other, the second reading a parameter | unresolved in the second | resolved: the first carries what the second needs |
| `const onSave = pick ? a : b;` at the top of a file, `onSave()` in a function | unresolved | `a` and `b` |
| `run(handler.bind(null))` | counted, no edge | `run` → `handler` |
| `let h = a; h(); h = b; h();` | unresolved twice | both calls show `a` and `b` ([03](03-design.md) §11, "assigned twice") |

## 2. `sem`: every variable, and everything assigned to it

```rust
pub(crate) struct Var<'p> {
    pub id: VarId<'p>,              // the file and the position of the name that declares it
    pub kind: VarKind,              // Local | Param(i) | Enclosing | Module
    pub assigned: Box<[Expr<'p>]>,  // every expression assigned to it, in source order (for a stable join)
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
its nested functions included, looking for writes to the function's own variables. S-TC2 measured that pass.

An identifier that names a variable becomes `Expr::Var(id)`. `Param(i)` of TC2 stays for the parameter as passed; a parameter the body
assigns to is `Var { kind: Param(i), assigned: […] }`, and its value is the key's slot joined with the assignments. That replaces
TC2's "assigned means open".

## 3. The env

```rust
pub(crate) struct Env<'a, 'p> {
    flow: Rc<Flow<'p>>,
    key: &'a Key<'p>,
    lang: &'a TypeScript<'p>,                              // for module values and other defs' tables
    values: RefCell<HashMap<VarId<'p>, Slot<'p>>>,         // Slot = InProgress | Done(Value)
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
`local-never-assigned` · `param-reassigned` (the handler **and** `noop`, complete) · `capture-handler` · `capture-sibling` ·
`capture-two-levels` · `capture-through-parameter` (the nested def is called three calls away, with its captures) ·
`capture-written-in-nested` (a nested def assigns the variable: both values) · `capture-entry` (asked at the nested def: today's tree) ·
`capture-anonymous` (an arrow inside the nested def reads the variable) · `module-const` · `module-const-imported` · `module-let`
(unknown) · `bind`.

| Test | |
|---|---|
| the three properties on every fixture | |
| `needs` printed in the table snapshots | reviewed against the source |
| every TC2 digest | unchanged (the skipped-when-empty rule) |
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
