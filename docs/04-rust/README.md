# 04. Rust

```
rust
├── what rust-analyzer gives ─────── 01-what-rust-analyzer-gives.md
├── the architecture ─────────────── 02-architecture.md          one design for both layers
├── decisions and questions ──────── 03-decisions-and-questions.md
│
├── LAYER 1: THE DEFAULT RESOLVER ── 04-default-resolver/         one context per function
│   ├── RS0  spikes ──────────────── 01-rs0-spikes.md
│   ├── RS1  outline ─────────────── 02-rs1-outline.md
│   ├── RS2  markers ─────────────── 03-rs2-markers.md            → tag rust-v0.1.0 → V-NOC tier 1–2
│   ├── RS3  project loading ─────── 04-rs3-project.md
│   ├── RS4  call graph ──────────── 05-rs4-calls.md              → tag rust-v0.2.0 → V-NOC tier 3, whole tree
│   └── RS5  what the driver needs ─ 06-rs5-driver-needs.md       → tag rust-v0.3.0 → V-NOC tier 3, lazy
│
└── LAYER 2: THE CONTEXT LAYER ───── 05-context-layer/            more contexts per function, added one step at a time
    ├── the design ───────────────── 01-design.md                 what "context" means in Rust · the key · the rules
    ├── RC0  spikes ──────────────── 02-rc0-spikes.md
    ├── RC1  type arguments ──────── 03-rc1-type-arguments.md     generics resolve per caller
    ├── RC2  callables ───────────── 04-rc2-callables.md          `F: Fn()` resolves to the function that was passed
    ├── RC3  local values ────────── 05-rc3-local-values.md       `dyn Trait` and `fn` pointers inside one body
    ├── RC4  values across calls ─── 06-rc4-values-across-calls.md   erased parameters in the key · return values
    ├── RC5  fields ──────────────── 07-rc5-fields.md             a struct that holds a `Box<dyn Trait>`
    └── RC6  recursion and the rest ─ 08-rc6-recursion-and-limits.md  the fixpoint · collections · statics · known limits
```

Written 2026-11-12, after reading the TypeScript track as built (`languages/typescript`, its `ENGINE.md`, D15–D24), Python's solver
(`languages/python/crates/python-analysis/src/interp/`, pylspt `plan/03-call-tree/`), `core/` as it is today, and
`../language_clone/rust-analyzer` at commit `2a362a4`. **Nothing here was built or run.** Every `[check]` and `[measure]` is a claim to verify
(the list is in [`03-decisions-and-questions.md`](03-decisions-and-questions.md) §4).

This folder replaces the earlier two-file plan (`02-phases.md`, deleted). What that file decided still holds and is carried over; what is new
is the detail of each phase, the changes the TypeScript track taught (entries by occurrence, passed edges, the wire, the counters), and the
whole of layer 2.

## The two layers, in one paragraph each

**Layer 1** is what D3 asks for: the outline, the markers, and a call graph where every edge is rust-analyzer's own answer. A method called
through a generic bound or a trait object goes to the **trait's** declaration; a call through a function value is `Unresolved` and counted.
One context per function (`type Key = ();`), on core's flat solver.

**Layer 2** makes the answer depend on who calls. In Rust that has an unusual shape, and it is good news: most of what Python needs an
abstract interpreter for, **Rust's type checker already knows**. A generic function called with `JsonWriter` is, to the compiler, a different
function from the same one called with `CsvWriter` (monomorphization), and rust-analyzer can answer "which `write` is that?" when we give it
the type arguments. So the first two steps of layer 2 (RC1, RC2) track **types, not values**, and are exact. Only what Rust erases on purpose
(`dyn Trait`, `fn` pointers) needs values followed through the code, and that is RC3–RC6, from one body, to across calls, to fields, to
recursion: the order pylspt's own plan was built in.

## The phases at a glance

| Phase | Builds | Crate | Depends on | Proof it is done | Size |
|---|---|---|---|---|---|
| **RS0** | four throwaway experiments | `languages/rust/spikes/` | B0 | answers in `languages/rust/ENGINE.md` | S |
| **RS1** | `outline(text, edition, style)` | `rust-syntax` | RS0 | fixture snapshots; `cargo tree -p rust-syntax` shows no `salsa` | M |
| **RS2** | `marker`, `plan_marker`, the header | `rust-syntax` | RS1 | round trip, never panics, `rustfmt` leaves the marker alone | S |
| | **tag `rust-v0.1.0`** | | | qualname rule frozen (rule 10) | |
| **RS3** | `Project::open(root, file)` | `rust-analysis` | RS0 | this repo's own workspaces load; clear errors without a toolchain | M |
| **RS4** | `call_graph`, the wire, `rust-cli` | `rust-analysis`, `rust-cli` | RS3 | every scenario at `Level::Default`; a fixture per policy row; our sites ⊇ the engine's `outgoing_calls` | L |
| | **tag `rust-v0.2.0`** | | | | |
| **RS5** | `Session` | `rust-analysis` | RS4; core's budget and cancel (TS5 steps 1–2) | tier-3 conformance | M |
| | **tag `rust-v0.3.0`** | | | | |
| **RC0** | three experiments and one measurement | `languages/rust/spikes/` | RS4 | answers in `ENGINE.md`; go / no-go for RC1 | S |
| **RC1** | `Key` holds type arguments | `rust-analysis` | RC0 | `two-callers`, `interface-dispatch` (generic form) match `expected.precise.tree` | L |
| **RC2** | function items and closures as type arguments | `rust-analysis` | RC1 | `callback-argument` matches the precise tree | M |
| **RC3** | `Env` inside one body | `rust-analysis` | RC2 | the `dyn` fixtures that never leave a body | M |
| **RC4** | erased parameters in the key, return values | `rust-analysis`, a core note | RC3 | `interface-dispatch` (`dyn` form), factories | L |
| **RC5** | struct fields | `rust-analysis` | RC4 | the "pipeline holds a writer" fixtures | L |
| **RC6** | fixpoint, collections, statics | `rust-analysis`, maybe `core` | RC5 | recursion fixtures; the list of known limits, each a fixture | L |

S, M, L are relative. **[measure]** after RS1.

Layer 2 is **not scheduled** by this plan (D3 and the roadmap's "measure first" stand). It is designed now so that layer 1 is built on the
shapes it needs: [`02-architecture.md`](02-architecture.md) §7 is the list of what layer 1 does *because of* layer 2.

## What V-NOC gets, and when

```
rust-v0.1.0   .rs files appear in the tree and on the canvas · docs attach · docs survive renames
rust-v0.2.0   call trees, computed whole
rust-v0.3.0   call trees that open one level at a time, can be cancelled, are fast the second time
RC1, RC2      (minor versions) a generic function shows what it calls for THIS caller
RC3–RC6       (minor versions) trait objects and function pointers resolve
```

No tag in layer 2 changes a qualname, the outline, the wire's shape or the CLI. Context ids go from zeros to hashes at RC1, once.

## The rule for every phase

The TypeScript track's rule, unchanged ([`../03-typescript/README.md`](../03-typescript/README.md)):

1. the proof named in the phase's file runs, and the report says what actually passed;
2. `lib.rs` is still a short list of names (rule 3);
3. `languages/rust/ENGINE.md` lists every engine item `sem.rs` calls;
4. the phase's contract is copied to `languages/rust/docs/`: `qualnames.md` after RS1, `markers.md` after RS2, `policy.md` after RS4, and
   `policy.md` again (its "precise" column) after each RC phase.

Next: [`01-what-rust-analyzer-gives.md`](01-what-rust-analyzer-gives.md)
