# Note: what runs when a class is constructed, and so what is under a `Construct`

Status: **a design note, no code, for the owner to choose.** Written on 2026-11-12 after TS4 step 7a, from reading `core/analyzer-core/src/{graph,solve,render}.rs`,
`languages/typescript/ENGINE.md` and the policy snapshots. Two things were run, and are marked **[ran]**: a JavaScript program under node 22.17 for the order of what runs, and
`ts.transpileModule` of TypeScript 5.8.3 (the copy in the V-NOC frontend's `node_modules`) for what the compiler emits. The rest is reading; every `[check]` is a claim to verify
before building on it.

```
what a construction runs
├── 1. what the language runs
├── 2. what the analyzer draws today
├── 3. what is fixed
├── 4. options
├── 5. the recommendation, and the core/ change it needs
└── 6. questions for the owner, and what was not checked
```

## 1. What the language runs

Two events, and they are not the same event.

**Defining the class** runs when the code that holds the class statement runs: at module load for a class at the top of a file, each time the function runs for a class declared
inside one. Once per definition, never per instance.

**Constructing an instance** (`new C()`, `<C />` of a class component, `super()` from a derived class) runs once per instance.

| What | Event | Where it is in the source | Today it is |
|---|---|---|---|
| the `extends` expression | defining | `class D extends mixin(B)` | a site of the class def's body |
| computed member names (`[key()] = …`) | defining | member names | a **nested** class's belong to the enclosing def (`Collector::collect`, as in tsrs); a **top-level** class's are in **no tree and no counter** **[ran]**: `class Top { [key()] = init(); }` as the entry draws `init` and not `key`, `body_of(class)` never reads the names |
| static field initializers, static blocks | defining | `static s = f()`, `static { g() }` | sites of the class def's body |
| decorators (class, member, parameter) | defining (experimental decorators apply after the definition) | `@dec` | sites of the class def's body (owner: they stay so) |
| **instance field initializers** | **constructing** | `x = f()` | sites of the class def's body; **not drawn under a construction** |
| parameter defaults of the constructor | constructing | `constructor(p = f())` | sites of the constructor: drawn |
| parameter properties (`constructor(private x = f())`) | constructing | the default is a parameter initializer; `this.x = x` is an assignment with no site | the default is a site of the constructor: drawn |
| the constructor body | constructing | `constructor() { … }` | drawn (`init`) |
| the base class's whole construction | constructing | `super(…)`, explicit or implicit | `super(x)` is a site → `Construct` of the base; an implicit `super` has no site, and `init_of` follows the chain to the nearest constructor instead |

**The order [ran]** (node 22.17; a base class `Base`, a `Derived extends Base`, each with a constructor with a parameter default and a field):

```
-- defining, once:   Base static field, Derived extends expr, Derived computed key, Derived static block, Derived static field
-- constructing:     Derived param default
                     Derived ctor before super
                     Base field a            <- the base's fields run when the base is entered, BEFORE its parameter defaults
                     Base param default
                     Base ctor body
                     Derived field (computed key), Derived field d    <- the derived class's fields run right after `super()` returns
                     Derived ctor after super
-- implicit constructors (E2 extends D2 extends B2, only B2 has a constructor):   B2 field, B2 ctor body, D2 field
```

So: in a **base** class the field initializers run before the constructor's parameter defaults and body; in a **derived** class they run right after `super()` returns, and the
parameter defaults ran before `super()`. A class with no constructor behaves as if it had `constructor(...args) { super(...args); }`, so a derived class's fields still run after its
base's construction, and a base class's still run.

**TypeScript's emit changes the order, not the set [ran].** With `useDefineForClassFields` (the default for target ES2022 and later) the fields stay native fields and the order is
the one above. With it off (older targets) the compiler moves them into the constructor: `this.p = p; this.a = f(); body` for a base class, so the fields run *after* the parameter
defaults; in a derived class `super(); this.q = q; this.d = f();`. Parameter-property assignments (`this.p = p`) are plain assignments: no site. **The sites that run are the same in
both; only their order moves.** A tree lists a def's sites in source order and is not an execution trace, so the order is a presentation choice, not a correctness question (§6).

## 2. What the analyzer draws today

Pinned in `languages/typescript/docs/policy.md` (rows `class-body`, `construct`, `counter-initializers`, `counter-local-decorators`):

| Case | The tree |
|---|---|
| `new C()` where `C` has a constructor | `C` ▸ the constructor's sites. Its `super(x)` is a `Construct` of the base, drawn the same way |
| `new C()` where `C` has none | `C` ▸ the nearest base class's constructor's sites (`init_of`), or nothing |
| the instance field initializers of `C` (and of each class the construction goes through) | **not drawn**; counted, one for each site, in `uncovered_construct_initializer_sites` |
| `C` as the **entry** (`call_graph(… "Body" …)`) | `Root::Context(class def)`: the class def's own body is walked: decorators, `extends`, instance **and** static initializers, static blocks. **Not the constructor** (`Body ▸ a, b, c` in the `class-body` snapshot) |
| `C` as a callee | `Construct { class, init: constructor }`: the constructor's sites, **none of the class body** |

The last two differ. **D20 point 4 says they cannot**: "a graph's entry may be a class (`Root::Construct`), drawn exactly as a class callee". The plan defers the class entry to TS5
(`05-python-migration/05-entries-and-targets.md` §6), so TypeScript is out of line with D20 until then, and a request for a class by name today gives the body of the definition, which
is not what Python gives for the same request.

What is lost on the V-NOC frontend: nothing. Its 5 classes are `ReplayRunner` (fields initialized with literals), the two one-method services, and two `Error` subclasses; every counter
of step 7a reads 0 there, and the source says so (`ENGINE.md`). The question is about the next project.

How the core builds a root (**[check]**: read, not run): `Solver::graph(&[(Func, Key)])` names every entry `Root::Context` (`solve.rs`, the `Graph::new(entry_ids.into_iter().map(Root::Context)…)` at the end of `graph`). A `Root::Construct`
exists in the type and the renderer draws it (`render.rs`), and Python builds it in its own solver; **the flat solver cannot produce one**. A class entry through it is a class
*context*, which is why it walks the body.

## 3. What is fixed

- **Python's shape** (owner): a class is constructed and never walked, the children come from `init`, a class entry is `Root::Construct`. Python's tests must stay green (rule 5).
- **No def is renamed or moved** (rule 10; the owner, for D22 and step 7a). Adding a kind of def is allowed; changing how an existing def is named is not.
- **Decorators stay sites of the class body, and are not drawn under `new C()`** (owner). **A class component's `render` is not drawn** (owner). **Instance field initializers under a construction are not built now** (owner): this note comes first.
- **What is not drawn is a number** (rule 8): a counter now, an edge later.
- **The wire** is the output (D17): whatever is drawn must be representable in `Frame` without removing a field. `Frame.contexts` is already a list (**[check]** `wire.rs`).
- `Func` in TypeScript is `(file, def)`, a handle to a def of the outline; `describe(Func) → DefInfo` gives the row of `Graph::defs`; `Site` is a byte range **relative to the start of the def the edge leaves**, and the wire turns it into a line
  and column with that def's `range.start`.

## 4. Options

For each: what the tree shows for the `class-body` fixture (`class Body { x = a(); static s = b(); static { c(); } }`), and what must change.

### A. Keep it counted, and make the class entry a construction

- **Under a `Construct`:** `init` only, as today. Instance initializers stay a number.
- **A class entry** becomes `Root::Construct { class, init }`, drawn as a callee: `Body` with no children (it has no constructor). `new Body()` draws the same.
- **The cost:** the definition-time sites (`b()`, `c()`, decorators, `extends`) have **no entry that draws them any more**. Today `Body ▸ a, b, c` is the only place they are drawn. They would need their own counter (`uncovered_class_definition_sites`) so that they are a number.
- **`core/`:** one additive change, so that the flat solver can take a construction as an entry (a second method, or an entry type with two variants), because `Solver::graph` names every root `Root::Context`. Nothing for Python.
- **TypeScript:** `call_graph` asks `Sem` whether the entry def is a class; `Sem::init_of` already gives `init`.

### B. A second context under the `Construct`: the instance initializers of the class

- **Under a `Construct`:** the constructor's context (`init`, as today) **and** a context for the class's instance field initializers, drawn as children of the same class node.
  `makes ▸ Body ▸ a` for the fixture; `new Derived()` shows its own initializers and, through `init`, the base's construction (whose initializers come with the base's own `Construct`, or with its implicit constructor).
- **What the second context is.** It cannot be the class def's own context: that one is the whole class body (static blocks, decorators, `extends`), and drawing it under every construction would show things that run once as if they ran every time.
  Three ways to give it an identity:
  - **B1, a synthetic def, `C.<fields>`.** The graph's `defs` table gets a row with `id = Path { module, qname: "C.<fields>", nth: 0 }`, kind `Other("fields")`, the class's file and a range from the first to the last instance initializer. The outline (`typescript-syntax`) is **not** touched, so no qualname moves. The walk of that def is the instance initializers of the class. The cost: a def in a tree that the outline does not list, so a driver that maps a frame to its index by (file, qualname, occurrence) **cannot find it** (**[check]** with the `vnoc` side; this repository has no driver).
  - **B2, a mode in `Key`.** The context is `(class def, Key::Fields)`. `type Key = ()` is the seam kept for calling contexts and its digest feeds `ContextId`; a mode is not a calling context, and the first real use of `Key` would be a repurposing. Rejected.
  - **B3, one def per field with a site** (`C.x`, D21-style). The outline would have to decide, from syntax alone, which fields hold a site; an accessor read (`x = this.y`) needs the checker, which the outline never uses. Rejected.
- **`core/`:** `Target::Construct`, `EdgeTo::Construct` and `Root::Construct` each gain a second, optional context (`fields`, or `init` becomes a list); the solver discovers it; `Renderer::class` takes both; `check_invariants` checks its row; the wire shows a class node with up to two contexts. Additive, but every `match` on `Construct` in Python and in core is touched (`#[non_exhaustive]` forces the `..`).
  Python passes `None`. **Not a small change.**
- **The implicit constructor.** A derived class with no constructor has no site for its implicit `super(...)`. With B, `init` still jumps to the nearest constructor and the fields of the classes it skips need a `Vec`, or the implicit constructor becomes a synthetic context whose one site is the `extends` expression (a real span: the base's name). That is a bigger design; it is not needed for B to be useful for the common case (a base class, or a derived class with its own constructor).

### C. The initializers are sites of the constructor's context

- **Under a `Construct`:** one context, the constructor's, whose first sites are the initializers: `makes ▸ Body ▸ a`, one node as Python draws it.
- **Why not.** (1) A `Site` is relative to the start of the def the edge leaves; an initializer can lie before the constructor, and a site cannot be negative, and the wire computes the position from `def.range.start + site`. It would need a signed or anchored `Site`: a change of core for every language. (2) A class with no constructor has no context to hold them (`init: None`). Making one means B1's synthetic def anyway.
  And (3) the constructor would no longer be the def the frame names, which the wire relies on for "for a construction it is the constructor, not the class" (`05-entries-and-targets.md` B1). **Rejected.**

### D. Draw the initializers as calls at the construction site

- Add `Call(<some context>)` targets at the site of `new C()` next to the `Construct`.
- **Why not:** the edge says the *caller* calls the initializers, and the merged wire cannot tell a `Construct` of `C` from a `Call` into a context of `C` (the merge key has no edge kind). It needs B's context anyway and adds a lie on top. **Rejected.**

## 5. The recommendation, and the core/ change it needs

**Adopt A at TS5, and keep B1 as the design of record for the later edge; build B1 only when one of the counters reads above 0 on a project the owner cares about.**

- The counters of step 7a already make the omission a number, and they read 0 on the only project there is. B needs a real change of `core/` (§4) and a def the drivers may not be able to look up; that is a lot to pay for 0.
- A is cheap, removes the one inconsistency that exists today (§2: entry and callee differ, against D20), and matches Python for the same request. What it costs is a way to see the *definition* of a class, so that gets a counter, not an entry, until someone asks for it.
- B1 is the shape because it moves no existing def, touches no outline rule, and puts the initializers where the language puts them: under the construction, not under the definition.

**Does it need a `core/` change?** A: **yes, one, additive and small**: the flat solver must be able to take a construction as an entry (`Solver::graph` names every root `Root::Context`). It does not touch Python's solver; Python's tests must be re-run after it (rule 5). B1: **yes, a larger one** (§4), only when built. Nothing in this note is built, and nothing in `core/` was changed to write it.

## 6. Questions for the owner, and what was not checked

1. **A class entry and the definition.** With A, no request draws `decorators`, `static` blocks and `extends` of a top-level class. Is a counter enough (`uncovered_class_definition_sites`), or should a request be able to ask for the definition (a flag on the entry)? A class declared **inside a function** is already half-counted (`local_class_decorator_sites`); under A its definition-time sites could instead be sites of the enclosing function, which is where the language runs them, and that would turn counter 4 into edges. That is a change of what the class def owns, and it contradicts "decorators stay sites of the class body"; it is yours to decide, not mine.
2. **The order of the children under a `Construct` with two contexts (B).** A base class: fields before the constructor's children. A derived class: the constructor's children up to the `super(…)` child, then the fields, then the rest, which a list of two contexts cannot interleave. Choices: fields first always; or after `init` for a derived class. The tree is source-ordered and not a trace (§1), so either is defensible; one has to be written down.
3. **B1's synthetic def and the driver.** `C.<fields>` is in no outline. Whether the V-NOC drivers tolerate a frame whose (file, qualname, occurrence) is not in their index is a question for the `vnoc` side.
4. **`useDefineForClassFields`.** The set of sites does not depend on it, the order does (§1). The analyzer does not read the option today (nothing needs it); a tree that wanted to show the order would.

**Found while writing this, not fixed and not counted:** the computed member names of a top-level class (`[key()]`) are in no tree (the row of §1 above; a probe, outside the repository, with a top-level and a nested class). It is the same family as the definition-time sites of option A (they run when the class is defined), so whichever way question 1 goes, this one goes with it; it needs a counter at the least.

Not checked: the ES specification's wording (the order is from running node 22.17, one program, and tsc 5.8.3, one program, not from the specification text); how the wire's merge treats a class node with two contexts (**[check]** `wire.rs`); how `check_invariants` treats `Root::Construct` (only read); whether the V-NOC drivers accept a def that is not in the outline; classes with accessors as fields (`accessor x = f()`), class expressions held by a variable, and mixins (`extends mixin(B)`: the chain is `Unknown`, counted by `inherited_constructor_unknown`).
