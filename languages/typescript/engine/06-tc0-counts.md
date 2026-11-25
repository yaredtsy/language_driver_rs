# TC0: counts and spikes for the context layer

Part of `languages/typescript/ENGINE.md` (split out on 2026-11-24, text unchanged). Sections in the order they were written.

## TC0: what was counted

### S-TC1: does tsrs answer what the plan says? (spike 9b91422, `spikes/s-tc1`; reviewed 2026-11-23)

The cases and their expectations were written before the spike ran (`docs/03-typescript/12-context-layer/04-tc0-counts-and-spikes.md`
§3.1). **40 answers of 40 as expected**, the same on two runs and on the reviewer's run; no engine panic. Built in the TypeScript target
(`CARGO_TARGET_DIR=../target`): only the spike compiled.

| # | Engine call | What it returned |
|---|---|---|
| E1 | `Checker::get_resolved_signature_exported` (`tsrs_checker/src/exports.rs:370`), `Signature::parameters`, `::this_parameter` (`types.rs:3171`–`3209`) | `withThis(handler)` with `this: Ctx`: parameters `[cb]`, this parameter `this` |
| E2 | the same, generic | `run(1, handler)`: each parameter's `value_declaration` is the `Parameter` node of `run`'s own declaration |
| E3 | `get_type_at_position_exported` (`services.rs:1215`), `type_to_string_exported` (`printer.rs:59`) | `useState(false)`, slot 0: `boolean` (the signature is instantiated) |
| E4 | `has_rest_parameter`, `min_argument_count` | `many(first, ...fs)`: `true`, `1`, two parameters; the type at positions 1, 2, 3 is `() => void` (a rest answers past the count) |
| E5 | `is_type_assignable_to_exported` (`exports.rs:528`), `get_type_at_location` | `encode` to `each`'s slot: `true`; `1` to a `() => void` slot: `false` |
| E6 | `get_symbol_at_location_exported`, `Symbol::value_declaration`, `tsrs_ast::get_root_declaration` | `onClose` in `Panel`: `BindingElement`, root the `Parameter`; `f` in `each`: `Parameter`; `h` in `main`: `VariableDeclaration` |
| E7 | `Type::object_flags` (`ObjectFlags::Class`), `Type::symbol` | `new Json()`: a class, `Json`; `w` declared `Writer` and set to `new Json()`: not a class, `Writer` (the checker keeps the declared type) |
| E8 | `get_property_of_type_exported` (`exports.rs:193`) | `write` on `Json` and on `Sub extends Json`: `Json.write`'s `MethodDeclaration` |
| E9 | `get_references_to_symbol_in_file` (`services.rs:743`), `tsrs_ast::is_write_access` (`ast.rs:1567`) | `Box.x`: three references, the declaration's name included; `false`, `true` (`this.x = 2`), `false` |
| E10 | `get_resolved_signature_exported` on a `JsxSelfClosingElement` | `<Panel … />`: declaration `Panel`, one parameter, named `__0` (destructured) |
| E11 | the references of a variable and `is_write_access` | `k`: seven references; the declaration's name `false`; `k = join`, `({ k } = o)`, `[k] = xs`, `k ||= join`, `for (k of xs)` all `true`; `k()` `false` |

**Probed by the reviewer, the same way, beyond the eleven** (a scratch copy of the spike; not committed):

| Shape | Resolved signature |
|---|---|
| `ov(1, handler)`, two overloads and an implementation | declaration: the **second overload**, not the implementation; parameters `a`, `cb` of that overload |
| `x(handler)`, `x: any` | no declaration, no parameters, min 0; every position `any` |
| `new C(handler)` | the `Constructor`; parameter `cb` |
| `obj.m(handler)`, `m(this: C, cb)` | parameters `[cb]`, this parameter `this` |
| `each(...arr)` and `each(handler)`, `g = handler` a default | parameters `f`, `g`; min 1; slot 1 `(() => void) \| undefined` |
| `many(1, handler)`, a rest | rest `true`; position 2 is still `() => void` |
| any non-rest signature, past its last parameter | `any`, not nothing |

What it changes is in `03-design.md` §7: binding is by position onto the callee's implementation, the resolved signature giving the count
and the rest; no type is asked past the count; an `any` callee binds nothing.

**Found:** a `declare namespace JSX` in a file with an `export` is local to that module, so the intrinsic elements are TS7026; a fixture
that needs JSX types writes them in `declare global`. The fixture's two semantic diagnostics: that one, and TS2684 (E1's `this`, expected).

**The load sequence** (the spike's, and `Sem::build`'s with a config): `bundled::wrap_fs(cachedvfs::from(osvfs::fs()))`; a `ConfigHost`;
`Region::new_scratch(1 << 20)` entered for the build, `ProgramRegions::default()`; `tsrs_tsoptions::get_parsed_command_line_of_config_file`;
`CompilerOptions { checkers: Some(1), .. }`; `tsrs_compiler::new_compiler_host`; `ProgramOptions` with `use_source_of_project_reference`,
`checker_regions`, `regions`; `new_program`, `bind_source_files`; then one `get_type_checker(&Context::default())` handle for every
question. Questions work outside the region's scope. `get_semantic_diagnostics` takes a handle of its own, so it runs after the held one is
dropped (H1).

### S-TC4: the per-site measure and the baseline (`b0f21fb`; reviewed 2026-11-23)

`tests/sweep.rs`: `TS_SWEEP_SAMPLE` runs a list of entries (the whole graph unless `TS_SWEEP_DEPTH`), `TS_SWEEP_SITES` writes one line per
site open in some context, `TS_SWEEP_BASELINE` compares a run with such a file (every / some / none / not reached, and new open sites).
A site is its def (file, qualname, occurrence) and its `Site`; the definitions are in
`docs/03-typescript/12-context-layer/04-tc0-counts-and-spikes.md` §3.2, and a test on a hand-built graph pins them.

**The baseline** (reviewer, release build of `b0f21fb`, the sample of 04 §3.2, whole graphs, no cut). With one context per function
every open site is open in every context that reaches it; for a single entry the open sites equal the `Unresolved` edges of the CLI.

| Project | Sample | Open sites | `function_typed_value` | `any_or_untyped` | `no_declaration` | `anonymous_function_called` | other | All graphs | Peak footprint |
|---|---|---|---|---|---|---|---|---|---|
| V-NOC frontend | `Dashboard`, `HomePage`, `TaskDetailPanel` | 733 | 723 | 7 | 0 | 3 | 0 | 1.2 s | 147 MB |
| the private codebase | its `main` | 12,807 | 2,506 | 10,225 | 35 | 37 | 4 (`union_receiver` 3, `computed_name` 1) | 3.2 s | 281 MB |
| TypeScript 5.9.3 `src/compiler` | `createProgram` | 5,715 | 5,672 | 6 | 34 | 1 | 2 (`computed_name`) | 1.5 s | 183 MB |
| vscode `src` | not chosen yet (swap full) | | | | | | | | |

The private codebase is a bare checkout (no `node_modules`): its imports are `any`, which is most of its open sites; the layer cannot
answer those. V-NOC against its own file: 733 every, 0 some, 0 none, 0 not reached, 0 new. The site files are kept outside git, in the
reviewer's scratch folder; a later phase is compared against a fresh baseline run of the commit before it, not against these files.

**Found at the review:** the measure keeps every site of every graph of a run, even when no site file is asked for. A full depth-1 sweep
of the private codebase took 339 MB at its peak instead of 214 MB (`5362028`'s sweep); on vscode that would be gigabytes. **Fixed** (`f444d71`): the
sites are kept only when `TS_SWEEP_SITES` or `TS_SWEEP_BASELINE` is set; the full private sweep peaks at 213 MB again, and V-NOC's site file is
byte-identical.

### N2, N3, N8: inside the parameter and call-result columns (`25a04c8`, feature `__tc0_counts`; reviewed 2026-11-23)

Full depth-1 sweeps (every site of the project once), release build with the feature, run by the reviewer. Every group adds up to its
column (checked on each project). The keys are defined in `docs/03-typescript/12-context-layer/04-tc0-counts-and-spikes.md` §3.3.

**N2 and N3: `function_typed_value` through a parameter**

| Project | Column | Own | Enclosing def | Anonymous fn | Depth 0 | 1 | 2 | 3+ | Value called | Method called on it |
|---|---|---|---|---|---|---|---|---|---|---|
| V-NOC frontend | 177 | 130 | 40 | 7 | 30 | 139 | 8 | 0 | 177 | 0 |
| the private codebase | 1,981 | 1,440 | 349 | 192 | 660 | 1,223 | 96 | 2 | 1,588 | 393 |
| TypeScript `src/compiler` | 2,252 | 1,649 | 549 | 54 | 820 | 1,145 | 270 | 17 | 964 | 1,288 |

**N8: `function_typed_value` through a call's result**

| Project | Column | Project function | Package | of which bound straight from the call | Unknown |
|---|---|---|---|---|---|
| V-NOC frontend | 799 | 33 | 766 | 748 | 0 |
| the private codebase | 1,007 | 978 | 21 (all the bundled library: `Map.get` 16, `Array.shift`, `find`, `splice`) | 7 | 8 |
| TypeScript `src/compiler` | 1,616 | 1,609 | 7 (`Map.get`) | 0 | 0 |

V-NOC's package calls, the top: `zustand` (the store hook, whose call signature has no name, so `?`) 416, `react` `useState` 235,
`useCallback` 31, `react-hook-form` `useForm` 21, `class-variance-authority` (`cva`, no name) 17, `memo` 15, `forwardRef` 7,
`useEffectEvent` 6, `useRef` 4, `react-router` `useNavigate` 4; 18 keys in all.

**What they say** (the rules of 04 §4):

- **Captures are not rare** (a parameter of an enclosing def: 23%, 18%, 24% of the column): TC3 stays before TC4.
- **Depth 3 or more is rare** (0, 2, 17): `max_depth` 2 stands.
- **"Called on" is the larger half only in the TypeScript compiler** (57%; 20% in the private codebase, 0 in V-NOC). It does not move TC5
  before TC4 yet: a method called on an interface parameter is answered by TC4 when an object literal is passed, by TC5 when an instance
  is. N1 says which is passed.
- **TCL's forecast is large in V-NOC and nothing elsewhere.** 748 of V-NOC's 1,035 `function_typed_value` edges (72%) are a name bound
  straight from a package call. In the other two, the call-result column is project functions (97% and 99.6%): that is TC6's.
- **The package half is not only "makers".** `useCallback`, `memo` and `forwardRef` (53 in V-NOC) return the project function they were
  given: TCL names them after the library, which is right until TC10 follows them. And the declaration of a maker can have no name
  (zustand's store hook and `cva` are call signatures: 433 of V-NOC's 766): TCL names a library-made function after **the call as written
  in the project** (`useStore()`, `create()()`), never after the declaration (`11-later.md` §4.1).

### N1: what callers pass (`d8c56e9`, feature `__tc0_counts`; reviewed 2026-11-23)

Full depth-1 sweeps, release build with the feature, run by the reviewer. At every site with a project target, each argument in a slot
that can hold a function or an object (TC0's over-counting rule, 04 §3.4), and each JSX attribute written in braces whose value can.
Shares are of the classified arguments; the arguments in a data slot are counted apart.

| Project | Classified | Data slot | Def by name | Anonymous fn | Parameter passed on (and its member) | Local (and member) | Object literal (with a function) | `new` | Static class | Call result (project) | Other syntax |
|---|---|---|---|---|---|---|---|---|---|---|---|
| V-NOC frontend | 1,655 | 1,242 | 26 (2%) | 151 (9%) | 521 (31%) | 358 (22%) | 140 (3) | 0 | 24 | 42 (19) | 383 (23%) |
| the private codebase | 18,646 | 22,981 | 268 (1%) | 914 (5%) | 5,421 (29%) | 7,216 (39%) | 2,723 (126) | 196 | 228 | 382 (265) | 1,189 (6%) |
| TypeScript `src/compiler` | 49,012 | 15,367 | 2,953 (6%) | 1,126 (2%) | 24,883 (51%) | 12,695 (26%) | 162 (28) | 13 | 25 | 3,415 (3,277) | 1,889 (4%) |

In V-NOC, 480 of the 1,655 are JSX attributes (anonymous 148, a parameter 105, a local 103, a def 26, spread 26). Module variables are
under 1% everywhere.

**S-TC2's list of syntax with no `Expr`** (the `other:` keys, largest first): V-NOC `StringLiteral` 286, `BinaryExpression` 47,
`ConditionalExpression` 31, `ArrayLiteralExpression` 7, `JsxElement`/`JsxSelfClosingElement` 8, `TaggedTemplateExpression` 2, `NullKeyword`
2; the private codebase adds `NullKeyword` 119, `TemplateExpression` 19, `AwaitExpression` 15, `ThisKeyword` 6, `Identifier` 27 (`undefined`
and globals); the compiler `Identifier` 541 (`undefined`), `PrefixUnaryExpression` 76, `ThisKeyword` 50. **Every one is in the design's
`Expr`** (data, `OneOf`, `This`, `Call`, arrays for TC9): no syntax is missing from the plan.

**What it says** (04 §4):

- **A value the caller can name is rare at the first step**: a def, a function literal, an object literal with a function, a `new`: 11% in
  V-NOC, 8% in the private codebase, 8% in the compiler. Most arguments are **a parameter passed on** (29–51%) or **a local** (22–39%): the
  value comes from further up. So TC2's chains (a parameter's value from the caller's key) and TC3's locals carry the layer, not the first
  call; N6 and the forecast of step 7 say how far.
- **TC7 does not move up** (04 §4's rule needs "a parameter passed on" to be small; it is the largest class). But in React code an
  anonymous function is passed more often than a def by name (9% to 2%, 5% to 1%): D54 is worth its revisit before TC7, as D58 says.
- **TC5 does not move before TC4.** Almost nothing passed is an instance (an argument whose static type is a class: 24, 228 and 25; a `new` among them: 0, 196, 13), so the compiler's
  57% "method called on a parameter" (N3) has no instance to bind at the first step.
- **TC4's share in React code is the JSX attributes** (29% of V-NOC's classified arguments), not object literals passed to calls (3).

### N4, N7: reading the writes of a name (found at step 4b, 2026-11-23; checked by the reviewer at `13a31a4`)

- **`get_possible_symbol_reference_nodes` is not reachable.** It is a `pub fn` in `tsrs_checker/src/services.rs:827`, but `lib.rs:177`
  declares `mod services;` without `pub`, and re-exports only `get_resolved_signature_for_signature_help` from it. A free function is
  reached through its module's path, so a private module hides it; a method is reached through its type, which is why
  `Checker::get_references_to_symbol_in_file` (the same file) can be called. The same mistake as S-TS3's call-hierarchy resolver: **a `pub`
  is checked together with the path to it.** `tsrs_ls`'s copy (`findallreferences.rs:2397`) is private too, and `tsrs_ls` is not a
  dependency.
- **The engine's scoped search is not scoped.** `get_possible_symbol_reference_positions` (`services.rs:845`) finds the first hit in
  `bytes[container_start..]` and uses that index as an absolute position (`tsrs_ls`'s copy says so: "Go quirk kept: the first index is
  relative to container.Pos() but is used as an absolute position"), so the walk starts near the top of the file and runs to the
  container's end. Callers filter by symbol, so answers stay right; the cost is not one container's.
- **So `sem` searches itself**: the container's span for the name, word boundaries by `tsrs_scanner::is_identifier_part`, the node at each
  hit by `tsrs_checker::astnav::get_touching_property_name` (exported: `pub use tsrs_astnav as astnav`), then the symbol and
  `ast::is_write_access`. TC3 uses the same helper (03 §5).

### N4, N7: how often a local is written, and where `this.x` gets its value (`25793b3`, feature `__tc0_counts`; reviewed 2026-11-23)

Full depth-1 sweeps **with the files' own code** (`TS_SWEEP_FILE_CODE=1`), release build with the feature, run by the reviewer. Locals of
a capable type (04 §3.5), per def walked; the `this` column split by the field's first declaration, which adds up to the column.

**N4**

| Project | `const` | Written once | Written more than once | of them, a write TC3 does not read | Data type | Parameters (capable) | of them written by the body |
|---|---|---|---|---|---|---|---|
| V-NOC frontend | 1,642 | 14 | 16 (1.0%) | 0 | 985 | 409 | 0 |
| the private codebase | 23,511 | 1,426 | 4,854 (16.3%) | 29 | 14,972 | 6,752 | 3 |
| TypeScript `src/compiler` | 9,357 | 710 | 952 (8.6%) | 148 | 4,700 | 15,006 | 336 (2.2%) |

The share is of the capable locals. React code writes a variable once (V-NOC: 98% are `const`); imperative code reassigns (`let`
accumulators, loop state). A write TC3's model does not read (a destructuring assignment, `for … of`, `||=`, `++`) is rare: 0, 29, 148.

**N7**

| Project | Column | Parameter property | Set only in the constructor (or its initializer) | Written elsewhere | Never written | A method | Other | Sites in classes the project builds by `new` |
|---|---|---|---|---|---|---|---|---|
| V-NOC frontend | 7 | 0 | 7 | 0 | 0 | 0 | 0 | 7 (1 class) |
| the private codebase | 144 | 23 | 70 | 35 | 10 | 4 | 2 | 136 (31 classes); 6 in 2 others |
| TypeScript `src/compiler` | 17 | 0 | 12 | 0 | 0 | 0 | 5 | 12 (1 class) |

Times: the three sweeps 0.3 s, 2.3 s and 2.6 s; the compiler's `createTypeChecker` (one def, its nested defs apart) 0.4 s, so the scoped
search of 04 §3.5 holds on the largest def there is.

**What they say** (04 §4):

- **N4 crosses the rule**: more than a few percent of capable locals are written more than once in two projects (16% and 9%). So "a value
  per place" (11 §3) is planned as a phase, not left as an option. Not moved before TC6: a reassigned variable costs precision only where it
  reaches a call (a callee or an argument), and that share is what is counted before the phase is designed.
- **TC8 is not decided by these three.** The `this` column is small here (7, 144, 17) and two thirds of it is fields the constructor sets in
  classes the project builds by `new`, which is TC8's good case. vscode's `this` column was 47% of its function-typed calls (70,118): its N7
  decides TC8 against TC6.
- **Not seen, said:** a write to a private name (`this.#x`) is a `PrivateIdentifier`, which the TC0 helper skips; such a field reads as never
  written or constructor-only. TC3's and TC8's reader takes private names too.

### N5: what is done with an object (`fc98c18`, feature `__tc0_counts`; reviewed 2026-11-23)

Full depth-1 sweeps with the files' own code, release build with the feature, run by the reviewer. Each flag once per object; an object
can have several (04 §3.6).

| Project | Kind | Objects | Untouched | Written through | Handed to a project function | Handed to a library | To a JSX attribute | In an array literal | A second name | Returned | Exported |
|---|---|---|---|---|---|---|---|---|---|---|---|
| V-NOC frontend | parameter | 393 | 233 | 1 | 83 (21%) | 37 (9%) | 2 | 17 (4%) | 17 | 21 | |
| | local literal | 36 | 9 | 13 (36%) | 8 | 7 (19%) | 2 | 3 | 0 | 5 | |
| | module `const` | 46 | 37 | 0 | 0 | 5 | 4 | 0 | 0 | 0 | 22 |
| the private codebase | parameter | 6,323 | 3,591 | 89 (1.4%) | 1,486 (24%) | 419 (7%) | 92 | 69 (1%) | 671 (11%) | 118 | |
| | local literal | 839 | 198 | 304 (36%) | 244 | 159 (19%) | 4 | 23 | 2 | 154 | |
| | module `const` | 357 | 276 | 5 | 17 | 29 | 2 | 12 | 5 | 17 | 124 |
| TypeScript `src/compiler` | parameter | 13,926 | 5,247 | 278 (2%) | 8,147 (58%) | 277 (2%) | 0 | 205 (1.5%) | 186 | 479 | |
| | local literal | 94 | 12 | 24 (26%) | 48 | 14 (15%) | 0 | 0 | 0 | 36 | |
| | module `const` | 78 | 27 | 2 | 41 | 1 | 0 | 11 | 0 | 1 | 24 |

**What it says** (04 §4 and the trust rule of 03 §11):

- **An object passed in is almost never written through by the function that holds it** (0.3%, 1.4%, 2%). A local literal often is (26–36%):
  it is built field by field. TC4's rule is cheap for parameters (props) and costly for local literals.
- **Handed to a project function is the large row** (21%, 24%, 58% of object parameters). It is an upper bound: the rule opens the object
  only if the receiving function writes through it (or hands it on to one that does). So the receiver's "writes through parameter i" fact
  (`Flow::facts.touches`, 03 §4) is what keeps TC4 useful; without it TC4 loses a fifth to a half of the objects.
- **Handed to a library: 2–9% of object parameters, 15–19% of local literals.** Q-TC7 (`docs/07`) asks whether a library may be assumed to
  leave an object's members alone. 04 §4's trigger ("handed to external functions common, written through rarely") holds for parameters only
  at the low end; the question is asked anyway, since it is a stated exception to "never guess" either way.
- **An array literal is rare** (1–8%): the row "an object put in an array literal opens it" stays, with no exception for dependency lists.
- **Exported module objects are a third to a half of module `const` objects** (22 of 46, 124 of 357, 24 of 78): the TC4 row that opens an
  exported one (`module_object_shared`) costs that much. A project-wide check of writes through an exported `const` is the refinement, if
  TC4's sweep says it matters.

### S-TC2, S-TC3, N6: answered without a spike, or moved (reviewer, 2026-11-23)

- **S-TC3, the digest: answered by reading.** Python (`interp/canon.rs` `stable_id`) and Bend (`key.rs` `digest`) both write the canonical
  key, its function included, with `serde_json::to_vec` and hash it with `xxhash_rust::xxh3::xxh3_128`, little-endian (`to_le_bytes`):
  deterministic by the algorithm's specification and by `serde_json` over structs and vectors (no map whose order can move). The function
  goes to core in TC2's first step (D57), as `fn digest<T: Serialize>(canonical: &T) -> [u8; 16]`; the open key's zeros stay each
  language's rule. "Equal on two machines" is a test that pins the bytes of a few keys, run by CI on Linux, macOS and Windows.
  `xxhash-rust` 0.8.19 is in TypeScript's lockfile (through tsrs); core takes it with the `xxh3` feature, as Python does.
- **S-TC2, a body as data: its design risk is retired by N4; its cost is TC1's gate.** The risk was whether an assignment made inside a
  nested function can be given to the variable's scope. N4 does it through the engine's symbol on a scoped search, nested anonymous
  functions included (the fixture's `g`), and it is fast on the largest def there is (0.4 s for `createTypeChecker`). What the table costs
  can only be measured on the real reader, so it is measured in TC1, not on a copy of it: `05-tc1` §8 step 2.
- **N6, contexts per callee: moved into TC2.** Counting distinct argument tuples now would be an approximation of TC2's key (first-step
  values only, no chains), and N1 says most first-step values are unknown, so it would under-count. TC2 measures it on its real keys, with
  and without telling anonymous functions apart and with and without relevance (two capability bits), before it is released: `06-tc2`.

### TC0's forecast: which phase can answer which open sites (reviewer, 2026-11-23)

The sample of 04 §3.2, with the counts on; the open `function_typed_value` sites by column. **Upper bounds**: a phase answers a site only
where a caller passes something it can follow, and N1 says that is rare at the first step (8–11% of arguments). The rows overlap (depth
and method are other splits of the same parameter column). Shares of the sample's function-typed edges.

| Phase | V-NOC | the private codebase | TypeScript `src/compiler` |
|---|---|---|---|
| open sites in the sample (all reasons) | 733 | 12,807 (10,225 `any`: a bare checkout) | 5,715 |
| `function_typed_value` among them | 723 | 2,506 | 5,672 |
| **TCL**: bound straight from a package call | **76%** | 0.2% | 0 |
| **TC2**: a parameter's value, called as it is (depth 0) | 1% | 15% | 6% |
| **TC3**: of the parameter column, a parameter of an enclosing def | 2% | 8% | 6% |
| **TC4**: a member or a pattern of a parameter (props) | 14% | 29% | 14% |
| **TC5**: a method called on a parameter; `this` | 0 | 13% | 13% |
| **TC6**: what a project function returned | 3% | 26% | 23% |
| not placed: "other" (a module name, an import, no leftmost name) | 4% | 25% | 56% |

**TC2 as measured** (TC2 step 9, `engine/09-tc2.md`): 0%, 1.8% and 3.4%, below its bound.

**The order is confirmed**: TCL first (three quarters of V-NOC's function-typed sites, and no layer needed), then TC1–TC6 as planned. No
phase is below "a few percent" on every project; TC2's own yield is small in React code, but every later phase needs its key. The
"other" column, the compiler's largest, is not placed by any count: TC3's module constants answer some of it, and TC3's own sweep says
how much.

### TC0 closed (`c123d0f`; reviewed 2026-11-24)

The counts and the feature `__tc0_counts` are removed in one commit; `src/` differs from H17 (`5362028`) only by the view extensions TC1
reads (`HasExpression` with `JsxExpression`, `JsxSpreadAttribute`, `SpreadElement`; `Initialized` with `JsxAttribute`). The new fixture
`view-kinds` is what makes the totality walk meet the two spread kinds (removing it makes the walk fail, shown by the coder): 197,105
nodes, 0.57 s. Reviewer: 380 pass, 11 ignored; V-NOC's 1,476 depth-1 graphs hash the same as at H17. The release sweep binary built with
the counts is kept outside the repo, for vscode's N7 when the machine has room. **Still owed from TC0: vscode's row of every table** (and of
H17's before/after), which decides TC8's place.
