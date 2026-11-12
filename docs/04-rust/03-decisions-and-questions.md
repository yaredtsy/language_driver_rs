# 04.03 Rust: decisions and questions

```
decisions and questions
├── 1. what is already decided, and where Rust uses it
├── 2. draft rows (RS-M1 … RS-M12): decided by me, yours to veto
├── 3. questions only you can answer (Q-RS1 … Q-RS5, Q-RC1 … Q-RC3)
└── 4. things I did not check
```

The repo's decision log is [`../07-decisions-and-questions.md`](../07-decisions-and-questions.md). Nothing there is reopened here.
When a question below is answered, the answer goes into that log as a `D` row and this file's row is struck through.

## 1. Already decided

| | Decision | Where Rust uses it |
|---|---|---|
| D3 | start on the engine's default resolver; context sensitivity later, without a rewrite | the two layers; [`02-architecture.md`](02-architecture.md) §7 |
| D6 | the marker is a `/// ID: …` doc line; the header a plain comment | RS2 |
| D7 | where the engine resolves it, follow the engine | RS4 §4; the oracle test |
| D10 | `<S as T>::m` | RS1 §3 |
| D13 | `Graph::defs`; a construction's class has a row and no context | RS4 §5 |
| D15 | a qualname is not unique in a file; `Skip` carries a reason | RS1 §4, RS2 §5 |
| D16 | a call through a function-typed value is `Unresolved`, counted | RS4 §4 (`FnPointer`, `FnParam`, `Closure`) |
| D17, D23 | the wire is the output; merged on request; format version 2 | RS4 §2, §8 |
| D18, D20 | 0-based `occurrence`; entries and targets named (file, qualname, occurrence); a class can be an entry | RS1 §4, RS4 §1 |
| D22 | a def handed over gets a passed edge | RS4 §3–4; RC2 §1 |
| M12, M13 | `Key = ()`, the module names, `resolve(site, &env)`; an anonymous function's calls belong to the enclosing def | RS4; the closure question (Q-RC1) builds on M13 |

## 2. Draft rows

Structure and engineering choices, made as drafts the way M1–M18 were. Each can be vetoed.

| Row | Decision | The other option | Where |
|---|---|---|---|
| **RS-M1** | `rust-analysis` is written on `ra_ap_hir` (`Semantics`), not on `ra_ap_ide`. `ide::outgoing_calls` is the **test oracle** | call `outgoing_calls` and regroup (it drops unresolved sites, constructions and type arguments) | [01](01-what-rust-analyzer-gives.md) §2 |
| **RS-M2** | `Project::open(root, file)`: the workspace that owns the file, as TypeScript | `open(root)` (fails on this repo: five workspaces, no root manifest) | [RS3](04-default-resolver/04-rs3-project.md) §2 |
| **RS-M3** | A body is read in two steps: its sites (syntax, cached per def), then what each reaches (per key) | one pass | [02](02-architecture.md) §5 |
| **RS-M4** | Every resolved site says whether its answer is **open**, and why; each reason is a counter from RS4 on | count only unresolved edges, as TypeScript | [02](02-architecture.md) §6 |
| **RS-M5** | Layer 1 calls the engine's substitution-returning functions and drops the substitution; every callee key comes from `callee_key` | the plain functions now, switch later | [RS4](04-default-resolver/05-rs4-calls.md) §4 |
| **RS-M6** | Operators, `?`, `Deref`, `Drop`, `for` are not call sites | sites (the tree drowns) | RS4 §3 |
| **RS-M7** | A function a macro generated is `External` (named, not entered, counted) | `Unresolved`; or enter it with no def to show | RS4 §5 |
| **RS-M8** | Build scripts and proc macros are off by default, counted, switchable | on by default (slow, may compile the project) | RS3 §3 |
| **RS-M9** | Layer 2's order is **types before values**: RC1 type arguments, RC2 callables, then RC3 local values, RC4 across calls, RC5 fields, RC6 the rest | Python's order (values first), which would build an interpreter before using what the type checker gives for nothing | [design](05-context-layer/01-design.md) §1 |
| **RS-M10** | The six rules of the context layer, in particular: a closed site is never overridden; `Unknown` means the engine's answer; a set is complete or `Unknown` | best-effort sets | design §5 |
| **RS-M11** | A per-function context cap **cuts**; it never widens to the open key | widen (depends on the walk's order: breaks rule 10) | design §7 |
| **RS-M12** | `Precision::Default` (layer 1, byte for byte) stays selectable for good | replace layer 1 | RC1 §6 |

## 3. Questions

| # | Question | My recommendation | Needed before | Why it matters |
|---|---|---|---|---|
| **Q-RS1** | **Qualname of a trait-impl method when the trait has generic arguments**: `impl From<A> for S` and `impl From<B> for S`. Keep them (`<S as From<A>>::from`) or drop them like the self type's (`<S as From>::from` twice, told apart by `occurrence`)? | **keep the trait's arguments**, as written, whitespace normalised; keep dropping the self type's | RS1 step 3. **Frozen at `rust-v0.1.0`** | with them dropped, reordering two impls in a file swaps the docs attached to their methods. With them kept, renaming a type used as the argument renames the def (as renaming the trait would) |
| **Q-RS2** | **A call in the body of a project `macro_rules!`** (`log!()` expanding to `crate::emit()`): is it a call site of the function that uses the macro, drawn at the macro call? | **yes** for project macros; **no** for external macros (`println!`'s inside is external code) | RS4 step 7 | without it, a project that wraps its calls in macros has trees with holes. With it, one site can have several targets that the user never typed there |
| **Q-RS3** | **May `sem` be a folder** (`sem/mod.rs` + private files), with rule 4 reading "nothing outside `sem/` imports the engine"? | **yes**, for Rust. TypeScript's `sem.rs` is 3,800 lines; it could follow when convenient | RS3 step 1 | CLAUDE.md rule 4 and `scripts/check-all.sh` name `sem.rs` |
| **Q-RS4** | **A scenario tree per language where names differ** (`expected.default.rust.tree` looked for before the shared file)? The shared trees say `Counter.bump`; Rust prints `Counter::bump` and `<Json as Writer>::write` | **yes**: a per-language file only where needed, and the scenario's README says the shape is the same | RS4 step 9 | the other way is a name-mapping function in the testkit, which hides Rust's real names from the one test that is meant to show them |
| **Q-RS5** | **Are `Some(x)`, `Ok(x)`, `Err(e)` edges?** They are calls to external constructors, so by rule 5 they are `External` edges: hidden in the tree by default, present in the graph and counted | **yes, keep them**: uniform, and "never dropped" stays true without an exception list. **[measure]** their share in RS4; if they are most of the graph, ask again with the number | RS4 step 5 | graph size, and the unmerged wire when `show_external` is on |
| **Q-RC1** | **Where do a closure's calls show once we know who calls it?** A: stay under the def the closure is written in (layer 1's rule, nothing lost, nothing twice). B: move under the function that calls it (Python's rule; needs a second kind of summary and a rule for closures handed to external code) | **A now**; B later, designed once for TypeScript and Rust | RC2 | under A the shared scenario `callback-argument` stays at `Default` for Rust, with the reason in its README |
| **Q-RC2** | **"Possible implementations"**: when a `dyn` value is unknown, list the project's impls of the trait as children marked possible? Needs a new `Basis` value and a render option in core | **not now**. Decide from RC0's numbers; it is the cheap fallback if `open_dyn` is large and RC3–RC5 resolve little | whenever asked; before RC6 part D3 at the latest | it changes what an edge means (may, not does) |
| **Q-RC3** | **The solver hands a callee's return value to its caller**: grow core's flat solver (a handle passed to `walk`, `Summary::returns`), or keep return values inside `rust-analysis`? | **grow core**, after a design note you approve; flat languages stay byte-identical | RC4 step 2 | the first change to `Lang` since `Basis`; TypeScript's C4 and the fixpoint (RC6 B) depend on the same answer |

Smaller suggestions for the shared code, each with the phase that needs it, are in [`02-architecture.md`](02-architecture.md) §9. Two
of them are not questions but notes for work already planned elsewhere:

- **for TS5 step 1 (core):** resolve a `more` token by looking its `ContextId` up, not by decoding it; and let the solver's memo be
  handed in and out in an owned form ([RS5](04-default-resolver/06-rs5-driver-needs.md) §2, §5). Both cost nothing with `Key = ()`
  and are required the day any language has a real key.
- **for the testkit:** the level per scenario ([02](02-architecture.md) §9, row 2). TypeScript's C-steps need it as much as Rust's.

## 4. Things I did not check

I read source at `2a362a4` and the repo as of 2026-11-12. I built and ran nothing.

**Layer 1** (answered by RS0 unless a phase is named)

- Which `ra_ap_*` version is `2a362a4`; that the crates build on 1.99.1 on three operating systems; sizes and times.
- That `rust-analysis` can leave `ra_ap_ide` out of the release build and still load a workspace.
- `resolve_method_call` gives the impl's method for a concrete receiver and the trait's for a parameter, `dyn`, `impl Trait`, opaque
  and `Self` (read in `source_analyzer.rs`; not run).
- That the receiver's type can be classified from outside `hir` well enough to compute `Open` (RS4 step 3 is the test).
- What `original_range_opt` returns for a call that comes from a macro's body; that it tells arguments from body.
- Whether `Semantics` users must call `attach_db`, and where.
- How a blank line affects which doc comments belong to an item's node (`n_attached_trivias`).
- That `rustfmt` leaves `/// ID: …` alone (a test in RS2).
- Whether a load with no `rust-src` fails or quietly succeeds without `std`.
- Whether `cargo metadata` writes a `Cargo.lock` into a fixture folder.
- The cost of loading one tiny Cargo package (decides how scenarios are loaded).
- That the JSDoc block helper is not yet in `analyzer-core` (RS2 step 3 moves it; if it already moved, the step is a no-op).
- The exact type for applying a file change to the database (RS5).
- That TS5's core work (`Budget`, tokens) is not built: true on 2026-11-12, re-check when RS5 starts.

**Layer 2** (answered by RC0 unless a phase is named)

- `hir::Type` equality and hashing across aliases, `Self`, lifetimes, crates; a stable canonical text; closure ids across runs.
- `Type::instantiate` with a partly unknown argument list; what "the same origin" requires of the arguments.
- `resolve_trait_impl_method`: the order of `subst`, what `env` must be, blanket impls and their own arguments, built-in derives, a
  missing impl, an associated function with no receiver; its cost.
- That every birth of a `dyn` or a `fn` pointer has an adjustment the engine reports, and on which expression it sits.
- Telling "a `&mut` to the box" from "an auto-borrow to reach the object behind it" (RC3 §6).
- How to get from an opaque type to its function, and that function's hidden type (RC4 §2).
- How an impl's fixed type arguments (`impl Pipe<Json>`) are exposed for an entry (RC1 §6).
- How much of pylspt's `solver.rs` is free of ty's types (RC4's note).
- Every size (S / M / L) in [`README.md`](README.md).

Back to: [`README.md`](README.md)
