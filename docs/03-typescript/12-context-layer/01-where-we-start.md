# 03.12.01 Where we start

```
where we start
├── 1. what was read
├── 2. the seam, checked against the code
├── 3. Python's layer, module by module: what TypeScript takes, and what it does not
├── 4. what tsrs gives: public, private, and what follows for the design
├── 5. five things in the way
├── 6. the old steps C1–C6, mapped
└── 7. the verdict
```

## 1. What was read

| What | Where | For |
|---|---|---|
| Python's context layer | `languages/python/crates/python-analysis/src/interp/` (16 files, about 6,000 lines without its test harness) and its fixtures (`tests/fixtures/01-functions` … `04-class-summaries`) | what a finished layer looks like, and which parts are Python |
| TypeScript as built | `languages/typescript/crates/typescript-analysis/src/` (`walk.rs`, `calls.rs`, `env.rs`, `key.rs`, `session.rs`, `sem/`) | whether the seam of TS4 is really there |
| the solver | `core/analyzer-core/src/solve.rs` | what a real key costs in core: nothing until a value must come back |
| Rust's context design | `docs/04-rust/05-context-layer/` | the rules both languages should share, and the core changes it already lists |
| tsrs | `../language_clone/tsrs` at `13a31a4` (the pin), `tsrs_checker`, `tsrs_ast`, `tsrs_ls` | what the checker can be asked from another crate |
| the numbers | `languages/typescript/ENGINE.md`, "TS5 step 6 as measured, and the sweep on four projects" | which unresolved calls exist, by where the function comes from |

## 2. The seam, checked against the code

[`02-tree-rules/03-the-seam.md`](../../02-tree-rules/03-the-seam.md) promised that phase 1 leaves the door open. Line by line:

| Promise | The code today | Holds? |
|---|---|---|
| the answer is a `Graph` of contexts | `calls::graph_within` returns `Graph`; tree and wire are views | yes |
| `resolve(site, &env)` in its final form | `walk.rs:97` `resolve` calls `resolve_from_env` (line 128, returns `None`) and falls back to `sem.callee_with_facts` | yes |
| `Env` exists and is passed through the walk | `env.rs`: `pub(crate) struct Env;`, passed from `Lang::walk` to `walk::body` to `resolve` | yes |
| `Key` is a name, never `()` | `key.rs`: `pub(crate) type Key = ();`, `type Key = Key` in `calls.rs`. **But the value is written `()` in five places**: `walk.rs` lines 84, 147, 160, 172 and `calls.rs` line 172 | almost: five literals to replace by `Key::open()` |
| sites in source order | `walk::sites`, sorted again after the hand-overs are added (`walk.rs:88`) | yes |
| one module talks to the engine | `sem/` (a folder, D35); a site leaves it as a **span and an index** only | yes, and see §5.1 |
| a reason counter per kind | `Why`, `FunctionOrigin` (the split by parameter / call result / `this` / other) | yes |
| digest zeros | `calls.rs:110` | yes |
| the solver is keyed | `Solver.memo: HashMap<(L::Func, L::Key), Summary<L>>`; `Target::Call(func, key)`; `Start::Call(func, key)` | yes: a real key needs **no core change** |

So TC1–TC5 are work inside `typescript-analysis`. Core is touched for two small things in TC2 (how the scenarios are levelled, a cut
per function) and for one real thing in TC6 (a value coming back from a callee).

## 3. Python's layer, module by module

Python is the reference: it is built, tested and in use. What TypeScript takes from it is its **rules and its tests**; what it does not
take is its **machine**.

| Python module | Its job | TypeScript |
|---|---|---|
| `value.rs` | `AbstractValue`: function, closure with an activation, class, instance with an **origin** (an identity), bound method, the engine's type, unknown, pending | **takes the idea, drops identity.** A value says *what* is there (this function, an object of this class, this literal), never *which object*. No origins, no activations, no alias vector. Fields come in TC8, and only as "what the constructor stored" |
| `env.rs` | names → values **on this path**; attributes of objects; join at branches | **replaced.** One value per variable: the join of every expression assigned to it in its scope (§3.1) |
| `walk.rs` | executes statements into the env | **replaced.** Nothing is executed. `sem::flow` reads the body once into plain data; the layer evaluates expressions of that data on demand |
| `bind.rs` | arguments → parameters (`*args`, keywords, defaults) | **takes it, smaller.** Positional only; a spread argument and a rest parameter are `Unknown`; a missing argument starts from the default |
| `member.rs` | attribute lookup: instance dict, class, MRO | **the checker does it.** `get_property_of_type_exported` on the class's type, inherited members included |
| `key.rs` | `Fp`: a fingerprint of each argument, to depth 2; unions kept as sets; a closure's free values in the key | **takes it.** The same shape: a value per parameter, sets kept as sets, depth-capped, plus the variables a nested def captures |
| `relevant.rs` | a parameter the walk cannot be affected by is `Unknown` in the key | **takes it**, and adds a cheaper first filter Python does not have: a parameter whose declared type cannot hold a function or an object is never in the key |
| `canon.rs` | the key written with nothing request-local, as JSON bytes, hashed with `xxh3_128` | **takes it as is.** The second user of that scheme, so it can move to core (the rule of two) |
| `summary.rs` | calls, returns, new objects, effects, relative to the inputs | **takes returns only** (TC6). No objects, no effects |
| `call.rs` | one call: evaluate, key, look up or walk, apply the summary | the same steps, without the apply |
| `defer.rs`, `solver.rs` | recursion groups, the fixpoint, parked walks | **not taken.** Core's flat solver stays. A value asked for around a cycle is `Unknown` and counted; a fixpoint is TC9, if the count asks for it |
| D27 (`or_engine_callee`) | where the walk cannot call, the engine's exact type is used | **takes it**, turned around: where the layer has no value for an argument, the static class of the argument is the value (TC5) |
| the tests | the open/no-reuse cross-check, determinism, `contexts_per_func`, a counter for every limit | **takes all of them** |

### 3.1 Why not Python's executing walk

Three reasons, each from something read, not from taste:

1. **A closure captures the variable.** In JavaScript `const back = () => onClose()` reads `onClose` when it runs, not when it is
   written. Python's layer needs activations (a record of each run of a body) to get this right. "One value per variable" gets it
   right by construction: whoever reads the variable, wherever, sees the join of everything ever assigned to it.
2. **tsrs keeps its flow logic private.** The binder's flow graph can be read, but the functions that interpret it
   (`get_control_flow_container`, `is_matching_reference`, `get_flow_type_of_reference_ex`) are `pub(crate)`. An executing walk would
   mean writing our own, beside the checker's.
3. **The code is mostly `const`.** A variable assigned once has the same value under both models. What is lost is
   `let h = a; h(); h = b; h()`: both calls show `a` and `b`. That is a precision limit, written in the policy table, and it can be
   refined later behind the same function (`Env::value_of(variable, at)`) without touching anything that calls it.

What this buys: no fixpoint inside a body, no order of evaluation to get wrong, and the body's data can be snapshotted and tested with
no engine at all.

## 4. What tsrs gives

The full list, with file and line in tsrs, goes into `languages/typescript/ENGINE.md` at TC0 (S-TC1 confirms each by calling it). The
part that shapes the design:

| Need | tsrs | So |
|---|---|---|
| the signature a call resolves to, its declaration, its parameters in order | **public**: `get_resolved_signature_exported`, `Signature::declaration`, `::parameters`, `::has_rest_parameter` (`sem` already calls the first two) | binding by position is ours to write; the pieces are there |
| argument → parameter, with spreads, JSX attributes, tagged templates | **private** (`get_effective_call_arguments`, `get_spread_argument_index`, `get_parameter_count`) | we bind by position ourselves; a spread makes the rest `Unknown`; JSX attributes are read from the syntax as one object |
| is this identifier a parameter, which one, by which destructuring path | **public pieces**: `get_symbol_at_location_exported`, `Symbol::value_declaration`, `get_root_declaration`, `Node::property_name`; `sem/handed.rs` already walks binding patterns | `sem::flow` names every variable by its declaration |
| the class of an expression's static type | **public**: `get_type_at_location`, `ObjectFlags::Class`, `Type::symbol` | the static class of an argument is a value (TC5) |
| a member of a class by name, inherited ones included | **public**: `get_property_of_type_exported` (in use) | dispatch on a tracked instance is one query |
| `const w: Writer = new JsonWriter()`: what is `w`? | the checker says **`Writer`**: assignment narrowing only applies when the declared type is a union | local values add something the engine does not have, even in typed code |
| the type arguments the checker inferred for a call | **private**: `Signature.mapper` has no accessor | Rust's "types in the key" is not open to us. Values it is |
| which variables a function captures | **nothing**. `ast::is_node_descendant_of` and `Node::locals` are the pieces | captures are computed by `sem::flow`: a name whose declaration is outside the function's node |
| who implements this interface | only through the LSP layer (`provide_implementations`, LSP types in and out) | the old C2 is not built (Q-TC5) |
| every write of a property | **public**: `get_references_to_symbol_in_file` + `ast::is_write_access` | TC8's "is this field only written in the constructor?" can be asked |

## 5. Five things in the way

None is large. Each is a step of TC1 or TC2.

1. **No expression leaves `sem`.** A site is a span and an index; `sem` keeps the nodes and answers `callee(index)`. That is right for
   phase 1 and it is the gap: the layer must know *what a site passes* and *what a variable holds*. `sem::flow` is that, as owned data
   (TC1). It is the largest new piece of the whole plan.
2. **Five `()` literals** stand where a key is built (§2). They become `Key::open()` in TC1.
3. **Counters are kept per walk.** `TypeScript::note` adds one each time a body is walked. With two contexts of one function, every
   "what this body has" counter (`uncovered_*`, `signature_passed`, `engine_kind_unhandled`) would count twice. TC2 keeps them per def.
4. **A cut node continues as a new entry, by its def.** `Session::graph` documents it: "asking for that def as a new entry continues
   from there". A def has no key, so the continued subtree starts from the open key and is less precise than the same subtree drawn in
   one request: the same question, two answers, depending on the budget. A request must be able to start from a **context**
   (Q-TC3, [03](03-design.md) §10).
5. **The shared scenarios are levelled per language**, one line for all of them (`analyzer_testkit::scenarios::run(&TS, Level::Default)`).
   A language becomes precise one scenario at a time. Rust's plan lists the same change
   ([`04-rust/02-architecture.md`](../../04-rust/02-architecture.md) §9, row 2); TC2 does it.

Two more, noted and not blocking: the origin counter cannot tell a function's own parameter from one it captures from the function
around it (TC0 splits it); and a sweep at depth 1 says nothing about this layer, because an entry always has the open key (TC0 defines
a measure that does).

## 6. The old steps, mapped

| [`08`](../08-later-context-sensitive.md) §3 | Here | Changed |
|---|---|---|
| C1 local values | TC3 | no longer first: alone it moves few numbers in typed code, and it is the same code as captures |
| C2 show implementations | not built (Q-TC5) | follows D36 |
| C3 arguments in the key | TC2 (functions), TC4 (objects), TC5 (instances) | split by the kind of value, so each can be released |
| C4 return values | TC6 | the one core change |
| C5 attributes and effects | TC8 | fields a constructor stored; no general effects |
| C6 recursion fixpoint | TC9 | only if a counter asks for it |
| — | TC1 the body as data | new: C1–C6 assumed an executing walk |
| — | TC7 anonymous functions, TC10 library models | new, each behind a question |

## 7. The verdict

Yes, start, with TC0. The architecture question that mattered ("does a real key force a rewrite?") is answered by the code: no. The
choice this plan makes that the earlier direction did not is to **not port Python's interpreter**: TypeScript gets a table per body and
an evaluator over it. If TC0's counts show that the lost precision (one value per variable) matters on real code, the refinement has a
place to go; nothing above `env.rs` would change.

Next: [`02-patterns.md`](02-patterns.md)
