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

## 2. The pipeline

pylspt's seven steps, with the ones a default resolver does not need crossed out:

```
1. sem       load, parse, bind                              TS3   (types are computed on demand, per question)
2. collect   functions and classes                          TS1's outline rule, run on the program's own trees
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
| interface method, overload signature, anything in a `.d.ts` | nothing: there is no body. A leaf |

**Where the walk stops:** at any nested node that is itself a def by TS1's rule (a named nested function, a class, an object-literal method). Its
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

implemented with the checker, because `tsrs_ls::callhierarchy::resolve_call_hierarchy_declaration` is in a private module (S-TS3, `ENGINE.md`):
`Checker::get_resolved_signature_exported(call)` → `Signature::declaration()`, else `get_symbol_at_location_exported(callee)` through
`get_aliased_symbol` → `value_declaration()`. `sem` then does what the call hierarchy did for us: it maps a matched overload signature to the
implementation, and accepts only declarations that are function-like **with a body** (the symbol path also returns parameters and variables).
`FunctionType`, `MethodSignature`, `CallSignature` and `ConstructSignature` answers have no body: on the V-NOC frontend 140 of 437 sites were a
`FunctionType` (a call through a function-typed value). Whether those are `Unknown` (→ `Unresolved`) or a distinct edge is **open, for the owner**.
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

`describe` runs `typescript_syntax::outline_of` once per file (memoized), and looks the declaration up **by its range**. If a declaration the
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
| a **union** receiver (`w: JsonWriter \| CsvWriter`) | one target per member that declares the method | `union_receiver.ts` |
| overloads | the implementation | `overloads.ts` |
| generics | the generic declaration; type arguments are ignored | `generics.ts` |
| imports, re-exports, `export default`, namespace imports | followed to the real declaration | `imports/` (a folder) |
| `new C()` | `Construct`; `init` is the class's own `constructor`. An inherited constructor: `init` is the base class's | `constructors.ts` |
| property initializers and static blocks | belong to the **class** def's body | `class_body.ts` |
| decorators | a site in the body of the **class** that holds the decorated member (or of the decorated class itself). The decorated function is treated as if it had no decorator | `decorators.ts` |
| `async` / `await`, generators | transparent; the body is walked | `async.ts` |
| a callback **parameter** being called (`function run(cb) { cb() }`) | `Unresolved`: the declaration is a parameter, not a function. This is the headline case for context sensitivity | `callback_param.ts` |
| a value of type `any` or an untyped JavaScript value | `Unresolved` | `any.js` |
| `obj[name]()` with a computed name | `Unresolved` unless the checker narrows the name to a literal | `computed.ts` |
| project code | not under `node_modules`, not a `.d.ts`, not a bundled lib | `external/` |
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
├── walk.rs       one body → Summary: the visitor of §3, `resolve(site, &env)`, the mapping of §4
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
| 1 | `sem::entry`: find a def by (file, qualname, occurrence) through `outline_of` | unit test |
| 2 | `sem::body_parts` + `walk.rs` for plain calls only; targets all `Unresolved` | `sites.ts`: the right sites, in order |
| 3 | `sem::callee` with the engine's resolver; `describe`; the bridge by range | `straight-line` and `method-on-class` scenarios pass |
| 4 | `impl Lang`, `call_graph`, `typescript-cli calls <dir> <file> --entry <qualname> [--graph]` | the CLI prints a tree for a fixture |
| 5 | `Construct`, `External`, project-code test. Write `scenarios/construct-marked-class/typescript/`: the Python program of that scenario in TypeScript, with an `ID: …` line in the JSDoc of both classes | `constructor`, `external-call` and `construct-marked-class` scenarios; a `new` of a library class is an `External` edge |
| 6 | the remaining site kinds of §3 | their fixtures |
| 7 | the policy table, row by row, each with its fixture | the `policy` test, including the "row without fixture" check |
| 8 | all scenarios at `Level::Default`; invariants; determinism | CI green on three operating systems |
| 9 | the golden run on two real projects; note load time, solve time, unresolved share in `ENGINE.md` | numbers written down |
| 10 | `docs/policy.md`; **tag `typescript-v0.2.0`** | — |

Next: [`07-ts5-driver-needs.md`](07-ts5-driver-needs.md)
