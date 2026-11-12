# 04.05.07 RC5: fields

```
RC5
├── 1. what the user sees
├── 2. which types carry erased values
├── 3. the value of a struct: Value::Fields
├── 4. building, reading, writing
├── 5. how a struct's value travels  (nothing new: RC4's roads)
├── 6. what is still Unknown, on purpose
├── 7. tests
└── 8. steps
```

## 1. What the user sees

| Code | Before | RC5 |
|---|---|---|
| `struct Pipe { sink: Box<dyn Writer> }` · `impl Pipe { fn run(&self) { self.sink.write() } }` · `Pipe { sink: Box::new(Json) }.run()` | `Pipe::run` → `Writer::write` | `Pipe::run` → `<Json as Writer>::write` |
| `Pipe::new(Box::new(Json)).run()` where `new` is `Pipe { sink }` | the same | the same as above: the value goes in through `new`'s key and comes out through its return value |
| two pipelines, one with `Json`, one with `Csv`, both calling `run` | one `run`, the declaration | two contexts of `run`, each with its own impl |
| `struct App { pipe: Pipe }` · `self.pipe.sink.write()` | the declaration | resolved, to `field_depth` steps |
| `struct Handler(fn())` · `(self.0)()` | `Unresolved` | the function |
| `enum Sink { To(Box<dyn Writer>), Nowhere }` · `match s { Sink::To(w) => w.write(), … }` | the declaration | resolved |

Remember what RC1 already did: `struct Pipe<W: Writer> { sink: W }` never needed this phase. RC5 is for structs that chose `dyn`.

## 2. Which types carry erased values

A type is a **carrier** when a value of it can hold an erased value that we can name by a path of fields:

| Type | Carrier? |
|---|---|
| `&dyn T`, `Box<dyn T>`, `fn()`, and references to them | yes, since RC3: the path is empty |
| a struct, tuple struct, tuple or enum **defined in project code**, with a field whose type is a carrier, within `KeyPolicy::field_depth` steps | yes, from RC5 |
| a reference to a carrier (`&Pipe`, `&mut Pipe`) | yes: references are transparent |
| a generic struct whose field is a type parameter (`Pipe<W>`) | no: RC1 |
| `Option<…>`, `Vec<…>`, `Arc<…>`, `Rc<…>`, `Mutex<…>`, any type from outside the project | no, until RC6's wrapper rule |

"Carrier" is a property of the type alone (`sem.carrier_paths(ty)`, memoized), so it decides once, per type, which locals `sem.flow`
tracks and which parameters get a slot in `Key.erased`.

## 3. `Value::Fields`

```rust
// value.rs
Value::Fields(Box<[(FieldPath, Value<'p>)]>)       // sorted by path; a path not listed is Unknown

pub(crate) struct FieldPath(Box<[FieldStep]>);
pub(crate) enum FieldStep { Named(Name), Index(u32), Variant(Name, Box<FieldStep>) }    // `.sink` · `.0` · `Sink::To.0`
```

- The leaves are `Behind` sets or `Unknown`. A `Fields` value never claims to know the struct's *other* state: it speaks only about
  the carrier paths.
- `join` of two `Fields` is per path. For an enum, each variant's paths join separately; a value built as `Sink::Nowhere` has no
  `To.0` path, and joining it with one that has it keeps that path's set (a variant that holds nothing adds nothing to what can be
  behind `To.0`).
- A path deeper than `field_depth` is `Unknown`, counted `value_widened_depth`. **[measure]** 2 against 3, as pylspt's plan asks for
  its `fp_depth`: edges found, contexts made, time.
- Still no identity: two `Pipe`s with the same sink are the same value, so they share a context of `run`. That is the sharing we
  want, and (unlike Python) nothing is lost by it, because a value cannot be changed behind our back (§4, writes).

## 4. Building, reading, writing

`sem.flow` gains three `Expr` forms and one event:

| Code | Event / `Expr` | Value |
|---|---|---|
| `Pipe { sink: e }`, `Handler(e)`, `(a, b)`, `Sink::To(e)` | `Expr::Record(fields)` | `Fields` from each carrier field's `eval`. Layer 1 already has these as `Construct` sites; the lowering reuses the site |
| `Pipe { sink: e, ..base }` | `Expr::Record` with a base | the base's paths, overwritten by the written fields |
| `p.sink`, `self.pipe.sink`, `t.0` | `Expr::Field(base, step)` | the path's value in `eval(base)`; `Unknown` if the base is |
| `let Pipe { sink, .. } = p;` · `Sink::To(w) => …` · `let (a, b) = pair;` | `Bind` with an `Expr::Field` | the same as a read. (RC3 §6's third row is now answered for project types) |
| `p.sink = e;` where `p` is a local we track, or `self.sink = e` inside a `&mut self` method | `AssignField { local, path, value }` | a **strong** update of that path in the local's value, on this path of the body |

About writes:

- Inside the method that does it, `self.sink = Box::new(Csv)` updates the env's value of `self`, and later sites of the same body see
  `Csv`.
- In the **caller**, the object was lent as `&mut`: RC3's rule already made it `Unknown` after the call. Sound, and deliberately
  blunt: RC6 replaces it with what the callee reports.
- A write through a shared reference is not possible without interior mutability, and those types are not carriers (§2).

## 5. How a struct's value travels

No new road is built in this phase. A `Fields` value moves exactly as a `Behind` value did in RC4:

| Road | Example |
|---|---|
| into a callee, through `Key.erased` | `pipe.run()`: the receiver slot of `run` holds `Fields { sink: [Json] }` |
| out of a callee, through its return value | `Pipe::new(b)` returns the `Fields` built from its parameter |
| through locals, branches and loops | RC3's env |

That is the test of the earlier phases: if RC5 needs to touch `callee_key`, `ret.rs` or the solver handle for anything but accepting
a new `Value` variant, RC4 was built too narrowly, and the fix belongs there.

## 6. What is still `Unknown`, on purpose

| Case | Why | Later |
|---|---|---|
| a field of type `Option<Box<dyn T>>`, `Vec<Box<dyn T>>`, `Arc<dyn T>` inside `Arc<State>` | external wrappers | RC6 |
| a struct built by a derive or by external code (`Pipe::default()` derived, `serde`) | no body to read | never |
| a field changed by a callee through `&mut` | the blunt rule | RC6 |
| a struct read from a static or a thread-local | not a local | RC6 |
| a field behind `RefCell` / `Mutex` | can change through `&` | stays |
| `unsafe` writes, `mem::swap`, `mem::replace` on a tracked local | they take `&mut`: the RC3 rule fires | stays |

## 7. Tests

| Test | |
|---|---|
| `Value::Fields`: `join`, depth, variants | unit tests, no engine |
| `tests/fixtures/context/`: `field-record`, `field-constructor-fn`, `field-two-pipes`, `field-nested`, `field-tuple-struct`, `field-enum`, `field-destructure`, `field-update-syntax`, `field-write-self`, `field-write-in-callee` (stays, counted), `field-option` (stays), `field-derived` (stays) | snapshots |
| the tour, line g | final |
| a real project: `open_dyn` before and after | the number this phase exists for. If it barely moves, the project's `dyn`s live in wrappers, and that decides whether RC6's wrapper rule is worth building |
| the design's §11 list | |

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `sem.carrier_paths`; `Value::Fields`, `FieldPath`, `join` | unit tests |
| 2 | `Expr::Record`, `Expr::Field`; locals of carrier struct types tracked | `field-record`, `field-nested`, `field-tuple-struct` |
| 3 | carrier struct parameters in `Key.erased`; return values of carrier type | `field-constructor-fn`, `field-two-pipes` |
| 4 | patterns; enums; update syntax | `field-destructure`, `field-enum`, `field-update-syntax` |
| 5 | `AssignField` | `field-write-self` |
| 6 | `field_depth` measured and set; `policy.md`, `ENGINE.md` | the numbers |

Next: [`08-rc6-recursion-and-limits.md`](08-rc6-recursion-and-limits.md)
