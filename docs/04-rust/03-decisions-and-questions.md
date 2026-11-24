# 04.03 Rust: decisions and questions

```
decisions and questions
├── 1. what is already decided, and where Rust uses it
├── 2. draft rows (RS-M1 … RS-M13): decided by me, yours to veto
├── 3. questions only you can answer (Q-RS1 … Q-RS8, Q-RC1 … Q-RC5; Q-RS1 … Q-RS8, Q-RC2 and Q-RC4 answered: D29, D30, D35, D36, D38, D45, D48, D60)
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
| D29 | the self type's and the trait's generic arguments are in a qualname; one rule spells every type in a name | RS1 §3, §3.1 |
| D30 | a project macro's body calls are sites, an external macro's are not; per-language scenario trees; `Some(x)` / `Ok(x)` are `External` edges | RS4 §3, §5, §8 |
| D35 | the `sem` module may be a folder: `sem/` in `rust-analysis`, one `sem.rs` in `rust-syntax` | RS3 §5, RS4 §7 |
| D36 | a `dyn` value is resolved by following the value (Python's attribute tracking), never by listing the trait's impls as possible | RC3–RC5, RC6 A and D; design §9 |
| D37 | an incomplete or old Rust install is loaded with what there is, and a `Warning` says what is missing; with no `cargo` the crate graph is read from `Cargo.toml` by us, with no manifest the file alone is loaded; only a file that is not there is an error | RS3 §6, the ladder |
| D60 | where nothing is known about a value, the default the code writes is used (an entry's `self` from its type's own init run with no arguments; a parameter is its declared type); what other code passes or stores is never searched | design §12.2; RC1 §6, RC4 §3, RC5 step 7; RC6 §2 |
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
| **RS-M13** (2026-11-24) | Which form `sem.flow` reads a body from, the syntax (RC3 §2 as written) or the engine's MIR, is **decided by S-RC4's numbers**, after this folder is read against TypeScript's context-layer plan. Either way `sem` hands `walk` an owned form with no engine type | fix it now: keep RC3 §2 and never look (cheaper today; we then write and test every control-flow rule ourselves), or switch to MIR unmeasured (a body with a type error has no MIR) | design §12.1; RC0 S-RC4 |

## 3. Questions

| # | Question | My recommendation | Needed before | Why it matters |
|---|---|---|---|---|
| ~~**Q-RS1**~~ | **Qualname of a trait-impl method when the trait has generic arguments**: `impl From<A> for S` and `impl From<B> for S`. Keep them (`<S as From<A>>::from`) or drop them like the self type's (`<S as From>::from` twice, told apart by `occurrence`)? | **ANSWERED (D29), 2026-11-13: keep the trait's arguments, and keep the self type's too** (`<S<u8> as From<A>>::from`), in inherent impls as well (`S<u8>::m`); every type in a name follows one spelling rule, RS1 §3.1. Was: **keep the trait's arguments**, as written, whitespace normalised; keep dropping the self type's | RS1 step 3. **Frozen at `rust-v0.1.0`** | with them dropped, reordering two impls in a file swaps the docs attached to their methods. With them kept, renaming a type used as the argument renames the def (as renaming the trait would) |
| ~~**Q-RS2**~~ | **A call in the body of a project `macro_rules!`** (`log!()` expanding to `crate::emit()`): is it a call site of the function that uses the macro, drawn at the macro call? | **ANSWERED (D30), 2026-11-13: as recommended**. Was: **yes** for project macros; **no** for external macros (`println!`'s inside is external code) | RS4 step 7 | without it, a project that wraps its calls in macros has trees with holes. With it, one site can have several targets that the user never typed there |
| ~~**Q-RS3**~~ | **May `sem` be a folder** (`sem/mod.rs` + private files), with rule 4 reading "nothing outside `sem/` imports the engine"? | **ANSWERED (D35)**: yes. CLAUDE.md rule 4 and `scripts/check-all.sh` allow it since commit `1a2a39d` (2026-11-12); recorded 2026-11-16. Was: **yes**, for Rust. TypeScript's `sem.rs` is 3,800 lines; it could follow when convenient | RS3 step 1 | CLAUDE.md rule 4 and `scripts/check-all.sh` named `sem.rs` |
| ~~**Q-RS4**~~ | **A scenario tree per language where names differ** (`expected.default.rust.tree` looked for before the shared file)? The shared trees say `Counter.bump`; Rust prints `Counter::bump` and `<Json as Writer>::write` | **ANSWERED (D30), 2026-11-13: as recommended**. Was: **yes**: a per-language file only where needed, and the scenario's README says the shape is the same | RS4 step 9 | the other way is a name-mapping function in the testkit, which hides Rust's real names from the one test that is meant to show them |
| ~~**Q-RS5**~~ | **Are `Some(x)`, `Ok(x)`, `Err(e)` edges?** They are calls to external constructors, so by rule 5 they are `External` edges: hidden in the tree by default, present in the graph and counted | **ANSWERED (D30), 2026-11-13: as recommended**, the share still **[measure]**d in RS4. Was: **yes, keep them**: uniform, and "never dropped" stays true without an exception list. **[measure]** their share in RS4; if they are most of the graph, ask again with the number | RS4 step 5 | graph size, and the unmerged wire when `show_external` is on |
| ~~**Q-RS6**~~ | **A project with no `Cargo.lock`**: the engine's `cargo metadata` writes one into it (rule 2 says a library never writes a file). Accept it (A), redirect it to a temporary folder (B), or load with `--locked` (C, every dependency call `Unresolved`)? | **ANSWERED (D38), 2026-11-16, by the rule set when it was parked: B, with A below 1.97.** RS3 step 1 measured D: it loses every cross-package edge of a workspace (45/45, 16/16, 7/7) and, through the sysroot, `std`'s own links (`ENGINE.md`, "In-project edges by load mode"). Was: **Parked (owner, 2026-11-16) until RS3 step 1 measures a load with `no_deps` (option D: no lockfile at all), counting only in-project edges** ([RS3](04-default-resolver/04-rs3-project.md) §5). If D loses nothing that matters, D; else **B**: `CARGO_RESOLVER_LOCKFILE_PATH` in `extra_env` when there is no lock and the toolchain is ≥ 1.97; a temporary file deleted before `open` returns is not a write in rule 2's sense. **A** as the fallback on older toolchains, said by the `LockfileWritten` warning (D37) | RS3 step 1 | the user's project gains an untracked file each time V-NOC opens a fresh clone. Design and three **[check]**s in [RS3](04-default-resolver/04-rs3-project.md) §5 |
| ~~**Q-RS7**~~ | **ANSWERED (D45), 2026-11-19: `class_prefix` for `Struct`, `Enum` and a new `Union` kind; the real kind stays in `target_kind`.** Was: **Which wire prefix does a marked Rust type get?** Core's `wire::target_id` gives `class_prefix` only to `DefKind::Class` and `function_prefix` to every other kind. A Rust `struct`, `enum` or `union` is `DefKind::Struct` / `Enum` (a union is `Struct`, RS1), so a marked struct as an entry or a construction is `F/marked-struct` today (probed: `rust-cli calls … --entry MarkedStruct --wire --function-prefix F/ --class-prefix C/`). V-NOC has only `ClassSchema`, `FunctionSchema`, `FileSchema`, `FolderSchema` (`src/lsp/ts_js/src/driver/constants.ts`) | **`class_prefix` for every kind a construction can name: `Class`, `Struct`, `Enum`** (one match arm in core's `target_id`). A Rust type is built, not called, as a class is; Python and TypeScript never put `Struct` or `Enum` in a wire node, so their output does not change. Traits, modules and aliases are never nodes | the Rust tag (the wire ids are what V-NOC stores) | a marked struct's id in the wire names the wrong schema folder, so V-NOC would look for a `FunctionSchema/…` that is a `ClassSchema/…` |
| ~~**Q-RS8**~~ | **ANSWERED (D48), 2026-11-20: `debug_assertions` on, `miri` off, for now; the `cfg` set becomes the user's choice later (RS3 §3).** Was: **Which `cfg` flags does a load turn on besides `test`?** Today none: code under `#[cfg(debug_assertions)]` is turned off, so a debug-only function is no def (`None` as an entry) and a debug-only statement no site (found by the 2026-11-20 sweep: ruff's `Cursor::previous` and three tests). `cargo build` and `cargo test` compile with debug assertions on by default; rust-analyzer's own default is `["debug_assertions", "miri"]` | **`debug_assertions` on, `miri` off**: the code a default `cargo build` / `cargo test` compiles, as `test` is already on. `miri` would turn on code written only for the Miri interpreter and turn off its `not(miri)` twins, which a normal build runs | the first Rust tag (it changes which defs exist; no qualname changes) | a user marks a debug-only function and the tree says it is not there |
| **Q-RC1** | **Where do a closure's calls show once we know who calls it?** A: stay under the def the closure is written in (layer 1's rule, nothing lost, nothing twice). B: move under the function that calls it (Python's rule; needs a second kind of summary and a rule for closures handed to external code) | **A now**; B later, designed once for TypeScript and Rust | RC2 | under A the shared scenario `callback-argument` stays at `Default` for Rust, with the reason in its README |
| ~~**Q-RC2**~~ | **"Possible implementations"**: when a `dyn` value is unknown, list the project's impls of the trait as children marked possible? Needs a new `Basis` value and a render option in core | **ANSWERED (D36), 2026-11-16: no. Follow the value, as Python's attribute tracking does**; an unknown `dyn` stays the trait's declaration, counted. Was: **not now**. Decide from RC0's numbers; it is the cheap fallback if `open_dyn` is large and RC3–RC5 resolve little | whenever asked; before RC6 part D3 at the latest | it changes what an edge means (may, not does) |
| ~~**Q-RC4**~~ | **ANSWERED (D60), 2026-11-24: no.** Where nothing is known, the default the code writes is used (the type's own init for an entry's `self`, the declared type for a parameter), and what other code passes or stores is never searched, as in Python. Was: **A private field's set**: answer a field nobody handed over from everything the field's own module writes into it (complete, because a private field can be written nowhere else) | was: yes, if S-RC5's numbers were worth it | | the owner: "I do not want to show which other calls, like jedi did … I disabled that feature … I only run init and use the default". The declined proposal and why are kept in [design §12.2](05-context-layer/01-design.md) |
| **Q-RC5** (2026-11-24) | **Which function is a Rust type's "init", run with no arguments, for a method asked as an entry (D60)?** Python has one `__init__`; Rust has none by name. (a) the order: `impl Default` in project code, then an associated function `new`, else nothing (the open key). (b) may a type parameter's slot start from a default the type writes (`struct Pipe<W: Writer = Json>`)? | **(a) yes, in that order**; `new` is walked under the open key, so a field it takes from a parameter stays the declaration and a field it fills itself is known. Never a join over several constructors. **(b) yes**, counted. Both after S-RC5's numbers | RC5 step 7; (b) before RC1's entry rule is built | choosing `new` by name is a convention (the Rust API Guidelines' C-CTOR), not something the language says; and a type with `json()` and `csv()` but no `new` gets nothing. [design §12.2](05-context-layer/01-design.md) has the table and an example |
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
- (2026-11-24, S-RC4) That `mir_body` can be asked from `sem` for a `hir::Function`; the share of real bodies that lower; that every
  layer-1 site has exactly one `Call` terminator; what a partly open key gives `monomorphized_mir_body`. Read, not run: the items and
  the two limits in `ENGINE.md`, "Read at the pin for the plan".
- (2026-11-24, S-RC5) How often a project type with an erased field has a `Default` impl or a `new`, and how often that init fills
  the field itself; how the engine gives a derived `Default`'s value for a field.
- ✅ (2026-11-24) What Python does for the `self` of a method asked as an entry: one object of the class is built and `__init__` is
  walked on it with no arguments, hidden (`interp/entry.rs`, `build_object`); a parameter with no argument is its default value, else
  its annotation (`interp/bind.rs`, `default_value`, `annotation_fallback`); callers are never searched. Read in the code, not run.
- Every size (S / M / L) in [`README.md`](README.md).

Back to: [`README.md`](README.md)
