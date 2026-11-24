# 03.12.11 Later: TC7 to TC10

```
later
├── 1. TC7   an anonymous function under the function that calls it   (D54)
├── 2. TC8   fields a constructor stored
├── 3. TC9   arrays and maps; a value per place
├── 4. TCL and TC10  library-made functions; a table of library functions   (Q-TC2)
└── 5. what is not planned
```

These are **sketches, not plans**. Each is designed in a file of its own when it is reached, from the numbers TC0–TC6 give. They are
written down now for one reason: so that nothing in TC1–TC6 closes a door they need. Each section ends with what it asks of the
earlier phases, and that part is binding.

The rule for every phase ([README](README.md)) holds for these too.

## 1. TC7: an anonymous function under the function that calls it

**Not scheduled (D58, 2026-11-23): the easy part first.** D54 (2026-11-21) says an arrow's calls move; you chose to build only the part
where they stay, and to look at D54 again before this phase is ever built. Python's layer has no example of it: it evaluates a `lambda` to
unknown. Today, and through TC6, an arrow's calls are drawn under the def it is written in. TC7 draws them where the arrow
is **called**, when a project function calls it.

| Code | Through TC6 | TC7 |
|---|---|---|
| `each(() => join())` in `main` · `function each(f) { f(); }` | `main` → `each`, `join` | `main` → `each` → `join` |
| `each(x => x.run())` · `function each(f) { f(new Job()); }` | `x.run()` by the checker's type | `each` → `Job.run`: the arrow's parameter is filled by the caller |
| `xs.map(x => work(x))`, `useEffect(() => load())`, `<button onClick={() => save()} />` | under the writer | **under the writer**: no body we walk calls the arrow |

A sketch:

- `One::Anon` gains the arrow's captures, as `One::Fn` did in TC3: the variables it reads from its writer, read where it is written.
- At a site whose callee value is `Anon(a)`: the arrow's own sites (they are in its writer's table, each marked with the anonymous
  function it sits in) are resolved in an env made of the captures and of the arrow's parameters bound to this call's arguments.
  Their targets become **targets of the calling site**: one site, several targets, ordered by rule 7.
- The hard part is **removing** them from the writer. Removing is right only if the receiver really calls its parameter. "Does `g`
  call its parameter `i`, itself or through what it passes it to" is a fact about `g`'s table, like `touches` in TC4. If it does not
  (it stores the arrow), the calls must stay with the writer, or they vanish from every tree.
- An arrow reached at two calling sites is drawn at both, and at neither twice. An arrow inside an arrow repeats the rule.
- In a key, an arrow that moves gets its identity back (through TC6 a key writes every arrow `AnonAny`), with its captures.
- **The tree marks recursion by function, not by context** (rule 8 of [`02-tree-rules/02-the-rules.md`](../../02-tree-rules/02-the-rules.md)).
  An arrow moved under `each` whose body leads back to `each` (a continuation, a visitor, a retry) makes that second `each` a `[Recursive]`
  leaf, and the calls the arrow makes after it are in the graph and not in the tree. Three ways out, yours to choose when TC7 is designed:
  an arrow moves only when its body cannot lead back to the function it is handed to; or arrows never move; or rule 8 changes in core, for
  every language. The wire tree is the output that is used (D17), so this is decided on a tree, not on a graph.

It is designed once, with Rust (Q-RC1), in `docs/02-tree-rules/`: the question "where is a closure's call drawn" is about the tree,
not about a language.

**Asks of the earlier phases:** every site in a table says which anonymous function it is written in (TC1 does); `One::Anon` is a
value from TC2; `anonymous_function_called` is the reason of the calls TC7 will draw, so their count is its forecast.

## 2. TC8: fields a constructor stored

```ts
class Pipe {
  constructor(private readonly sink: Writer) {}
  run() { this.sink.write(); }           // open today: `this` column
}
new Pipe(new Json()).run();              // TC8: Pipe.run → Json.write
```

- `One::Instance` gains fields: `Instance(class, fields)`, where `fields` is what the **construction** stored: a parameter property,
  `this.x = e` in the constructor (an assignment to `Member(This, x)` in the constructor's table), a property initializer.
- A field is in the value only if **nothing outside the constructors can write it**:

  | The field is | Checked by |
  |---|---|
  | `readonly` | the checker enforces it; nothing to scan |
  | `private` or `#x` | its writes can only be in the class's own text: the scoped search of 04 §3.5 over the class, + `ast::is_write_access`; a `#x` is a `PrivateIdentifier`, which that search must take too |
  | public and not `readonly` | a write may be anywhere: unknown, counted `field_public_mutable`. A project-wide scan is possible and is not planned before N7 says it is needed |

- `Member(This, x)` in a method reads the receiver's field. TC4's trust rule covers an instance as it covers an object: one that is
  written through, or handed to code that may write through it, has open fields.
- A class component's `this.props` is the field React's base class stores: the constructor's parameter 0.
- **Not planned:** a field set by a later method call (Python's effects), two names for one object (Python's alias vector).

**N7 on three projects (2026-11-23, `ENGINE.md`):** the `this` column is small (7, 144, 17), two thirds of it fields set by a constructor of a
class the project builds by `new`. vscode's N7 is still to run. N7 decides whether this phase is built and for whom: if a project's `this.x` sites are fields of classes that a container builds
(vscode's services), no constructor argument is ever seen and TC8 resolves nothing there.

**Asks of the earlier phases:** `Key.this` is a `Value` (TC5), so an instance with fields fits in it; the depth cap counts fields
like members; `Expr::Member` on `This` is in the table from TC5 and evaluates to unknown until TC8.

## 3. TC9: arrays and maps; a value per place

Two things that each wait for a counter. (A third, a value around a cycle, was here until the review of 2026-11-23.)

**A value around a cycle: moved into TC6.** It was planned here, "if `return_in_cycle` is large". But answering unknown inside a cycle is not
only less precise: it makes a context's edges depend on where the request entered ([10](10-tc6-return-values.md) §3.1). So the rounds are
part of the solver's handle from the day return values exist, as they are in Python.

**Collections.** `for (const h of handlers) h()` needs "an array is the join of what was put in it": a literal's elements, with every
`push` a write-through (open). `handlers.forEach(h => h())` needs that **and** a library model (who calls the arrow, with what):
TC10. A `Map` or a `Set` keyed by run-time values stays unknown.

**A value per place. Planned (TC0 N4, 2026-11-23):** 16% of the private codebase's capable locals and 9% of the compiler's are written more
than once (V-NOC 1%), over 04 §4's "a few percent". Before it is designed, a count says how many of those variables reach a call (a callee
or an argument): only those cost an edge. Then `Env::value_of(variable)` becomes
`value_of(variable, at)`: the join of the assignments that can reach a place, read from the binder's flow graph (public in tsrs) with
our own walk of it. Captured variables keep the whole-scope join, which is the sound one for them.

**Asks of the earlier phases:** `eval` never assumes a variable has one value except through `Env::value_of`; the solver's handle
answers the value so far, and never panics, for a context in progress.

## 4. TCL and TC10: library-made functions; a table of library functions

### 4.1 TCL: a function a library made has a name (pulled forward, 2026-11-23)

**Decided (D56, 2026-11-23): build it the simple way, and spend no more time on it.** It needs nothing of the layer: no key, no value, no
table. It can be built any time after TC0, and before TC1.

```tsx
const [open, setOpen] = useState(false);     setOpen(true);       // today: Unresolved(function_typed_value), counted `_from_call_result`
const { mutate } = useMutation(opts);        mutate(task);
const navigate = useNavigate();              navigate("/");
```

| | |
|---|---|
| what it is | the callee is a `const` name bound straight from the result of a call, by a plain name, an array pattern or an object pattern, and that call's target is **outside the project** |
| what it becomes | an `External` edge named after the call that made it and the path to the name: `react` · `useState()[1]`, `@tanstack/react-query` · `useMutation().mutate`. The spelling is the simplest one that tells two of them apart, and it is not a frozen name (rule 10 is about defs). **The maker is spelled from the call as written in the project** (its callee's text, the arguments left out: `useStore()`, `create()()`), never from the declaration it resolves to: 433 of V-NOC's 766 package-made values come from a call signature that has no name (zustand's store hook, `cva`; TC0 N8, `ENGINE.md`). Each such edge is counted `library_made_function` in `Stats.language` |
| **how it is kept inside** (D56) | as a kind of its own: `sem` answers `Resolved::LibraryMade { package, maker, path }`, not `Resolved::External`. **One** function (`walk::target`) turns it into what the graph shows. Today that is an `External`; the day you choose another display (a mark, a kind of node, hidden), that function changes and nothing else: the finding is not done twice, and no fixture of the finding moves |
| what it is not | a project function's result (that is TC6); a `let`; a name bound through anything but one pattern; a function passed in |
| where | `sem/resolve.rs`, beside the split that already tells a call's result from a parameter (`FunctionOrigin`) |
| what changes | the **default** graph: so it is a policy row with its fixture, a line in the tag notes, and N8 (TC0) is its forecast. On `TaskDetailPanel` the plan said 43 of the 86 unresolved calls (the setters); measured at `aa32042`, before condition 7: 64 (86 → 22); **as built** (`fbee1b4`, conditions 5 and 7): 56 (86 → 30). Whole V-NOC, depth 1: unresolved 1,130 → 509; the private codebase and the TypeScript compiler unchanged, graph for graph (`ENGINE.md`, "TCL as built and measured") |
| why now | it is the largest single group of "unresolved" in React code, no phase of the layer can ever reach it (the function is made inside the library), and it risks nothing the layer depends on |

**As decided for the build (2026-11-24).** At a site whose answer holds `function_typed_value` and whose origin is a call's result:

1. the callee, under `unwrap_expression`, is an `Identifier` (no member step: `setOpen()`, `setOpen?.()`);
2. its symbol, through an import alias, has a first declaration that is a `VariableDeclaration` with a plain name (path `""`), or a
   `BindingElement` that is not a rest element, of a pattern whose parent is that `VariableDeclaration` (one pattern only): in an array
   pattern its path is `[i]`, `i` its index among the pattern's elements, holes counted; in an object pattern `.k`, `k` the property's name
   (`property_name`, else the name), spelled by `spelled`;
3. the `VariableDeclaration` is `const`;
4. its initializer, under `unwrap_expression` and `await`, is a call `c`;
5. `c`'s resolved signature has a declaration, in a file that is not project code **and not a bundled library** (`lib.*.d.ts`,
   `is_source_file_default_library`; added at review, 2026-11-24); the package is named as `classify` names an external's (`package_of`,
   else `place_of`). TypeScript's own library makes no function: what it hands back is a value the project put there (`map.get(k)`,
   `queue.shift()!.resolve`, `V | undefined` and `T | undefined`, which condition 7 does not see). Measured on the private sample at
   `aa32042`: all 6 of its library-made calls were of this kind. `f.bind(x)` is not one: the engine answers its call as `f` itself
   (`Call`), so it never reaches this rule;
6. the maker is `c` spelled from the source: a call is its callee's spelling and `()`; a callee that is itself a call is spelled the same
   way (`create<S>()(…)` is `create()()`); any other callee is its text as written, whitespace taken out, type arguments not part of it;
7. **the library made it** (added at review, 2026-11-24): the return type written on that declaration (`type_node()`) is not a type
   reference to a type parameter (of the signature or of the type around it). `useCallback<T>(callback: T, deps): T`, `useMemo<T>(…): T`,
   `useEffectEvent<T>(e: T): T`, zustand's selector `<U>(selector: (state: S) => U): U` and `getState: () => T` hand back a value the
   **caller** supplied, a function the project wrote (the callback, the store's action): naming it after the library would be a wrong
   answer (rule 8). Those stay `function_typed_value` (`_from_call_result`), for TC10 (Q-TC2 a) or the layer to reach. Path-blind on
   purpose: a type parameter's part is still the caller's. What it misses: a return type *computed* from a type parameter
   (zustand's no-selector `(): ExtractState<S>`, so `const { closeModal } = useStore()`) is still named after the library; 4 of 650 on
   the V-NOC sample, accepted as the simple way (D56). A package's `T | undefined` is missed the same way (a union is not a type
   reference): none on the V-NOC and private samples.

**Measured before condition 7** (`aa32042`, V-NOC sample of three entries, whole graphs, counted by edge per context): 620 of 662
call-result sites became library-made, unresolved 827 → 207; by name, `create()()` 237, `useState()[1]` 193 (+ `React.useState()[1]`
11), zustand selectors (`useProjectStore()` and the like) 87, `useCallback()` 25 (+ `React.useCallback()` 4), `useForm().…` 21, `cva()` 7,
`useEffectEvent()` 4, `memo()` / `forwardRef()` 6. The selectors, `useCallback` and `useEffectEvent` (about 120) are what condition 7
removes. `memo(C)` and `forwardRef(C)` stay named: their result is an object React made, and React calls `C`.

Then `sem` answers `Resolved::LibraryMade { package, maker, path }` in place of each `Unknown(FunctionTypedValue)` of the site, and
`walk::target` turns it into `External { package, name: maker + path }` and notes `library_made_function`. The site is then no longer
counted in `function_typed_value` nor in `_from_call_result`. The breakdown prints the count when it is above 0. Policy row
`library-made`; fixture `policy/src/library_made.tsx`, with a package of its own (`policy/node_modules/hooks/`).

### 4.2 TC10: a table of library functions

**Needs Q-TC2.** Some of the most common unresolved calls go through a library function whose behaviour is fixed and simple, and
that no amount of value tracking can see into, because we never walk a package.

| Kind of row | Examples | Effect |
|---|---|---|
| **returns its argument** | `useCallback(f, deps)` → `f`; `memo(C)`, `forwardRef(C)` → `C`; `Object.freeze(o)` → `o` | the call's value is the argument's value |
| **returns what its argument returns** | `useMemo(() => v, deps)` → `v` | the anonymous function's return value (TC6) |
| **leaves its arguments alone** | `console.log`, `JSON.stringify`, `Object.keys` | TC4's trust rule does not open the object |
| **calls its argument** | `Array.prototype.forEach/map/filter`, `Promise.prototype.then`, `setTimeout` | with TC7 and TC9: what the arrow's parameter is |

- **Data, not code**: (package, export) → behaviour, in one file, each row with a fixture. A row is a statement about a library. It is
  the second place the analyzer would say something it did not read in the user's code (D28 is the first), which is why it is a
  question and not a step.
- **What it cannot do**: `const [open, setOpen] = useState()`. `setOpen` is a function React makes. No row turns it into project code,
  because it is not. The honest choices are to leave it `Unresolved(function_typed_value)` (today), or to say what it is: an
  `External` edge named after the call that made it. That is the second half of Q-TC2. On `TaskDetailPanel` it is 43 of 86 unresolved
  calls, so it decides whether "unresolved" on React code reads as "half unknown" or as "a few unknown".
- The rows worth writing are the top of `external_call_result` by package (TC6 step 9, N8).

**Asks of the earlier phases:** an external call's value is produced in one place (`eval(Call)` on an `External` target, TC6 §4), so a
table has one place to answer.

## 5. What is not planned

| | Why |
|---|---|
| listing the classes that implement an interface (the old C2) | Q-TC5: tsrs has it only behind LSP types; an edge keeps one meaning, "is called"; D36 decided the same for Rust |
| type arguments in the key (Rust's RC1) | tsrs keeps a signature's inferred type arguments private ([01](01-where-we-start.md) §4). If upstream exposes them, it is a cheap, exact addition for generic code |
| objects a dependency-injection container builds | no `new` to follow |
| an event name connecting `on("save", f)` to `emit("save")` | a string, not a value |
| a tree asked for at a def knowing what its callers pass | a request names one def. A view that joins a def's contexts across its callers is a product feature on top of the graph, not a phase of the analyzer |

Back to: [`README.md`](README.md)
