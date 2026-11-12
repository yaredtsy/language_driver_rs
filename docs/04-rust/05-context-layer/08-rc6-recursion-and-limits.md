# 04.05.08 RC6: recursion, and the rest

```
RC6
├── 1. four parts, each built only if a number asks for it
├── A. what a callee did to a `&mut` argument
├── B. recursion: return values around a cycle
├── C. statics and consts
├── D. wrappers: Box / Rc / Arc, Option / Result, collections
├── 2. the known limits  (each one a fixture)
├── 3. when layer 2 is finished
└── 4. steps
```

## 1. Four parts

RC1–RC5 are a line: each needs the one before. RC6 is not. Its parts are independent, ordered here from cheapest and most principled
to most expensive and least, and **each starts from a counter that RC3–RC5 already produce**:

| Part | Replaces | Build it when this is large on real code |
|---|---|---|
| A | the blunt rule "lent as `&mut` → `Unknown`" | `value_lost_mut_borrow` |
| B | "a recursive call returns `Unknown`" | `return_unknown_recursion` |
| C | "a static is `Unknown`" | `open_dyn` sites whose receiver is a static (a note counter added in RC3) |
| D | "a wrapper is `Unknown`" | `open_dyn` sites whose receiver's value was lost in a wrapper (the same) |

A part that no number asks for is not built. That is a result, written into `ENGINE.md` with the number.

## A. What a callee did to a `&mut` argument

```rust
fn retarget(p: &mut Pipe) { p.sink = Box::new(Csv); }
let mut p = Pipe { sink: Box::new(Json) };
retarget(&mut p);
p.run();                          // RC5: Unknown after the call, so the declaration.  Part A: Csv's.
```

A context already computes its return value. It now also reports, for each parameter that is a `&mut` to a carrier, the parameter's
value **at exit** (the join over every exit of the body): its *out value*.

```rust
// ret.rs — what a context gives back (core's `L::Ret`)
pub(crate) struct Ret<'p> { pub value: Value<'p>, pub outs: Box<[Value<'p>]> }
```

In the caller, where the argument is exactly `&mut local` (or `&mut local.path`), the `Lose` event becomes "the local now holds the
join of the callees' out values". Any other shape of `&mut` use keeps `Lose`. Rule 6 holds: an out value is complete or `Unknown`
like any other, and an external or cut callee gives `Unknown`.

This is pylspt's "effects", reduced to what Rust needs: no paths into arbitrary objects, no weak writes, no alias vector, because the
only door is the `&mut` parameter itself ([design §4](01-design.md)).

## B. Recursion

```rust
fn build(n: u32) -> Box<dyn Writer> { if n == 0 { Box::new(Json) } else { build(n - 1) } }
build(3).write();                 // RC4: the recursive call is Unknown → the join is Unknown → the declaration
```

With RC4's solver handle, asking for a context that is still being walked answers `None`. Part B makes the solver finish the job:

```
a context that got `None` from the handle is provisional, together with the cycle it is on
round 0:  assume every in-progress context returns the EMPTY value (nothing can be behind it yet)
round n:  walk the cycle's contexts again, answering the handle with round n-1's values
stop:     when no value changed → final.   After `KeyPolicy::max_rounds` → the values are Unknown, the contexts are marked (Cut::MaxRounds)
```

- It ends: a `Behind` set only grows from round to round and is capped by `max_behind`, after which it is `Unknown`, which is the top
  and cannot change again.
- The empty set is the right start, not `Unknown`: "nothing reaches here" is a claim that the next round can only enlarge.
  `Behind([])` is already a legal value; this part is why.
- Whole bodies are walked again, not pylspt's replay of deferred operations only (`defer.rs`). Our bodies are cheap (the types are
  salsa's) and cycles are short; **[measure]** before anything smarter.
- `Cut::MaxRounds` exists in core's `Cut` already, with exactly this meaning.
- The sites of a provisional context can change between rounds (a target depends on a value). Only the final round's edges are kept.

Where it lives follows RC4's note: in core's solver if the handle is core's (then the toy language tests it, and TypeScript's C6 gets
it for nothing), else in `rust-analysis`.

Not this part: recursion in **types** (`fn f<T>(x: T) { f(Some(x)) }`) is RC1's `max_type_depth`.

## C. Statics and consts

```rust
static PLUGINS: &[&dyn Plugin] = &[&Alpha, &Beta];
const DEFAULT: &dyn Writer = &Json;
```

A const or a static is a def with a body since RS1, and an entry since RS4. So its value is simply **the return value of its own
context** (the open key; it has no parameters), asked through the same handle as any callee's. Reading one in a walked body is an
`Expr::Static(def)`.

Only immutable ones. `static mut`, `Mutex<…>`, `OnceLock<…>`, `LazyLock<…>`, `thread_local!` are `Unknown`: their value is whatever ran
before, which no summary of this function can know (rule 1).

## D. Wrappers

The part where the analyzer first holds knowledge about a **library**. That is a line worth crossing slowly: every row below is a
claim about `std` that the engine does not check for us. So: one file (`std_model.rs`), one table, one fixture per row, and anything
not in the table is `Unknown`.

| Tier | Types | Rule | Why it is sound |
|---|---|---|---|
| D1 | `Box<T>`, `Rc<T>`, `Arc<T>`, `Pin<P>` around a carrier | transparent, like `&`: `Arc::new(v)` is `v`; a deref is the same value | they are pointers. `Rc` / `Arc` contents cannot change without `get_mut` / `make_mut`, which take `&mut` and trigger the RC3 rule |
| D2 | `Option<T>`, `Result<T, E>` | enums, as RC5's: paths `Some.0`, `Ok.0`, `Err.0`. A short list of methods that keep the shape: `unwrap`, `expect`, `as_ref`, `as_mut`, `as_deref`, `unwrap_or` (a join), `?`, `clone` on the pointer kinds of D1 | their definitions are public and frozen; the listed methods are identity on the payload |
| D3 | `Vec<T>`, `VecDeque<T>`, `HashMap<K, T>`, `BTreeMap<K, T>`, slices and arrays of carriers | **one abstract element** per collection value: everything ever put in is joined into it (`vec![…]`, array literals, `push`, `insert`, `extend` from another tracked collection); everything taken out is it (`[i]`, `get`, `first`, `pop`, `iter()` and a `for` over it) | may-contain is an over-approximation, like a branch join. It is complete only while every way in is in the table: any other `&mut` use of the collection triggers the RC3 rule, as it must |

D3 is the expensive tier, and the honest advice is to compare it with the cheap alternative before building it. The typical
`Vec<Box<dyn Plugin>>` is a registry that holds **every** implementation; for that shape, "possible implementations"
([design §9](01-design.md), Q-RC2) gives nearly the same tree for a fraction of the work, and says "possible" where D3 would say
"is" on the strength of a hand-written table.

## 2. The known limits

pylspt's plan has a chapter of these, each with a fixture that pins today's answer including the missing edges, so a change is
noticed. The same here; `tests/fixtures/context/limits/`.

| Limit | Stays because |
|---|---|
| interior mutability: `RefCell<Box<dyn T>>`, `Mutex<…>`, `Cell<fn()>` | a value can change through `&`; rule 1 cannot hold |
| `unsafe`, raw pointers, `transmute`, FFI callbacks | outside what the borrow checker guarantees |
| `dyn Any` and `downcast` | a value-dependent type test |
| trait objects made by external code (a library returns `Box<dyn Error>`) | its body is never entered (rule 5) |
| values sent through channels, stored in thread-locals, captured by spawned tasks | no path in one body's events |
| `async`: what a future's `poll` runs | not a site (RS4) |
| closures' calls are listed where the closure is written | Q-RC1, answer A |
| code a proc macro or a build script generates | not loaded by default (RS3); not in the text |
| const generics, specialization, `impl` selection by a const | not in the key |
| the entry's own parameters | nobody told us; the open key |
| a type deeper than `max_type_depth`, a set larger than `max_behind`, a field deeper than `field_depth` | precision limits, counted |

## 3. When layer 2 is finished

- every scenario that has a Rust program runs at `Level::Precise`, or its README says which limit of §2 keeps it at `Default`;
- every row of `languages/rust/docs/policy.md` has a fixture for both columns;
- every `open_*` site on the three measured projects is explained by a row of §2 or by a counter, and the table of that explanation
  is in `ENGINE.md`;
- `Precision::Default` still gives layer 1, byte for byte.

## 4. Steps

Each part is its own small phase with its own commit series and its own go / no-go from §1's counter.

| # | Do | Proof |
|---|---|---|
| A1 | `Ret.outs`; the caller replaces `Lose` for `&mut local` | `field-write-in-callee` flips; `value_lost_mut_borrow` falls |
| B1 | the fixpoint, where RC4's note put the handle | toy-language tests (a cycle that needs two rounds; one that hits `max_rounds`); `dyn-return-recursive` flips |
| B2 | mutual recursion; recursion through a `&mut` out value | fixtures |
| C1 | `Expr::Static`; immutable statics and consts | fixtures: a registry slice, a const writer; `static mut` stays |
| D1 | pointer wrappers | fixtures; `std_model.rs` exists with its first rows |
| D2 | `Option` / `Result` | a fixture per listed method, and one for a method **not** listed (stays `Unknown`) |
| D3 | collections, only after comparing with Q-RC2 on real numbers | fixtures; the comparison in `ENGINE.md` |
| end | §2's fixtures; §3's checklist | reviewed |

Back to: [`../README.md`](../README.md)
