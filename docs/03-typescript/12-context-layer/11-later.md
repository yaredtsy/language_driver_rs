# 03.12.11 Later: TC7 to TC10

```
later
├── 1. TC7   an anonymous function under the function that calls it   (Q-TC1)
├── 2. TC8   fields a constructor stored
├── 3. TC9   recursion with values; arrays and maps; a value per place
├── 4. TC10  a table of library functions                              (Q-TC2)
└── 5. what is not planned
```

These are **sketches, not plans**. Each is designed in a file of its own when it is reached, from the numbers TC0–TC6 give. They are
written down now for one reason: so that nothing in TC1–TC6 closes a door they need. Each section ends with what it asks of the
earlier phases, and that part is binding.

The rule for every phase ([README](README.md)) holds for these too.

## 1. TC7: an anonymous function under the function that calls it

**Needs Q-TC1 = B.** Today, and through TC6, an arrow's calls are drawn under the def it is written in. TC7 draws them where the arrow
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

## 3. TC9: recursion with values; arrays and maps; a value per place

Three things that each wait for a counter.

**A value around a cycle.** TC6 answers unknown when a context asks for the return value of one that is still being walked. If
`return_in_cycle` is large on real code: iterate. Values are finite sets with caps, and a join only adds members, so repeating the
walks of a cycle until no return value changes must end. Python's `solver.rs` does this with groups, parked walks and a round limit;
core would get the smallest form of it, in the note style, with Rust's RC6.

**Collections.** `for (const h of handlers) h()` needs "an array is the join of what was put in it": a literal's elements, with every
`push` a write-through (open). `handlers.forEach(h => h())` needs that **and** a library model (who calls the arrow, with what):
TC10. A `Map` or a `Set` keyed by run-time values stays unknown.

**A value per place.** If N4 shows many variables assigned more than once, `Env::value_of(variable)` becomes
`value_of(variable, at)`: the join of the assignments that can reach a place, read from the binder's flow graph (public in tsrs) with
our own walk of it. Captured variables keep the whole-scope join, which is the sound one for them.

**Asks of the earlier phases:** `eval` never assumes a variable has one value except through `Env::value_of`; the solver's handle
answers `None` and not a panic for a context in progress.

## 4. TC10: a table of library functions

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
