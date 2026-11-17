# The module def: a file's own code as an entry (TS5 row M)

**Decided (owner, 2026-11-13, D32):** a file's own code gets a def of its own, the **module def**, before `typescript-v0.3.0`, as its own row
of [`07-ts5-driver-needs.md`](07-ts5-driver-needs.md) §7 (row M). Row 3b comes after it. This note is the design. Every choice below marked
**(recommended)** is the reviewer's and waits for the owner's yes; the rest follows from decisions already made.

## 1. Why

Row 3c's coverage test (`02aeb03`, `src/sem/coverage.rs`) found that the only gap on real projects is code at the top of a file: a call
there is in no tree and no counter, and neither is a call inside an anonymous function there ("in phase 1, calls inside an anonymous
function belong to the enclosing def", and at the top there is none).

| Project | call-like nodes | drawn | missed: top level | missed: in an anonymous function at the top |
|---|---|---|---|---|
| V-NOC frontend | 7657 | 5728 | 325 | 461 |
| postcss (a) js only | 968 | 832 | 36 | 13 |
| postcss (b) with `.d.ts` | 312 | 277 | 23 | 0 |

**The 461, split by what holds the outermost anonymous function** (reviewer, 2026-11-13: a throwaway print in `category_of`, run once,
then removed; outputs in that session's scratchpad):

| Pattern | Sites | Example |
|---|---|---|
| a test callback: `describe(…, () => { … })`, `vi.mock(…)` | 209 (205 + 4) | the test files' `describe` / `it` bodies |
| a store: `create<S>()(devtools(immer((set) => ({ … }))))` | 124 (devtools 62, immer 57, create 5) | `useVersioningStore.ts:129 set(…)`, the store's actions |
| a component: `const X = React.forwardRef<…>((props, ref) => { … })` | 116 (82 `forwardRef`, 34 `React.forwardRef`) | `components/ui/tree-view.tsx`: `TreeView`, `TreeItem`, `TreeLeaf` are not defs at all (`calls --entry TreeView`: "no def") |
| a schema callback: `z.object(…).superRefine((data, ctx) => …)` | 6 | `CreateProjectDialog.tsx:50` |
| an editor extension, a highlighter: `createExtension(…)`, `createdBundledHighlighter({ … })` | 6 | |

postcss (a)'s 13 are all one pattern: a function assigned to a property at the top, `Container.registerParse = dependant => { … }`.

So the gap is not setup code alone. It is whole components, a store's every action, and every test.

## 2. What the engine does

tsrs's call hierarchy (`tsrs_ls/src/callhierarchy.rs`) answers each question of this note:

- **Who can be a caller** (`is_valid_call_hierarchy_declaration`, line 115):
  - the **source file**;
  - a namespace with an identifier name;
  - function, class and method declarations;
  - a **class static block**;
  - get and set accessors;
  - a named function expression;
  - an arrow or function expression held by a `const` (`is_assigned_expression`).

  An arrow passed to a call (`forwardRef((props, ref) => …)`) or assigned to a property (`X.y = () => …`) is none of these.
- **What a caller's calls are** (`collect_call_sites`, line 1069):
  - a source file: all of its statements, down to the next caller;
  - a namespace (not ambient, with a block body): its statements;
  - a static block: its body;
  - a class: its decorators, `extends`, the members' decorators, every property initializer, the constructor and static blocks.
- **How a caller is named** (`get_call_hierarchy_item_name`, line 180): a source file by its file name; a static block `C static {}`.

Under **"follow the engine"**: the file is a caller, and its calls are flat, anonymous functions included. A `forwardRef` component stays
unnamed, because the engine does not name it either.

## 3. Design

### 3.1 The name and the id: the file's own (owner, 2026-11-13)

A file already has a unique name, its path, and V-NOC already has an id for it, the file header marker. So the module root uses those
directly. **There is no `<module>` qualname and no `module_prefix`** (an earlier draft of this note had both; the owner asked what they give
that the file does not, and the answer was nothing):
- `<module>` existed only to fit the old entry shape `{ qualname, occurrence }`, and it would have been one more name frozen at the tag;
- `module_prefix` existed only for the root frame's `target_id`. No other frame carries its caller's id (`wire.rs` `head` reads the
  caller for call-site positions only). The module is only ever a root, asked for by a reader that already holds the file's id.

**The entry API accepts the file.** The request names the file by its path, as every entry already does: the driver maps V-NOC's file id to
the path, the way it maps a function id to `{ file, qualname, occurrence }`. The analyzer never sees a product id (rule 1).
**Decided (owner, 2026-11-13): core's `Entry` becomes a `#[non_exhaustive]` enum, for every language**, not a TypeScript-only API:

```rust
pub enum Entry<'a> {
    Def { qualname: &'a str, occurrence: u16 }, // what `Entry` is today
    File,                                       // the file's own code
}
```

What `Entry::File` means in each language (one request shape; each language answers it or says `None`, which is "no such def" today):

| Language | `Entry::File` | When |
|---|---|---|
| TypeScript | the file's top-level code, as this note says | row M |
| Python | `None` at first. Python has the same kind of code (a module's top-level statements, `<module>` scope in `sem/qname.rs:63`), so it is a Python row of its own, with a coverage test like row 3c's to show its size | after row M; the Python session's plan |
| Rust | always `None`, by the language: nothing runs at the top of a Rust file (a `const` or `static` initializer is evaluated at compile time, and `fn main` is a def) | when Rust is built; `docs/04-rust/02-architecture.md` line 197 ("`Entry` is core's, used as it is") needs one line for it, by the Rust planning session |

It is a core change:
- every `Entry { qualname, occurrence }` becomes `Entry::Def { qualname, occurrence }`: TypeScript's `calls.rs`, its tests and CLI;
  Python's `lib.rs`, `tests/api.rs` and CLI (mechanical; no Python behaviour changes);
- `#[non_exhaustive]`, so a language that matches on it keeps a `_ => None` arm and the next kind of entry is not a break;
- Python's tests stay green (rule 5), and the Rust workspace, if it exists by then, compiles.

**The root's row (`DefInfo`):**
- `kind: DefKind::Module` (already in core): the language-neutral kind, so the reader knows the root is a file and not a function;
- `qname`: the file's path, which is what the engine's own name for it is (its file name);
- `qualname`: empty, `occurrence: 0`;
- `range`: the whole file, so a site's offset is its byte position;
- `id`: `StableDef::Path { module: file, qname: "", nth: 0 }`. With no `Marked` id, the wire's `target_id` is empty by the existing rule
  (`target_id`, `wire.rs:144`), and the reader uses the file id it asked with. A path id also cannot collide with a def's marker text.

**The outline is unchanged**: no new def, no rename, no qualname rule (rule 10). `typescript-cli outline` prints what it printed.

### 3.2 The wire

- **`target_kind: "Module"`** on the root frame, `target_file` the file, `target_qualname` empty, `target_id` empty. No new field and no
  change in `core/analyzer-core/src/wire.rs`.
- **No wire version bump** (**[check] answered, reviewer, 2026-11-16: holds**: a callee is a def only through `FileDefs::def_of`, the
  outline's nodes (`sem/resolve.rs:400`); a source file is not in the outline (§3.1 keeps it out), so the checker's answer can never
  become a `Module` callee; it is `decl_not_in_outline` at worst): nothing calls a module (`import()` and `require("x")` are module loads, excluded by policy), so a
  `Module` frame is only ever a root, and only when the reader asked for the file. A reader that predates it never sees one. Every node
  under it is an ordinary call node. If the check finds a module as a callee anywhere, this is wrong and the version goes to 3
  (coordinated with row 2b, which also wants 3).

### 3.3 What the module owns

The module def's body is the file's top-level statements, walked like any body (`walk::body`), down to the next def. So it owns:
- the top-level calls (`const x = f();`, `export default g();`, `f();`);
- the calls in **anonymous functions** at the top (the phase-1 rule, now with an enclosing def): the 461 above, the IIFEs, `X.y = () => …`;
- **hand-overs** at the top (D22): `export const C = memo(function C() {…})` draws the call of `memo` and `C [Passed]`.

**(recommended) The same rule for the code that runs when a declaration is evaluated:** *code that runs when a class or an enum is defined
belongs to the body that declares it.* That covers:
- a class's decorators (its own, its members', its parameters'), its `extends` expression, its static field initializers and its static
  blocks;
- an enum member's initializer (`enum E { A = f() }`).

This is not a new rule. The walker already draws a nested class's **computed names** in the function that declares the class
(`Collector::collect`). Applied everywhere, the definition code is:
- in the **module** for a top-level class or enum;
- in the **namespace** for one in a namespace;
- in the **function** for one in a function.

Instance field initializers are unchanged: they run at construction, so they stay counted there (`uncovered_construct_initializer_sites`,
D25).

This departs from the engine on one point, and the departure is already decided. The engine makes a class a caller of its own definition
code, but D25 makes a class entry its **construction**. So the definition code needs another owner, and the declaring body is where it
runs.

What it does to the counters (an owner decision, it changes `policy.md`):
- `uncovered_class_definition_sites`, `uncovered_computed_member_name_sites` and `uncovered_local_class_decorator_sites` would count what
  is now drawn, so they **retire**;
- they are not qualnames, so rule 10 does not freeze them, but `typescript-v0.2.0`'s `policy.md` names them: say so in the tag notes.

### 3.4 Flat or grouped

**(recommended) Flat**, in source order, as every def is today:
- the engine's file caller is flat;
- the phase-1 rule puts an anonymous function's calls in the enclosing def, flat;
- the merged wire (`call_count`) already folds repeated targets.

Grouping (a node per anonymous function, `TreeView`'s calls under a `TreeView` node) is a better tree for a component file or a store. But:
- a group is not a call, so it is a new kind of node in core's tree (render, wire, merge);
- it is the same question for an anonymous function inside **any** def, and for Python's lambdas and closures. So it is a cross-language
  decision (rule of two), not this row's.

Record it as the next decision after row M, with the numbers of §1 (116 component sites in 7 `forwardRef` components, 124 store sites) and
row M's measure of the largest module tree.

### 3.5 Namespace owner

The engine makes a namespace with an identifier name a caller of its statements, and the outline already has `DefKind::Namespace`
(`typescript-syntax/src/outline.rs:699`), with the qualname it has had since TS1.

**[check] answered (reviewer, 2026-11-16): a namespace is already an entry, and has been since TS3.** `Sem::entry` matches any def of the
outline by qualname, of any kind, and `Collector::body_of` walks a namespace's module block (`sem/collect.rs:443`); `sem/tests.rs:430`
pins it (`expect("Ns", &["f"])`). `typescript-cli calls tests/fixtures/coverage src/namespace-class.ts --entry N` draws `key` (the
computed name of the class `N.C`) and `f` (the namespace's own statement), and counts `@dec` in `local class decorator sites`.
**So row 3c's `namespace-level` and `namespace-class-computed-name` are faults of the coverage test, not gaps:** its "every def as an entry"
is `Sem::project_defs`, which keeps `Function | Method | Class` only (`sem/mod.rs:315`), so no namespace is ever entered and
what a namespace draws or counts is never seen. The known-gap text "a namespace is not a def a request can start from, and nothing walks
it" is wrong. Real projects read 0 in both categories, so no measured number moves. What is left:
- its name is the outline's, so nothing new is named;
- it gets the definition code of what it declares (§3.3): today a class in a namespace has its computed names drawn and its decorators
  counted there, but its static initializers, static blocks and `extends` are in no tree and no counter (`nested-class-definition`);
- an ambient namespace (`declare namespace`, `declare module "x"`) has no body: an empty tree, as today;
- **[check] answered (M step 3, 2026-11-17): no gap.** A dotted namespace `namespace A.B { f(); }` is two outline defs, `A` and `A.B`; `A`
  walks nothing, `A.B` owns the statements and draws `f`. Fixture `coverage/src/namespace-dotted.ts`.

### 3.6 Static block owner

The engine makes `static { … }` a caller named `C static {}`. **(recommended) Not a def of its own.** A static block runs when its class is
defined, so §3.3 draws its calls in the declaring body (the module for a top-level class), with the class's other definition code. A def per
static block would be a new kind with a new name (`C.<static>`, with an occurrence for a second block), frozen at the tag, for code that all
three real projects have 0 of. Keep it as the engine's answer, for when a project needs to open one static block alone.

## 4. Acceptance: the coverage scan

Row 3c's test is the acceptance test, with every file (`Entry::File`) and every namespace added to "every def as an entry":

- `namespace-level` and `namespace-class-computed-name` leave the known-gap list at step 3 (the test's fault, §3.5); `top-level` and
  `top-level-anonymous` at step 2. With §3.3 accepted, `enum-member` and `nested-class-definition` leave too. The list is then **empty**.
- The test's stale-entry check makes each removal compulsory: a category no fixture hits fails.
- The fixtures' per-file numbers move from *missed* to *drawn*. Write the new numbers before the run.
- `measure_coverage` on V-NOC and both postcss inputs: **missed 0**. Any miss left is a new category, which the test already refuses unless
  the owner lists it.
- The 461 split of §1 becomes a fixture per pattern (store, `forwardRef` component, test callback, schema callback, property-assigned
  function). Each one is drawn under its file's module.

The other proofs:
- **Golden unchanged** for the five function entries (TaskDetailPanel, Processor.process, both Parser.parse, MapGenerator.generate), byte
  for byte. None of them declares a class or an enum, so §3.3 moves nothing into them. If one changes, stop and explain.
- New golden entries: the file entry of `TaskDetailPanel.tsx`, of `components/ui/tree-view.tsx`, of one store file, and of postcss's
  `container.js`. Read them by hand before blessing them.
- **[measure]** the largest module tree on V-NOC (nodes, depth, time), for the grouping decision of §3.4.
- Rule 10: no existing qualname, occurrence or outline changes (the outline snapshots are byte-identical).
- With §3.3, the policy rows whose counters retire are reworded, and their snapshots are read against the fixtures.
- Rule 5 for core's `Entry` enum: Python's and Rust's tests green.

## 5. Steps (one commit each, stop after each for review)

1. **Built** (2026-11-17, `133c6c2`; D2 `cf0eb8b`). **core**: `Entry` becomes the enum of §3.1; TypeScript's and Python's call sites and both CLIs become `Entry::Def`; Python answers `None` for `File`; TypeScript answers `None` for `File` too until step 2. Python green, Python's snapshots unchanged. **D2 of [`10-review-fixes.md`](10-review-fixes.md) §4 goes with it, as its own commit** (both change `core/`, one run of Python's tests).
2. **Built** (2026-11-17, `7e4a870`; reviewed, accepted). `Func` is `{ file, at: At }` with `At::Module | At::Def(index)`, so no code can index
   the outline without saying what it does for the module. The sweep went from 595 nodes in 42 programs to 599 in 43; `top-level` (21) and
   `top-level-anonymous` (10) are drawn, and what is left is steps 3 and 4 (`enum-member` 3, `namespace-class-computed-name` 1,
   `namespace-level` 2, `nested-class-definition` 7). Two effects that step 4 settles: the module's walk already **draws** the computed
   member names of a top-level class (`top-class.ts`: one site from counted to drawn), while the construction still counts them; and a file
   entry counts a top-level class's decorators under `uncovered_local_class_decorator_sites`, a name meant for nested classes. Both are what
   §3.3 asks for, one step early or under an old name, and both go when step 4 retires the three counters.
   **TypeScript, the file entry**: `Entry::File` in `Sem::entry` and in the coverage test's "every def", the `DefInfo` of §3.1, the body of the file's statements;
   fixtures; the coverage test drops `top-level` and `top-level-anonymous`; golden unchanged plus the new module entries.
   A file outside the root or in `node_modules` has no file entry (`None`), as R3 makes it for a def entry (D33); a test for each.
   **Also here** (reviewer, 2026-11-17, at D2's review): the coverage sweep gets one more outcome, `Counted("merged_defs")`, for a site
   whose def lost its row to another def with the same id (D34 across files; `Graph::new` counted it). That is rule 8's "counted", per
   def, so the site is accounted for, and `in-named-function` stays a bug category with nothing listed in it. Then
   `tests/limits/duplicate-marker-across/` moves back under `tests/fixtures/`, where the sweep sees it, and `tests/limits/` goes.
3. **Built** (2026-11-17, `8a997dc`; reviewed, accepted). `project_defs` keeps `Namespace`; `namespace-level` and
   `namespace-class-computed-name` left `KNOWN_GAPS` and are bug categories now; a namespace class's decorator is counted under
   `uncovered_local_class_decorator_sites` (checked by entering `N`). Sweep 599 → 600 nodes, misses 13 → 9: what is left is
   `enum-member` 3 and `nested-class-definition` 6, both step 4's. No analyzer change.
   **The coverage test enters every namespace** (§3.5, a test fix: namespaces are already entries): `project_defs` keeps
   `DefKind::Namespace` too; `namespace-level` and `namespace-class-computed-name` leave the list; `nested-class-definition`'s text loses
   "a decorator of a class declared in a namespace" (it is counted); the dotted-namespace fixture. No analyzer change. Can run before step 1.
4. **(next) Definition code in the declaring body** (§3.3, accepted 2026-11-17). How it is built (reviewer, from the code, 2026-11-17):
   - **Where.** `Collector::collect` stops at a nested def (`def_of`) and, for a class, already collects its computed member names
     (`collect.rs`, "as in tsrs"). At that stop it also collects the class's **definition code**: its modifiers (decorators), its
     members' decorators, its constructor's and methods' parameter decorators, the `extends` expression, the **static** property
     initializers and the static blocks. That is what `body_of`'s `ClassDeclaration` arm collects, **minus instance property
     initializers**, which run at construction and stay `uncovered_construct_initializer_sites` (D25). Write it once (one function both
     use), not as a second copy of the class arm.
   - **An enum is an outline def too** (`Enum E` in `typescript-cli outline coverage/src/enum.ts`), so the walk stops at it the same way;
     at that stop it collects each member's initializer.
   - **One rule for every declaring body**: the module for a top-level class or enum, the function for one in a function, the namespace
     for one in a namespace. The sites are drawn at their source position among the body's other sites.
   - **The counters retire**: `uncovered_class_definition_sites` and `uncovered_computed_member_name_sites` (noted by a construction,
     `walk::construction`) and `uncovered_local_class_decorator_sites` (`handed.rs`). Their variants go from `Uncovered`, and the
     coverage test's counted spans for them go with them. So a class entry (its construction, D25) no longer counts definition code: it
     is drawn where the class is declared. `uncovered_construct_initializer_sites` stays.
   - **The coverage test's acceptance**: `enum-member` and `nested-class-definition` leave `KNOWN_GAPS`, which is then **empty**, and
     both become bug categories. The per-file table moves from counted and missed to drawn: predict each row before the run.
   - **Snapshots that change**: only trees of a body that declares a class or an enum with definition code, by nodes added at those
     sites; every other snapshot byte-identical. Real projects read 0 for this code (§1), so their function-entry goldens should not move.
    the counters retire, `enum-member` and `nested-class-definition`
   leave the list. **This replaces row 3b**, which counted what this draws. If §3.3 is not accepted, row 3b
   stays as written and runs here. **Also here** (review, 2026-11-16): the label table in `Breakdown`'s `Display` is a second list of the
   `Uncovered` kinds, and a key it does not know is silently not printed. This step edits it anyway: each `Uncovered` gives its label
   next to its `counter()`, and the table goes.
5. **Built** (2026-11-17, `5dc6ba8`; reviewed, accepted): with no `--entry` the tree, `--graph`, `--wire` (root `target_kind` `Module`),
   `--merged` and the prefixes work as for a def entry; `--id` alone is a usage error (exit 2); a file with no file entry prints the
   not-project-code message (exit 1). Fixture `file-entry/src/twice.ts` (two calls of `h`) shows the merge; sweep 602 nodes.
   The CLI: `calls <dir> <file>` with no `--entry` is the file's own code (today it is an error). `typescript-cli outline` is unchanged.

## 6. For the owner

1. ~~§3.1~~ **decided**: core's `Entry` is an enum with `File`, for every language; no `<module>`, no `module_prefix`.
2. ~~§3.2~~ **decided (owner, 2026-11-17)**: no wire change and no version bump.
3. ~~§3.3~~ **decided (owner, 2026-11-17)**: definition code in the declaring body, which retires three counters and **replaces row 3b**.
4. ~~§3.4~~ **decided (owner, 2026-11-17)**: flat now; grouping is the next decision, cross-language.
5. ~~§3.5~~ **a fact, not a choice**: a namespace is already an entry; only the coverage test missed it (step 3).
6. ~~§3.6~~ **decided (owner, 2026-11-17)**: a static block is not a def; its calls are drawn with its class's definition code.
