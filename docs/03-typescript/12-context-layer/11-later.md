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

**Decided by D54 (2026-11-21), to be confirmed for TypeScript (README).** Python's layer has no example of it: it evaluates a `lambda` to
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
  | `private` or `#x` | its writes can only be in the class's own text: `get_references_to_symbol_in_file` + `ast::is_write_access`, one file |
  | public and not `readonly` | a write may be anywhere: unknown, counted `field_public_mutable`. A project-wide scan is possible and is not planned before N7 says it is needed |

- `Member(This, x)` in a method reads the receiver's field. TC4's trust rule covers an instance as it covers an object: one that is
  written through, or handed to code that may write through it, has open fields.
- A class component's `this.props` is the field React's base class stores: the constructor's parameter 0.
- **Not planned:** a field set by a later method call (Python's effects), two names for one object (Python's alias vector).

N7 decides whether this phase is built and for whom: if a project's `this.x` sites are fields of classes that a container builds
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

**A value per place.** If N4 shows many variables assigned more than once, `Env::value_of(variable)` becomes
`value_of(variable, at)`: the join of the assignments that can reach a place, read from the binder's flow graph (public in tsrs) with
our own walk of it. Captured variables keep the whole-scope join, which is the sound one for them.

**Asks of the earlier phases:** `eval` never assumes a variable has one value except through `Env::value_of`; the solver's handle
answers the value so far, and never panics, for a context in progress.

## 4. TCL and TC10: library-made functions; a table of library functions

### 4.1 TCL: a function a library made has a name (pulled forward, 2026-11-23)

**Needs Q-TC2 (b). Needs nothing of the layer**: no key, no value, no table. It can be built any time after TC0, and before TC1.

```tsx
const [open, setOpen] = useState(false);     setOpen(true);       // today: Unresolved(function_typed_value), counted `_from_call_result`
const { mutate } = useMutation(opts);        mutate(task);
const navigate = useNavigate();              navigate("/");
```

| | |
|---|---|
| what it is | the callee is a `const` name bound straight from the result of a call, by a plain name, an array pattern or an object pattern, and that call's target is **outside the project** |
| what it becomes | an `External` edge named after the call that made it and the path to the name: `react` · `useState()[1]`, `@tanstack/react-query` · `useMutation().mutate`. The spelling is part of Q-TC2 (b) |
| what it is not | a project function's result (that is TC6); a `let`; a name bound through anything but one pattern; a function passed in |
| where | `sem/resolve.rs`, beside the split that already tells a call's result from a parameter (`FunctionOrigin`) |
| what changes | the **default** graph: so it is a policy row with its fixture, a line in the tag notes, and N8 (TC0) is its forecast. On `TaskDetailPanel` it is 43 of the 86 unresolved calls |
| why now | it is the largest single group of "unresolved" in React code, no phase of the layer can ever reach it (the function is made inside the library), and it risks nothing the layer depends on |

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
