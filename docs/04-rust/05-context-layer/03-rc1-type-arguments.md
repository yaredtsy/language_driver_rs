# 04.05.03 RC1: type arguments in the key

```
RC1
├── 1. what the user sees
├── 2. what is added, in one picture
├── 3. the callee's key
├── 4. answering an open site
├── 5. relevant type parameters
├── 6. the entry, the limits, the digest
├── 7. between requests
├── 8. tests
└── 9. steps
```

The first phase of the layer, the largest win, and the one that needs the least machinery: **no values, no joins, no change to core's
solver.** A context becomes a function *with its type arguments*, which is what the compiler itself compiles.

## 1. What the user sees

| Code | Layer 1 | RC1 |
|---|---|---|
| `fn emit<W: Writer>(w: &W) { w.write() }`, called with `&Json` | `emit` → `Writer::write` | `emit` → `<Json as Writer>::write` → what it calls |
| the same function called from two places with two types | one `emit`, the same under both | two contexts of `emit`; each caller sees its own |
| `fn emit(w: &impl Writer)` | the same as the first row | the same as the first row |
| `struct Pipe<W: Writer> { sink: W }` · `impl<W: Writer> Pipe<W> { fn run(&self) { self.sink.write() } }` · `Pipe { sink: Json }.run()` | `Pipe::run` → `Writer::write` | `Pipe::run` → `<Json as Writer>::write`. **A generic struct needs no field tracking**: its type says what it holds |
| a trait's default method: `Json.flush()` where `flush` calls `self.write()` | `Writer::flush` → `Writer::write` | `Writer::flush` → `<Json as Writer>::write` |
| `W::new()`, `W::default()`, `<W as Parse>::parse(s)` | the trait's declaration | the impl's function |
| a chain: `outer::<Json>` → `middle::<W>` → `inner::<W>` | declarations all the way | resolved all the way: each key is built from the caller's |

## 2. What is added

```
                 Layer 1                                   RC1
key.rs           type Key = ();                            struct Key { types: Box<[Option<Ty>]>, erased: [] }  (design §3)
env.rs           struct Env;                               struct Env { types: the context's own slots }
walk.rs          callee_key()  → ()                        → the callee's slots, closed with env.types          (§3)
                 resolve_from_env() → None                 → the impl's method, for TypeParam · ImplTrait · SelfInTrait  (§4)
sem              drops the site's substitution             keeps it: sem.site_types(f, site), cached per site
                                                           + sem.close(ty, slots) · sem.impl_method(trait_fn, self_ty, args)
calls.rs         digest → zeros                            → hash of the key's canonical text; zeros for the open key
core             —                                         nothing in `solve`. The solver already memoizes by (Func, Key)
```

> **🦀 Rust note: why core's solver needs no change.** `Solver<L: Lang>` is written against `L::Key: Clone + Eq + Hash` and keeps its
> memo in a `HashMap<(L::Func, L::Key), Summary<L>>`. With `Key = ()` every function had one entry. Changing the associated type to a
> struct that implements the same three traits gives every (function, key) pair its own entry, and not one line of the solver is
> edited: it was generic over the key from the day B0 tested it with a toy `Key = u8`. This is what "program against a trait" buys.
> The price is that `Key` must implement `Eq` and `Hash` *consistently*: two keys that are equal must hash equally, which is why
> S-RC1 asks whether the engine's type does.

## 3. The callee's key

Every `Target::Call(g, key)` is built by `callee_key` (it exists, returning the unit key, since RS4):

```
callee_key(g, site, env):
    written = sem.site_types(f, site)          one type per slot of g, as the engine inferred them at this site.
                                               They may mention f's own parameters:  emit::<W>  inside  fn outer<W>()
    for each slot i of g:
        closed = sem.close(written[i], env.types)        fill f's parameters from this context. None if any is unknown
        key.types[i] = policy.admit(g, i, closed)        None if the slot is not relevant (§5) or the type is too deep (§6)
```

- `sem.site_types` is the substitution layer 1 threw away (`resolve_method_call_fallback`, `resolve_path_with_subst`,
  `GenericSubstitution::all_types`). It does not depend on the key, so it is cached with the site.
- `sem.close` is `Type::instantiate` with the context's slots, plus the normalisation S-RC1 chose. It is the **only** place a type
  crosses from one function's parameters to another's.
- The slots of `g` are the engine's: the enclosing impl's or trait's parameters first, then the function's own. For a method the
  engine resolved to an impl (`json.write()`), they are that impl's.

A construction, an external and an unresolved target have no key.

## 4. Answering an open site

```
resolve_from_env(f, site, env)   — only for open ∈ { TypeParam, ImplTrait, SelfInTrait }; anything else → None
    on    = sem.dispatch_type(f, site)         the type the trait method is looked up on: the receiver after auto-deref,
                                               or the qualifier of a path (`W` in `W::new()`)
    self_ty = sem.close(on, env.types)?        not closed → None → the engine's answer stands (rule 5)
    hit   = sem.impl_method(trait_fn, self_ty, the trait's own arguments, closed)?
    match hit:
        an impl's method in project code   → Target::Call(g, key of the impl's and the method's slots)
        the trait's own default method     → Target::Call(trait_fn, key with Self = self_ty)
        an impl outside the project        → Target::External
        a built-in derive, no source       → Target::External (named by the trait and the type)
```

`sem.impl_method` is `Semantics::resolve_trait_impl_method` (S-RC2). Three details that are easy to get wrong:

1. **The answer can still be open.** `emit::<dyn Writer>` closes `W` to a trait object; the impl lookup gives the trait's function
   back. That is correct: it is now an `open_dyn` site, RC3's business.
2. **A blanket impl has parameters of its own** (`impl<T: Display> Writer for T`). The callee's key needs them. If S-RC2 found no public
   way to get them, those slots are `None`, the blanket method is walked under a partly open key, and the counter
   `blanket_impl_open` says how often.
3. **Associated types** in the looked-up type (`W::Item`) must be normalised after closing (`Type::normalize_trait_assoc_type`
   **[check]**). If normalising fails, `None`.

The edge's `Basis` is `Type`, as in layer 1: it is still the type checker's answer, now asked the full question.

## 5. Relevant type parameters

`fn log<T: Debug>(x: &T) { record(format!("{x:?}")) }` is called with two hundred types. Under §3 as written it gets two hundred
contexts with two hundred identical summaries. Nothing is wrong in the tree; everything is slow, and RS5's memo misses.

A slot of `g` is **relevant** when its type can change what some walked body calls:

```
relevant(g) = the slots of g that appear in
    (a) the dispatch type of an open site of g's body, or the type of a callable that g calls        (RC2)
    (b) a type argument g passes into a relevant slot of a project function it calls
```

(b) refers to other functions' answers, so it is a fixpoint over the functions involved; the sets only grow and are bounded by the
number of slots, so it ends. It depends on the code alone, not on any key or caller, so it is computed once per function per request
and satisfies rule 3 (it is a precision rule, the same on every run). `policy.admit` puts `None` in a slot that is not relevant.

This is pylspt's `relevant_params` (`interp/relevant.rs`), on types. It is step 6, **after** the phase works without it: first exact,
then measured, then cheaper, with a test that the edge sets are identical with and without it.

## 6. The entry, the limits, the digest

| | Rule |
|---|---|
| the entry | walked under the open key. A generic entry therefore shows declarations at its own open sites, exactly as layer 1: nobody told us its types. Its callees get whatever closed types it passes |
| a method as the entry, in `impl Pipe<Json>` | the impl's own slots are closed by the impl's header, not by a caller; `sem` reads them from the impl's self type, so this entry is **not** open. **[check]** how the engine exposes an impl's fixed arguments |
| `max_type_depth` | a closed type nested deeper than this is `None`. Stops `fn f<T>(x: T) { f(Some(x)) }`. Counted `key_type_widened` |
| contexts per function | a **cost** limit: past it, further keys of that function are cut and marked `Incomplete` (design §7). Never "fall back to the open key" |
| the digest | zeros for the open key, else a hash of the canonical text of the slots (S-RC1). A non-generic function keeps the id it had in layer 1 |
| `Options` | gains `precision: Precision` (`Default` · `Precise(KeyPolicy)`). `Default` is layer 1, byte for byte, and stays selectable for good: it is the comparison every later measurement is made against |

## 7. Between requests

RS5 keeps an owned memo keyed by (`StableDef`, digest). A hit gives a summary whose targets are (stable def, digest) pairs. To walk
*below* a memoized context, the solver needs the callee's live `Key` (engine types), which a digest cannot give back. Rule: a memoized
summary is used when every context below it that the request needs is memoized too; otherwise that body is walked again, which
recomputes its callees' keys. Bodies are cheap; types are salsa's to remember. **[measure]** the hit rate before doing more.

## 8. Tests

| Test | |
|---|---|
| the design's §11 list | the open key is layer 1 · never fewer edges · no-reuse mode · determinism across processes |
| `tests/fixtures/context/`: `generic`, `impl-trait`, `generic-struct`, `default-method`, `assoc-fn`, `chain`, `blanket`, `generic-trait`, `dyn-as-type-arg`, `polymorphic-recursion` | snapshots flip from the layer-1 column; each names its row of the policy table |
| scenarios | `two-callers` (its Rust program uses a generic `emit`) and the generic form of `interface-dispatch` match `expected.precise.tree`; the others stay at `Default` (needs the per-scenario level) |
| relevance | with and without §5: equal edge sets, fewer contexts |
| a real project | contexts, walks, time, before and after; `open_type_param` should fall to about the number of open sites reachable only from generic entries |

## 9. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `sem::Ty`, `close`, the canonical text, as S-RC1 decided; `sem.site_types` kept and cached | unit tests in `sem`: the same type reached four ways is one `Ty` and one text |
| 2 | `Key`, `Env.types`, `callee_key`, `digest`; `Options::precision` | every existing snapshot unchanged under `Default`; under `Precise` only the context ids of generic functions differ, and **no edge reaches a different function yet** |
| 3 | `resolve_from_env` for `TypeParam` and `ImplTrait`; `sem.impl_method` | `generic`, `impl-trait`, `generic-struct`, `chain` |
| 4 | `SelfInTrait`, default methods, associated functions | `default-method`, `assoc-fn` |
| 5 | blanket impls, generic traits, `max_type_depth`, the per-function cut | their fixtures; `polymorphic-recursion` ends and is marked |
| 6 | relevance | §8's relevance test; the real-project numbers |
| 7 | the testkit's per-scenario level (core; every language green); the two scenarios | scenarios |
| 8 | `policy.md`: the "precise" column for these rows; `ENGINE.md` | reviewed |

Step 2 is deliberately a step with **no visible result**: it proves the key travels correctly before anything depends on it. If a
snapshot's edges move in step 2, the bug is in `callee_key`, and it is found while it has one possible cause.

Next: [`04-rc2-callables.md`](04-rc2-callables.md)
