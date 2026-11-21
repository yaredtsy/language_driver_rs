# 08.07 The value layer: the design (closures, returns, factories, constructor fields)

```
the value layer
├── 1. what the owner decided, and what Python really does
├── 2. one program, before and after
├── 3. values                                               (BV-D1)
├── 4. the body as data: `Flow`                             (BV-D2)
├── 5. evaluating a body                                    (BV-D3)
├── 6. where a lambda's calls are drawn                     (BV-D4, D54)
├── 7. returns, factories, partial application              (BV-D5)
├── 8. `do` blocks                                          (BV-D6)
├── 9. constructor fields                                   (BV-D7)
├── 10. the key, the digest, the caps                       (BV-D8)
├── 11. what changes, and what does not
├── 12. the rows
├── 13. the forecast on the demos, site by site
├── 14. the proof
├── 15. steps for the coder, in four phases
└── 16. the decision rows
```

Written 2026-11-21, the day BC1+BC2 was finished and measured ([`06-context-layer-design.md`](06-context-layer-design.md), `languages/bend/ENGINE.md`
§BC1+BC2). **Nothing here was built or run when it was written.** It replaces §5 of [`04-phase-3-context-layer.md`](04-phase-3-context-layer.md)
(BC3 to BC5, "not worth building today"): the owner has asked for them.

## 1. What the owner decided, and what Python really does

Three decisions, 2026-11-21, recorded as D53, D54 and D55:

1. **The layer is on by default.** "That is the main feature."
2. **A lambda's calls are drawn under the function that calls it** (Q-BD9: yes; it is also Q-TC1's and Q-RC1's answer, for the day those layers reach it).
3. **The layer follows values as Python's does: returns, closures, callbacks, factories, where Bend has the thing.**

A correction before the design, because this plan said it wrong three times (the scenario's README, 06 §11, `ENGINE.md`): **Python's interpreter
does not follow a `lambda`.** `interp/walk.rs` evaluates `Expr::Lambda` to `Unknown`. What it follows is a **nested `def`**: a function with a name,
a row in the outline and a node of its own in the tree, as the value `Closure { func, activation }`. So "designed like Python" has two halves here:

| Python has | Bend's form | This design |
|---|---|---|
| a function as a value, passed, stored in a variable, returned | the same: a def by name | built for arguments (BC1+BC2); §5 and §7 add variables and returns |
| a closure: a nested `def` and the variables of the run that made it | a **lambda** and the binders it reads. Bend has no nested def | §3, §6. Further than Python goes: its lambda is unknown |
| a factory: a def that returns a function or an object holding functions | the same; and **partial application**, which Python does not have (`U32.add(2)` is a closure, the guide says) | §7 |
| an object's attributes, set by its constructor | a constructor's fields, and nothing can write them afterwards | §9: far smaller than Python's (no identity, no aliasing, no effects) |
| recursion solved by a fixpoint over pending values | the same need, for return values | §7, small: sets only grow and are capped |
| a callback | done (BC1+BC2) | |

What is **not** copied is Python's code: it is welded to its engine's types and most of it handles mutation. The rules are taken; the 400 lines
they need here are Bend's own.

## 2. One program, before and after

```bend
def work(x: U32) -> U32:
  x

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def pick(b: Bool) -> U32 -> U32:          # a factory
  match b:
    case True{}:
      work
    case False{}:
      other

def main() -> U32:
  a = each(x => work(x), 1)               # a lambda as an argument
  g = pick(True{})                        # a function from a call, kept in a variable
  b = g(a)                                # ... and called
  App{v, t} = App{work, other}            # a constructor's fields
  t(b)
```

| Line | BC1+BC2 (today, `Precise`) | The value layer |
|---|---|---|
| `each(x => work(x), 1)` | `main` ▸ `each` · `work` (the lambda's call is `main`'s) | `main` ▸ `each` ▸ `work` (phase 1) |
| `g = pick(True{})`, `g(a)` | `main` ▸ `pick`; `g(a)` unresolved | `main` ▸ `pick`, then `work` and `other` at `g(a)`: `pick` returns either, no condition is evaluated (phase 2) |
| `App{v, t} = App{work, other}`, `t(b)` | `work [Passed]`, `other [Passed]`; `t(b)` unresolved | the passed edges stay; `other` at `t(b)` (phase 3) |

## 3. Values

```rust
// value.rs (new). `key.rs`'s `Known` becomes these.
/// One thing a place can hold.
pub(crate) enum One {
    Def(DeclId),                                          // BC1+BC2: a project def with code
    Lambda { writer: DeclId, nth: u32 },                  // BC1+BC2: a lambda that STAYS with its writer (§6): known, never entered
    Closure { writer: DeclId, nth: u32, captures: Captures },   // phase 1: a lambda that MOVES: entered where it is applied
    Partial { head: Box<One>, args: Box<[Value]> },       // phase 2: a def or a closure applied to fewer arguments than it takes
    Ctor { ty: DeclId, ctor: u32, fields: Box<[Value]> }, // phase 3: a constructor of a project type and what it was given
}

/// What can be at a place: these, and, when `open`, something the layer does not know too.
pub(crate) struct Value { known: Box<[One]>, open: bool }     // sorted, no duplicates, at most `max_set`

/// What a lambda read from its writer when it was made: the writer's binders it mentions, by number, with a value worth carrying. Sorted.
pub(crate) type Captures = Box<[(u32, Value)]>;
```

**BV-D1.** Three named values: `Value::unknown()` is no member and `open` (ask the book, as layer 1); `Value::data()` is no member and closed (a
number, a string, a Base constructor, a type: nothing callable is there, and nothing is lost by saying so); every other value lists what can be
there. The shape is TypeScript's (03-design §3: a set that says whether it is complete), because Bend now has a join: the cases of a `match`.
A value is **closed**: it names defs and lambdas by identity and carries its captures with it, so it means the same in a key.

> **🦀 Rust note: a recursive `enum` needs a `Box`.** `One::Partial` holds a `One`, and `Value` holds `One`s that hold `Value`s. A type that
> contained itself directly would have no finite size, and Rust must know every type's size to put it on the stack. `Box<T>` is a pointer to a `T`
> on the heap: its size is one word whatever `T` is, so the cycle is broken. `Box<[T]>` is the same idea for a run of values: a pointer and a
> length. Both own what they point to, so `#[derive(Clone, PartialEq, Eq, Hash)]` works straight through, which is what lets a `Value` be part of
> a key.

## 4. The body as data: `Flow`

Today `sem` hands `walk.rs` a flat list of places (`Item`). A list cannot be evaluated: it has no `let`, no `match`, no body of a lambda. So
`sem` also hands back the body as a small owned tree (TypeScript's `Flow` and Rust's `sem.flow(f)` are the same idea; two languages with the same
table is what the rule of two waits for, and nothing moves to core before that):

```rust
// sem/flow.rs (new): owned data, no `bend_parser` type. Memoized per file beside the walks.
pub(crate) struct Flow {
    pub params: u32,              // binders 0..params are the def's parameters, in written order
    pub lambdas: Box<[LamFacts]>, // by `nth`: how many binders its chain has (`w => b => body` is 2), which outer binders it reads
    pub body: Expr,
}
pub(crate) enum Expr {
    Unknown,                                              // a term the layer does not model
    Data,                                                 // a literal, Base's sugar on Base, a type
    Var(u32),                                             // a binder of this def, by number
    Name { item: Option<u32>, name: String },             // an unbound name; `item` is its Passed/Deep place when it has one
    Lam { nth: u32, binders: Box<[u32]>, body: Box<Expr> },
    Call { item: u32, head: Box<Expr>, args: Box<[Expr]> },   // as written: `f(a, b)` is one node with two arguments
    Ctr { item: u32, name: String, fields: Box<[Expr]> },
    Sugar { item: u32, operands: Box<[Expr]> },           // an operator or an index on a project type: its call, operands walked
    Bind { item: u32, monad: String, types: Box<[Expr]>, value: Box<Expr>, rest: Box<Expr>, binder: u32, pure: bool },  // a `do` statement
    Let { pats: Box<[Pat]>, values: Box<[Expr]>, rest: Box<Expr> },
    Match { on: Box<[Expr]>, cases: Box<[(Box<[Pat]>, Expr)]> },
    Seq(Box<[Expr]>),                                     // terms read for their sites; the value is the last one's
}
pub(crate) enum Pat { Bind(u32), Ctr { name: String, fields: Box<[Pat]> }, Opaque(Box<[u32]>), Skip }
```

**BV-D2. Every node that is a site carries the index of its `Item`.** The item table stays the one place that says where a site is and what
layer 1 makes of it. The evaluator never invents a site: it decides the **targets** of items. That is what makes "knows nothing ⇒ layer 1"
checkable site by site (§14), and it keeps the counters about the code (`type_position`, `sugar_base`, `passed_deep`, …) out of the evaluator:
they are counted from the items, once per def, whether or not a lambda's body is entered somewhere else.

## 5. Evaluating a body

`Precision::Precise` walks a def by evaluating its `Flow` under its key. `Precision::Default` keeps the item walk of layer 1, untouched, as the
comparison every measurement is made against.

```
eval(expr, frame) -> Value            frame = the def whose text is being read, its binders' values, and where sites go (§6)

Data, Unknown        the value of the same name
Var(i)               frame.env[i], or unknown
Name                 the book: a project def with code → {Def}; a type, a law alone → data; Base's, a hub's, outside the root → unknown
Lam                  §6: it moves → {Closure} with its captures, body not read here; it stays → body read here, binders unknown, value {Lambda}
Let                  values, then patterns (below), then the rest
Match                the scrutinees; every case, with its patterns bound; the value is the JOIN of the cases. No condition is evaluated,
                     no case is pruned: a case that cannot match still shows its calls, as layer 1 shows them
Ctr                  the site as layer 1 (a Construct of a project type); the value is {Ctor} with its fields' values (phase 3)
Sugar                the site as layer 1, under the open key (BC-D5 stands for operators and indexes); value data
Bind                 §8
Call(head, args)     the arguments' values, then  apply(eval(head), values)  at this item
```

**`apply(v, args)` at an item**, one answer per known member of `v`, and the book's own answer beside them when `v` is open (rule 6 of 06 §7):

| The head is | The site gets | The value is |
|---|---|---|
| a project def `g` with `n` written parameters, `args.len() == n` | `Call(g, key)`, the key from the arguments (§10) | `ret(g, key)` (§7); unknown before phase 2 |
| ... `args.len() > n` | the `Call`, and what applying its result to the rest gives, at the same item | that application's value |
| ... `args.len() < n` | a **passed** edge to `g` under the open key (§7); before phase 2, the `Call` layer 1 draws | `{Partial}`; before phase 2, unknown |
| a `Closure` | **the calls its body makes**, read in a frame of its captures and its binders bound to the arguments (§6) | its body's value |
| a `Lambda` | `Unresolved(anonymous_function_called)`: known, drawn where it is written | unknown |
| a `Partial` | its arguments first, then as its head | |
| a `Ctor`, data | `Unresolved(function_typed_value)`: nothing callable is there | unknown |
| unknown, and the head is a name of Base, a hub, outside the root | the `External` of layer 1 | unknown |
| unknown otherwise | `Unresolved(function_typed_value)`, as layer 1 | unknown |

**Patterns.** `Pat::Bind(i)` takes the value. `Pat::Ctr` takes the fields of a value that is exactly one `Ctor` of that name, closed; anything
else binds its names to unknown (phase 3; before it, every name of a constructor pattern is unknown). `Pat::Opaque` (a list, a tuple, `3n+p`,
`h <> t`) binds its names to unknown.

**BV-D3. A variable has the value of its binding.** Bend has no assignment: `x = v` binds a new `x`, so there is no join over writes, no order of
execution to model and no "assigned twice" row. This is the whole of what Python's `Env` and its writes come to here.

## 6. Where a lambda's calls are drawn

**BV-D4 (D54).** A lambda **moves** to the function that calls it when the text guarantees that function is walked, here, with the lambda in hand:

> a lambda written as a **direct argument** of a call whose callee is a **project def by name**, when the call gives that def **all its written
> parameters**, at a position the def **applies**.

`applies(g)` is a fact of `g`'s code alone, the least set such that position `i` is in it when `g` calls parameter `i` (anywhere in its body, the
bodies of its lambdas included), or passes parameter `i` as a direct argument, in a call that gives all the written parameters, to a position in
`applies` of a project def by name. It is 06 §3.2's "called" closed over passing along: a small fixpoint over the defs involved, sets that only
grow. One corner is known and pinned (row `recursion-keyed`): a def that applies its parameter only inside a lambda it passes to itself never
runs that application, and the layer draws none, where layer 1 listed an unresolved site.

Every other lambda **stays** with the def that writes it, exactly as today: one handed to Base or a hub (`List.map(~(x => f(x)), xs)`: Base runs
it), to a sugar, to a call through a variable, to a def that does not apply that parameter or is given too few arguments; one bound by a `let`,
put in a constructor, or returned; the continuation of a `do` statement (§8).

| | It moves | It stays |
|---|---|---|
| its value | `Closure`, with what it captured | `Lambda`, as BC1+BC2 |
| its body is read | **where it is applied**: its calls are targets of the applying site, in the applying context, with its binders bound to that call's arguments | **where it is written**, binders unknown: layer 1's picture |
| applied somewhere | drawn there, once per site that applies it (`h(h(x))` draws it twice, as it runs twice) | the site says `anonymous_function_called` |
| a lambda inside it | read when its body is read, by the same rule: it moves or it stays, relative to that body | |
| a bare def name handed over inside it (`[Passed]`) | **stays where the name is written**, in the writer, under the open key: D22 draws a hand-over where the text names the def, and a site cannot hold both a call and a hand-over | the same |

### 6.1 Found at the review of V2, before V4 is built: what this does to continuation-passing code

**V4 is held until the owner has seen this.** The rule above was written for a callback (`each(x => work(x), 1)`: `each` ▸ `work`). Read against the
demos' own lambdas it does harm, and the reviewer's recommendation of Q-BD9 did not see it. A lambda handed to a project def in the demos is nearly
always a **continuation**, and a continuation leads back to the def it is handed to:

```bend
def Msg.read(text: String) -> Maybe<&2, Msg>:
  String.word(Maybe<&2, Msg>, text, tag => s => Msg.read.tag(Msg.tag(tag), s))
def Msg.read.tag(k: Nat, s: String) -> Maybe<&2, Msg>:
  …   String.word(Maybe<&2, Msg>, s, room => r1 =>
        String.word(Maybe<&2, Msg>, r1, time => r2 =>
          String.word(Maybe<&2, Msg>, r2, name => data => Msg.send(room, time, name, data))))
```

| | The tree of `Msg.read` |
|---|---|
| today (the lambda stays) | `Msg.read` ▸ `String.word` · `Msg.read.tag` ▸ `String.word` · `String.word` · `String.word` · `Msg.send` · …: flat, complete, in the order the code reads |
| §6 as written, with the tree's rule 8 as it is (recursion is marked **by function**) | `Msg.read` ▸ `String.word` ▸ `String.word.go` ▸ `Msg.read.tag` ▸ `String.word [Recursive]`: every `String.word` under the first is a leaf, and `Msg.send` and the rest of the parser are **not in the tree** (they are in the graph) |
| §6 with rule 8 changed to mark by context | complete, but `String.word.go` applies `k` at two sites, so the rest of the chain is drawn under each: twice per level, 16 copies of the last call under a chain of four |

Rule 8 is core's and every language's ([`../02-tree-rules/02-the-rules.md`](../02-tree-rules/02-the-rules.md)); the wire tree is the output that is used (D17).
So "no call can be lost" above is true of the graph and **false of the tree** for a lambda whose body leads back to the def it is handed to. Three ways out,
the owner's to choose (asked 2026-11-21):

1. **A lambda moves only when its body cannot lead back to the def it is handed to** (a code-only reachability over the calls that are fixed by name): a
   callback moves (`each` ▸ `work`), a continuation or a loop body stays flat as today. Recommended. Most of the demos' 15 lambda sites are continuations and
   would stay as they are: known, their calls under the writer.
2. **Leave every lambda with its writer** (BC-D6 as built): D54 is not built in Bend; phases 2 and 3 are unaffected.
3. **Build §6 as written** and accept the trees of the middle row, or change rule 8 in core for every language.

Nothing else in this design depends on the choice: variables (V3), returns, partial application and constructor fields go on.

Why this rule and not "wherever it turns out to be applied": that would be a fact about the whole graph (the plan's own worry, 06 §6 and
TypeScript's 03-design §9), a tree that changes with the budget, and a lambda whose calls vanish when the value is lost on the way. Here the
writer decides from the text and the callee's code, the callee's context is the target of an edge at that very site, and the position is in the
callee's key because it is applied. **Every call is still drawn exactly once per time it can run, and none can be lost**, short of the callee's
context being cut, which hides everything under a cut node anyway.

**Captures.** When a closure is made, the binders of the writer that its body reads (`Flow.lambdas[nth]`) are looked up in the frame; those whose
value is neither unknown nor data go with it. So `outer(~h, x)` writing `each(y => h(y), x)` under `{h ↦ g}` makes a closure that knows `h` is
`g`, and `each` draws `g` at `f(x)`. A lambda that captures nothing is the same value in every context of its writer.

**Guards.** A closure applied while it is already being applied on the same stack of applications is not entered again: the site gets
`Unresolved(function_typed_value)` and `closure_cycle` is counted. An application nested deeper than `max_inline` (8) likewise, counted
`closure_depth`. Both are precision limits: a function of the key and the code.

## 7. Returns, factories, partial application

**BV-D5.** The value of a call that gives a def all its written parameters is **what the def's body evaluates to under that key**: `ret(g, key)`.

- It is computed by Bend's own evaluator, which keeps `(def, key) → (sites, value)`; the core solver still asks for the sites of one context at
  a time and is not changed. Core's `Summary` carries no value, and does not need one here: the value is read by Bend, inside Bend. (TypeScript's
  TC6 and Rust's RC4 plan a value in core's summary; when one of them builds it, Bend's memo is the second implementation the rule of two asks for.)
- **A factory** is then nothing special: `pick(b)` returns the join of its cases, `{work, other}`; `Walkers.game()` returns `{Ctor Game [World.step,
  Input.read, Input.show, data, data]}`; `adder(2)` returns `{Lambda}`, a lambda that stays, so its calls are drawn under `adder`, where they are
  written, and `add2(1)` says `anonymous_function_called`. A def with no code (a law, a foreign def) and an `External` return unknown.
- **Partial application** (`drop(2n)` for `drop(n, k, s)`; the guide: "partial applications are closures too"): the value is `{Partial}`, and the
  site, which layer 1 draws as a call, is a **passed** edge under the open key: the def is handed over, its body does not run there. Where the
  rest of the arguments arrive (`m(a => f(a)(k), s)` in `P.bind`), the site is the `Call` with the key of all of them.
- **A returned lambda's own binders stay unknown** (`P.pure(x)` is `k => s => k(x, s)`: `k` is called in `P.pure`, and nothing there says what it
  is). It is the layer's known limit, row `limit-lambda-binder`, unchanged.

**Recursion.** `ret` of a context asked for while it is being evaluated is its value so far, which starts as **data** (the empty, closed value:
"nothing yet"). The outermost context of the cycle is evaluated again until its value stops changing; nothing computed inside the cycle is kept
before that. Values only grow and are capped (§10), so it ends; after `max_rounds` (4) every context of the cycle gets unknown and
`return_in_cycle` is counted. Starting from "nothing" and repeating gives the smallest value consistent with the code whichever context was
entered first, which is what rule 1 (a summary is a function of its key and the code) needs. Python's solver does the same with pending values
and rounds.

> **🦀 Rust note: a memo with an "in progress" state.** `ret(f)` may ask `ret(g)`, which asks `ret(f)` again. A plain cache would recurse for
> ever. The cure is three states per entry: absent, `Running(value so far)`, `Done(value)`. Asking for a `Running` entry returns its value so far
> and sets a flag on every caller up to it ("my answer depended on something unfinished"), and such an answer is not stored as `Done`. In Rust the
> map sits in a `RefCell` so the evaluator can stay `&self`; the borrow must be dropped before recursing, or the inner call panics on a second
> `borrow_mut`.

## 8. `do` blocks

**BV-D6.** A `do` statement of a project monad is its call, as `bend.ts` builds it (`parse_term_do_stmt`): `x : A <- v` in `do M<T…, B>:` is
`M.bind(T…, A, B, v, x => rest)`, and `return e` is `M.pure(T…, B, e)`. So `v` and `e` sit at known positions and their values go into the key:
`Unit <- drop(2n)` gives `P.bind` its `m` as `Partial(drop)`, and `P.bind` draws `drop` where it applies `m`. **The continuation always stays**: a
`do` block is drawn flat under the def that writes it, one `M.bind` per statement, as layer 1 draws it, and never as a chain nested one level per
statement. Its slot is left out of the key, so `M.bind` has a context per distinct value bound, not per statement. Base's monads are not sites,
as before.

## 9. Constructor fields

**BV-D7.** `K{a, b}` of a project type has the value `Ctor` with its fields' values; a pattern `K{x, y}` (in a `let`, a `case`, a `match`) of a value
that is that constructor binds `x` and `y` to them. Nothing can write a field after construction, so there is no identity, no aliasing and no
effect to track: the value **is** the object. A lambda in a field stays (§6); a def in a field keeps its passed edge (D22), and is called where the
field is called. A constructor of a Base type (`Some{f}`, a list) is unknown: what Base does with it is not ours to read.

## 10. The key, the digest, the caps

**BV-D8.**

- **A slot holds a `Value`.** It is admitted when the position is relevant and the value is neither unknown nor data. That second test is what
  keeps numbers out of keys, so **relevance can be generous**: a parameter is relevant when the evaluator reads it as a value anywhere (a callee,
  an argument, a `let`, a scrutinee, a field, the reply, a capture). 06 §3.2's `called` set stays, for the `key_slot_*` counts.
- **The canonical form grows by variants, and nothing that exists moves.** `Canon` gains `Closure(def, nth, captures)`, `Partial(head, args)`,
  `Ctor(type, name, fields)` and `Set { known, open }`; a slot that holds exactly one closed `Def` or `Lambda` is written as it is today, so every
  digest of BC1+BC2 stays (the unit test that pins the JSON bytes says so).
- **Caps, all precision limits** (deterministic, counted): `max_depth` 3, a value nested deeper inside a value is unknown there, the outer `Closure`,
  `Partial` or `Ctor` is kept (`value_widened_depth`); `max_set` 8, a larger set is unknown (`value_widened_set`); `max_inline` 8 and `max_rounds` 4
  (§6, §7).
- **Cost.** `Limits::max_contexts` cuts, as today. A cap of contexts per def would be a core change (TypeScript's TC2 step 1 proposes it); it is
  not made here. Phase 1's measurement reports the largest count; if a def passes 64 on the demos, that change is proposed then.

## 11. What changes, and what does not

| Piece | Changes |
|---|---|
| `sem/flow.rs` | new: `Flow`, `Expr`, `Pat`, `LamFacts`, built in the same pass that builds the items |
| `value.rs` | new: `Value`, `One`, `join`, the caps |
| `eval.rs` | new: `eval`, `apply`, patterns, `applies`, `ret` and its memo |
| `key.rs` | slots hold `Value`; `Canon` grows; `relevant` reads the `Flow`; `rebuild` handles the new variants |
| `walk.rs` | under `Precise`, a body is `eval`'s sites; the item walk stays for `Default` and for `walked()` |
| `calls.rs` | `Options::new` gives `Precision::Precise` (D53); the new counters |
| `bend-cli` | `--level default\|precise`, precise unless said; `--all` stays the open-key sweep at the default level |
| **unchanged** | `bend-parser`, `bend-syntax`, the outline, qualnames, markers, the book, the renderer, the wire and its merge, `Session`'s API, layer 1's table and snapshots (they are asked at `Default`), **`core/`** |

## 12. The rows

The precise table of `languages/bend/docs/policy.md` changes with the code, phase by phase. The reviewer writes each phase's rows here first, in
the table's grammar, and **the coder copies them into `policy.md` in the commit that brings their fixtures** (a row without a fixture fails the
table's test). Phase 1's rows:

| Precise row | Change | Bend | Precise |
|---|---|---|---|
| `template-lambda` | flips | `twice(~(x => f(x)), 40)` | `main` ▸ `twice` ▸ `f` · `f`: the lambda moved, and `twice` applies it twice |
| `param-lambda-captures` | flips | `outer(~h, x)` writes `each(y => h(y), x)`; `outer(~g, 1)` | `outer` ▸ `each` ▸ `g`: the closure captured `h ↦ g` |
| `recursion-keyed` | flips | `r(~g, n)` is `r(~(y => g(y)), n)` | `r` ▸ `r [Recursive]`, and `g(y)` is drawn nowhere: the lambda moves into `r`, and `r` only ever passes it to itself, so nothing applies it, which is what the program does. The key nests a closure in a closure to `max_depth`, then repeats (`value_widened_depth`). `f`'s half of the row is unchanged |
| `local-let` | replaces `limit-local` | `g = work`, then `each(g, 1)` | `each` ▸ `work`; layer 1's `passed_deep` for the name in the `let` stays |
| `lambda-curried` | new | `pace(k)` calls `k(1, 2)`; `pace(a => b => work(a))` | `pace` ▸ `work`: both binders bound by one application |
| `lambda-passed-along` | new | `relay(f, x)` is `each(f, x)`; `relay(y => work(y), 1)` | `relay` ▸ `each` ▸ `work`: `applies` is closed over passing along |
| `lambda-two-sites` | new | `both(f)` calls `f(1)` and `f(2)`; `both(y => work(y))` | `both` ▸ `work` · `work` |
| `lambda-inside-lambda` | new | `each(x => each(y => work(y), x), 1)` | `each` ▸ `each` ▸ `work`: the inner lambda moves relative to the body that is being read |
| `lambda-hands-over` | new | `each(x => apply(work, x), 1)` | `each` ▸ `apply` ▸ `work`; `work [Passed]` under `main`, where the name is written |
| `lambda-to-base-stays` | new | `List.map(~(x => work(x)), xs)` | `main` ▸ `work`, as layer 1 |
| `lambda-not-applied-stays` | new | `log(f, x)` never applies `f`; `log(y => work(y), 1)` | `main` ▸ `log` · `work` |
| `lambda-under-applied-stays` | new | `each(y => work(y))` with one argument of two | `main` ▸ `each` · `work`, as layer 1 |
| `lambda-through-variable-stays` | new | `apply(h, x)` is `h(y => work(y), x)`; `apply(each, 1)` | `apply` ▸ `each` · `work`; `each`'s `f(x)` says `anonymous_function_called` |
| `lambda-in-let-stays` | new | `g = y => work(y)`, then `each(g, 1)` | `main` ▸ `each` · `work`; `each`'s site says `anonymous_function_called` |
| `limit-closure-cycle` | new | `loop(f)` is `f(f)`; `loop(g => g(g))` | the second application is not entered: `closure_cycle` |

### 12.1 V3's rows, as `policy.md` takes them

From V3 on, the precise table of `languages/bend/docs/policy.md` is the only living copy (06 §9 is the table as BC1+BC2 left it). These five lines go
into it, after `passed-open`, in the commit that brings their fixtures; **the row `limit-local` is removed** in the same commit (its fixture and snapshot
become `local-let`'s). `local-lambda` is the row the table above calls `lambda-in-let-stays`.

| Precise row | Bend | Default | Precise | Counter | Where it is checked |
|---|---|---|---|---|---|
| `local-let` | `g = work`, then `each(g, 1)` | `each` a leaf, its `f(x)` `Unresolved` | `each@{f ↦ work}` ▸ `work`: a variable has the value of its binding (BV-D3) | layer 1's `passed_deep`, for the `work` in the `let` | `local-let` `main.bend` `main` |
| `local-alias` | `run(f, x)` is `g = f`, then `g(x)`; `main` calls `run(work, 1)` | `run` a leaf, its `g(x)` `Unresolved` | `run@{f ↦ work}` ▸ `work`: a parameter is relevant as the value of a `let`, and the variable is called as what it holds | | `local-alias` `main.bend` `main` |
| `local-shadow` | `run(f, x)` is `f = other`, then `f(x)`; `main` calls `run(work, 1)` | `run`'s `f(x)` `Unresolved` | `run` ▸ `other`: the innermost binder of a name decides. `run` has the open key (its parameter is never read); `work [Passed]` stays | `passed_deep` | `local-shadow` `main.bend` `main` |
| `local-join` | `g = (match b: case True{}: work; case False{}: other)`, then `g(1)` | `g(1)` `Unresolved` | `work` and `other`, both at the one site `g(1)`: the value of a `match` is the join of its cases, no condition is evaluated | `passed_deep` | `local-join` `main.bend` `main` |
| `local-lambda` | `g = y => work(y)`, then `each(g, 1)` | `main` ▸ `work` · `each`; `each`'s `f(x)` `Unresolved` | the same tree; `each@{f ↦ lambda 0 of main}`'s `f(x)` says `anonymous_function_called`: a lambda in a `let` stays with its writer | `anonymous_function_called` | `local-lambda` `main.bend` `main` |

### 12.2 Phase 2's rows (V6 returns, V7 partial application and `do`)

Written 2026-11-21 after V3, ahead of phase 1's measurement, because none of them depends on where a lambda's calls are drawn (§6.1). Each goes into
`policy.md` in the commit that brings its fixture. A cell is the reviewer's reading of the rules; where the code says otherwise, the coder reports the row.

**V6.** The value of a call that gives a def all its written parameters is its body's value under that key (§7). A slot now takes **any** argument's
value that is neither unknown nor data (a call's result too), and a slot may hold a set. A parameter that reaches the reply is relevant.

| Precise row | Bend | Default | Precise | Counter | Where it is checked |
|---|---|---|---|---|---|
| `return-def` | `pick()` is `work`; `g = pick()`, then `g(1)` | `main` ▸ `pick`; `g(1)` `Unresolved` | `main` ▸ `pick` · `work`: the value of a call is what the def's body evaluates to | `passed_deep` | `return-def` `main.bend` `main` |
| `return-join` | `pick(b)` is `match b: case True{}: work; case False{}: other`; `main` calls `pick(b)(1)` | the outer call `Unresolved` | `work` and `other` at the outer call's site, beside `pick`: a factory returns the join of its cases | `passed_deep` | `return-join` `main.bend` `main` |
| `return-param` | `id(f)` is `f`; `main` calls `id(work)(1)` | the outer call `Unresolved`; `work [Passed]` | `id@{f ↦ work}`, and `work` called at the outer call; the passed edge stays. A parameter that reaches the reply is relevant | | `return-param` `main.bend` `main` |
| `return-chain` | `make()` is `pick()`; `pick()` is `work`; `main` calls `make()(1)` | `main` ▸ `make` ▸ `pick`; the outer call `Unresolved` | the same, and `work` at the outer call: a value through two returns | `passed_deep` | `return-chain` `main.bend` `main` |
| `return-as-argument` | `each(pick(), 1)` | `each` a leaf | `each@{f ↦ work}` ▸ `work`: a call's value fills a slot | `passed_deep` | `return-as-argument` `main.bend` `main` |
| `return-set-argument` | `each(pick(b), 1)`, `pick` by case as above | `each` a leaf | one context `each@{f ↦ {other, work}}`, whose one site `f(x)` has both targets: a set stays a set, it is not two contexts | `passed_deep` | `return-set-argument` `main.bend` `main` |
| `return-lambda` | `adder(k)` is `x => add(x, k)`; `add2 = adder(2)`, then `add2(1)` | `main` ▸ `adder` ▸ `add`; `add2(1)` `Unresolved` | the same tree; `add2(1)` says `anonymous_function_called`: a returned lambda stays with the def that writes it, where its calls are drawn | `anonymous_function_called` | `return-lambda` `main.bend` `main` |
| `return-recursive` | `loop(n)` returns `work` at zero and `loop(p)` otherwise; `main` calls `loop(3n)(1)` | `loop` ▸ `loop [Recursive]`; the outer call `Unresolved` | `work` at the outer call: the value of a recursive def starts as nothing and is evaluated again until it stops changing | `passed_deep` | `return-recursive` `main.bend` `main` |
| `return-cycle` | `a(n)` returns `work` or `b(p)`; `b(n)` returns `other` or `a(p)`; `from_a` calls `a(2n)(1)`, `from_b` calls `b(2n)(1)` | the outer calls `Unresolved` | both entries draw `work` and `other` at the outer call: the value of a cycle does not depend on which def was entered first | `passed_deep` | `return-cycle` `main.bend` `from_a` `from_b` |
| `limit-return-base` | `g = List.head(fs)`-like: the result of a Base def, then `g(1)` | `g(1)` `Unresolved` | the same: what Base returns is unknown | `function_typed_value` | `limit-return-base` `main.bend` `main` |

**V7.** A by-name call that gives a def fewer arguments than it has written parameters is a **passed** edge under the open key and the value
`Partial`; the `Call` is drawn where the rest arrive, under the key of all of them. A `do` statement of a project monad is its call, with its value at
its position in the key; its continuation stays and is not in the key (§8). The three renamed rows take their old fixtures.

| Precise row | Bend | Default | Precise | Counter | Where it is checked |
|---|---|---|---|---|---|
| `partial-def` | `add(a, b)`; `inc = add(1)`, then `inc(2)` | `main` ▸ `add` (the under-applied call, drawn as a call); `inc(2)` `Unresolved` | `add [Passed]` at `add(1)`: the def is handed over, its body does not run there; `add` called at `inc(2)` | | `partial-def` `main.bend` `main` |
| `partial-twice` | `add(1)(2)` | `main` ▸ `add`; the outer call `Unresolved` | `add [Passed]` at the inner call, `add` called at the outer one | | `partial-twice` `main.bend` `main` |
| `partial-callback` | `each(add(1), 5)` | `main` ▸ `each` · `add`; `each`'s `f(x)` `Unresolved` | `each@{f ↦ add with one argument}` ▸ `add`: the partial application is in the key and `each` gives it the rest; `add [Passed]` under `main` | | `partial-callback` `main.bend` `main` |
| `partial-keyed` | `app(f, x)` is `f(x)`; `p = app(work)`, then `p(1)` | `main` ▸ `app`; `work [Passed]`; `p(1)` `Unresolved` | `app [Passed]`, `work [Passed]`, and at `p(1)` `app@{f ↦ work}` ▸ `work`: the arguments a partial application holds make the key | | `partial-keyed` `main.bend` `main` |
| `limit-partial-base` | `inc = U32.add(1)`, then `inc(2)` | `inc(2)` `Unresolved` | the same: a partial application of a Base def is unknown | `function_typed_value` | `limit-partial-base` `main.bend` `main` |
| `do-flat` | a `do P<U32>:` of a project monad with two binds of `item()` and a `return` | `main` ▸ `P.bind` · `item` · `P.bind` · `item` · `P.pure` | the same shape, never one statement nested under the one before; the two binds are one context of `P.bind`: the continuation is not in the key | | `do-flat` `main.bend` `main` |
| `do-partial` (was `limit-partial-application`) | `drop(n, k, s)` calls `k(…)`; `Unit <- drop(2n)` in a `do P<Unit>:` | `main` ▸ `P.bind` · `drop` · `P.pure`; `drop`'s `k` `Unresolved` | `drop [Passed]` under `main`; `P.bind@{m ↦ drop with one argument}` ▸ `drop@{k ↦ lambda of P.bind}`, where `k` says `anonymous_function_called`. In the open context of the passed edge `k` is still `function_typed_value`, and `P.bind`'s `f(a)` and `f(a)(k)` are too (the continuation is not in the key) | `anonymous_function_called`, `function_typed_value` | `do-partial` `main.bend` `main` |
| `do-handed` (was `limit-handed-to-bind`) | `peek(k, s)` calls `k(…)`; `h : U32 <- peek` in a `do P<U32>:` | `peek [Passed]`; its `k` `Unresolved` | `peek [Passed]` stays; `P.bind@{m ↦ peek}` ▸ `peek@{k ↦ lambda of P.bind}`, where `k` says `anonymous_function_called` | `anonymous_function_called`, `function_typed_value` | `do-handed` `main.bend` `main` |

### 12.3 Phase 3's rows (V8 constructor fields)

A constructor of a project type has the value `Ctor` with its fields; a constructor pattern `K{x, y}` binds `x` to **the join of that field over the
members of the value that are `K`**, and to unknown when the value is open or has no such member. A parameter that is a scrutinee, or the value of a
`let` with a constructor pattern, is relevant. The first row takes the fixture of `limit-constructor-of-defs`.

| Precise row | Bend | Default | Precise | Counter | Where it is checked |
|---|---|---|---|---|---|
| `ctor-fields` (was `limit-constructor-of-defs`) | `run(~app: App, s)` matches `case App{v, t}: t(s)`; `main` calls `run(~App{view, tick}, 0)` | `view`, `tick` `[Passed]`; `t(s)` `Unresolved` | `run@{app ↦ App{view, tick}}` ▸ `tick`; the two passed edges stay | | `ctor-fields` `main.bend` `main` |
| `ctor-let` | `App{v, t} = App{work, other}`, then `t(1)`, in one body | `work`, `other` `[Passed]`; `t(1)` `Unresolved` | `other` called at `t(1)` | | `ctor-let` `main.bend` `main` |
| `ctor-factory` | `game()` is `Game{step, read}`; `play(g)` is `Game{s, r} = g`, then `s(1)`; `main` calls `play(game())` | `play` a leaf; `step`, `read` `[Passed]` under `game` | `play@{g ↦ Game{step, read}}` ▸ `step`: a factory of an object, the demos' `Walkers.game()` | | `ctor-factory` `main.bend` `main` |
| `ctor-passed-along` | `outer(g)` calls `play(g)`; `main` calls `outer(game())` | `outer` ▸ `play`, a leaf | `outer` ▸ `play` ▸ `step`, each under the key that holds the constructor: it flows down as a def does | | `ctor-passed-along` `main.bend` `main` |
| `ctor-join` | `pick(b)` returns `Box{work}` or `Box{other}`; `Box{f} = pick(b)`, then `f(1)` | `f(1)` `Unresolved` | `work` and `other` at `f(1)`: a field of a set of one constructor is the join of that field | | `ctor-join` `main.bend` `main` |
| `ctor-two` | `type Sum` with `L{f}` and `R{f}`; `x = L{work}`; `match x: case L{f}: f(1)` and `case R{f}: f(2)` | both `f` `Unresolved` | `work` at `f(1)`; `f(2)` stays `Unresolved`: no member of the value is an `R`, and no case is pruned | `function_typed_value` | `ctor-two` `main.bend` `main` |
| `limit-ctor-base` | `m = Some{work}`, then `match m: case Some{f}: f(1)` | `f(1)` `Unresolved` | the same: a constructor of a Base type is unknown | `function_typed_value` | `limit-ctor-base` `main.bend` `main` |

The rows of V4 (§12, the first table) wait for §6.1's answer. **The scenario `callback-argument`'s shared `expected.precise.tree`
changes in phase 1** (the README says why, and that Q-TC1 and Q-RC1 are answered):

```
main
  each
    work
  work [Passed]
  each
    work
```

## 13. The forecast on the demos, site by site

From `ENGINE.md` §BC1+BC2's list, read against the rules above. A forecast, to be measured, phase by phase:

| Sites | Today | Rule that reaches them | Forecast |
|---|---|---|---|
| 16 template parameters, `app_slash_boss_3d` | drawn | | unchanged |
| 12 lambdas (`Play.pace`, `Play.serve`, `Play.turn`, `Client.sync.go`, `Post.insert.go`, `Post.next.go` ×2, `String.word.go`'s `rest`, `World.input.go`, `Bot.more`, `echo_sent`, …) | known, not drawn | §6, where the lambda is a direct argument of a by-name call that applies it | **drawn**, most of them. One that reaches its caller through a `let` or a Base call stays known: the measure says which |
| 3 of `String.word` | a lambda in 14 of 15 contexts | §6: `k2 => String.word(R, t, k2)` moves into `String.word.go`; the continuation `w => r => k(…)` is an argument of a call through a parameter, so it stays | drawn in the 14, a known lambda in the 15th; no context without a name |
| 3 constructor fields, `Game.step`, `Game.read`, `Game.show` | unresolved | §7 (`Walkers.game()` returns the constructor) and §9 (`Game{step, read, show, rate, tol} = game`) | **drawn**, if `game` is passed along as a direct argument all the way (phases 2 and 3) |
| `P.bind`'s `m`, `pure_hvm5_mini` | unresolved | §8, §7: `m` is `Partial(drop)`, `peek`, `eof`, … | **drawn**, in a context per bound value |
| 14 `k` of the parser's primitives (`drop`, `peek`, `eof`, `pop`, `bindc.go`, …) | unresolved | reached from `P.bind` with `k` its lambda `a => f(a)(k)`, which stays | **known lambdas** in the contexts `P.bind` reaches; unresolved in the open context of the passed edge. Not drawn: a parser in continuation-passing style is a chain of closures, and drawing it would be running it |
| `P.bind`'s `f` and `f(a)`, `prog()`, `pop.go(…)`, `name.go`, `P.pure`'s `k` | unresolved | the continuation of a `do` (left out of the key), a returned lambda's binder | unresolved: the layer's limits |

So the forecast is about **31 to 34 of the 55 drawn** (16 today), about 15 more known, and 6 unresolved. It is the reviewer's reading of the
source, not a count.

## 14. The proof

| Test | Proves |
|---|---|
| **knows nothing ⇒ layer 1**: the evaluator with every value unknown (a test-only policy) gives, for every def of every fixture, scenario and demo book, the sites and targets of the item walk, item by item | BV-D2: the evaluator and the item table agree; the seam |
| **tracks what BC1+BC2 tracked ⇒ BC1+BC2**: with lets, moves, returns and fields off, the 21 context snapshots, both scenarios and the demos' pinned numbers are byte-identical | phase 1 starts from where the last one ended, not near it |
| **never fewer**, restated: every def and external layer 1's graph reaches from an entry, the precise graph reaches; a site keeps its named targets unless it sits in a lambda that moved or is an under-applied call | D54's "none can be lost" |
| every row of §12 flips as written; digests in the snapshots | the design; rule 9 |
| the digests of BC1+BC2's keys do not move | BV-D8 |
| `return-cycle` entered from each of its defs gives the same values | §7's fixpoint is order-free |
| the cut-and-continue test, on a fixture whose keys hold a closure, a partial and a constructor | `rebuild` |
| the demos' measurement, extended: per site, drawn / lambda / unresolved per context, against §13; contexts per def, walks, time | what each phase bought and cost; written in `ENGINE.md` |
| the oracle tests, at 0 | the walk's places did not move |

## 15. Steps for the coder, in four phases

One commit per step, a report per step, the rules of 06 §13. A phase ends with the demos' measurement and the reviewer's write-up in `ENGINE.md`;
the next phase's rows and prompt are written from it.

| # | Do | Proof |
|---|---|---|
| **phase 0** | **on by default (D53)** | |
| V0 | `Options::new` gives `Precise`; every test of layer 1 says `Default` in words; `bend-cli --level default\|precise` (`--precise` goes; `--all` refuses `--level precise`); one wire snapshot at `Precise` (`template-def`) | no snapshot of layer 1 changes; the CLI's default tree of `two-callers` is the precise one |
| **phase 1** | **the evaluator; lambdas move; variables** | |
| V1 | `sem/flow.rs`, built with the items; a text form of a `Flow` for the unit tests | unit tests per node kind; every snapshot unchanged; oracle at 0 |
| V2 | `value.rs`, `eval.rs`; `Precise` walks by `eval`, tracking exactly what BC1+BC2 tracks (`Def`, `Lambda`, parameters from the key) | §14's first two rows: knows nothing ⇒ layer 1; BC1+BC2's snapshots and demo numbers byte-identical |
| V3 | variables: `Let` and `Pat::Bind` carry values | row `local-let` (the fixture `limit-local` renamed, its row replaced) |
| V4 | `applies`; `Closure` and captures; a moved lambda's body read at the applying site; the guards; `Canon` grows; `rebuild` | the rows of §12; `callback-argument`'s new precise tree; never fewer, restated; BC1+BC2's digests unchanged |
| V5 | the demos' measurement, extended | §13's first three rows, as counted |
| **phase 2** | **returns, factories, partial application, `do` as a call** (rows after phase 1) | |
| V6 | `ret` and its memo, the fixpoint; `Call`'s value | factory rows; `return-cycle` |
| V7 | `Partial`; an under-applied call is a passed edge; `Bind` as its call | partial rows; the `do` rows; `limit-partial-application` and `limit-handed-to-bind` flip |
| **phase 3** | **constructor fields** | |
| V8 | `Ctor`, `Pat::Ctr` | `limit-constructor-of-defs` flips; the three `Game` sites |
| V9 | the demos' measurement, final | §13, as counted |

## 16. The decision rows

| # | Row | Where |
|---|---|---|
| BV-D1 | **A value is a closed set of defs, lambdas, closures, partial applications and constructors, that says whether it is complete**; data is the empty closed set | §3 |
| BV-D2 | **`sem` hands the body back as an owned tree whose sites are the items**; the evaluator decides targets and never invents a site | §4 |
| BV-D3 | **Under `Precise` a body is evaluated; a variable has the value of its binding**; no condition is evaluated, no case pruned | §5 |
| BV-D4 | **A lambda moves to the def that calls it when it is a direct argument of a by-name project call that gives all written parameters, at a position that def applies; every other lambda stays** (D54). A hand-over inside it stays where the name is written. BC-D6 is replaced for the lambdas that move | §6 |
| BV-D5 | **The value of a full call is the body's value under that key, computed in Bend with a fixpoint from "nothing"; an under-applied call is a passed edge and a `Partial`**; a returned lambda stays with its def | §7 |
| BV-D6 | **A `do` statement of a project monad is its call, its value in the key; its continuation always stays and is not in the key**. BC-D5 stands for operators and indexes | §8 |
| BV-D7 | **A project constructor is a value with its fields; a constructor pattern binds them** | §9 |
| BV-D8 | **A slot holds a value that is neither unknown nor data; the canonical form grows by variants and no existing digest moves; depth 3, set 8, inline 8, rounds 4** | §10 |

**V3 built (2f5766d), reviewed 2026-11-21**, every proof reproduced: 144 tests, the five rows of §12.1 as written (the coder's four hand-count slips
were in the counts, the code was right), the 20 other context snapshots untouched, knows-nothing still layer 1 over the same 30,471 items, never fewer
edges over 184 entries, the demos' measurement unchanged (no `let` holds a function there, as BC0 said), both oracle tests at 0. Two things it settled
beyond the prompt, both kept: a `case` binds pairwise (the first pattern to the first scrutinee), and several lambdas at one site give one
`anonymous_function_called`. One thing it left, taken up in phase 3: a parameter used only as a scrutinee is not relevant yet. **The order of the steps
changes: V6, V7 and V8 (phases 2 and 3, rows in §12.2 and §12.3) go before V4 and V5**, since V4 waits for §6.1 and nothing in them depends on it.

**State.** V0, V1 and V2 built (0e1667d, 4375dfc, acdeba3), reviewed 2026-11-21, every proof reproduced: 139 tests; the evaluator that knows nothing is
layer 1 item by item over 625 items of the fixtures and scenarios and 29,846 items of the 16 demo books and 15 `PROOF.bend` books; the 21 context
snapshots, both scenario trees, the wire snapshots and the pinned numbers of the demos' measurement untouched; both oracle tests at 0; no layer-1 snapshot
changed by the flip of the default. Judged at review, all accepted: `Precision`'s own default is `Precise` too; `--all` refuses `--level precise` with the
sentence naming the new flag; `Bind.binder` is an `Option`; each `=>` of a chain has its own number and its own `LamFacts` (the `Lam` node is still one,
with all its binders); a slot is read off a bare name or a lambda only, and an `Opaque` call follows a value only when the callee is the def's own
parameter (both are what BC1+BC2 did, so both are what V2's proof needed; V3 lifts them for variables). Owed by later steps: `mod flow`'s
`allow(dead_code)` goes when its last field is read; `OUTSIDE_ROOT` and `OPEN_LAW` are counted once per place of the code, not once per reading; the join
gets its cap. **`Precise` is now about 1.5 times `Default`'s time on `app_slash_boss_3d`** (it was 1.3 before the evaluator: an item is settled by the tree
and the leftovers by the item walk); V5 says where the time goes. **V4 is held: §6.1.**

Recorded in the repo's register as D53, D54 and D55 ([`../07-decisions-and-questions.md`](../07-decisions-and-questions.md)).
