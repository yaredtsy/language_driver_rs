# 04.05.02 RC0: spikes, and the decision to start

```
RC0
├── 1. the measurement: is it worth it, and in which order?
├── S-RC1  a type as a key
├── S-RC2  a trait method and concrete types → the impl's method
├── S-RC3  where erased values are born
├── S-RC4  the body: our own events, or the engine's MIR          (added 2026-11-24)
├── S-RC5  what an entry's `self` gets from its type's own init   (added 2026-11-24)
└── 2. what RC0 hands on, and the go / no-go
```

RC0 runs after RS4 (it needs RS4's counters and a loaded project). Spike code lives in `languages/rust/spikes/` and is deleted when its
answer is in `ENGINE.md`.

**Owed before RC0 starts (written down 2026-11-24).** This folder was written on 2026-11-12, before TypeScript's context layer was
designed and partly built ([`../../03-typescript/12-context-layer/`](../../03-typescript/12-context-layer/README.md), TC0–TC2). That
plan made choices this one has not been read against: one value per variable instead of a state per path, keys that are interned
numbers, return values through a worklist. Reading this folder against it, and against Python (the reference), is RC0's step 0. S-RC4
below depends on its answer.

**S-RC4 and S-RC5 were added on 2026-11-24**, after reading another tool built on rust-analyzer,
[blaze](https://github.com/indexable-inc/blaze). What was read, and what each idea would change, is in
[the design, §12](01-design.md). S-RC4 is a spike with a number that decides. S-RC5 measures the owner's rule for a value nobody
passed (D60, decided 2026-11-24); the "field set" idea it was first written for was declined the same day.

## 1. The measurement

RS4 counts every open site by its reason. Run `rust-cli calls` with every def as an entry on three real projects (this repo's `core/`
and `languages/typescript/`, and one outside project that uses `dyn` on purpose, such as a plugin or a middleware design), and fill:

| Counter | Sites | Share of all sites | Answered by |
|---|---|---|---|
| `open_type_param` + `open_impl_trait` + `open_self_in_trait` | | | RC1 |
| `open_fn_param` + `open_closure` | | | RC2 |
| `open_dyn` + `open_fn_pointer` | | | RC3–RC5 |
| `open_opaque` | | | RC4 |
| `open_other` | | | must be ~0, or RS4's classification has a hole |
| `unknown_callee` + `unknown_method` | | | not this layer: a loading problem (proc macros off, a missing `cfg`) |

The decision this table makes:

- If almost nothing is open, layer 2 is not built, and that is a good result (the "measure before deciding" of
  [`03-typescript/08`](../../03-typescript/08-later-context-sensitive.md) §5).
- If the first row dominates (what I expect for idiomatic Rust), RC1 alone is most of the value and the rest waits for a reason.
- If `open_dyn` dominates, RC3–RC5 are the answer, however they cost: listing the trait's impls as "possible" was decided against
  (D36). The table then says which road the values are lost on (a local, a call, a field, a wrapper), and so which phase comes first.

**A second table, for the last two rows** (added 2026-11-24). `open_dyn` and `open_fn_pointer` say that a value was not followed; they
do not say **where it would have to be followed from**. That decides the order of RC3–RC6, and what an entry asked alone can show. For each such site, where does the receiver (or the called value) come from?

| The receiver is | Example | Sites | Answered by | If the function is asked **as an entry** (the open key) |
|---|---|---|---|---|
| a local born in this body | `let w: &dyn Writer = &Json; w.write()` | | RC3 | answered |
| a parameter | `fn send(w: &dyn Writer) { w.write() }` | | RC4, through the caller's key | **not answered**: nobody told us |
| a field of a project struct, reached from `self` or a parameter | `self.sink.write()` | | RC5, through the caller's key | **not answered** by RC5; answered where the type's own init fills the field itself (D60, S-RC5), else the declaration |
| the result of a call | `pick().write()` | | RC4 | answered when the callee is project code |
| a static or a const | `PLUGINS[0].run()` | | RC6 C | answered |
| taken out of a wrapper or a collection | `self.plugins.iter()` … `p.run()` | | RC6 D | **not answered**: nobody told us (D60: callers and other writers are never searched) |
| anything else | | | must be small, or this table has a hole | |

The last column matters more for us than for a compiler: V-NOC asks for a tree per def, so most requests start at a function with the
open key, and a `&self` method is the usual entry.

One more number, because it bounds the cost of RC1: for each generic function, **how many distinct type-argument lists** it is called
with across the project (a histogram). Contexts in RC1 grow with this, not with the number of call sites.

## S-RC1: a type as a key

`Ty` ([`../02-architecture.md`](../02-architecture.md) §4) must be: comparable, hashable to a digest that is the same in every process,
closed, and accepted back by the engine. Find out whether the engine's `hir::Type` can be `Ty`, or whether `sem` needs a tree of its own.

| Question | Why |
|---|---|
| Are two `Type`s of one type equal and equally hashed when reached two ways: through an alias (`type W = Json`), through `Self`, from two call sites, from two crates of the workspace? | key equality |
| Does `&'a Json` equal `&'static Json`? If not, is there a public way to erase lifetimes, or must the key be built from the type's structure (`as_adt`, `type_arguments`, `as_reference` …)? | monomorphization ignores lifetimes; a key must too |
| `caller_type.instantiate(caller_key_types)`: does it give the closed type when the caller's slots are all known? What comes back when one is unknown (an error type, a panic)? What does "the same origin" mean for the arguments (the doc comment's warning)? | this one call is how a key crosses a call site |
| A canonical text for the digest: does a display with fully qualified paths exist (`HirDisplay` with a `DisplayTarget`)? Is it stable across runs for ADTs, tuples, references, `dyn`, function items, closures (`display_with_id` prints an id: is that id stable across runs?) | rule 9. If closure ids are not stable, a closure is named by its enclosing def and its number in source order, which RS4 already computes |
| Is `Type<'db>` cheap to keep in a `HashMap` key for a request? Does holding thousands of them pin memory? | **[measure]** |
| Does anything here need `attach_db` (S-RS2)? | a panic otherwise |

Outcome, written in `ENGINE.md`: "`Ty` is the engine's type, normalised by <function>" or "`Ty` is our own tree; here is the
conversion both ways, and here is what cannot be converted back". In the second case, also: which types cannot be rebuilt through the
public API (they become `None` slots and are counted).

## S-RC2: the impl's method

`Semantics::resolve_trait_impl_method(env, trait_, func, subst)` is the function RC1 stands on. On a hand-written file:

| Case | Expect | Write down |
|---|---|---|
| `Writer::write` with `Self = Json`, a direct `impl Writer for Json` | that impl's `write` | |
| a method the impl does not override (the trait's default body) | the trait's own function | this is how RC1 learns "walk the default method with `Self = Json`" |
| a blanket impl: `impl<T: Display> Writer for T`, `Self = i32` | the blanket impl's method. **And its own type arguments?** The plain function returns only the `Function`; the callee's key needs `T = i32` | whether a public function returns the impl's substitution too (the private `resolve_impl_method_or_trait_def_with_subst` does). If none does, RC1 computes it by matching the impl's self type against `Self`, or leaves those slots `None` |
| a generic trait: `impl From<A> for S`, `impl From<B> for S` | chosen by the trait's arguments in `subst` | the order `subst` expects (`Self` first?) |
| no impl exists (a wrong program) | the trait's function, or `None`? | |
| a built-in derive (`#[derive(Clone)]`, proc macros off) | a `Function` with no source (`BuiltinDeriveImplMethod`) | the bridge must answer `External`-like for it, not panic |
| an associated function with no receiver: `W::new()` with `W = Json` | through the same call, given the trait and its function | |
| what is `env` for? | "used to derive the trait environment", with a `FIXME` | pass the caller function's own type? `Self`'s type? Try both; a wrong one must not change the answer for closed types |
| cost of one call, cold and warm | | **[measure]**: RC1 makes one per open site per context |

Also: the function carries a `FIXME: better api`. Record its signature in `ENGINE.md` as a **contract test** (a test that fails to
compile when the signature changes), so a bump that moves it is noticed in `sem` and nowhere else.

## S-RC3: where erased values are born

`Semantics::expr_adjustments(&expr)` on each of these, to see `Adjust::Pointer(PointerCast::Unsize)` with its `source` and `target`:

| Place | Code |
|---|---|
| a `let` with a type | `let w: &dyn Writer = &Json;` · `let w: Box<dyn Writer> = Box::new(Json);` |
| an argument | `send(&Json)` · `store(Box::new(Json))` |
| a return | `fn pick() -> Box<dyn Writer> { Box::new(Csv) }` (tail) · `return Box::new(Csv);` |
| a field | `Pipe { sink: Box::new(Json) }` |
| an explicit cast | `&Json as &dyn Writer` |
| a method receiver | `Box::new(Json).boxed_method()` where the method is on `Box<dyn Writer>` |
| other pointers | `Arc<Json>` → `Arc<dyn Writer>`, `Rc`, `&mut` |
| trait upcasting | `&dyn Sub` → `&dyn Super` (dyn to dyn: the value must pass through unchanged) |
| not a trait object | `&[u8; 4]` → `&[u8]` is `Unsize` too: must be told apart by the target (`as_dyn_trait`) |
| function pointers | `let f: fn() = work;` (`ReifyFnPointer`) · `let f: fn() = \|\| work();` (`ClosureFnPointer`) · as an argument |
| inside a macro's arguments | `vec![Box::new(Json) as Box<dyn Writer>]` |

Write down, for each: on **which expression** the adjustment sits (the inner `Box::new(Json)` or an outer node), and whether `source`
is closed when the surrounding function is generic (`Box<W>`, to be instantiated with the context's key).

If any row has no adjustment the engine reports, that birth is invisible and values born there are `Unknown` (rule 6 of the design):
note the row in the policy table as a limit, with its fixture.

## S-RC4: the body: our own events, or the engine's MIR

RC3 §2 lowers a body to our own list of events, with a rule we write for every control construct. The engine already lowers a body to
a control-flow graph, its **MIR**, and that module is public at our pin (`ENGINE.md`, "Read at the pin for the plan": the items, their
lines, and the two limits). [Design §12.1](01-design.md) compares the two. This spike gets the numbers that choose.

| Question | Why |
|---|---|
| From a `hir::Function` (and a closure), how does `sem` get the `InferBodyId` that `mir_body` takes? Which crates must `Cargo.toml` name (`ra_ap_hir_def`, `ra_ap_hir_ty`)? | the first line of the spike; both are in the lockfile already |
| **[measure]** the share of bodies that lower, on `core/`, terminusdb and ruff (the sweeps' copies): of all project bodies, and of the bodies that hold a local of a carrier type (the only ones RC3 looks at). With proc macros off and on. A histogram of the refusals (`HasErrors`, `NotSupported`, `UnresolvedName`, …) | **the deciding number.** A body with any type error has no MIR at all, and with proc macros off such bodies exist (the sweeps: about 2% of terminusdb's sites are `unknown_*`). A refused body falls back to layer 1 whole (rule 5), so a low share means MIR answers little |
| For every layer-1 site of a body: is there exactly one `Call` terminator whose `MirSpan` leads back to the site's range? Which terminators have **no** site (the calls the lowering adds for `?`, `for`, `Deref`, operators, formatting: RS-M6 says they are not sites) and which sites have no terminator (a call inside a closure is in the closure's own MIR; M13 keeps it under the enclosing def) | RC3's rule "the sites are the same list as layer 1's" must still hold. Without an exact join, MIR cannot be used |
| Each row of S-RC3's table: is the birth an explicit `Cast(PointerCoercion(Unsize \| ReifyFnPointer \| ClosureFnPointer))`, and is its operand's type closed in the monomorphized body? | births without `expr_adjustments` |
| `let mut w: Box<dyn Writer> = …;` then `w.write()` (a `&mut self` method of the trait), `retarget(&mut w)`, `w = Box::new(Csv)`: which place does each borrow or write name? | RC3 §6's **[check]**: "a `&mut` to the box" against "a borrow to reach the object behind it". In MIR the second has a `Deref` in its place |
| `monomorphized_mir_body(def, subst, env)` with a key that has a `None` slot: what is passed for a slot we do not know (the parameter itself)? Does a closed slot close the types of casts and callees in the body? | a key is partly open by design (§3 of the design) |
| How does a local captured by a closure appear in the closure's MIR (a field of the closure's environment)? | RC3's row "a tracked value moved into a closure" |
| **[measure]** time and memory of `mir_body` for the bodies RC3 would ask, on the largest of the three projects | it is a salsa query: kept in the database for the session |

Outcome, in `ENGINE.md`: "`sem.flow` reads the syntax, as RC3 §2 says" or "`sem.flow` reads MIR, with this fallback and this join", and
RC3 §2 rewritten in the same commit if it is the second. Either way `walk.rs` sees an owned form with no engine type (rule 4).

## S-RC5: what an entry's `self` gets from its type's own init

[Design §12.2](01-design.md), D60: where nobody passed a value, the default the code writes is used, as Python runs `__init__` with
no arguments for a method asked as an entry. Rust has no `__init__`; Q-RC5 asks which function stands for it (a `Default` impl, then
`new`). This spike says how often that gives an entry anything, so that Q-RC5 is answered with a number. It needs no engine item
beyond S-RC2's and S-RC3's.

For every struct or enum defined in project code that has a field of a carrier type (a `dyn` or a `fn` pointer behind any
pointers), on the same three projects:

| Write down | Why |
|---|---|
| does it have `impl Default` in project code (written, or derived)? an associated function `new`? how many associated functions return the type at all? | Q-RC5's candidates 1 and 2, and how often "the init" is not one function |
| for each carrier field, what the init stores in it: a birth (S-RC3: a known type), a parameter of the init, the result of a call, other | only the first kind gives the entry a known value; a parameter is its declared type, so the field stays the declaration |
| for a generic type: does a type parameter with a `Writer`-like bound have a default (`struct Pipe<W: Writer = Json>`)? | Q-RC5 part (b) |
| **the two numbers:** the share of carrier fields that the init fills itself with a known type; and the share of §1's "a field of a project struct" sites those fields cover | the first says how often the rule gives something; the second, what the user sees |

A likely result is that the common shape, `fn new(sink: Box<dyn Writer>) -> Self { Self { sink } }`, leaves most fields at the
declaration. That is the rule working as the owner wants it, not a hole to fill: what callers pass is never searched (D60).

## 2. What RC0 hands on

| To | What |
|---|---|
| the decision | §1's table, in `ENGINE.md`, with the date and the projects. **Go for RC1** needs: the first row is worth it, S-RC1 has an answer for `Ty`, S-RC2's first four cases work |
| RC1 | what `Ty` is; the digest text; the substitution order; whether blanket impls give their arguments |
| RC2 | closure naming |
| RC3 | the table of births, and its holes; **which form `sem.flow` reads the body from** (S-RC4), after step 0's alignment with TypeScript's plan |
| RC5, and Q-RC5 | S-RC5's two numbers, with §1's second table: they are what the owner needs to answer Q-RC5 (which function is a Rust type's init) |
| this plan | every `[check]` of [`../01`](../01-what-rust-analyzer-gives.md) §7 answered there; any phase file that assumed otherwise fixed in the same commit |

A "no-go" is a result too: write the numbers and the reason into [`../03-decisions-and-questions.md`](../03-decisions-and-questions.md),
and layer 1 stands as the finished Rust analyzer.

Next: [`03-rc1-type-arguments.md`](03-rc1-type-arguments.md)
