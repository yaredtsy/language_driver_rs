# 03.06 TS4: the call graph

```
TS4
├── 1. the public function
├── 2. the pipeline
├── 3. the walk: what is a call site
├── 4. the default resolver: from a site to targets
├── 5. the policy table
├── 6. the crate, file by file
├── 7. tests
└── 8. steps
```

## 1. The public function

```rust
// typescript-analysis/src/lib.rs
pub use analyzer_core::graph::{Graph, DefInfo, Context, Edge, EdgeTo, StableDef, ContextId};
pub use analyzer_core::render::{render, Node, Mark, RenderOptions, to_text};
pub use analyzer_core::solve::{Limits, Cancel};

/// The context graph under `entry`, a def in `file` (named by its outline qualname and occurrence).
/// `None` when there is no such def.
pub fn call_graph(project: &Project, file: &str, entry: &Entry, opts: &Options, cancel: &Cancel) -> Option<Graph>;

pub struct Entry<'a> { pub qualname: &'a str, pub occurrence: u16 }
#[non_exhaustive]
pub struct Options { pub limits: Limits, pub marker: MarkerStyle<'static> }
```

It returns the graph (rule 1). The tree is `render(&graph, &graph.entries[0], &render_options)`, from the core. The same two calls work for
Python and Rust, which is what lets the V-NOC drivers share their conversion code.

**The wire (D17).** What the owner consumes is the wire tree, not the graph: `wire(&graph, entry, sources, &WireOptions)` gives a `Frame`
(`target_qname`, `target_id`, `call_count`, `call_site`, `contexts`, `marks`, `children`), merged when `merged: true`. It is a view of the
graph, like the tree, so `call_graph` still returns a `Graph`. The module is written for Python first
([`05-python-migration/04-the-wire.md`](../05-python-migration/04-the-wire.md)); TypeScript is its second user, so **TS4 moves it to
`analyzer-core`** (rule 5) and `typescript-analysis` re-exports `wire`, `merge`, `WireOptions`, `WireStyle`, `Frame` and implements `Sources`.

## 2. The pipeline

pylspt's seven steps, with the ones a default resolver does not need crossed out:

```
1. sem       load, parse, bind                              TS3   (types are computed on demand, per question)
2. collect   functions and classes                          TS1's outline rule (`typescript_syntax::outline`) run on the TEXT of each program file: a second parse
3. markers   the ID comment of each def                     TS2
4. entry     one context for the requested function         key = ()
5. solve     core's flat solver: walk each body ONCE        ← TS4
6. graph     contexts + edges, stable ids, sorted           core
7. render    the tree                                       core
```

What is new code in TS4 is step 5's `walk` for TypeScript, and the bridge between a tsrs declaration and our identity.

## 3. The walk: what is a call site

`walk.rs` visits one def's body and produces its sites, in source order. It follows tsrs's own `CallSiteCollector`
(`../language_clone/tsrs/crates/tsrs_ls/src/callhierarchy.rs`, the `collect` function): the same node kinds, the same places where it does not
descend. That function is private, so we write our own, and keeping it parallel to the original makes an engine bump easy to check.

**Which parts of a def are "its body":**

| Def | Walked |
|---|---|
| function, method, accessor, arrow assigned to a `const` | parameters (default values can call things) and the body |
| class | decorators, the `extends` expression, property initializers, static blocks. **Not** its methods: each is its own def |
| constructor | parameters and body |
| namespace (`namespace N { f(); }`) | the statements of its body (executable code in a namespace, unless it is ambient); the defs inside are their own |
| interface method, overload signature, anything in a `.d.ts` | nothing: there is no body. A leaf |

**Where the walk stops:** at any nested node that is itself a def by TS1's rule (a named nested function, a class, an object-literal method). As built this is a lookup in `FileDefs::def_of`, the map of step 2a, so the rule is not written a second time. Its
calls are its own. It **does** descend into anonymous functions (rule 3).

**What is a site:**

| Node | Site? | The expression that is resolved |
|---|---|---|
| `f(x)`, `a.b(x)`, `a?.b(x)` | yes | the callee expression |
| `new C(x)` | yes, as a construction | the class expression |
| `` tag`text` `` | yes | the tag |
| `@decorator` | yes | the decorator expression |
| `<Component prop={x} />` | yes (D8) | the tag name. This is how a React component "calls" another, and TypeScript's call hierarchy counts it |
| `a.b` with no call, where `b` is a getter or setter | yes when it resolves to an accessor declaration (D8) | the property access |
| `super(x)` | yes | resolves to the base class constructor |
| `import("./m")`, `require("./m")` | no | module loading, not a call into a function |
| anything in a type position (`typeof f`, a type argument) | no | not descended into |

Each site's `Site` is the byte range of the resolved expression (not the whole call with its arguments), relative to the def's start. That is
what an editor should highlight, and it is what tsrs's call hierarchy reports.

## 4. The default resolver

One function in `sem.rs`:

```rust
pub(crate) enum Callee<'p> {
    Decls(Vec<Decl<'p>>),        // one, or several (a union type; merged declarations)
    Unknown,
}
pub(crate) fn callee(&self, site: &RawSite<'p>) -> Callee<'p>;
```

implemented with the checker. `tsrs_ls::callhierarchy::resolve_call_hierarchy_declaration` is in a private module (S-TS3, `ENGINE.md`), so `sem` carries a port of it
(`sem.rs`, module `resolve`, names and shapes kept so an engine bump can be checked line by line): from the callee expression, a declaration that is itself callable, else the
declaration of the symbol the checker finds there, once, through aliases. Three things are added to it, found in step 3 by writing the expected answer for each site before running:
(1) the engine names an overload's **first signature** (no body), and the outline's def is the implementation, so a bodiless function or method is mapped to the declaration of its own
symbol that has a body; (2) when the hierarchy finds nothing, the checker's `get_resolved_signature_exported(call)` → `Signature::declaration()` is tried, which is what resolves
`counter["bump"]()`; (3) a call on a **union** is expanded through the property of each member (`Type::types()`, `get_property_of_type_exported`), because the hierarchy gives no declaration for
it and the signature gives only the first member's, which would pass for the whole answer. The result is one `Resolved` per declaration, in (file, offset) order: a def of project code, an
`External { package, name }`, or `Unknown(why)`. `FunctionType`, `MethodSignature`, `CallSignature` and `ConstructSignature` answers have no body: on the V-NOC frontend 140 of 437 sites were a
`FunctionType` (a call through a function-typed value). Those are `Unknown` (→ `Unresolved`), and `walk` counts them in
`Stats.language["function_typed_value"]` so that they can be told apart from a call on `any` (D16). There is no distinct edge kind.
When there are several declarations, `sem` sorts them by (file, offset) before returning (rule 11).

Then `walk.rs` turns each declaration into a core target:

```
declaration
├── is its file project code?            (Sem::is_project_code: not node_modules, not .d.ts, not a bundled lib)
│     ├── no  → External { package, name }        a class too: `new Map()` is an external call
│     └── yes
│           ├── a class (or `new` was used) → Construct { class, init: its `constructor` member if it has one in project code }
│           └── anything else               → Call(func, ())           the solver will walk it
└── Unknown → Unresolved
```

The project-code test comes **first**. `Construct` is only for a class in project code: the graph describes every constructed class in
its `defs` table, and a row holds a file relative to the project root. pylspt does the same (an external class is an external target).

**The bridge.** A tsrs declaration node must become the *same* def the outline reports, so that a tree node can be joined to V-NOC's index:

```rust
impl Sem {
    /// file (relative to the root), qualname, occurrence, marker — computed by typescript-syntax's outline rule on this file's tree
    pub(crate) fn describe(&self, f: Func<'_>) -> DefInfo;
}
```

`describe` takes the file's outline from `Sem::outline` (made once per file, memoized) and finds the def of a declaration through the def ↔ node map of step 2a, keyed on the **start of the def's name** (`name_range.start`), not on its range: an overload def's range spans the signatures and the implementation, and `const f = () => {}` has the whole statement as its range, so neither matches the one node the checker hands back. If a declaration the
resolver returns is not in the outline, that is a bug in one of the two rules; a debug assertion catches it in tests, and in release the target
becomes `Unresolved` and is counted in `Stats.language["decl_not_in_outline"]`. (Rule 6: never fail. Rule 10: always count.)

`StableDef` for a def: `Marked(text)` when it has a marker, else `Path { module: "<file relative to root, with />", qname, nth }`.

**What `describe` must cover (D13, [`02-tree-rules/04`](../02-tree-rules/04-the-defs-table.md)).** The graph keeps what a definition is
in a table, `Graph::defs`, apart from its contexts. The core solver fills the table from `describe`, so:

| Do | Why |
|---|---|
| return `analyzer_core::graph::DefInfo` (`id`, `qname`, `kind`, `file`, `range`). It is the row type of `Graph::defs`; its identity field is `id` | one description type, for functions and classes |
| answer for a **class** declaration as well as a function: its outline entry, with kind `Class` | the solver calls `describe(class)` for every `Construct` target; that row is where the tree gets the class's name |
| a class that is not in the outline (a class expression passed to `new`) follows the rule above: `Unresolved`, counted | a `Construct` edge without a row breaks an invariant |
| never add a context for a class, and never build `defs` yourself | the solver writes one row per listed context and per constructed class; `check_invariants` rejects a missing or an unused row |
| read a name in a test with `graph.def(&id.func)` | a `Context` holds only `id` and `status` |

## 5. The policy table

This is `languages/typescript/docs/policy.md`. Every row has a fixture. S-TS3's output confirms or corrects each "what the engine gives" cell
before the table is frozen.

| Feature | Phase-1 policy | Fixture |
|---|---|---|
| what is a def | TS1 §2 | TS1's |
| what is a call site | §3 above | `sites.ts` |
| anonymous functions | their calls belong to the enclosing def, at their source position | `callbacks.ts` |
| named nested functions | their own def; appear where they are **called**, not where they are declared | `nested_calls.ts` |
| `if`, `switch`, `?:`, `&&`, loops, `try` | every call expression in the body is a site, whatever branch it is in. No condition is evaluated | `branches.ts` |
| a call on a **class** instance (`w: JsonWriter`) | that class's method (or the inherited one, where it is declared) | `method_on_class.ts` |
| a call through an **interface** or abstract method (`w: Writer`) | the interface's method signature: a leaf with no body. **Not** the implementations | `interface_dispatch.ts` |
| a **union** receiver (`w: JsonWriter \| CsvWriter`) | one target per member that declares the method. **Not the engine's answer** (its call hierarchy gives none, its signature gives the first member only): `sem` asks each member. A member with no declaration to enter adds `Unknown(union_receiver)`, so the answer is never mistaken for complete | `union_receiver.ts` |
| overloads | the implementation | `overloads.ts` |
| generics | the generic declaration; type arguments are ignored | `generics.ts` |
| imports, re-exports, `export default`, namespace imports | followed to the real declaration | `imports/` (a folder) |
| `new C()` | `Construct`; `init` is the class's own `constructor`. An inherited constructor: `init` is the base class's | `constructors.ts` |
| property initializers and static blocks | belong to the **class** def's body | `class_body.ts` |
| decorators | a site in the body of the **class** that holds the decorated member (or of the decorated class itself). The decorated function is treated as if it had no decorator | `decorators.ts` |
| `async` / `await`, generators | transparent; the body is walked | `async.ts` |
| a callback **parameter** being called (`function run(cb) { cb() }`) | `Unresolved`: the declaration is a parameter, not a function. This is the headline case for context sensitivity | `callback_param.ts` |
| any other **function-typed value** being called (a prop, a `setState`, a store action, a field of type `() => void`) | `Unresolved`, like the callback parameter: the engine answers with a `FunctionType`, which is a type and has no body. Counted in `Stats.language["function_typed_value"]` (the callback parameter too) (D16) | `function_typed.ts` |
| a value of type `any` or an untyped JavaScript value | `Unresolved` | `any.js` |
| `obj[name]()` with a computed name | `Unresolved` (`computed_name`) unless the checker narrows the name to a literal, in which case it is resolved (`counter["bump"]()`: through the checker's signature, not the call hierarchy, which finds nothing for an element access) | `computed.ts` |
| project code | not under `node_modules`, not a `.d.ts`, not a bundled lib. A call that resolves into the **repo's own** `.d.ts` is `External`, not a def (TS3 pinned that such a file is absent from `files()`; this is the half that needs call resolution) | `external/` (include a repo `.d.ts`) |
| an external call | an `External` edge with the package name; never entered | `external/` |
| a `.d.ts` beside its `.js` in the project | the `.d.ts` wins in the checker; the target is `External`. **[check]** with S-TS3 whether this is common in your target projects | `dts_shadow/` |

## 6. The crate, file by file

```
typescript-analysis/src/
├── lib.rs        the facade of §1
├── sem.rs        TS3's, plus: entry, body_parts, callee, describe, site ranges
├── project.rs    TS3
├── env.rs        pub(crate) struct Env;                       ← empty on purpose (02-tree-rules/03 §3)
├── key.rs        pub(crate) type Key = ();                    ← empty on purpose
├── walk.rs       one body → Summary: `sem`'s sites put in the solver's terms (relative to the def), `resolve(site, &env)`, the mapping of §4. The visitor of §3 itself is `Collector` in `sem.rs`
└── calls.rs      `impl Lang for TypeScript` (describe, digest → zeros, walk); `call_graph` = Solver::new(..).graph(..)
```

`calls.rs` is short, because the solver is in the core:

```rust
pub fn call_graph(project: &Project, file: &str, entry: &Entry, opts: &Options, cancel: &Cancel) -> Option<Graph> {
    let lang = TypeScript { sem: &project.sem, marker: opts.marker };
    let func = lang.sem.entry(file, entry.qualname, entry.occurrence)?;
    let mut solver = Solver::new(lang, opts.limits.clone());
    Some(solver.graph(&[(func, ())], cancel))
}
```

### 6a. The proof rule for handles

`sem` hands out handles that wrap tsrs's `'static` pointers (`File` in TS3, `Func` in TS4, and any later one: a class, a site, a declaration). The compiler cannot see the
program being freed, so **every handle type is written together with its proofs, in the same commit and before any code uses it**. The proofs are `compile_fail` doctests in
`typescript_analysis::proof` (behind the feature `__proof`; `scripts/check-proofs.sh` runs and counts them against `proof-doctests.txt`):

| Proof | A `compile_fail` that must fail | Its twin, which must compile |
|---|---|---|
| lifetime | `drop(sem)` while the handle is used afterwards | the same two lines swapped |
| not `Send` | the handle moved into a spawned thread | the same closure called on this thread |
| not `Sync` | `&handle` used inside a spawned thread | the same closure called on this thread |

and for `Sem` itself: not `Sync` (shared `&Sem` into a thread fails); it stays `Send`, because a `Project` is opened on one thread and dropped on another (`tests/threads.rs`).

How to write one, which is how the TS3 proofs were made:

1. **Red first.** Write the doctest, run `scripts/check-proofs.sh`, and see it fail for the right reason (the compile_fail "did not fail"), then add the marker or the lifetime and see it pass.
2. **Mutate until it fails.** Remove the marker, or give the constructor an unbound lifetime (`fn f<'p>(&self) -> H<'p>`), and the proof must fail. Where one property has several routes, remove them all: since `Sem` is not `Sync`, `&'p Sem` is not `Send`, so every handle that carries `PhantomData<&'p Sem>` is neither `Send` nor `Sync` even without a marker of its own, and the `Func` thread proofs fail only when both `Sem`'s marker and the handle's are gone. Keep every marker anyway: each is a separate guard, and a proof must not depend on the other one staying. A proof that passes under that mutation protects nothing
   (a first version that went through a wrapper function passed it: the wrapper's own signature re-tied the borrow). Use the real `Sem` and the real handle, not a wrapper.
3. **The twin is the guard on stable.** Stable rustdoc ignores `compile_fail,E0xxx` (checked), so the pinned code is documentation only; the twin differs from the failing example only in
   the ordering or the spawn, so a typo cannot hide behind the failure.
4. **The count moves with the code.** Add the pair and raise `proof-doctests.txt` in the same commit.
5. **A handle that must be `Send` or `Sync` is a decision for the owner**, written here with the reason. The default is neither, because one checker answers one question at a time.

## 7. Tests

| Test | What |
|---|---|
| `scenarios` | `analyzer_testkit::scenarios::run(&TS, Level::Default)`: every folder in the repo's `scenarios/` |
| `policy` | one small project per row of §5 under `tests/policy/`; the rendered tree is a snapshot. A row without a fixture fails a test that reads `docs/policy.md` and looks for the named file |
| `sem_contract` | one test per `sem` function, on tiny inputs: these are what goes red on an engine bump |
| `invariants` | `check_invariants` on every graph the other tests build; each scenario run twice and compared (rule 11) |
| `memory` | TS3's loop, now building a graph each round |
| a real project, by hand | `typescript-cli calls` on S-TS3's two projects; keep the output outside git as the golden run for later engine bumps (the PA plan does the same with `cli-wire.json`) |

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `sem::entry`: find a def by (file, qualname, occurrence). The outline is `typescript_syntax::outline` run on the file's own text (a second parse, in a region of its own; there is no `outline_of` on a tree, see [05](05-ts3-project.md) §5). `occurrence` is 0-based, the number `StableDef::Path::nth` holds (D18). **Done** | unit test |
| 2a | the def ↔ node map. `typescript_syntax::outline_with_nodes` reports, with each def, the span of the node it names (one walk, so the def rule is written once); `Sem::file_defs` finds the program's node with that span and a kind a def can name, and the way back (`def_of`). Keyed on the **node span**, not on `name_range.start` (that would need a second, kind-by-kind rule in this crate). **Done** | every def of the TS1 fixture suite (44 files, 292 defs) maps to exactly one node and back; mutation-checked |
| 2b | the plain calls of a body are its sites, in source order, relative to the def's start. **Done.** `Sem::call_sites` (the traversal, in `sem.rs`: it is the only file that touches tsrs nodes, and it returns plain spans, so there is no `Node` handle to prove) and `walk::sites`. Targets are not attached yet: they come with `Lang` in steps 3 and 4 | `sites.ts`: the right sites, in order, expected lists written from the rules before the run; mutation-checked |
| 3 | `sem::callee(func, site_index)` with the engine's resolver (`sem` keeps each def's call nodes privately; an index is plain data, so no handle and no new proofs, and `resolve(site, &env)` stays in `walk.rs`); `describe` (on `FileDefs`; **fix first**: `Sem::file_defs` allocates a `String` per lookup and keys on the label only, ignoring `placement`, and `Sem::def` clones a whole `Def`, and `describe` calls both once per def); `callee` finds the call node of a site again from its span, or `Sem` keeps the call nodes per def **Done** (`callee`, `describe`, the call nodes kept per def). Reasons for an `Unknown`, one counter each: `function_typed_value`, `any_or_untyped`, `computed_name`, `union_receiver`, `decl_not_in_outline`, `no_declaration` | a `sem_contract` test on a small fixture: for every site of every def, the resolved `(file, qualname)` (or `Unknown`), listed in full and written from the language rules before the run. The `straight-line` and `method-on-class` scenarios are **step 4**'s proof, where a graph exists to run them (`scenarios/` has no `method-on-class`, and `straight-line` has only a Python program: both TypeScript programs are written in step 4) |
| 4 | `impl Lang`, `call_graph`, `typescript-cli calls <dir> <file> --entry <qualname> [--id <marker>] [--graph]`. A qualname is not unique in a file (D15): when `--entry` matches several defs, the CLI prints every match (kind, line, marker) and asks for `--id`; it never picks the first. With `--id`, the def carrying that marker is the entry **A def with no node in the program's tree** (`FileDefs::nodes` is `None`: a bug in one of the two parses) loses its sites, so it is counted in `Stats.language["def_without_node"]` and its context is marked, never silently empty (rule 8). **The output ends with the breakdown** (the first `calls` output already has it, not a later addition): edges by kind (call, construct, external, unresolved); `unresolved` split by reason, every `Unresolved` carrying exactly one reason counter in `Stats.language` (`function_typed_value`, `any_or_untyped`, `computed_name`, `union_receiver`, `decl_not_in_outline`, `no_declaration`, and `def_without_node` for a def whose node is missing); the cuts. So "why is this unresolved" is a number and not a read of the code, and step 9's unresolved share comes from the same place | the CLI prints a tree for a fixture; an ambiguous `--entry` prints the list and exits 2; **the reasons sum to `Stats.unresolved`** (checked on every graph the suite builds), and the fixture's breakdown is a snapshot |
| 4a | the wire (D17): move `wire.rs` from `python-analysis` to `analyzer-core` unchanged (needs W3 of [05/04](../05-python-migration/04-the-wire.md) done); `Sources` for `Project`; `typescript-cli calls … --wire [--merged] --function-prefix … --class-prefix …` | Python's wire tests still pass from core; a fixture's wire has a `target_id` with the passed prefix for a marked def and `""` for an unmarked one; `--merged` turns two sibling calls of one function into one node with `call_count: 1` |
| 5 | `Construct`, `External`, project-code test. Write `scenarios/construct-marked-class/typescript/`: the Python program of that scenario in TypeScript, with an `ID: …` line in the JSDoc of both classes | `constructor`, `external-call` and `construct-marked-class` scenarios; a `new` of a library class is an `External` edge |
| 6 | the remaining site kinds of §3 | their fixtures |
| 7 | the policy table, row by row, each with its fixture | the `policy` test, including the "row without fixture" check |
| 8 | all scenarios at `Level::Default`; invariants; determinism | CI green on three operating systems |
| 9 | the golden run on two real projects; note load time, solve time, unresolved share in `ENGINE.md` | numbers written down |
| 10 | `docs/policy.md`; **tag `typescript-v0.2.0`** | — |

Next: [`07-ts5-driver-needs.md`](07-ts5-driver-needs.md)
