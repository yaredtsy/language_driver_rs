# 02.01 The graph and the tree

```
graph and tree
├── 1. three things that are easy to mix up
├── 2. the graph: the types in analyzer-core
├── 3. stable identity
├── 4. the tree is a view
└── 5. what changed from pylspt's types
```

This file is pylspt's `plan/03-call-tree/14-output.md` with the Python taken out. It is the contract every language's `<lang>-analysis` crate
fulfils.

## 1. Three things that are easy to mix up

```
   call graph                    context graph                       call tree
   ──────────                    ─────────────                       ─────────
   main ─► read_json ─► emit     main ─► read_json ─► emit[Json] ─► JsonWriter.write   main
     └───► read_csv ──┘  │         └───► read_csv ──► emit[Csv] ──► CsvWriter.write    ├── read_json
                         ▼                                                            │   └── emit
                       write                                                          │       └── JsonWriter.write
                                                                                      └── read_csv
   one node per function         one node per (function, context)                         └── emit
   "what exists?"                "what exists, told apart by input"                           └── CsvWriter.write
                                                                                      one node per call on a path
                                                                                      "what happens on this path?"
```

- A **call graph** is what an engine's call hierarchy gives. Each function once.
- A **context graph** is what an analyzer in this repo returns. Its nodes are *contexts*: a function together with what the analysis knew about
  its inputs.
- A **call tree** is what V-NOC shows. It is exponential in depth, so it is never the stored answer: it is drawn from the graph, with a budget.

The important sentence: **a call graph is a context graph in which every function has exactly one context.** So a language with a default
resolver and a language with a context-sensitive solver return the *same type*. The only difference is how many contexts a function has.

## 2. The graph

```rust
// core/analyzer-core/src/graph.rs
#[non_exhaustive]
pub struct Graph {
    pub entries:  Vec<ContextId>,
    pub defs:     Vec<DefInfo>,        // one row per function or class the graph mentions; sorted by id
    pub contexts: Vec<Context>,        // everything reachable from the entries; sorted by id
    pub edges:    Vec<Edge>,           // sorted by (from, site, to)
    pub stats:    Stats,
}

/// What a definition is. A context is an analysis of one of these; a constructed class is one too, and has no context.
#[non_exhaustive]
pub struct DefInfo {
    pub id:    StableDef,
    pub qname: String,                 // the language's qualified name, for display
    pub kind:  DefKind,
    pub file:  String,                 // path relative to the project root, with `/`
    pub range: Span,                   // the whole definition, bytes
}

#[non_exhaustive]
pub struct Context {
    pub id:     ContextId,             // id.func is the key into `defs`
    pub status: Status,                // Finished · Incomplete(why)
}

#[non_exhaustive]
pub struct Edge {
    pub from: ContextId,
    pub site: Site,                    // the call expression: a byte range RELATIVE to the start of `from`'s function
    pub to:   EdgeTo,
}

#[non_exhaustive]
pub enum EdgeTo {
    Context(ContextId),                                       // a call into project code
    Construct { class: StableDef, init: Option<ContextId> },  // `Foo()`, `new Foo()`, `Foo { .. }` with a constructor function
    External(External),                                       // a library or the standard library: a named leaf
    Unresolved,                                               // there was a call; we do not know the target
}

pub struct External { pub package: String, pub name: String }
```

Why `site` is relative to the function: an edit *above* a function moves every absolute offset in it, but not the offsets inside it. A relative
site stays the same, so a cache keyed by it survives the edit. To show the call in an editor, add the range start of the function's `DefInfo` (`graph.def(&edge.from.func)`).

The graph describes definitions in a table of their own, not on the contexts: a function with several contexts has one row, and a class
that is constructed has a row and no context. `ContextId::func` and `Construct::class` are both keys into `defs`
([`04-the-defs-table.md`](04-the-defs-table.md) has the reasons and the four invariants).

`Stats` is pylspt's `Stats` reduced to what every language can count: `walks`, `contexts`, `reuse_hits`, `cuts` (by kind), `unresolved`. A
language may add its own counters in a `language: BTreeMap<String, u64>` field. Rule 10 in the next file says why every limit must be counted.

## 3. Stable identity

Inside one request, the analyzer uses the engine's own handles (a ty `Definition`, a tsrs node pointer, a rust-analyzer `Function`). None of them
means anything in the next request. Everything that leaves the analyzer uses identities that do:

```rust
pub enum StableDef {
    /// The def carries a marker. This is its text, exactly as written. Survives renames and moves.
    Marked(String),
    /// No marker. Named by where it is.
    Path { module: String, qname: String, nth: u16 },
}

pub struct ContextId {
    pub func:   StableDef,
    pub digest: [u8; 16],        // a hash of the context's key. All zeros = "the function, whatever was passed"
}
```

- `module` is whatever names a file in that language: a dotted module in Python, a project-relative path in TypeScript, a crate path in Rust. The
  core never looks inside it.
- `nth` separates two definitions with the same qualname in one module, in source order.
- `digest` is **only** for identity outside the process: a cache, a diff of two runs, the UI keeping a selection. In memory, keys are compared in
  full. A language with a default resolver always writes zeros.

> **🦀 Rust note: why a 16-byte array and not a `String`.** `[u8; 16]` is `Copy`, is compared with one instruction, and is sorted byte by byte,
> which gives the same order on every platform. pylspt computes it with `xxh3` (128-bit) over a canonical encoding of the key; the reason given in
> its `Cargo.toml` is that "stable ids must not change across Rust versions or platforms", which rules out `std`'s `Hasher` (its algorithm is
> allowed to change). Serialize it as 32 hex characters.

## 4. The tree is a view

```rust
// core/analyzer-core/src/render.rs
#[non_exhaustive]
pub struct Node {
    pub func:     StableDef,
    pub qname:    String,
    pub site:     Option<Site>,          // None for the root
    pub contexts: Vec<ContextId>,        // which graph contexts this node stands for
    pub marks:    Vec<Mark>,
    pub children: Vec<Node>,
}

pub enum Mark {
    Recursive,     // this function is already on the path above: shown as a leaf, not entered
    Cut,           // the node budget or depth limit ran out here: children are partial
    Incomplete,    // a context this node shows has a gap (a cost limit was hit while solving it)
    External,      // a library function: a leaf by definition
    Unresolved,    // a call with no known target (only when RenderOptions::show_unresolved)
}

pub struct RenderOptions {
    pub max_nodes: u32,              // 100_000 in pylspt
    pub max_depth: Option<u32>,      // none by default: the node budget is the guard
    pub only_marked: bool,           // hide a function with no marker, and everything under it
    pub show_external: bool,         // off: externals are in the graph, not in the tree
    pub show_unresolved: bool,
}

pub fn render(graph: &Graph, entry: &ContextId, opts: &RenderOptions) -> Node;
pub fn to_text(root: &Node) -> String;       // one line per node, two spaces per level: the format of the scenario files
```

The renderer's rules (one child per call site and callee, recursion as a leaf, cuts marked) are rules 7 to 9 in the next file.

What the V-NOC driver sends is built from `Node`. V-NOC's own merge (`call_count`, siblings with one name become one node) stays where the
2026-10-27 decision put it: in V-NOC, when saving. The analyzer always sends one node per call site.

## 5. What changed from pylspt's types

| pylspt | analyzer-core | Why |
|---|---|---|
| `GraphOut`, `ContextOut`, `EdgeOut` | `Graph`, `Context`, `Edge` | shorter; nothing else is called `Graph` here |
| `StableContextId` | `ContextId` | pylspt's in-memory `ContextId` (a counter) stays private to `python-analysis` under another name |
| `StableDef::Node(String)` | `StableDef::Marked(String)` | "this def carries a marker" says no product (PA plan `02-target/02` §3) |
| `ContextOut` held the name, file and range | `DefInfo`, one row per definition | a class is constructed and never walked, so it needs a description without a context ([`04-the-defs-table.md`](04-the-defs-table.md)) |
| `ContextOut::target_id: "FunctionSchema/<id>"` | removed | product. The driver builds it from `StableDef::Marked` |
| `Span { start: (line, col), end }` on an edge | `Site` relative bytes on the edge; `file` + `range` on the def's row in `defs` | the wire's line and column rule (UTF-16) is the driver's; bytes are what all three engines give |
| `EdgeTo::External(StableDef)` | `EdgeTo::External(External { package, name })` | the V-NOC driver contract asks for `{ package, name }` on external leaves (`09/02` §4) |
| `StableType` | stays in `python-analysis` | it encodes ty's types inside a key. No other language has keys yet |
| `FrameJson`, `merge_for_output` | not moved | V-NOC's stored shape (PA5) |
| `Node { qname, site, marks, children }` | the same, plus `func` and `contexts` | so a consumer can join a node to an outline entry and to the graph |
| `Mark::{Recursive, Cut, Incomplete}` | the same three, plus `External` and `Unresolved` | only produced when the two `show_*` options are on; off by default, so Python's output is unchanged |

Next: [`02-the-rules.md`](02-the-rules.md)
