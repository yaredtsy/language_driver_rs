# 04.05.05 RC3: values inside one body

```
RC3
├── 1. what the user sees
├── 2. the body as a list of events: sem.flow
├── 3. the env, and the join
├── 4. the value of an expression
├── 5. answering Dyn and FnPointer sites
├── 6. three places a value is lost on purpose
├── 7. what core is asked for: a site's basis
├── 8. tests
└── 9. steps
```

The first phase that follows **values**. It is kept small on purpose: one body, no calls crossed, the key untouched. Everything the
later phases need (the event list, the env, the join, `Value`) is built and proven here, where a wrong answer has one body to hide in.

## 1. What the user sees

| Code (all in one function) | Before | RC3 |
|---|---|---|
| `let w: &dyn Writer = &Json; w.write();` | `Writer::write` | `<Json as Writer>::write` |
| `let w: Box<dyn Writer> = if fast { Box::new(Json) } else { Box::new(Csv) }; w.write();` | `Writer::write` | both impls' methods, two nodes at one site |
| `let mut w: Box<dyn Writer> = Box::new(Json); w.write(); w = Box::new(Csv); w.write();` | the declaration, twice | `Json`'s, then `Csv`'s: a later binding wins |
| `let f: fn() = work; f();` | `Unresolved` | `work` |
| `let f: &dyn Fn() = &\|\| work(); f();` | `Unresolved` | no edge, `closure_call_known` (RC2 §4) |
| `let w: Box<dyn Writer> = make(); w.write();` | the declaration | **the declaration**: a call's value is `Unknown` until RC4 |

Honestly: on its own this resolves little real code, because a `dyn` that never leaves a function is rare. RC3 is the foundation RC4
and RC5 stand on, and it is a phase of its own so that it can be reviewed and released without them.

## 2. The body as a list of events

Layer 1 reads a body as a flat list of sites. Values need to know what happens **between** sites. `sem` lowers the body once more, to
an owned, engine-free list of events; `walk.rs` interprets it.

```rust
// sem/flow.rs — produced by sem, read by walk. No engine type inside.
pub(crate) enum Event<'p> {
    Site(u32),                                // an index into sem.sites(f): the SAME list, the SAME order as layer 1
    Bind   { local: LocalId, value: Expr<'p> },   // `let w = …`, a parameter's pattern
    Assign { local: LocalId, value: Expr<'p> },   // `w = …`
    Lose   { local: LocalId },                // a `&mut` to it leaves our sight (§6)
    Branch(Vec<Vec<Event<'p>>>),              // if / match / if let / let-else: every arm starts from the state before
    Loop(Vec<Event<'p>>),
    Exit,                                     // return, break, continue, `?`: this path does not reach what follows
}

/// What an expression of an erased-carrying type can be.
pub(crate) enum Expr<'p> {
    Local(LocalId),
    Born(Behind<'p>),          // a coercion the engine reported here (S-RC3); its source type is closed by walk with env.types
    Call(u32),                 // the result of the call at this site (RC4; Unknown in RC3)
    Field(Box<Expr<'p>>, FieldRef),   // RC5
    Join(Vec<Expr<'p>>),       // `if c { a } else { b }` used as a value; a `match`
    Unknown,
}
```

Rules of the lowering:

- **Only locals whose type holds an erased type are tracked** (`dyn Trait`, a `fn` pointer, behind any pointers; from RC5 also a
  project struct holding one). Every other `let` produces no event. Most bodies have none, and for them `sem.flow(f)` is `None` and
  the walk is layer 1's flat loop, untouched. The cost of RC3 is paid only where a `dyn` is.
- **Transparent expressions** are looked through when building an `Expr`: `&x`, `&mut x` (in a read position), `*x`, parentheses, a
  block's tail, `as` between pointer kinds, a `dyn`-to-`dyn` upcast.
- **The sites are the same.** A test asserts, for every fixture: flattening `flow(f)` gives exactly `sites(f)`, in order. The event
  list adds structure around the sites; it never has an opinion about what a site is.
- **A closure's body** is lowered in place, as a `Branch` with one arm that may not run (we do not know whether or when it is
  called). Values it reads are the ones at its position; §6 covers what it writes.

> **🦀 Rust note: why lower to our own little enum instead of walking the engine's tree in `walk.rs`.** Rule 4 says only `sem` may
> name engine types, and this is the phase where the rule earns its keep. An interpreter written directly over
> `ra_ap_syntax` / `ra_ap_hir` nodes would spread hundreds of engine calls through `walk.rs`, and every weekly engine release could
> break any of them. With `Event` in between, the engine-shaped half (finding `let`s, asking for coercions, mapping macro expansions)
> stays in `sem/flow.rs`, and the half that is *our* logic (joins, the policy table) is plain Rust over a plain enum that you can
> build by hand in a unit test. Compilers call this an *intermediate representation*; ours has seven variants.

## 3. The env, and the join

```rust
// env.rs
pub(crate) struct Env<'p> {
    pub types: Rc<[Option<Ty<'p>>]>,           // RC1: this context's slots
    locals: Vec<(LocalId, Value<'p>)>,         // RC3: sorted by LocalId; a local not listed is Unknown
}
impl<'p> Env<'p> {
    pub fn join(arms: Vec<Env<'p>>) -> Env<'p>;    // per local: Value::join over the arms that did not Exit
}
```

`Value::join` is union, sort, dedup; `Unknown` joined with anything is `Unknown` (rule 6 of the design: complete or unknown); a set
past `max_behind` is `Unknown`, counted.

| Event | Does |
|---|---|
| `Bind`, `Assign` | the local now holds `eval(value)`. A **strong** update: the old value is gone on this path (precision at bindings) |
| `Branch` | clone the env per arm; run each; `join` the arms that reach the end. An arm that `Exit`s contributes nothing |
| `Loop` | run the body from `join(before, end of the previous round)` until the state at the loop's head stops changing. Sets only grow and are capped, so it ends; a local still changing after `KeyPolicy::loop_rounds` is `Unknown`. After the loop: `join(before, end)` |
| `Site(i)` | `resolve(f, site i, &env)`: the seam's function, now with something in the env |
| `Lose` | the local is `Unknown` from here on this path |

The loop rule is stricter than pylspt's ("walked once"). It has to be: `loop { w.write(); w = Box::new(Csv); }` under a single pass
would claim *only* `Json` at `w.write()`, and rule 6 forbids a set that is not complete.

A sorted `Vec` cloned at branches is enough to start: a body tracks a handful of locals. If a measurement says otherwise, a persistent
map (what pylspt uses, `rpds`) replaces the field and nothing outside `env.rs` changes.

## 4. The value of an expression

| `Expr` | Value |
|---|---|
| `Local(l)` | the env's value, or `Unknown` |
| `Born(Type(t))` | `Behind([Type(close(t, env.types))])`; if `t` does not close, `Unknown`. So in `fn f<W: Writer + 'static>(w: W) { let b: Box<dyn Writer> = Box::new(w); … }` under `W = Json`, the value is `Json` |
| `Born(Fn(g, ..))`, `Born(Closure(c))` | `Behind([that])` |
| `Join(es)` | `Value::join` of their values |
| `Call(_)`, `Field(..)`, `Unknown` | `Unknown` in RC3 |

## 5. Answering `Dyn` and `FnPointer` sites

```
resolve_from_env(f, site, env)   — open = Dyn
    recv = eval(the receiver's Expr, env)             sem attached it to the site when lowering
    Behind(set) → for each Type(t) in set:  RC1 §4's lookup with self_ty = t and the dyn's own trait arguments
                  → several targets at ONE site, each Target::Call / External; the solver draws one edge per target,
                    and the tree orders them by where each is defined (rule 7 of the tree)
    Unknown     → None: the engine's answer, the trait's declaration (rule 5)

— open = FnPointer
    Behind(set) → Fn(g, key) → Target::Call(g, key);   Closure(_) → no edge, closure_call_known
```

A set with several members is the first time one site has several targets in Rust. Core, the renderer and the wire already handle it
(Python does it daily; rule 7).

## 6. Three places a value is lost on purpose

| Where | Rule | Why it is sound |
|---|---|---|
| `&mut w` passed to a call, stored, or returned; `*r = …` through a reference | `Lose { w }` at that point. Counted `value_lost_mut_borrow` | only a `&mut` can rebind `w` from elsewhere (the borrow checker's guarantee, [design §4](01-design.md)). A method call that merely auto-borrows `&mut *w` to reach the trait object behind the box does **not** lose it: it can change the object, not which type it is. **[check]** telling the two apart from the engine's adjustments |
| a closure that **assigns** to a tracked local it captured | `Lose` at the closure, for the rest of the body | the closure may run at any later point, any number of times |
| a tracked local bound by a pattern that takes a value apart (`let (a, b) = pair;`, `Some(w) => …`) | `Unknown` | RC5 (fields) and RC6 (wrappers) |

Each is a fixture with its expected, deliberately incomplete tree, so the day a later phase improves one, a snapshot changes on purpose.

## 7. What core is asked for

An edge found through the env rests on a **path the analyzer followed**, not on the engine's type. `graph::Basis` already has that
value (`Path`), and Python uses it. Core's flat solver stamps one basis for the whole language (`Lang::BASIS`).

Proposal (a note in `02-tree-rules/` first, then core, every language green): `SiteCalls` gets an optional basis, set like `passed`
is: `SiteCalls::new(site, targets).with_basis(Basis::Path)`; `None` means `L::BASIS`. Per **site**, not per target, because of how the
seam is built: at one site either `resolve_from_env` answered (all targets tracked) or the engine did (all targets its own).
TypeScript's C1 will need exactly this.

## 8. Tests

| Test | |
|---|---|
| `Env` and `Value::join` | unit tests on hand-built `Event` lists, with no engine and no project: every row of §3, including the loop that must reach `{Json, Csv}` |
| flatten(`flow`) = `sites` | every function of both fixture workspaces |
| `tests/fixtures/context/`: `dyn-local`, `dyn-branch`, `dyn-rebind`, `dyn-loop`, `fnptr-local`, `dyn-in-generic`, `dyn-upcast`, `dyn-lost-mut`, `dyn-closure-assign`, `dyn-from-call` (stays the declaration), `dyn-destructure` (stays) | snapshots; each edge that RC3 found has basis `Path` |
| a body with no erased local is byte-identical in events, time and result to RC2 | the fast path of §2 is real |
| the design's §11 list | |

## 9. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `value.rs`: `Value`, `Behind`, `join`; `Env.locals` | unit tests |
| 2 | `sem.flow`: tracked locals, `Bind`, `Assign`, `Site`, `Branch`, `Exit`; births from coercions (S-RC3's table) | the flatten test; a snapshot of the event list per fixture |
| 3 | `walk::body` interprets events when `flow` is `Some`; `resolve_from_env` for `Dyn` | `dyn-local`, `dyn-branch`, `dyn-rebind`, `dyn-in-generic` |
| 4 | the note for a site's basis; core; `with_basis(Path)` | core's tests; every language green; the fixtures' bases |
| 5 | `Loop`, `Lose`, closures | `dyn-loop`, `dyn-lost-mut`, `dyn-closure-assign` |
| 6 | `FnPointer`, upcasts | `fnptr-local`, `dyn-upcast` |
| 7 | `policy.md`, `ENGINE.md`, numbers | reviewed |

Next: [`06-rc4-values-across-calls.md`](06-rc4-values-across-calls.md)
