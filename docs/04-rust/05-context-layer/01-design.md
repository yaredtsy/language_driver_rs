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
├── 9. not built: possible implementations  (D36)
├── 10. which file changes in which phase
├── 11. how it is tested
└── 12. the engine's MIR (an option RC0 decides), and a value nobody passed (D60)  (added 2026-11-24)
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
2. **Nothing request-local leaves a summary.** A summary that is kept between requests (none is in RS5; a memo added later) holds stable ids and digests,
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

## 9. Not built: possible implementations (D36)

When a `dyn` value is `Unknown`, the engine can still list every impl of the trait in the project (`Impl::all_for_trait`). Showing them
as children marked "possible" is class-hierarchy analysis ([`02-tree-rules/03`](../../02-tree-rules/03-the-seam.md) §7). **Decided
against (D36, 2026-11-16)**: a `dyn` is resolved the way Python resolves an attribute, by following the value that was put there
(RC3–RC5, RC6 A and D). An edge keeps one meaning, "is called"; where the value is not known, the site keeps layer 1's answer (the
trait's declaration) and is counted `open_dyn`. A large `open_dyn` on real code is a reason to extend the value tracking (the counter
that names the lost road says which part), not to list impls.

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
| the frozen `open_*` counts of every policy and context entry (`rust-analysis/tests/snapshots/trees/open_counts.txt`, from RS4 step 9b) | layer 1 never changes how open a site is by accident. A phase that closes sites **on purpose** edits that file in its own commit, and the review reads each changed line |

## 12. The engine's MIR, and a value nobody passed

Added 2026-11-24, after reading [blaze](https://github.com/indexable-inc/blaze): a Rust build tool whose first version was built on
rust-analyzer, and which runs its own whole-program analyses (which impl a `dyn` call reaches, what a function's effects are) over
function bodies. I read its design notes (`docs/v2/plan/design/`: `devirt.md`, `effect-summary.md`, `polymorph.md`,
`refined-values.md`, `frontend.md`); I ran none of it, and its numbers are its own claims. Two of its ideas looked as if they fit this
layer. The first (§12.1) is a spike in RC0 with a number that decides. The second was put to the owner as Q-RC4 and **declined** the
same day; §12.2 now holds the owner's rule instead (D60).

What it confirms, so that nobody reads it again for these: its summaries are keyed by a function **plus its type arguments**, as §3;
it shares one summary between type arguments the body cannot tell apart, as RC1 §5's relevant parameters; and a set that grew past a
cap becomes "unknown" rather than a part of the truth, as rule 6.

### 12.1 The body: our own events, or the engine's MIR (S-RC4)

RC3 §2 has `sem` lower a body to a list of `Event`s, with `Branch`, `Loop` and `Exit` for control flow. That means one lowering rule,
written and tested by us, for each construct: `if`, `match` with guards, `if let`, `let else`, `?`, `while let`, `for`, a `break` with
a label or a value. `Exit` already shows the strain: "this path does not reach what follows" is not true of `break 'outer`, which
does reach what follows the outer loop.

The engine has done that lowering. Its **MIR** (mid-level intermediate representation) is the body as a graph: *basic blocks*, each a
straight run of simple statements that ends in one jump, branch or call. rust-analyzer builds it for its const evaluator and borrow
checker; blaze runs every one of its body analyses on it.

> **🦀 Rust note: what MIR looks like.** `let w: Box<dyn Writer> = if fast { Box::new(Json) } else { Box::new(Csv) }; w.write();`
> becomes, roughly:
>
> ```
> bb0:  switch(fast) → bb1, bb2
> bb1:  _2 = Box::new(Json)  → bb3            _2: Box<Json>
> bb2:  _3 = Box::new(Csv)   → bb4            _3: Box<Csv>
> bb3:  _1 = _2 as Box<dyn Writer> (Unsize)   → bb5        ← a birth, written out as a cast
> bb4:  _1 = _3 as Box<dyn Writer> (Unsize)   → bb5
> bb5:  Writer::write(&*_1)  → bb6            ← the call; `&*_1` borrows what the box points at, not the box
> ```
>
> There is no `if` left, only blocks and arrows. An analysis over such a graph is always the same loop, whatever the source looked
> like: keep a state per block, and when a block's end state changes, visit the blocks it points to again, until nothing changes. A
> loop in the source is just an arrow that points backwards, so it needs no rule of its own. This is the standard *worklist* algorithm,
> and it is what the borrow checker itself runs.

| | Our events (RC3 §2 as written) | The engine's MIR |
|---|---|---|
| control flow | one rule per construct, ours | already a graph |
| loops | RC3 §3's "until the state at the head stops changing", per `Loop` | the worklist; nothing special |
| births | `expr_adjustments` on each expression (S-RC3's table, row by row) | explicit casts, each with its operand and target type |
| "a `&mut` to the box" against "a borrow to reach the object" (RC3 §6's **[check]**) | to be told apart from the adjustments | told apart by the borrowed place: the second has a `Deref` in it |
| fields (RC5) | `Expr::Record`, `Expr::Field`, `AssignField`, patterns: each lowered by us | a place with a `Field` step, on either side of an assignment; patterns are already taken apart |
| the context's type arguments | `close(t, env.types)`, ours | `monomorphized_mir_body` puts them in |
| the sites | the same list as layer 1's, by construction | **must be joined** to layer 1's list through each call's source span. If that join is not exact, MIR cannot be used |
| a body with a type error | lowered anyway; what resolves is followed | **no MIR at all** (`ENGINE.md`): the whole body keeps layer 1's answers. With proc macros off such bodies exist |
| the engine surface it leans on | `Semantics`, the surface layer 1 already uses | `hir::mir`, public under a `FIXME: Properly encapsulate`: more likely to move at a bump |

Rule 4 holds in both: `sem` hands `walk.rs` an owned form with no engine type, and `walk.rs` can be unit-tested on forms built by
hand. With MIR that form is a small graph instead of a tree of events:

```rust
// sem/flow.rs, if S-RC4 chooses MIR. Block 0 is the entry.
pub(crate) struct Flow<'p> { pub blocks: Box<[Block<'p>]> }
pub(crate) struct Block<'p> { pub steps: Box<[Step<'p>]>, pub next: Box<[u32]> }     // `next`: the blocks this one can jump to
pub(crate) enum Step<'p> {
    Site(u32),                                   // an index into sem.sites(f), as today
    Assign { to: PlaceRef, value: Expr<'p> },    // a local, or a field path of one (RC5)
    Lose { local: LocalId },                     // a `&mut` to the local itself left our sight
}
```

**It depends on a choice not made yet.** TypeScript's plan keeps **one value per variable**, the join of everything assigned to it,
and no state per path ([`03-typescript/12-context-layer/01`](../../03-typescript/12-context-layer/01-where-we-start.md)). If Rust
takes that rule when this folder is read against TypeScript's (RC0's step 0), the order of statements stops mattering, and the first
two rows of the table are no longer a gain: what is left for MIR is the births, the `&mut` test and the fields. So S-RC4 is answered
after step 0, not before.

My recommendation: run S-RC4 and let its first number decide. MIR is worth its risks only if nearly every body that holds an erased
local lowers on the three projects **and** the join to layer 1's sites is exact. Otherwise RC3 §2 stands as written, and this section
has still paid for itself with the answer to RC3 §6's check.

### 12.2 A value nobody passed: the default written in the code (D60; Q-RC5)

**Decided by the owner on 2026-11-24 (D60), and it replaces what this section proposed earlier the same day.**

Three `Unknown`s of this design share one cause, that no walked caller handed the value over:

| Case | Where the design gives up |
|---|---|
| the receiver of an **entry**: `Pipe::run` asked for by itself, so `self` is the open key | RC6 §2: "the entry's own parameters: nobody told us" |
| a struct inside a wrapper or a collection: `Arc<Pipe>`, `Vec<Pipe>`, `Mutex<Pipe>` | RC6 D, tier by tier |
| a struct read from a static, sent through a channel, captured by a spawned task | RC6 §2, known limits |

The first is the one that matters. V-NOC asks for a tree per def, so a `&self` method is the usual entry, and for it RC5 changes
nothing: `self.sink.write()` still shows the trait's declaration.

**The rule (D60): where nothing is known, use the default the code itself writes; never go looking at what other code passes or
stores.** It is what Python does, read in its code on 2026-11-24:

| | Python, as built | Where |
|---|---|---|
| a method asked as an entry | one new object of its class is made and `__init__` is walked on it **with no arguments**, hidden (its nodes are dropped, what it stored on `self` stays). The method then runs on that object | `interp/entry.rs`, `build_object` |
| a parameter that got no argument | its **default value**; if it has none, its **annotation**, as an object of that type; else unknown | `interp/bind.rs`, `default_value`, `annotation_fallback` |
| the callers of the function, or other code that writes the attribute | **never searched** | nothing in `interp/` does |

The owner's words: "if there was no context, use the default available on init or type. I do not want to show which other calls, like
jedi did: it checks which parameters are sent to that function and shows them. I disabled that feature. When I analyse a method in a
class, I only run init and use the default."

**What was proposed here, and why it is not built.** "A private field's set": a field without `pub` can be written only inside its
struct's module, so the join of every value that module writes into it is complete, and an entry's `self.sink` could be answered from
it. It is sound, and it is declined all the same, because of what it shows: under `Pipe::run` asked alone it lists every type **some
other code** stored in `sink`. That is jedi's search turned from parameters to fields, and the answer to "what does this method call"
would change when an unrelated function elsewhere in the module is edited. The same goes for its "one step further" (a private
function's parameter as the join of its callers' arguments). Do not propose either again without a new reason.

**What the rule means in Rust.** Two of Python's three rows carry over directly:

- *A parameter nobody passed is its declared type.* Rust parameters have no default values, so this is all there is, and it is what
  the open key already says: an erased parameter is `Unknown`, and its sites show the engine's answer, the trait's declaration (RC4 §3).
  Nothing to build.
- *Callers and other writers are never searched.* Nothing to build; a rule to keep. So the last two cases of the table above (a
  struct inside a wrapper, a struct from a static or another thread) stay `Unknown` until RC6 follows the value there.

The third does not carry over by itself, and is **Q-RC5**: Rust has no `__init__`. A type can have a `Default` impl, a `new`, several
constructors, or none. Which one is "init, run with no arguments"?

| Candidate, in the order I would try them | What `self` becomes for the entry | Note |
|---|---|---|
| 1. `impl Default for S` in project code | the return value of `<S as Default>::default`, walked under the open key | the language's own name for "the default value of this type". A derived `Default` gives each field its type's `default()`, which for a `Box<dyn T>` exists only if the project wrote one |
| 2. an associated function `new` (no `self`, returns `Self`, `S`, or a `Result` / `Option` of it) | the return value of `S::new`, walked under the open key: a parameter is its declared type, so a field **set from a parameter** stays `Unknown` (the declaration), and a field `new` **fills itself** (`sink: Box::new(Json)`) is known | `new` is the Rust API Guidelines' name for the primary constructor (C-CTOR). Choosing by name is a convention, so it is counted (`entry_self_from_new`) |
| 3. neither | the open key, as today: every field is its declared type | layer 1's answer. Never a join over the type's other constructors: with several there is no "the init", and joining them is the declined proposal again |

One more default that Rust writes **on the type**: `struct Pipe<W: Writer = Json>`. For an entry in `impl<W: Writer> Pipe<W>`, the
slot of `W` could start from `Json`, the default the author wrote, instead of `None`. Part (b) of Q-RC5.

**How it sits with the rules.**

- Rule 1: the init's value goes into the **entry's key** (the receiver's slot of `Key.erased`), as Python's built object is bound
  before the root context's key is made. The entry's summary is still a function of its key. An entry whose type has no init keeps
  the open key and its zero digest.
- The init is asked for as a context of its own, through the same handle that gives any callee's return value (RC4 §4). It is not
  drawn under the entry: Python's is hidden too.
- Rules 5 and 6: init gives a complete value or `Unknown` per field, like any return value; an `Unknown` field shows the declaration.
- It needs RC4 (return values) and RC5 (`Fields`). So it is a step at the end of RC5, not a phase.

**An example.**

```rust
struct Pipe { sink: Box<dyn Writer>, log: Box<dyn Writer> }
impl Pipe {
    fn new(sink: Box<dyn Writer>) -> Pipe { Pipe { sink, log: Box::new(Json) } }
    fn run(&self) { self.sink.write(); self.log.write() }
}
```

| Request | Shows |
|---|---|
| entry `main`, which calls `Pipe::new(Box::new(Csv)).run()` | `run` → `<Csv as Writer>::write`, `<Json as Writer>::write`: followed through the key (RC5) |
| entry `Pipe::run`, today and after RC5 | `run` → `Writer::write`, `Writer::write`: two declarations |
| entry `Pipe::run`, with the rule (candidate 2) | `run` → `Writer::write` (the declaration: `sink` came from a parameter nobody passed), `<Json as Writer>::write` (`new` filled `log` itself) |

RC0's S-RC5 measures how often an entry gains something: how many project types with an erased field have a `Default` or a `new`, and
how many of those fields the init fills itself.

Next: [`02-rc0-spikes.md`](02-rc0-spikes.md)
