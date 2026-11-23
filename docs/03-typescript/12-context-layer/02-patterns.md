# 03.12.02 The patterns

```
the patterns
├── 1. how to read the tables
├── 2. A: a function goes in          (callbacks)
├── 3. B: an object goes in           (options, props)
├── 4. C: an instance goes in         (interfaces, base classes)
├── 5. D: something comes back        (factories, hooks, curry)
├── 6. E: something is stored         (fields, collections)
├── 7. R: recursion
├── 8. F: what no phase reaches, and what it is instead
├── 9. the measured columns, and which patterns fill them
└── 10. the order, and why
```

A call tree is unlocked when a site that says "a function-typed value is called here" starts to say **which function**. These are the
shapes of code where that is knowable, grouped by how the function gets there. Each row becomes a fixture
(`tests/fixtures/context/<row>/`) and a row of the precise policy table, in the phase named.

## 1. How to read the tables

- **Today** is the default resolver's answer, from `languages/typescript/docs/policy.md` where a row exists; `[check]` where I
  inferred it from a neighbouring row. TC1 writes every fixture with its default snapshot first, which checks them all.
- `open` is short for `Unresolved(function_typed_value)`: the engine found a function *type*, or a signature with no body (D16, D43).
- **Then** is what the tree shows once the phase is built, under a caller that passes the value. Under an entry asked for directly, a
  parameter is unknown and the answer stays today's (§8, F4).

## 2. A: a function goes in

| | Pattern | Code | Today | Then | Phase |
|---|---|---|---|---|---|
| A1 | a callback, by name | `function run(cb: () => void) { cb(); }` · `run(handler)` | `cb()` open; `handler` `[Passed]` under the caller | `run` → `handler`; the passed edge stays (D22: a def can have both) | **TC2** |
| A2 | passed through | `function relay(f) { each(f); }` · `relay(handler)` | open at the end of the chain | resolved at the end: each function evaluates its argument where its own parameter already has a value | TC2 |
| A3 | either of two | `run(flag ? a : b)` | open | `run` → `a` and `b`, two nodes at one site, **one** context of `run` (the key holds the set) | TC2 |
| A4 | a default | `function run(cb = noop) { cb(); }` · `run()` | open | `run` → `noop` | TC2 |
| A5 | two callers | `run(a)` in one function, `run(b)` in another | both show `run` with nothing under it | two contexts of `run`; each caller sees its own | TC2 |
| A6 | a method handed over | `run(api.load)`, `run(this.save)` | open; `[Passed]` | `run` → `Api.load` (the receiver is not followed: `this` inside is unknown) | TC2 |
| A7 | a local alias | `const h = handler; run(h)` · `const h = flag ? a : b; h()` | `[check]` open | resolved | **TC3** |
| A8 | **a nested handler reads a parameter of the function around it** | `function Panel({ onClose }) { const back = () => { onClose(); }; return <button onClick={back} />; }` | `onClose()` open inside `back` | `back` → what the parent passed as `onClose` | TC3 (with TC4 for the props) |
| A9 | a module constant | `const onSave = flag ? a : b;` at the top of a file · `onSave()` in a function | `[check]` open | resolved; the value belongs to the module, not to a key | TC3 |
| A10 | bound | `run(handler.bind(null))`, `run(this.save.bind(this))` | counted (`uncovered_bind`), no edge | the function behind the `bind`; `this` not followed | TC3 |
| A11 | an anonymous function | `each(() => work())` | `work` under the caller (it is written there); `f()` in `each` open | TC2: `f()` is known to be that arrow: `Unresolved(anonymous_function_called)`, as `(() => {})()` already is (H10). **TC7** moves `work` under `each` (D54) | TC2, TC7 |

A8 is the reason captures are in the plan from TC3 and not "later": in a React component almost every call of a prop sits inside a
nested `const` arrow, which is a def of its own.

## 3. B: an object goes in

| | Pattern | Code | Today | Then | Phase |
|---|---|---|---|---|---|
| B1 | an options bag | `function open(opts: Options) { opts.onDone(); }` · `open({ onDone: save })` | open | `open` → `save` | **TC4** |
| B2 | **props** | `<Panel onClose={handleClose} />` · `function Panel(props) { props.onClose(); }` | open; `handleClose` `[Passed]` | `Panel` → `handleClose` | TC4 |
| B3 | destructured | `function Panel({ onClose, actions: { save } })`, `const { onDone } = opts` | open | resolved: a pattern is a path into the object | TC4 |
| B4 | spread through | `<Inner {...props} />`, `open({ ...defaults, onDone: save })`, `function P({ a, ...rest }) { use(rest) }` | open | members written **after** a spread are known; the others are known only if the spread value is | TC4 |
| B5 | a component as a prop | `<Layout Icon={SaveIcon} />` · `<Icon />` inside `Layout` | `<Icon />` open (a JSX tag that is a value) | `Layout` → `SaveIcon` | TC4 |
| B6 | a class as a value | `register(JsonWriter)` · `function register(C) { new C(); }` | counted (`uncovered_class`); `new C()` open | a `Construct` of `JsonWriter` | TC4 |
| B7 | an object that implements an interface | `emit({ write() { encode(); } })` · `function emit(w: Writer) { w.write(); }` | open | `emit` → the literal's method (it is a def, D9) | TC4 |
| B8 | a shorthand, a method, an arrow in the literal | `open({ onDone })`, `open({ onDone() {} })`, `open({ onDone: () => {} })` | `[Passed]` (D22 steps 3 and 4) | the first two resolve in `open`; the arrow is A11's question when it has no stable name, a def when it has (D21) | TC4 |

## 4. C: an instance goes in

| | Pattern | Code | Today | Then | Phase |
|---|---|---|---|---|---|
| C1 | through an interface | `function emit(w: Writer) { w.write(); }` · `emit(new JsonWriter())` | open (D43) | `emit` → `JsonWriter.write` | **TC5** |
| C2 | through a base class | the scenario `two-callers`: `Writer` is a class, `JsonWriter` overrides `write` | `emit` → `Writer.write` | `emit` → `JsonWriter.write`: the one place the layer **replaces** an answer the engine gave, because a method reached through a receiver can be overridden | TC5 |
| C3 | the argument's static class | `emit(this.json)` where the field is declared `JsonWriter` | open | `emit` → `JsonWriter.write`: where the layer has no value, the checker's class of the argument is the value | TC5 |
| C4 | `this` in a base method | `class Job { run() { this.step(); } }` · `new Upload().run()`, `Upload` overrides `step` | `Job.run` → `Job.step` (or open, if `step` is abstract) | `Job.run` → `Upload.step` | TC5 |
| C5 | declared wider than built | `const w: Writer = new JsonWriter(); w.write();` | open: the checker answers the declared type | `JsonWriter.write` | TC5 |
| C6 | plain JavaScript | `function emit(w) { w.write(); }` · `emit(new JsonWriter())` | `Unresolved(any_or_untyped)` | `emit` → `JsonWriter.write` | TC5 |

## 5. D: something comes back

| | Pattern | Code | Today | Then | Phase |
|---|---|---|---|---|---|
| D1 | a factory of instances | `function pick(): Writer { return new Csv(); }` · `pick().write()` | open | `Csv.write` | **TC6** |
| D2 | returns its argument | `function ident<T>(x: T) { return x; }` · `ident(handler)()` | `Unresolved(no_declaration)` (as `f()()`) | `handler` | TC6 |
| D3 | a custom hook | `function useSaver() { const save = () => {…}; return { save }; }` · `const { save } = useSaver(); save()` | open (the "call's result" column) | `save`, with what it captured in `useSaver` | TC6 |
| D4 | a higher-order component | `function withAuth(C) { return function Guard(p) { return <C {...p} />; }; }` · `const Page = withAuth(Home)` · `<Page />` | open | `Guard`, and under it `Home` | TC6 (+ TC3) |
| D5 | curry | `function twice(f) { return () => { f(); f(); }; }` · `twice(work)()` | `Unresolved(no_declaration)` | TC6: known to be the arrow written in `twice`; TC7 draws `work` under it | TC6, TC7 |
| D6 | awaited | `const w = await makeWriter(); w.write()` | open | as D1: `await` is looked through | TC6 |
| D7 | a getter's result, a decorator factory's result | `b.fn()` with `fn` a getter; `@factory(1)` | counted (`uncovered_getter_result_calls`); `Unresolved(decorator_result)` | resolved | TC6, last |
| D8 | a recursive function's value | `function build(n) { return n ? build(n - 1) : leaf; }` · `build(3)()` | open | `leaf`: the value of a cycle is worked out by rounds ([10](10-tc6-return-values.md) §3.1) | TC6 |

## 6. E: something is stored

| | Pattern | Code | Today | Then | Phase |
|---|---|---|---|---|---|
| E1 | a parameter property | `class Pipe { constructor(private readonly sink: Writer) {} run() { this.sink.write(); } }` · `new Pipe(new Json()).run()` | open (the `this` column) | `Json.write` | **TC8** |
| E2 | stored by the constructor | `constructor(w) { this.w = w; }` | open | resolved if nothing else writes the field (the checker can list its writes) | TC8 |
| E3 | written elsewhere | `setWriter(w) { this.w = w; }` | open | **open**, counted | never, unless measured |
| E4 | an array or a map of functions | `handlers.forEach(h => h())`, `table[name]()` | open, or `Unresolved(computed_name)` | open until TC9 | TC9 |
| E5 | an event name | `bus.on("save", f)` · `bus.emit("save")` | `f` `[Passed]` | the same: a string connects them, and we follow no strings | never |

## 7. R: recursion

| | Pattern | Code | What happens | Phase |
|---|---|---|---|---|
| R1 | a callback carried around a cycle | `function walk(n, visit) { visit(n); n.kids.forEach(k => walk(k, visit)); }` | the recursive call passes the same value, so it is the **same key**: one context, an edge to itself, drawn `[Recursive]`. The flat solver needs nothing | TC2 |
| R2 | mutual recursion with a callback | `a(cb) { b(cb) }` · `b(cb) { cb(); a(cb) }` | the same: two contexts, a cycle of edges | TC2 |
| R3 | a key that grows | `function f(cb) { f(() => cb()); }`, `function g(o) { g({ next: o }); }` | each call would make a new key for ever. The depth cap of a key stops it: past the cap the member is `Unknown`, so the next key equals the last | TC4 (objects), TC7 (closures); the cap is in TC2 |
| R4 | a value around a cycle | D8 | worked out by rounds, the same from any entry | TC6 |

## 8. F: what no phase reaches, and what it is instead

| | Pattern | Why not | It stays |
|---|---|---|---|
| F1 | what a library call returned: `const [open, setOpen] = useState()` · `setOpen(true)`; a store made by `create(…)` | the function is made inside code we never enter. Half of `TaskDetailPanel`'s 86 unresolved calls are this one idiom | open today. **TCL** gives it a name: an `External` after the call that made it (Q-TC2 b; [11](11-later.md) §4) |
| F2 | an object a container built: vscode's `@IFileService private readonly files: IFileService` | no `new` in project code gives the constructor its arguments | open (`this` column) |
| F3 | a name computed at run time: `obj[name]()`, reflection | no value to follow | `Unresolved(computed_name)` |
| F4 | **the entry's own parameters** | a request names a def; nobody says what was passed to it | today's answer. A tree asked for at the parent is richer than one asked for at the child: that is the nature of the layer, and worth one sentence in the product |
| F5 | what a callback **receives**: `xs.map(x => x.run())`, `on(evt => handle(evt))` | the arrow's own parameters are filled by whoever calls it; when that is a library, never | the checker's type of `x`, as today |

## 9. The measured columns, and which patterns fill them

From the sweep of 2026-11-19 (`ENGINE.md`), `function_typed_value` by where the function comes from:

| Column | V-NOC | private CLI | TypeScript's compiler | vscode | Patterns |
|---|---|---|---|---|---|
| a parameter | 17% | 48% | 31% | 27% | A1–A8, B1–B8, C1–C3, C6: TC2–TC5 |
| a call's result | 77% | 24% | 22% | 14% | D1–D7 (TC6); **F1 is in this column too** and stays |
| `this` | 1% | 3% | 0% | 47% | C4 (TC5), E1–E2 (TC8); F2 stays |
| other | 5% | 24% | 47% | 12% | A7, A9 (TC3), E4 (TC9), and what TC0 finds |

The columns say where a function is *called*. They do not say whether a caller *passes* something we can follow, and that is what
decides how much a phase resolves. TC0 counts the other end (what the arguments are), so each phase has a forecast before it is built.

## 10. The order, and why

```
TC2  functions through parameters     the smallest real key; proves the whole path: key, digest, two contexts, tests
TC3  variables and captures           needed by A8, and A8 is most of React's callback calls
TC4  objects and props                with TC3, the first phase V-NOC's own code shows
TC5  instances                        the scenario every language shares (two-callers); JavaScript
TC6  return values                    the largest column on V-NOC; the only phase that changes core
```

Functions before instances, because a function value needs nothing from the checker (the hand-over code of D22 already finds every
named function in an argument), so TC2 tests the key and the solver with the least new `sem` code. Objects before instances, because
props are objects and V-NOC is the first project the result is judged on. Return values last of the five, because they need the core
change, and because a factory is of little use until what it returns (a function, an object, an instance) is a value the layer knows.

TC0 can reorder TC3–TC5. It cannot reorder TC1, TC2 or TC6.

Next: [`03-design.md`](03-design.md)
