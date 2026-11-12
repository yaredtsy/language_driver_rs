# 03.08 Later: making TypeScript context-sensitive

```
later
├── 1. what the user would gain
├── 2. what pylspt built, and which parts are about Python
├── 3. the order of work, when the day comes
├── 4. what moves into core then
└── 5. what to do NOW so that day is cheap  (the checklist)
```

This file is a **direction, not a plan**. Nothing here is scheduled. It exists so that the phases before it do not close a door by accident.

## 1. What the user would gain

From TS4's policy table, the rows that context sensitivity changes:

| Today (default resolver) | With context sensitivity |
|---|---|
| a call through an interface → the interface's method, a leaf | → the method of the class that was actually passed on this path |
| a callback parameter being called → `Unresolved` | → the function that was passed |
| an anonymous callback's calls belong to the function where it is written | → they appear under the function that calls the callback |
| plain JavaScript: most method calls `Unresolved` | → resolved through the objects the walk has seen being built |

The third and fourth rows matter most for real JavaScript and for React-style code. The scenario files `expected.precise.tree` already say what
"done" looks like.

## 2. What pylspt built

pylspt's `interp/` in one table, with a judgement of how much of each file is Python:

| pylspt module | Job | About Python / ty? |
|---|---|---|
| `value.rs` | abstract values: an instance with an origin, a function, a class, a bound method, unknown | the *idea* is general; the variants hold ty handles |
| `env.rs` | names → values; object attributes; join at branches | general |
| `walk.rs` | execute statements into the env; evaluate expressions | **Python's AST, all of it** |
| `bind.rs` | match arguments to parameters | Python's calling convention (`*args`, keywords, defaults) |
| `member.rs` | attribute lookup on a value: instance dict, class, MRO | Python's object model |
| `key.rs` | fingerprint a call's inputs into a key; the policy for how deep to look | general idea; fingerprints of ty types |
| `summary.rs` | what a walk returns: calls, return values, effects, relative to the inputs | general |
| `call.rs` | one call: snapshot, look up or compute the callee's summary, apply it | general shape; constructors are Python's |
| `defer.rs`, `solver.rs` | recursion groups, the fixpoint, cuts, limits | **general** |
| `canon.rs`, `graph.rs`, `render.rs`, `tree.rs` | stable ids, the output | general; already in `analyzer-core` after the Python migration |

So roughly: the solver and the summary machinery are language-independent; the walk, binding and member lookup are the language.

## 3. The order of work

When you decide to do it, the cheapest order is the one that shows a result at each step:

| Step | Adds | `Key` | User-visible result |
|---|---|---|---|
| **C1** local values | `env.rs` filled inside one body; `resolve_from_env` answers for `const w = new JsonWriter(); w.write()` | still `()` | plain JavaScript improves; typed code is unchanged |
| **C2** show implementations | `sem::implementations(func)`; a render option | still `()` | an interface method shows its possible targets |
| **C3** arguments in the key | the walk evaluates arguments; the key holds "which class / which function was passed"; parameters start from the key | real | interface dispatch and callback parameters resolve per caller. **This is context sensitivity** |
| **C4** return values | summaries carry what a call returns (`make().process()`) | real | factories resolve |
| **C5** attributes and effects | `this.writer = w` in a constructor is seen by a later method | real | object-oriented code resolves |
| **C6** recursion fixpoint | values that flow around a recursive cycle | real | completeness; few visible changes |

C1 and C2 are the two "cheaper steps" of [`02-tree-rules/03`](../02-tree-rules/03-the-seam.md) §7. C3 is where the scenario level flips toward
`Precise`; C4–C6 are what it takes to pass all of them.

TypeScript has one advantage Python did not: **the checker already knows the declared type of everything**. So the "engine second" fallback of
rule 4 is strong, and the env only has to add what types cannot say (which implementation, which callback). The TypeScript env can be much
thinner than Python's.

## 4. What moves into core then

At C3 the flat solver is no longer enough: a callee's summary must be able to return values to its caller (C4), and recursion needs the fixpoint
(C6). By the rule of two ([`01-monorepo/03`](../01-monorepo/03-what-is-shared.md) §5), that is the moment to lift the language-independent part
of pylspt's solver into `analyzer-core`, with Python as the first user and TypeScript as the second:

```
analyzer-core::solve            today: the flat solver (Lang: walk → sites)
                                then:  + slots and recursion groups        (from pylspt solver.rs)
                                       + Summary with returns and effects, written relative to the inputs  (summary.rs)
                                       + deferred operations and replay    (defer.rs)
                                       the Lang trait grows: an associated `Value` type, and `walk` returns the richer summary
```

A default-resolver language keeps working unchanged through this, if the richer trait has defaults (`type Value = ()`, no returns, no effects).
Rust would be exactly that language at that time, which is a good test of the design.

How much of pylspt's solver separates cleanly from ty's types is **unknown until tried**. If it does not separate, TypeScript gets its own
solver modelled on pylspt's, and the core keeps only the flat one. Seam one (the `Graph`) holds either way.

## 5. What to do now

The whole point of this file. Each line is already in a phase; this is the list to check them against.

| Do now | Where | Why it matters later |
|---|---|---|
| return `Graph` with contexts, never a tree | TS4 §1 | seam one |
| `resolve(site, &env)` with an empty `Env` passed through the walk | TS4 §6 | C1 is filling a struct, not threading a parameter through 30 functions |
| `type Key = ()` in a file of its own, used by name everywhere (`Key`, never `()`) | TS4 §6 | C3 is changing one line and following compiler errors |
| the walk visits sites **in source order** and descends into anonymous functions at the point they are written | TS4 §3 | an executing walk needs evaluation order; source order is its starting point |
| one module talks to the engine | every phase | C1–C5 each add `sem` functions; none of them adds an engine import elsewhere |
| every policy row has a fixture, and the scenarios have both expected trees | TS4 §7 | changing policy is changing a fixture on purpose, not discovering a regression |
| `Stats.unresolved` and a reason counter per kind (`callback_param`, `any`, `computed`) | TS4 | tells you, with numbers from real projects, whether C1–C3 are worth doing and in which order |
| stable ids with a `digest` field, zeros for now | core | caches and the UI already handle "a function with several contexts" |

The last-but-one row deserves a second look: **measure before deciding.** After TS4 runs on real projects, the share of `Unresolved` edges and
their reasons is the business case for context sensitivity. If 95% of sites resolve in typed TypeScript, C2 may be all that is ever needed there,
and the effort belongs in JavaScript (C1) or in another language.

Next: [`../04-rust/README.md`](../04-rust/README.md)
