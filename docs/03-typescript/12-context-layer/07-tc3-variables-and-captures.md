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
| `let h = a; h(); h = b; h();` | `a` twice (see below) | step 2: `a` and `b` at both calls; step 2b, if built: `a`, then `b` (D67, as in Python) |


**Checked 2026-11-24** (with the analyzer of `9530a7d`, `a` and `b` both `() => void`): the three rows marked "see below" are **not
unresolved today: the default graph shows `a` alone**, with nothing that says `b` is missing. For a callee that is a name, the resolver
takes the declaration of the checker's resolved signature (`sem/resolve.rs`, `is_named_callee`), and the type of `h` is the type of `a`:
`const h = flag ? a : b` reduces to one type (it is not a union, so `is_union_callee` does not see it) and `let h = a` keeps `a`'s type
after `h = b`. `const h = a; run(h)` is as the table says (`run` empty, `a` `[Passed]`). A written type (`const h: () => void = a; h()`) makes
the call `function_typed_value`, which is the tour's line c now. **TC3 adds `b` where the engine named `a` through a variable: D59**
(owner, 2026-11-24, answering Q-TC8: show every possible call, as Python joins both branches of an `if`). The layer adds and never removes
`a`; a variable with an assignment the table does not model stays open, so the engine's answer is kept beside what is known. The default
graph keeps the engine's answer ("follow the engine"). TC3's sweep counts how many named callees are variables holding more than one function.

**Re-read against the built code, 2026-11-25** (after TC2 closed, at `81485ce`; the corrections are in §2–§4 and marked "as built"):

- The row `run(handler.bind(null))` holds today: `handler` is handed over bound, counted `uncovered_bind`, no edge. A bind whose result
  is dropped (`h.bind(null);`) is not counted (`bindDiscarded`, `tests/calls.rs`); `const bound = h.bind(null)` hands `h` over.
- The rows that D59 fills ("see below") need the walk to speak where the engine **named** a function: today `walk::resolve_from_env`
  speaks only when the engine's answer has an open entry (`function_typed_value`, `any_or_untyped`). From step 2, a callee that is an
  `Expr::Var` with a known member is answered too, and the engine's targets stay (the layer adds, never removes).
- **V-NOC, before the design** (`engine/10-tc3.md`, "V-NOC's open sites"): of the 305 sites TC2 leaves open on the three samples, about
  94 are props read from a destructured parameter (TC4), about 154 are the results of a library hook (zustand selectors and `getState`,
  `useCallback`, `useEffectEvent`, `useRef`: TC10), about 33 a project hook's result taken apart (TC4 and TC6); at most 10 are TC3's
  (a nested def that calls a parameter of the function around it, a local that is one of two functions). TC3 is for imperative code
  (the private codebase, the compiler: N4); V-NOC waits for TC4 and TC10.

## 2. `sem`: every variable, and everything assigned to it

As built in TC2, `Flow` has `params`, `sites`, `anons`, `handed`. Step 1 adds the variables (`crate::flow`):

```rust
/// A variable as a number: the def that declares it (a def of the outline, or the file's own code) and where its name starts in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct VarId { pub(crate) def: DefId, pub(crate) at: u32 }

pub(crate) struct Var {
    pub(crate) at: u32,                 // with the table's own def, the VarId
    pub(crate) param: Option<u32>,      // Some(i): the def's own parameter i
    pub(crate) capable: bool,           // its type can hold a function, an object or an instance (TC2 §2's filter)
    pub(crate) assigned: Box<[Expr]>,   // every expression assigned to it, in source order (the initializer first); read only when capable
    pub(crate) open: bool,              // assigned somewhere in a way the table does not model; said only when capable
}

Expr::Var(VarId)                        // a name of a variable: of this def, or of a def around it (the file's code included)
Flow::vars: Box<[Var]>                  // sorted by `at`
Flow::unmodelled: u64                   // writes none of the rows below covers (`variable_write_unmodelled`)
```

**Which variables have a row.** Every variable the def **declares**: its parameters that have a plain name, and every name declared in
its body and its anonymous functions, not in a nested def (a variable, a name of a binding pattern, a `catch` variable, a parameter of an
anonymous function). Every one has a row, capable or not: a variable whose type holds only data is data, as a parameter is (TC2 step 5f),
and its row says so without reading its writes. The reads of a variable of **another** def (`Enclosing`, `Module` in the first draft)
have no row here: the kind is said by the id (`id.def` is the table's own def, or a def around it), and the row is in that def's table.

| Source | In the row |
|---|---|
| `const x = e`, `let x = e`, `var x = e` | `assigned += e` |
| `x = e`, `x \|\|= e`, `x ??= e`, `x &&= e` | `assigned += e` |
| `let x;` never assigned | nothing: the value is empty, and a site that calls it gets the engine's answer |
| `x += e` (any other compound operator), `x++`, `for (x of …)`, `for (x in …)`, a destructuring assignment (`[x] = e`) | `open` |
| declared by `for (const x of …)` / `in`, by `catch (x)`, by a binding pattern (`const { k } = e`, `const [k] = e`, a destructured parameter), as a parameter of an anonymous function, or by `declare` | `open` (a pattern is `Member(e, k)` from TC4) |
| a function or class **declaration** | not a variable: `Expr::Def` as today. Writing its name is an error in TypeScript (TS2630) and possible in JavaScript, where it stays the engine's answer: **[check]** how often (N4 did not count it) |
| an assignment **inside a nested function**, def or anonymous, to a variable of this scope | in this row: the assigned expression is read in this def's table, so a name declared in the nested function is `Unknown` there |
| any other write (`(x) = e`, `x! = e`) | `open`, and counted in `Flow::unmodelled` |

**How the writes are found (as built: not a text search).** The first draft searched the scope's text for the name (`names_in`, at
`25793b3`). TC2 step 4d built a better tool: `written_symbols` walks the whole node of the def, its nested defs and anonymous functions
included, and keeps every identifier that `ast::is_write_access` says is written, with its symbol. It finds every write by syntax (no hit
in a comment or a string, no engine-private search) in one walk per def. Step 1 keeps the write nodes too, sorted by place, and gives each
to the row whose symbol it writes. **The reader does not trust its own list**: a write that is none of the rows above opens the variable
and is counted. TC0 counted the writes it would not read: 0, 29 and 148 names on the three projects (N4, where `||=` was among them; it
is a row now).

**How a name is read.** `expr_of` keeps TC2's answer wherever TC2 had one: a def of the outline, then a parameter of this def
(`Expr::Param`), then the checker's functions for the name (`functions_named`: a `const` that holds an arrow is that def). Only where TC2
said `Unknown`, a name whose declaration is a variable, a parameter or a binding element **in this file**, declared by this def or by a def
around it, is `Expr::Var`. A parameter of this def stays `Expr::Param` (the parameter as passed); its row (`param: Some(i)`) holds what the
body assigns to it, and from step 2 its value is the key's slot joined with that, which replaces TC2's `Param.assigned` (joined with
unknown). An import is not followed in step 1 (step 4).

Step 1 reads only: `eval` gives `Expr::Var` the value unknown, which is what TC2 gave the same names, so every graph and digest is
unchanged with every capability bit. Step 2 gives it its value.

## 3. The env

As built, `Env` is plain data, `{ flow: Arc<Flow>, def: DefId, key: KeyData }`, made by `Lang::walk` for one body, and `eval` asks
what is not in a table through the `World` trait (a test answers it by hand). The first draft's `Env<'a, 'p>` with a `&TypeScript` inside
would tie `env.rs` to the engine; it is not done.

**Step 2, as decided (2026-11-25).**

- **The values of a body's variables are solved together, once per walk.** `Env` gets `vars: OnceCell<Box<[Value]>>`, one value per row of
  `flow.vars`, made the first time the walk reads a variable and kept for the walk. (🦀 A `OnceCell` is a slot that starts empty and is
  filled at most once, through a shared `&` reference: `get_or_init(|| …)` runs the closure the first time and hands back the stored value
  after.) The first draft's memo with an in-progress mark is **not** built: with a memo it is wrong for two variables defined through each
  other (`g = a ∪ h`, `h = g ∪ b`: if `h` is worked out while `g` is in progress, it is kept as `b` alone and loses `a`), and answering
  unknown there instead would open `cb = cb || fallback`. The expressions of a table only join (a def, a parameter, an anonymous function,
  a variable, `OneOf`, data, unknown), so the exact answer, the least one, is cheap: every row starts empty, every row is recomputed from the
  others' current values, and that is repeated until nothing grows. Values only grow and are capped (`MAX_SET`, then wide), so it stops;
  past 32 rounds every row is joined with unknown and `variable_rounds_capped` is counted (rule 8). The rounds before the last are run with
  the counting of `value_widened_set` off, and one last round with it on, so a cut is counted once.
- **A row's value**: data when not capable (TC2 step 5f's rule, for a variable); else the parameter's value as passed for a parameter's row
  (TC2's rest, capable and default rules), nothing for a local; joined with the value of each assigned expression; joined with unknown when
  `open`. A local never assigned is then empty, and a value with no member is not an answer: the site keeps the engine's answer.
- **`Expr::Param(i)`** of a parameter the body writes is that parameter's row: the slot joined with what the body assigns, which replaces
  TC2's "joined with unknown". `Param::assigned` stays as the switch (step 1 proved it equals "its row has a write").
- **A variable of another def** (`id.def` is not the walk's def) stays unknown: its value comes with captures (step 3) and module
  variables (step 4).
- **With `LOCALS` off** nothing above runs: `Expr::Var` is unknown, and a written parameter joins unknown, as in TC2. Proved by a snapshot of
  every entry's graph with `PARAMS` alone, written before the step's code changes and unchanged after.
- **Where the engine named a function (D59).** `walk::resolve_from_env` spoke only when the engine's answer had an open entry. When it has
  none and the callee holds an `Expr::Var` **or an `Expr::Param`** (a parameter is a variable too: `run(cb: typeof a)` called with `b`, row
  `callback-named-by-type`, found by the coder's mutation d of part 2), the layer now evaluates it and **adds** each function of the value that
  the engine's answer does not already call (with the tripwire, D61), an `anonymous_function_called` mark for an arrow, and keeps every target
  of the engine's. It notes nothing unless it adds (no `callee_undefined`: the engine's answer stands). A def or an arrow as the callee is
  not tracked by name and is left to the engine. A site where it added something counts `variable_added_to_engine`.
- **Counters**, per walk with `LOCALS` on: `variable_write_unmodelled` (a body's `Flow::unmodelled`, counted as a body note);
  `variable_added_to_engine`; `variable_rounds_capped`; and **the ceiling of 2b**, `variable_assigned_more_than_once`: a site whose callee or
  an argument reads (directly or in a `OneOf`) this body's capable variable or written parameter with two or more sources (its assignments,
  and the passed value for a parameter). Step 2b is built only if this is not near zero on the private codebase and the compiler, by site.

**In order (D67, owner, 2026-11-25), built in two steps.** The paragraph above is step 2: the **whole-scope join**, the value of every
assignment of the variable. It is built first and stays as the permanent fallback, because the in-order walk needs it anyway (a read or a
write inside a nested function, a flow node kind the walk does not know). **Step 2b** makes a read see only the assignments that **reach**
it, as Python's statement walk does (`interp/walk.rs`: `Env::join` after an `if`'s arms, a `try`, a `match`), and is built **only if it pays**.

*What 2b reads.* Not the checker's flow analysis: it tracks types, and a type forgets which function it was (`let h: () => void = a; h = b;`
keeps `() => void`; an assignment only filters a union's members). It has one public entry, caches per type inside the checker, and cannot
be changed. 2b reads the **binder's flow graph** as data (public at the pin: `Node::flow_node`, `FlowNode::flags` / `node` /
`antecedent(s)`): from each read's flow node, back through `Assignment` nodes (the first write of the variable on each path), labels
(`BranchLabel`, `LoopLabel`: every antecedent, with a visited set), conditions and calls (passed through: a condition's value is ignored,
as in Python), to the function's `Start` (the parameter's slot, or nothing for a local). The answer does not depend on the context, so
`sem` stores it in the table once (`Expr::Var` gets which of the row's assignments reach it, and whether the start does), and `eval` joins
only those, per context, with no engine. An assignment that reads itself through a loop (`h = h || cb`) is evaluated until its value stops
growing (values only grow, and are capped).

*What the graph does not give, so 2b writes it* (the spike that opens 2b checks each at the pin):
- **A write inside a nested function** (`let h = a; run(() => { h = b; }); h();`): the arrow has its own graph, and the walk back from
  `h()` never meets `h = b`. Rule: a variable (a parameter included) written inside a nested function, def or anonymous, gets those writes
  at **every** read. This rule is ours and does not lean on the checker, which keeps a narrowed type across such a call.
- **A read inside a nested function**: it runs at a time the order does not say (handed to a library, stored, returned). The
  whole-scope join (a superset of Python's answer, which reads the run around it when the closure is walked).
- **A loop**: both paths (`LoopLabel`), not Python's one pass over the body, which misses the second turn (`let h = a; while (c) { h(); h = b; }`
  calls `b` too). Python's one pass is a gap of Python's, to be written down there.
- **`try` / `finally`**: the `ReduceLabel` node's data is reached only through `as_flow_reduce_label_data_p`, an engine downcast banned
  by `clippy.toml` (H17): it needs a guarded view in `sem/view/`.
- **Unreachable code**: no flow node, so the value is unknown (the engine's answer stays).
- **Cost on huge functions**: a memo per (flow node, variable), and a step cap that is marked and counted when it cuts (rule 8).
- **Which write is to this variable**: for a plain name, the symbol (step 1's rows). A path (`a.b`, `this.x`) is TC4's and TC8's, with
  the rule that a field read is open once its object is copied, passed or stored (written through another name or by a callee).

*How 2b is kept honest.* `never_fewer_edges` does **not** guard it: it compares the layer off with the layer on, and a read that wrongly
loses one of its two assignments still has an edge. So:
- **Fail open**: a flow node kind the walk does not know gives the whole-scope join for that read, counted (`order_unknown_node`).
- **One fixture per flow node kind**, with the reaching assignments written by hand.
- **Its own capability bit** (`Caps::ORDER`): the same sweep with the bit off and on lists every site where 2b removed a member, and a
  sample of them is read by hand on the private codebase and the compiler.

*Whether 2b is built.* Step 2 counts the ceiling: the open sites whose callee or an argument reads a variable with more than one
assignment (`variable_assigned_more_than_once`, by site). If it is near zero on the private codebase and the compiler, 2b is not built, and
the join is the answer.

## 4. Captures

```
needs(g) =  the variables g's body reads (its anonymous functions included)
            that are declared in a function around g                       not in g, not at the top of a file
            and whose type is capable                                      the filter of TC2 §2
          ∪ needs(h) minus the variables g itself declares                 for every nested def h that g's body names
```

`needs` is the smallest solution: two sibling handlers that name each other each get the union. `sem` computes it once for an
outermost function and all the defs nested in it, and stores it in each one's `Flow`.

As built (step 1), "the variables g's body reads" are the `Expr::Var` of g's table whose `id.def` is a def around g: a read that no
callee, argument, hand-over, default or assignment holds cannot reach a call, so it is not needed.

**How a value holds its captures (decided at step 3, recommended now).** `One` is `Copy + Ord` and a `Value` keeps its members sorted
inline; a list inside `One::Fn` would end both. Python has the same problem and interns: a value that holds other values is a number into a
per-walk table (`interp/value.rs`, `ValueId`, "so a value can hold another and stay `Copy`"). Here the keys already work that way
(`KeyId` into `Keys`). So `One::Fn(DefId, Captures)`, where `Captures` is a number into a per-request table of capture lists, `0` for
none: every TC2 value is `Fn(def, 0)`, and the digest writes the list, never the number.

**Step 3, part 1, as decided (2026-11-25): `needs`, computed and printed, no graph change.**

- `Flow::around: Option<DefId>` (`sem`): the def of the outline whose node holds this def's node, the nearest one; `None` for a def at the
  top of its file and for the file's own code. It is what `var_of` already walks up to, kept.
- `needs(world, g) -> Box<[VarId]>`, in a new neutral file `needs.rs` (it asks only `World::flow`, so a test answers it by hand):
  - **reads(g)**: every `Expr::Var(id)` anywhere in g's table (a site's callee and arguments, a hand-over, a default, a row's assigned
    expressions, `OneOf` members included) with `id.def != g`, `id.def` not the file's code (module variables are step 4, and are not
    captured), and the row capable (`world.flow(id.def)`'s row at `id.at`).
  - **named(g)**: every `Expr::Def(h)` anywhere in g's table whose `h` is nested (`around` is `Some`): a def at the top of a file captures
    nothing, so it is skipped.
  - **needs(g)** = the least sets with `needs(g) ⊇ reads(g)` and `needs(g) ⊇ { v ∈ needs(h) : v.def ≠ g }` for each `h ∈ named(g)`: what a
    nested def that g names reads from around it, except g's own variables, which g reads from its own env when it makes that def's value.
    Solved over the defs reachable from g through `named`, repeated until nothing grows; sorted by `VarId`.
  - Why `v.def ≠ g` is enough: a def g can name is in scope at g, so it is g's own nested def, a sibling, or a def around g, and what it reads
    from around itself is g's or around g.
- Python's `free_variables` (`sem/scope.rs`) is the same idea read from the scope tree; ours reads the table, because a read that no callee,
  argument, hand-over, default or assignment holds cannot reach a call. Python needs no `named` part: a closure value there carries the run
  of the body around it (`activations`). Here a key carries values, so a def that calls a sibling by name must carry what the sibling reads.

**Step 3, part 2, as decided (2026-11-25).**

- **A new member, not a changed one**: `One::Closure(DefId, Captures)` for a nested def whose captured list is not empty; a def that
  captures nothing stays `One::Fn(DefId)`. So every TC2 value, digest and test is unchanged by construction, and the canonical form only
  gains a variant (`Canon::Closure`). `Captures` is a number into the request's table of capture lists (`CaptureSets`, as `Keys` is for keys):
  a list is sorted by `VarId`, has no unknown entry, and writes an arrow as `AnonAny`. (Python interns too: `ValueId`, "so a value can hold
  another and stay `Copy`".)
- **The depth limit is kept by the table**: a list's depth is 1 plus the deepest list of a closure among its values; when interning a list
  whose inner closure would make it deeper than 2 (`MAX_CAPTURE_DEPTH`, Python's `fp_depth`), that inner closure is written as its bare
  `Fn` (Python's `closure_at_limit`: the function kept, what it captured unknown) and `value_widened_depth` is counted (once per cut
  made while evaluating, so a sum over evaluations, not sites).
- **`solve` keeps what it had (as built, `8710c09`, found by the coder):** the cut makes a row's value shrink back (a deeper closure
  written as its bare `Fn`), so `capture-self`'s rounds swapped between two values for ever. Each round, the last one included, joins the
  row's new value with its value of the round before, so values only grow and the rounds end (they are capped). The answer is then at least
  the least one, never less: sound. For a row with no cut nothing changes (every snapshot of step 2 is unchanged).
- **Evaluating `Expr::Def(g)`** with `CAPTURES` on: each variable of `needs(g)` is read from this env (an own row; else this key's capture;
  else unknown), and the list is interned: `Closure(g, c)`, or `Fn(g)` when the list is empty.
- **`KeyData::captured`**: what a call passes for the callee's captured variables. A target that comes from a `Closure` member carries its
  list; any other call of, or hand-over of, a nested def takes the list from evaluating `Expr::Def(g)` here (sound: a variable this env does
  not have is unknown). In g's walk, a variable of a def around g is the key's capture, or unknown.
- **`needs` is memoized** for the request, the whole reachable component at once (each member's set is exact), with an index rather than a
  scan (the compiler's `createTypeChecker` holds thousands of nested defs).
- **Canonical form**: `CanonKey` gains `captured`, skipped when empty; `Canon::Closure { func, captured }`; a variable is written as
  (the declaring def's `StableDef`, its name, which declaration of that name among the def's rows).
- **With `CAPTURES` off** the layer is step 2: proved by a snapshot of every entry with `PARAMS | LOCALS`, written before the code changes.

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

**As decided for step 4 (2026-11-25), corrected against the built code.**

- **Its value is its row in the file's own table, under the open key** (`World::module_value`): the file's code (`Func::module`) has a
  table like any def, and its write walk covers the whole file, every function in it included (step 1). So the row is complete.
- **A `let` or `var` too, not only a `const`** (the first draft's "only a `const`" was there because the reader "would have to find its
  writes in every def of the file": it does). Another module cannot write it: an import is a read-only binding. **In a script file**
  (`!ast::is_external_or_common_js_module`) a top-level variable is a global that any other script may write: its row is `open`.
- **An import is the declaring file's variable**: `var_of` follows an alias (`get_aliased_symbol`, as imports of functions already are) to
  a top-level variable of a project file. `export { x } from` is an alias too.
- **One module's value does not read another's**: inside a module's own table, a variable of another module is unknown, counted
  `module_reads_module`. Following it would make an import cycle give different values depending on which file was asked first, and
  "the same from any entry" (the README's point 9) would not hold.
- A value made per request, one env per file (kept in `TypeScript`), solved once as any body's.
- Its own bit, `Caps::MODULES`, so "the bit off is step 3" is proved as before (`modules-off.txt`).
- A variable whose value is a call's result is unknown until TC6. Rule 1 holds: the value is a function of the code alone.

## 6. `bind`

**As decided for step 5 (2026-11-25), corrected.** `f.bind(thisArg)` is `f`'s value (`expr_of` of the call is `expr_of(f)`), when:

- **it binds no argument**: `f.bind(x)` or `f.bind()`. The first draft said "extra arguments are dropped", but `f.bind(x, a)` is a function
  whose first parameter is `f`'s **second**: taken as `f`, a call of it would put its arguments in the wrong slots of `f`'s key, a guess.
  A bind with an argument (or a spread) stays `Unknown`;
- **it is `Function.prototype.bind`**: the call's resolved signature is a `bind` declared in an interface `Function`, `CallableFunction` or
  `NewableFunction` of code that is not the project's. A function merged with a namespace that exports its own `bind` is not.

`this` is not followed (TC5). `call` and `apply` stay counted. **`uncovered_bind` stays as it is**: it is the default resolver's count
(TS4), the same with the layer off and on; with the layer on the edge appears in the callee's context (`run`'s), not at the hand-over.

## 7. Fixtures and tests

`local-alias` · `local-either` · `local-assigned-twice` · `local-open` (a `for … of` target: `a` and the engine's answer) · `local-cycle` ·
`local-never-assigned` · `local-write-unmodelled` (a write in a form the table has no row for: open, counted) · `local-data` · `param-reassigned` (built as
the row `callback-reassigned` flipping: the handler **and** `fallback`, complete) · `capture-handler` · `capture-sibling` ·
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
| 1 | `sem/flow.rs`: `Var`, `VarId`, assignments, `Expr::Var`, writes from nested functions; `eval` gives `Var` unknown | the table of `flow/src/vars.ts` and the tour's (line c: `call var h@35 ()`, `h@35 capable = def encode`) as written by hand; every `Expr::Var` has its row in the declaring def's table; `Param.assigned` equals its row's writes; no graph, digest or other snapshot moves |
| 2 | the whole-scope join (§3): `env.rs` `value_of`, the memo; parameters joined with their assignments; the counter of the ceiling of 2b | `local-*` (`local-assigned-twice`: `a` and `b` at both calls), `param-reassigned`; the ceiling on the four projects |
| 2b | **not built (owner, 2026-11-25, on the measure in `engine/10-tc3.md`: no edge at a callee would change); re-measured at TC6's close.** Was: only if step 2's ceiling is not near zero on the private codebase and the compiler: in order (D67, §3), opening with the spike (flow nodes at the pin, the `ReduceLabel` view, the nested-write rule, loops, unreachable code, the memo and the counted cap), behind `Caps::ORDER` | one fixture per flow node kind, by hand; fail-open counted; the sweep bit off/on with the removed members read by hand on two projects; the cost (D62) |
| 3 | part 1: `Flow::around`, `needs` (printed in the table snapshots, tested by hand); part 2: `Fn` with captures, `Key.captured`, the canonical form, the depth limit | part 1: needs as written by hand, no graph moves; part 2: `capture-*`; TC2's digests unchanged |
| 4 | module variables (§5 as decided): `var_of` through imports, script files open, `World::module_value`, `Caps::MODULES` | `module-*`; `modules-off.txt` unchanged |
| 5 | `bind` (§6 as decided): no bound argument, `Function.prototype.bind` only | `bind-*`; `uncovered_bind` unchanged |
| 6 | relevance (moved from TC2 step 8, [06](06-tc2-functions-through-parameters.md) §8), through locals | equal edges, fewer contexts |
| 7 | the four projects; the cost (`scripts/ts-try.sh cost`, D62) | `ENGINE.md` |
| 8 | (mine) `policy.md`, `ENGINE.md` | reviewed |

Next: [`08-tc4-objects-and-props.md`](08-tc4-objects-and-props.md)
