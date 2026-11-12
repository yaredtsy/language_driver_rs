# 04.05.02 RC0: spikes, and the decision to start

```
RC0
├── 1. the measurement: is it worth it, and in which order?
├── S-RC1  a type as a key
├── S-RC2  a trait method and concrete types → the impl's method
├── S-RC3  where erased values are born
└── 2. what RC0 hands on, and the go / no-go
```

RC0 runs after RS4 (it needs RS4's counters and a loaded project). Spike code lives in `languages/rust/spikes/` and is deleted when its
answer is in `ENGINE.md`.

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
- If `open_dyn` dominates and RC3–RC5 look expensive for it, §9 of the design (possible implementations) is the cheaper answer; ask
  Q-RC2 then.

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

## 2. What RC0 hands on

| To | What |
|---|---|
| the decision | §1's table, in `ENGINE.md`, with the date and the projects. **Go for RC1** needs: the first row is worth it, S-RC1 has an answer for `Ty`, S-RC2's first four cases work |
| RC1 | what `Ty` is; the digest text; the substitution order; whether blanket impls give their arguments |
| RC2 | closure naming |
| RC3 | the table of births, and its holes |
| this plan | every `[check]` of [`../01`](../01-what-rust-analyzer-gives.md) §7 answered there; any phase file that assumed otherwise fixed in the same commit |

A "no-go" is a result too: write the numbers and the reason into [`../03-decisions-and-questions.md`](../03-decisions-and-questions.md),
and layer 1 stands as the finished Rust analyzer.

Next: [`03-rc1-type-arguments.md`](03-rc1-type-arguments.md)
