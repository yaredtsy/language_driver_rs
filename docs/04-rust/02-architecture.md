# 04.02 The architecture: one design for both layers

```
architecture
├── 1. the picture
├── 2. the three crates
├── 3. rust-analysis, module by module  (what each holds in layer 1, and in layer 2)
├── 4. the handles: Func, RawSite, Ty
├── 5. a body is read in two steps: its sites, then what each site reaches
├── 6. what `sem` says about a site: the target, and whether the answer is open
├── 7. what layer 1 does because of layer 2  (the checklist)
├── 8. what the TypeScript and Python tracks taught, and how Rust takes each
└── 9. what I would change in the shared code  (suggestions; none is done without your yes)
```

The goal of this file: layer 1 is built once, and layer 2 is **filling in**, never rebuilding. The seam of
[`02-tree-rules/03`](../02-tree-rules/03-the-seam.md) is kept exactly (`sem` / `walk` / `env` / `key` / `calls`, `resolve(site, &env)`,
`Key` by name); this file adds what is specific to Rust on top of it.

## 1. The picture

```
                rust-cli ── outline · marker · calls [--wire] [--merged]         the only crate that writes files
                    │
     ┌──────────────┴───────────────┐
     ▼                              ▼
rust-syntax  (FAST)            rust-analysis  (SLOW)
  outline, marker, header        Project · call_graph · wire (re-exported from core)
  engine: ra_ap_syntax           engine: ra_ap_hir on a loaded Cargo workspace
  no salsa, no cargo             needs cargo + rust-src on the user's machine
     │                              │
     └──────────────┬───────────────┘
                    ▼
              analyzer-core      Graph · solve (the flat solver) · render · wire · outline types · edits
```

`rust-analysis` depends on `rust-syntax`: a def's name, range and marker always come from the outline, so the graph and the outline cannot
disagree about what a def is called (D20).

## 2. The three crates

| Crate | Public API (all of it) | Engine |
|---|---|---|
| `rust-syntax` | `outline`, `marker`, `plan_marker`, `header_marker`, `plan_header_marker`, `Edition`, `Skip`, and core's `Def`, `DefKind`, `Outline`, `Edit`, `MarkerStyle`, `Span` re-exported | `ra_ap_syntax` only |
| `rust-analysis` | `Project` (`open`, `files`, `manifest`, `environment_inputs`, `defs_named`), `OpenError`, `call_graph`, `Options`, `Breakdown`, `breakdown`, and core's graph, render, wire and `Cancel` / `Limits` re-exported, the same list as `typescript-analysis/src/lib.rs` | `ra_ap_hir` and the loaders |
| `rust-cli` | a binary | none directly |

Layer 2 adds **nothing** to either public API except fields on `Options` (which level to run at, the precision policy). That is the test of
the seam: if an RC phase needs a new public function, the design is wrong and the phase stops to say so.

## 3. `rust-analysis`, module by module

| Module | Job | Layer 1 | Layer 2 |
|---|---|---|---|
| `sem` | the only code that names `ra_ap_*` | load; outline ↔ engine bridge; the sites of a body; the engine's answer for a site | + types as values (`Ty`), substitution, impl lookup, coercions (RC1, RC3) |
| `walk.rs` | one body under one key → `Summary` | the loop over sites; `resolve(site, &env)`; the passed sites (D22) | the same loop; it now also runs `let` statements and assignments into `env` (RC3) |
| `env.rs` | what is known on this path | `pub(crate) struct Env;` | the type arguments of this context (RC1); locals of erased type → what is behind them (RC3) |
| `key.rs` | what a context holds besides the function | `pub(crate) type Key = ();` | `struct Key { types, erased }` and the `KeyPolicy` (RC1, RC4) |
| `calls.rs` | the public entry; `impl Lang`; `finish` (counters, `Breakdown`) | as TypeScript's | `digest` hashes the key; nothing else |
| `value.rs` | what is behind an erased type | does not exist | RC3 |
| `ret.rs` | what a call gives back to its caller | does not exist | RC4 |

`value.rs` and `ret.rs` are the only new files layer 2 creates. Every other change is inside a file that exists after RS4.

`sem` is one module, but it does not have to be one file. TypeScript's `sem.rs` is 3,800 lines, and Rust's will hold more kinds of work
(loading, the bridge, sites, resolution, later types). I suggest a folder, **`sem/`**, with `sem/mod.rs` exporting by name and private
files under it (`load.rs`, `bridge.rs`, `sites.rs`, `resolve.rs`, later `ty.rs`). Rule 4 keeps its meaning: nothing outside `sem/` imports
the engine. It needs your yes because the rule's text and `scripts/check-all.sh` say `sem.rs` (**Q-RS3**).

## 4. The handles

```rust
// sem — everything here is Copy, and valid for one request (the lifetime 'p is the borrow of the Project)

/// A def the engine knows and the outline has: a function, a method, a const or static (they have bodies), or a struct / enum (the
/// `class` of a construction). Two handles are equal exactly when they are the same definition.
pub(crate) struct Func<'p> { /* a hir id + PhantomData<&'p ()> */ }

/// One call site of a body, as the syntax gives it. No engine question has been asked yet.
pub(crate) struct RawSite {
    pub index: u32,        // its position in the body's list: stable for one text of the file
    pub span: Site,        // relative to the def's start, as core wants it
    pub kind: SiteKind,    // Call · MethodCall · Record · MacroBody · Passed
    pub closure: Option<u32>,  // written inside the n-th closure of the def (source order); None: in the def's own body
}

/// (layer 2) A type, closed: no parameter of any function is left in it. Opaque outside `sem`.
pub(crate) struct Ty<'p> { /* decided by S-RC1 */ }
```

> **🦀 Rust note: why the handles are small `Copy` ids, and what `'p` is for.** rust-analyzer keeps everything in a *database* (salsa): a
> function is a number, and "its name" or "its body" is a query you ask the database with that number. So `hir::Function` is 8 bytes, `Copy`,
> and has no lifetime at all. That is convenient and slightly dangerous: nothing stops a number from being used after the project was
> reloaded, when it may mean another function. We put the safety back with a *phantom* lifetime: `Func<'p>` carries
> `PhantomData<&'p ()>`, a field of size zero that tells the compiler "treat me as if I borrowed the `Project`". Now a `Func` cannot be
> stored past the borrow it was made under, and the compiler checks it, at no cost at run time. TypeScript needed the same trick for a
> harder reason (freed memory); here it guards against stale ids only, so no `unsafe` is involved and the workspace keeps
> `unsafe_code = "forbid"`.

`Ty` is the one handle whose inside is not decided. A key must be comparable, hashable into a digest that is the same on every run
(rule 9), and something the engine accepts back. Either the engine's own `Type` does all three, or `sem` keeps a small tree of its own
and converts. [S-RC1](05-context-layer/02-rc0-spikes.md) decides. The rule that holds either way: **`key.rs`, `env.rs`, `walk.rs` hold
`Ty` values and never look inside one.**

## 5. A body is read in two steps

```
step A   sem.sites(f)            → Arc<[RawSite]>     syntax only. Cached per Func. The same for every key.
step B   walk::resolve(f, site, &env)  → Vec<Target>  one engine question per site. Depends on the key in layer 2.
```

Layer 1 could do both in one pass. It does not, for three reasons that are all about layer 2:

1. **A generic function is walked once per key.** With the steps apart, the second and the hundredth walk of `emit::<W>` reuse step A and
   only repeat step B, and only for the sites whose answer is open (§6).
2. **Step A is where the policy "what is a site" lives; step B is where "what does it reach" lives.** A change of the first is a change of
   fixtures of one kind, a change of the second of another. TypeScript split them the same way after the fact (`sites` and
   `callee_with_facts`).
3. **`RawSite.index` is the name of a site** between the two steps, and later between a caller and a memoized answer. It never leaves the
   crate (the graph holds `Site` spans).

## 6. What `sem` says about a site

```rust
pub(crate) struct Callee<'p> {
    pub found: Found<'p>,
    /// Is this the last word, or did the engine stop because something is not known *in this body*?
    pub open: Option<Open>,
}

pub(crate) enum Found<'p> {
    Def(Func<'p>),                 // project code
    Construct(Func<'p>),           // a struct or an enum of the project is built
    External(External),
    Unknown(Why),                  // → Target::Unresolved(Reason::new(why.counter()))
}

/// Why a later layer could say more than the engine did. Each is a counter in `Stats.language` from RS4 on.
pub(crate) enum Open {
    TypeParam,      // `w.write()` with `w: &W`, `W::new()`: the trait's declaration was the answer
    SelfInTrait,    // `self.step()` inside a trait's default method
    ImplTrait,      // a parameter written `impl Writer`
    Opaque,         // the value of a `-> impl Writer` function
    Dyn,            // a trait object
    FnPointer,      // a `fn(..)` value
    FnParam,        // a parameter bounded by `Fn` / `FnMut` / `FnOnce`
    Closure,        // a closure value called where it is written or held in a local
    Other,          // a trait's item was the answer and none of the above explains it: a gap in this list, counted so it shows
}
```

`open` is the hinge between the layers:

- **Layer 1** counts it (`open_type_param`, `open_dyn`, …) and does nothing else. Those counters, on real projects, are the business case
  for each RC phase, as `function_typed_value` was for TypeScript (D16). RC0 reads them.
- **Layer 2** looks only at sites with `open: Some(_)`. A site with `open: None` reaches the same **function** under every key, so
  `resolve_from_env` returns `None` for it without thinking. That keeps layer 2 honest about rule 4 ("the engine second"): where the engine
  was sure, we never override it.

One more function is written in its final form in layer 1, next to `resolve`:

```rust
// walk.rs
/// The key the callee is walked under, for this site of this context. Layer 1: there is one key.
fn callee_key<'p>(_lang: &Rust<'p>, _callee: Func<'p>, _site: &RawSite, _env: &Env) -> Key { Key::default() }
```

Even a closed site needs it in layer 2: `emit(&Json)` always calls `emit`, and the key says *with which `W`*. So every
`Target::Call(func, key)` in `walk.rs` is built as `Target::Call(func, callee_key(..))` from the first day, never as
`Target::Call(func, ())`.

The first four values are answered by **types** (RC1, RC2 and the return half of RC4); the last four by **values** (RC2 for `FnParam`,
RC3–RC6 for the rest). That split is the plan of layer 2 in one line.

## 7. What layer 1 does because of layer 2

Each row is in a phase file; this is the list to check them against (the Rust copy of [`03-typescript/08`](../03-typescript/08-later-context-sensitive.md) §5).

| Do in layer 1 | Where | What it buys |
|---|---|---|
| return `Graph`; a tree and the wire are views | RS4 §1 | seam one |
| `resolve(f, site, &env)` with an empty `Env` | RS4 §4 | RC1 fills a struct |
| `Key` in a file of its own, used by name; every callee's key comes from `callee_key` (§6) | RS4 §4, §7 | RC1 changes one line and one function, and follows the compiler |
| sites and resolution are two steps (§5) | RS4 §3–4 | a second key costs only the open sites |
| every `Callee` says whether it is `open`, and it is counted | RS4 §4 | the numbers that decide whether and in which order RC1–RC6 are worth doing |
| call the engine's `_with_subst` / `_fallback` functions, and throw the substitution away | RS4 §4 | layer 1's answers are *by construction* layer 2's answers under the empty key; RC1 cannot change an edge it did not mean to |
| the outline bridge is by (file, range), in one place (`sem::bridge`) | RS4 §5 | a method reached through an impl lookup in RC1 gets its `DefInfo` the same way |
| a closure's sites are in the enclosing def's list, each tagged with **which closure** it is in | RS4 §3 | RC2 can say "the calls of *this* closure" without a second walk |
| fixtures name the policy row they pin; scenarios have both trees | RS4 §8 | a layer-2 change of a tree is a change made on purpose |
| the fixture workspace has the layer-2 programs from the start, with their **default** answers | RS4 §8 | RC phases flip expectations; they do not write programs under pressure |

## 8. What the other two tracks taught

| Learned in | What | Rust takes it as |
|---|---|---|
| TS, D15 | a qualname is not unique in a file; a tool lists the matches and never picks the first | the same: `#[cfg]` twins, two `impl S<u8>` / `impl S<u16>` blocks. `Project::defs_named` exists from RS4 |
| TS/PY, D18, D20 | `occurrence` is 0-based; a def is named (file, qualname, occurrence) going in and coming out; `Entry` is core's | used as it is. `call_graph(project, file, &Entry, …)` |
| TS, D16 | a call through a function-typed value is `Unresolved`, counted as `function_typed_value` | the same key for `FnPointer`, `FnParam` and `Closure`; the `open_*` counters split it |
| PY, D17 | the wire is the output that is used; merged on request; prefixes are parameters | `rust-analysis` re-exports core's `wire`, `merge`, `WireStyle`; `rust-cli calls --wire [--merged]` in RS4, not later |
| TS, D22 | a def handed over and not called gets a passed edge | `xs.iter().map(parse)`, `thread::spawn(worker)`, `.map(Self::helper)`: a path to a function in a value position ([RS4](04-default-resolver/05-rs4-calls.md) §3) |
| TS, D21 | defs only where a name is stable | nothing to add: Rust's closures are never defs; a `const F: fn() = …` is the const |
| TS | `def_without_node`: a def the two parses disagree on shows as one `Unresolved` edge, not as an empty body | the same reason and counter |
| TS | `Breakdown`: why edges are unresolved, printed by the CLI | the same struct, Rust's own reasons |
| TS, Q-TS1 | a construction cannot show a constructor outside the project | does not arise: Rust has no constructors, `init` is always `None` |
| TS | handles need a compile-fail proof (freed memory) | not needed: ordinary ownership. The phantom lifetime of §4 is checked by the compiler |
| PY | "union at branches, precision at bindings" | RC3's rule for the env, word for word |
| PY | a summary is a function of its key; nothing walk-local leaves it; cost limits only cut | the three rules of [`05-context-layer/01-design.md`](05-context-layer/01-design.md) §5 |
| PY | `relevant_params`: only parameters that can change a callee's calls go into the key | RC1's "relevant type parameters": the same idea, on types |
| PY | `Basis::Path` for an edge the walk tracked, `Basis::Type` for the engine's | RC3 needs the basis **per site**, which `Lang::BASIS` cannot say (§9) |

## 9. What I would change in the shared code

None of this is done by this plan. Each is a question in [`03-decisions-and-questions.md`](03-decisions-and-questions.md) with the phase
that first needs it. A change in `core/` keeps every language green (rule 5) and gets a note of its own first, as passed edges did
([`02-tree-rules/06`](../02-tree-rules/06-passed-edges.md)).

| # | Change | First needed | Why |
|---|---|---|---|
| 1 | `sem` may be a folder | RS3 | §3 |
| 2 | `analyzer_testkit::scenarios::run` takes the level **per scenario** | RC1 | a language becomes precise one scenario at a time; today one line flips all of them, so the line could only change at the very end, and every RC phase would have no shared proof. TypeScript's C-steps need the same |
| 3 | a site can say its `Basis` (`SiteCalls::with_basis`, set the way `passed` is) | RC3 | an edge found through a tracked value rests on a path, not on the engine's type, and `Lang::BASIS` is one value for the whole language. The note of [02/05](../02-tree-rules/05-edge-basis-note-for-ts4.md) already names a per-target basis as the next step; per site is enough here ([RC3](05-context-layer/05-rc3-local-values.md) §7) |
| 4 | the solver can hand a callee's **return value** to its caller | RC4 | the flat solver's `walk` cannot ask about a callee. [02/03](../02-tree-rules/03-the-seam.md) §6 calls this "the open part". RC4 writes the note; two shapes are compared there |
| 5 | a scenario's Rust program is a Cargo package (`scenarios/<name>/rust/Cargo.toml` + `src/main.rs`) | RS4 | rust-analyzer loads workspaces, not files. `scenarios/two-callers/rust/` exists today as an empty folder that git does not track: it is recreated properly in RS4 |
| 6 | a scenario may have a tree **per language** where names differ (`expected.default.rust.tree`, looked for before the shared file) | RS4 | the shared trees say `Counter.bump`; Rust's qualname is `Counter::bump`, and a trait method is `<Json as Writer>::write`. Python and TypeScript happen to agree on `.`; Rust cannot. The scenario's README then states that the *shape* is the same |

Next: [`03-decisions-and-questions.md`](03-decisions-and-questions.md)
