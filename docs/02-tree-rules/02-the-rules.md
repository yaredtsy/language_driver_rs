# 02.02 The rules

```
the rules
├── 1. the twelve rules, one line each
├── 2. rules about the analysis (1–6)
├── 3. rules about the tree (7–9)
├── 4. rules about limits, order and identity (10–12)
└── 5. the policy table every language writes
```

These are the rules pylspt's call tree follows (`plan/03-call-tree/03-design.md` and `14-output.md`), restated so that they do not mention Python
or ty. Every language in this repo follows all twelve. Where a rule has a part that only a context-sensitive solver can use, it says so.

## 1. The twelve rules

| # | Rule |
|---|---|
| 1 | **The answer is a graph of contexts. The tree is drawn from it.** |
| 2 | **A context is a function plus a key.** A summary is a function of its key and of nothing else. |
| 3 | **Calls are found by walking a body.** One body, one walk per context, in source order. |
| 4 | **Resolve from what the walk knows first, from the engine second.** |
| 5 | **A call has one of four kinds of target:** project code, a construction, external code, unresolved. |
| 6 | **Never guess, never fail.** Every step that can fail returns "no result". A partial graph is a valid answer. |
| 7 | **One tree node per call site and callee function, and whether it is called or handed over.** |
| 8 | **Recursion is shown, not entered.** |
| 9 | **Hidden means hidden with everything under it.** |
| 10 | **Precision limits and cost limits are different things.** A cost limit only cuts, is marked, is counted, and is never cached. |
| 11 | **Same input, same output.** |
| 12 | **Identity that leaves the analyzer is stable.** A language's qualname rule never changes after release. |

## 2. Rules about the analysis

### Rule 1: the answer is a graph

Covered in [`01-graph-and-tree.md`](01-graph-and-tree.md). The practical meaning: an analyzer's public function returns `Graph`. A function that
returns a tree directly is not allowed, because a tree cannot be cached, compared or expanded lazily.

### Rule 2: a context is a function plus a key

```
context  =  (function, key)
summary  =  walk(function, key)          the call sites of that body, and what each one calls
```

The key holds what the walk may know about its inputs. pylspt's key holds fingerprints of the arguments, the receiver and captured values. **A
default resolver's key holds nothing**, so there is one context per function, and the rule is trivially true.

The rule matters because it is what makes a summary reusable: two calls with the same key get the same summary, from any caller, in any order,
in a later request. pylspt's three sub-rules follow from it and apply to any language that grows a real key:

- the walk starts from what the key holds, and from nothing else;
- nothing local to one walk (an engine handle, a counter) may appear in a summary;
- which values are widened is decided by the key and the code, never by a cost limit (rule 10).

### Rule 3: calls are found by walking a body

There is no separate "collect every call expression in the project" pass. A body is walked when a context for it is asked for.

What belongs to a body is a per-language decision, written in the policy table (§5). The decision that matters most:

| Construct | pylspt (context-sensitive) | default resolver (TypeScript, Rust phase 1) |
|---|---|---|
| a named nested function | its own def; walked when it is called | the same |
| an anonymous function passed as an argument (`xs.map(x => f(x))`, a Rust closure) | a value; walked when something calls it | **its body is part of the enclosing def**: `f` is listed as called by the enclosing function, at its own source position |

The right column is what TypeScript's and rust-analyzer's own call hierarchies do, and it is the only honest choice without value tracking: the
default resolver cannot know who will call the closure, but it does know the enclosing function is where the call is written.

### Rule 4: the walk first, the engine second

pylspt's one-line design:

```rust
fn resolve(&self, expr, env) -> Targets {
    self.resolve_from_env(expr, env)                       // precise: what THIS path knows
        .unwrap_or_else(|| self.resolve_from_engine(expr)) // general: what the type checker knows
}
```

"Precision where the walk has information. The engine everywhere else."

A default resolver is this function with an environment that is always empty. It is not a different function. That is the whole trick of the
next file.

### Rule 5: four kinds of target

| Target | Meaning | In the tree |
|---|---|---|
| `Context` | a function in the project; it will be walked | a node with children |
| `Construct` | a class or struct is built. If it has a constructor function in the project, that is the `init` context | a node for the class; its children come from `init` |
| `External` | code outside the project: the standard library, a dependency, a declaration file with no body | not shown by default; a leaf when `show_external` is on. **Never entered** |
| `Unresolved` | the engine could not say | not shown by default; counted in `Stats.unresolved` |

"Project code" is a per-language test: not under a virtual environment or `site-packages` (Python), not under `node_modules` and not a `.d.ts`
(TypeScript), a workspace member and not a registry or sysroot crate (Rust).

One call site may have several targets (a union type, a trait object with a context-sensitive solver, both arms of a branch). They are several
edges with the same `site`.

### Rule 6: never guess, never fail

- A fallible step returns `Option`, not `Result`. There is no error path through the analysis, so one strange file cannot fail a request.
- A call whose target is unknown is an `Unresolved` edge. It is never dropped silently and never replaced by a "probably this one".
- A file with syntax errors is still walked as far as the engine's tree allows.
- The only things that may return an error to the caller are: the project cannot be loaded, the entry function does not exist.

## 3. Rules about the tree

### Rule 7: one node per call site and callee function, and whether it is called or handed over

When one site reaches several contexts of the **same** function (a union argument), they are one node whose `contexts` list has several entries,
and its children are the union of theirs. Without this, V-NOC's merge would count one call as two.

When one site reaches **different** functions, they are sibling nodes, ordered by where each is defined (file, then offset), never by the order
the analysis found them.

A def can also be **handed over** instead of called (`onClick={handleClose}`, `xs.map(parse)`; D22,
[`06-passed-edges.md`](06-passed-edges.md)). That edge is walked like a call and drawn as a node marked `[Passed]`; `passed` is part of what
makes two nodes one, so a def called at one site and handed over at another is two nodes, and a merged wire never counts a hand-over as a call.

Children of a node are ordered by `site`, which is source order.

### Rule 8: recursion is shown, not entered

While rendering, the renderer keeps the path of functions from the root. A callee that is already on the path becomes a leaf marked
`Recursive`. The check is by function (`StableDef`), not by context. This rule is about the *view* only; how a solver handles recursive values
(pylspt's fixpoint) is its own business.

### Rule 9: hidden means hidden with everything under it

A function that is not shown (external; or unmarked, when `only_marked` is on) is skipped together with its whole subtree. Its calls are still
in the graph. The view never "skips a level" and attaches grandchildren to a grandparent: that would show a call the code does not contain.

## 4. Rules about limits, order and identity

### Rule 10: precision limits and cost limits

| | Precision limit | Cost limit |
|---|---|---|
| examples | how deep a key looks into an argument; how many values per attribute | maximum contexts, stack depth, a deadline, a cancel request, the render's node budget |
| effect | widens a value, the same way every time | **cuts**: the result is `Incomplete` |
| may be cached | yes: it is a function of the key and the code | **no**: another run on a faster machine would see more |
| visible as | nothing (it is the policy) | `Mark::Incomplete` / `Mark::Cut`, and a counter in `Stats` |

pylspt's invariant 5: "every limit is counted. A missing edge should be explainable from a number in `Stats`, not from a debugger session."

A default resolver has no precision limits (it has no values). It has all the cost limits.

### Rule 11: same input, same output

Ten runs, ten identical outputs. What threatens it, and the answer:

| Threat | Answer |
|---|---|
| hash-map iteration order | never iterate a hash map to produce output. `Graph` is sorted by stable ids before it is returned; `analyzer_testkit::graph::check_invariants` checks it |
| the deadline | determinism tests run with the deadline off |
| threads | the tsrs checker and rust-analyzer are parallel inside; our walk over them is single-threaded and ordered |
| the engine's own order | where the engine returns "all declarations" in an unspecified order, `sem` sorts by (file, offset) before returning |

### Rule 12: stable identity

Covered in [`01-graph-and-tree.md`](01-graph-and-tree.md) §3. The part that needs discipline: a language's **qualname rule is frozen at its
first release**. It is stored in users' `.vnoc/` files. Adding a new kind of def is safe (nothing existing changes). Changing how an existing def
is named orphans every doc attached to it. So each language writes its qualname rule down *before* TS1 / RS1 / PY2, with fixtures, and the
fixtures' snapshots are the frozen rule.

## 5. The policy table every language writes

pylspt has a table "Python feature → v1 policy" (`03-design.md`). Every language writes the same table in `languages/<lang>/docs/policy.md`, and
**every row has a fixture**. Changing a row is a policy change: the fixture changes in the same pull request, and the version takes a minor step.

The rows every table must have:

| Row | The question it answers |
|---|---|
| what is a def | which syntax forms appear in the outline and can be an entry or a callee |
| what is a call site | call, `new`, operator, decorator, tagged template, JSX element, macro, property access that runs a getter… |
| anonymous functions | rule 3's table |
| branches and loops | all arms are possible; a loop body is walked once |
| dynamic dispatch | a call through an interface, a trait object, a base class: the declared method, or the implementations? |
| overloads / generics | which declaration is the target |
| aliases and re-exports | followed to the real declaration, or reported as the alias |
| constructors | what `Construct.init` is |
| decorators / attributes / macros | transparent, a call site, or invisible |
| async, generators | transparent or not |
| what is project code | rule 5's test |

The TypeScript table is in [`03-typescript/06-ts4-calls.md`](../03-typescript/06-ts4-calls.md) §4, the Rust one in
[`04-rust/02-phases.md`](../04-rust/02-phases.md). Python's exists already and moves with the solver's docs.

Next: [`03-the-seam.md`](03-the-seam.md)
