# 04.05.01 The context layer: the design

```
the context layer
├── 1. what "context" means in Rust: types first, values second
├── 2. one program, through every phase
├── 3. the key
├── 4. values: what is behind an erased type
├── 5. the rules
├── 6. the precision policy  (the table every fixture follows)
├── 7. limits: what widens, and what cuts
├── 8. closures: one decision to make  (Q-RC1)
├── 9. an option, not a phase: possible implementations  (Q-RC2)
├── 10. which file changes in which phase
└── 11. how it is tested
```

**Not scheduled** (D3; the roadmap's "measure first"). This is the design layer 1 is built to receive. Each phase has a file of its own;
this one holds what they share.

## 1. What "context" means in Rust

Layer 1 leaves a site **open** for one of eight reasons ([`../02-architecture.md`](../02-architecture.md) §6). They fall in two groups,
and the groups are the whole shape of this layer.

| Open because | Example | What decides the target | Who knows it |
|---|---|---|---|
| `TypeParam` | `fn emit<W: Writer>(w: &W) { w.write() }` | what `W` is for **this caller** | the type checker, at the call site |
| `ImplTrait` | `fn emit(w: &impl Writer)` | the same (an unnamed type parameter) | the type checker |
| `SelfInTrait` | `trait Job { fn run(&self) { self.step() } }` | what `Self` is for this receiver | the type checker |
| `FnParam` | `fn apply<F: Fn()>(f: F) { f() }` | what `F` is: a function item or a closure has a **type of its own** | the type checker |
| `Opaque` | `fn make() -> impl Writer` … `make().write()` | the type the body really returns | the type checker, in the callee |
| `Dyn` | `fn emit(w: &dyn Writer) { w.write() }` | which concrete type was turned into the `dyn` | **nobody**: Rust erased it on purpose |
| `FnPointer` | `fn apply(f: fn()) { f() }` | which function was turned into the pointer | nobody |
| `Closure` | `let f = \|\| work(); f()` | which closure the local holds | the type checker (a closure's type names it) |

> **🦀 Rust note: monomorphization, and why it is a gift here.** When you write `fn emit<W: Writer>(w: &W)`, the compiler does not
> produce one function. For every type `emit` is called with, it produces a copy with `W` replaced: `emit::<JsonWriter>`,
> `emit::<CsvWriter>`. That is *monomorphization* ("making one-shaped"), and inside each copy `w.write()` is an ordinary direct call to
> one known function. A "context" in the sense of rule 2 (a function plus a key) is exactly one of these copies, with the type arguments
> as the key. Python's analyzer has to *discover* what flows into a parameter by executing the code abstractly. For the first five rows
> above we only have to ask the type checker what it already worked out, and the answer is exact, not an approximation.
>
> `dyn Writer` is the opposite choice, made by the programmer: one copy of the function, and the type is forgotten at the moment a
> `&JsonWriter` is turned into a `&dyn Writer` (an *unsizing coercion*). That is why the last rows need what Python needs: following
> values from where they are made to where they are used.

So:

- **RC1, RC2 track types.** The key holds type arguments. No environment of values, no joins, no fixpoint. Exact.
- **RC3–RC6 track values**, and only values of *erased* types (`dyn Trait`, `fn` pointers, and things that hold them). Everything else in
  the program stays the type checker's business. This is a much smaller interpreter than Python's: it never models an integer, a string
  or an object's identity.

## 2. One program, through every phase

```rust
trait Writer { fn write(&self); fn flush(&self) { self.write() } }          // a default method
struct Json; impl Writer for Json { fn write(&self) { encode() } }
struct Csv;  impl Writer for Csv  { fn write(&self) { join() } }

fn emit<W: Writer>(w: &W) { w.write() }                                      // generic
fn each<F: Fn()>(f: F) { f() }                                               // a callable parameter
fn send(w: &dyn Writer) { w.write() }                                        // a trait object
fn pick() -> Box<dyn Writer> { Box::new(Csv) }                               // a factory
struct Pipe { sink: Box<dyn Writer> }
impl Pipe { fn run(&self) { self.sink.write() } }

fn main() {
    emit(&Json);                          // a
    Json.flush();                         // b
    each(encode);                         // c
    let w: &dyn Writer = &Json; w.write();// d
    send(&Csv);                           // e
    pick().write();                       // f
    Pipe { sink: Box::new(Json) }.run();  // g
}
```

| Line | Layer 1 shows under it | Becomes exact in | Then shows |
|---|---|---|---|
| a | `emit` → `Writer::write` (the declaration, a leaf) | **RC1** | `emit` → `<Json as Writer>::write` → `encode` |
| b | `Writer::flush` → `Writer::write` | **RC1** (`Self` = `Json`) | `Writer::flush` → `<Json as Writer>::write` → `encode` |
| c | `each` (nothing under it), `encode` `[Passed]` | **RC2** | `each` → `encode`; the passed edge stays |
| d | `Writer::write` | **RC3** | `<Json as Writer>::write` → `encode` |
| e | `send` → `Writer::write` | **RC4** (the parameter, in the key) | `send` → `<Csv as Writer>::write` → `join` |
| f | `pick`, `Writer::write` | **RC4** (the return value) | `pick`, `<Csv as Writer>::write` → `join` |
| g | `Pipe` (a construction), `Pipe::run` → `Writer::write` | **RC5** | `Pipe::run` → `<Json as Writer>::write` → `encode` |

This program is `tests/fixtures/context/tour.rs` from RS4 on, with the layer-1 column as its snapshot.

## 3. The key

```rust
// key.rs — layer 1 has `pub(crate) type Key = ();` on this line
pub(crate) struct Key<'p> {
    /// One slot per generic type parameter of the function: the enclosing impl's or trait's first (`Self` first for a trait method),
    /// then the function's own, in the engine's order. `None`: not known here, or does not matter (§7).               RC1
    pub types: Box<[Option<Ty<'p>>]>,
    /// One slot per parameter (the receiver first) whose declared type holds an erased type. What is behind it.      RC4
    pub erased: Box<[Value<'p>]>,
}
```

- **The open key** is every `types` slot `None` and every `erased` slot `Unknown`. A function walked under the open key gives exactly
  layer 1's summary. Its digest is **zeros**. So a function with no type parameters and no erased parameters has, in every phase, the
  one context it had in layer 1, with the same id; and the entry of a request (asked for by name, with nothing known about its inputs)
  is always the open key.
- **Keys are closed.** A `Ty` in a key mentions no type parameter of any function. If a caller's own slot is `None`, what it passes on
  is `None` too. That is what makes a key meaningful away from the caller that built it (rule 1 of §5).
- **The digest** is a hash of a canonical text of the key: for a type, its path from the crate root with its arguments; for a closure,
  its enclosing def and its number in source order. Never an engine id, a memory address or a hash-map order (rule 9). S-RC1 decides how
  the text is produced.
- Const generics and lifetimes are not in the key. Lifetimes never change which function is called; const generics could in theory
  (an impl for `[T; 3]` only), and are a listed limit.

## 4. Values

```rust
// value.rs — RC3
pub(crate) enum Value<'p> {
    /// Nothing is known: the engine's answer stands.
    Unknown,
    /// Exactly these, and nothing else, can be behind the erased type here. Sorted; at most `KeyPolicy::max_behind`.
    Behind(Box<[Behind<'p>]>),
    /// (RC5) A struct, tuple or enum value that holds erased values in its fields, by field.
    Fields(Box<[(FieldPath, Value<'p>)]>),
}

pub(crate) enum Behind<'p> {
    Type(Ty<'p>),            // the concrete type turned into `dyn Trait`
    Fn(Func<'p>, Key<'p>),   // a function item turned into a `fn` pointer (or held as a callable)
    Closure(ClosureRef<'p>), // a closure
}
```

A value is a fact about **which type is behind a pointer**, not about an object. Two things follow, and both are why this is far smaller
than Python's `value.rs`:

1. **No identity.** `Behind(Json)` is the same value wherever it appears. There are no origins, no alias vectors, no "the same object
   reached by two paths".
2. **Mutation of the object does not matter.** `w.write()` may change the writer's bytes; it cannot change that it is a `Json`. Only
   *rebinding* (`w = Box::new(Csv)`) changes a value, and Rust makes rebinding visible: it needs the variable itself or a `&mut` to it.

> **🦀 Rust note: the borrow checker as our alias analysis.** In Python, `a.h = JsonHandler()` can change what `b.h` is if `a` and `b`
> are the same object, and the analyzer cannot always know. pylspt's plan has a "known limits" chapter about exactly that. In safe Rust,
> a value can be replaced only through its owner or through a `&mut` reference, and while a `&mut` exists nothing else may touch the
> value. So the rule "when a `&mut` to a tracked variable is handed to code we do not follow, the variable becomes `Unknown`" is
> **sound**: there is no other door. We lean on a guarantee the compiler already enforces instead of building an approximation of it.
> (`unsafe` code and interior mutability, `RefCell<Box<dyn T>>`, are doors of their own; §6 has their rows.)

Where a value is **born**: at the coercion the compiler inserted, which the engine reports (`expr_adjustments`: `Unsize`,
`ReifyFnPointer`, `ClosureFnPointer`; [`../01`](../01-what-rust-analyzer-gives.md) §7). `source` is `&Json`, `target` is `&dyn Writer`:
the value is `Behind([Type(Json)])`. We never infer a birth from the look of the code.

## 5. The rules

The first three are pylspt's (`plan/03-call-tree/03-design.md`, "The rules"), restated for this crate. The last three are Rust's.

1. **A summary is a function of its key.** The walk of `(f, key)` starts from the key and the code, and from nothing else: not from the
   caller, not from the order contexts were found in.
2. **Nothing request-local leaves a summary.** A summary that is kept between requests (RS5's owned memo) holds stable ids and digests,
   never a `Func`, a `Ty` or a site index of another body.
3. **Precision is decided by the key and the code; cost limits only cut.** Every widening is a `KeyPolicy` rule and happens the same way
   on every run. A cut (too many contexts, a deadline) marks the context `Incomplete` and is never cached.
4. **A closed site is never overridden.** Where the engine gave a final answer (`open: None`), that answer is the edge, in every phase.
   The context layer only ever speaks at open sites. (For a closed site it still computes the **callee's key**: `emit(&Json)` always
   calls `emit`, and the key says with which `W`.)
5. **Unknown means the engine's answer, never fewer edges.** When a type slot is `None` or a value is `Unknown`, the site gets exactly
   its layer-1 target. So every phase can only replace an open answer by a better one; it cannot lose an edge layer 1 had.
6. **A set is complete or it is `Unknown`.** `Behind([Json])` claims *only* `Json` can be there. If one path into a join is not tracked,
   the result is `Unknown`, not the tracked part (rule 8 of the repo: never guess).

Rule 5 has a test of its own (§11), and it is the reason this layer can be released one phase at a time.

## 6. The precision policy

"Union at branches, precision at bindings", as Python. Each row is a fixture in `tests/fixtures/context/`.

| Rust feature | Policy | From |
|---|---|---|
| type arguments | exact, from the engine. Per caller | RC1 |
| a trait's default method | walked per `Self` | RC1 |
| associated functions and consts through a parameter (`W::new()`) | the impl's, per caller | RC1 |
| a function item or a closure passed where `F: Fn*` is expected | `F` is in the key like any type | RC2 |
| `if`, `match`, `if let`, `let else`, `?` | conditions are not evaluated. Every arm starts from the state before it; the arms are joined after | RC3 |
| a `let` or an assignment to a local | replaces what the local held (a later binding wins, on one path) | RC3 |
| loops | the body is walked until the state at its head stops changing (sets only grow and are capped), then joined with "did not run". Stricter than pylspt's single pass, because of rule 6 | RC3 |
| shadowing | a new local; the engine tells them apart | RC3 |
| `&x`, `&mut x`, `*x`, parentheses, `as` between pointer kinds, a block's tail, `Box::new(x)` **before** the coercion | transparent: the value of `x` | RC3 |
| a `&mut` to a tracked local passed to a call, or written through | the local is `Unknown` from there on, on that path. Counted `value_lost_mut_borrow` | RC3; refined in RC6 |
| a tracked value moved into a closure | the closure's body sees the value it had where the closure is written | RC3 |
| the result of a call | `Unknown` before RC4; the callee's return value after | RC4 |
| an erased parameter | starts from the key's slot | RC4 |
| `-> impl Trait` | the concrete type the body returns, from the engine, under the callee's key | RC4 |
| a struct, tuple or enum built in project code, holding erased values | `Fields`, to `KeyPolicy::field_depth` steps | RC5 |
| a field read (`self.sink`, `p.0`) | the field's value if the base is `Fields`; else `Unknown` | RC5 |
| a field written through `&mut self` in a callee | the caller's value becomes `Unknown` (the `&mut` rule) | RC5; refined in RC6 |
| `Option`, `Vec`, `Arc`, `Rc`, `HashMap` … holding erased values | `Unknown` | RC6 (the wrapper rule) |
| `RefCell`, `Mutex`, `Cell`, `OnceCell` holding erased values | `Unknown`: they can change through `&` | stays, unless RC6's numbers ask |
| statics and consts of erased type | `Unknown` | RC6 |
| recursion | a recursive call's return value is `Unknown` | RC6 (the fixpoint) |
| `unsafe` code, raw pointers, `transmute`, `Any::downcast` | `Unknown` | never |
| const generics, specialization | not in the key | never, unless measured |

## 7. Limits

| | Kind (rule 10) | Effect | Counted |
|---|---|---|---|
| `KeyPolicy::max_type_depth` (a type nested deeper than this in a slot) | precision | the slot becomes `None`. Stops `fn f<T>(x: T) { f(Some(x)) }` from making keys for ever | `key_type_widened` |
| relevant type parameters only (RC1 §5) | precision | a slot that cannot change any target is `None`, so callers that differ only there share one context | no (it changes no edge) |
| `KeyPolicy::max_behind` | precision | a larger set becomes `Unknown` | `value_widened_set` |
| `KeyPolicy::field_depth` | precision | deeper fields are `Unknown` | `value_widened_depth` |
| `Limits::max_contexts`, `max_depth`, the deadline, cancel | cost | cut, marked, not cached | core's `Stats.cuts` |
| contexts **per function** | cost | a function that reaches the cap gets no further keys: later ones are cut, marked `Incomplete` | needs a `Cut` value in core, or reuses `MaxContexts` (decided in RC1) |

A precision limit is a function of the key and the code, so two runs agree and results can be cached. Never make a per-function cap
*widen* (fall back to the open key) instead of cut: which callers arrive first depends on the walk's order, and rule 3 forbids it.

## 8. Closures: one decision to make

A closure is not a def (RS1), and in layer 1 its calls belong to the def it is written in (M13). When RC2 learns that `F` is *that*
closure, what does `f()` inside `each` show? **Q-RC1.**

| | A: the closure stays with its writer (recommended) | B: the closure moves to its caller (Python's rule) |
|---|---|---|
| `each(\|\| encode())` in `main` | `main` → `encode` (as layer 1); `main` → `each`, and `f()` in `each` is known to be that closure and draws nothing new | `main` → `each` → `encode`; `main` no longer lists `encode` |
| every call appears | exactly once, always | once **if** some walked body calls the closure. A closure handed to code we do not walk (`iter.map(\|x\| f(x))`, `thread::spawn`) would vanish, so those must stay with the writer |
| what decides where a call is drawn | the text | whether the closure's type reached a walked call: a fact about the whole graph, not about one summary (rule 1 strains) |
| the tree's order | calls a callback makes are listed where the callback is written, not where it runs | the order it runs in |
| cost to build | none beyond RC2 | a second kind of summary (the closure's body under the invoking context), and a pass over the graph to remove what moved |

A is the honest, cheap one: nothing is lost, nothing is counted twice, and layer 1's rule simply stays true. B is what a reader of
callback-heavy code would like best, and its hard part is the same in TypeScript (C3). I recommend A now and B as a later phase designed
once, for both languages, in `02-tree-rules/`. RS4 already tags each site with its closure, so B needs no change below `walk.rs`.

Named functions do not have this question: `each(encode)` under RC2 shows `encode` under `each` (it is called there) **and** keeps the
`[Passed]` edge under `main` (it is handed over there). D22 already says a def can have both.

## 9. An option, not a phase: possible implementations

When a `dyn` value is `Unknown`, the engine can still list every impl of the trait in the project (`Impl::all_for_trait`). Showing them
as children marked "possible" is class-hierarchy analysis ([`02-tree-rules/03`](../../02-tree-rules/03-the-seam.md) §7). It is cheap,
needs no key, and can be built at any point after RS4. It is **not** in the phase list because it changes what an edge *means* (may,
not does) and so needs a new `Basis` value and a renderer option in core, and a yes from you (**Q-RC2**). If the `open_dyn` counter is
large and RC3–RC5 resolve little of it on real code, this is the fallback that still gives the user something to click.

## 10. Which file changes in which phase

| | `sem` | `walk.rs` | `env.rs` | `key.rs` | `calls.rs` | new | core |
|---|---|---|---|---|---|---|---|
| RC1 | `Ty`; substitution of a site; impl lookup | `callee_key` returns a real key; `resolve_from_env` answers `TypeParam`, `ImplTrait`, `SelfInTrait` | holds the context's `types` | `Key`, `KeyPolicy` | `digest`; `Options::precision` | | testkit: level per scenario |
| RC2 | callable types; closure refs | answers `FnParam`, `Closure` (called where known by type) | | callables are `Ty`s | | | |
| RC3 | births (coercions); `sem.flow(f)`: the body as an owned list of events | interprets the events: bind, assign, branch, loop, site | locals → `Value`; `join` | | | `value.rs` | a site can say its `Basis` |
| RC4 | parameter slots; the hidden type of `impl Trait` | arguments evaluated into the callee's key; a call's value | parameters start from the key | `erased` | | `ret.rs` | the solver hands return values back (a note first) |
| RC5 | fields of project types | field reads; record expressions | | `Value::Fields` in slots | | | |
| RC6 | wrapper types; statics | the wrapper rule; effects | | | | | the fixpoint (from Python's solver, or Rust's own) |

`lib.rs`, `rust-syntax`, `rust-cli`, the wire, the renderer and the driver do not change in any row.

## 11. How it is tested

| Test | Proves |
|---|---|
| **the open key is layer 1**: every fixture and scenario, run with the context layer on and a `KeyPolicy` that tracks nothing, gives graphs equal to layer 1's snapshots | rules 4 and 5; the seam |
| **never fewer edges**: for every fixture, every layer-1 edge from a closed site is in the precise graph, and `Stats.unresolved` did not grow | rule 5 |
| **no-reuse mode** (pylspt's): solve with memoization off, so every call walks its callee again, and compare the edge sets | rule 1: a summary really is a function of its key |
| **determinism**: `assert_deterministic` on the precise graphs; digests equal across two processes | rule 9, and that no engine id leaked into a digest |
| the context workspace's snapshots flip, phase by phase, from the layer-1 column of §2 to the precise one | each phase's proof |
| scenarios at `Level::Precise`, **per scenario** | the shared definition of done; needs the testkit change of [`../02-architecture.md`](../02-architecture.md) §9 |
| a real project before and after: contexts, walks, time, and each `open_*` counter | whether the phase was worth it; written in `ENGINE.md` |

Next: [`02-rc0-spikes.md`](02-rc0-spikes.md)
