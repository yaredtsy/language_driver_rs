# 03.12.09 TC5: instances

```
TC5
├── 1. what the user sees
├── 2. `sem`: constructions, the static class, a member of a class
├── 3. values: `Instance`, and its two sources
├── 4. dispatch: the one place an engine's answer is replaced
├── 5. `this`
├── 6. the shared scenario
├── 7. fixtures and tests
└── 8. steps
```

Depends on TC2 (TC3 for locals; TC4 only for the mixed rows). No core change. This is the phase people mean by "context-sensitive":
a call through an interface shows the class that was passed.

## 1. What the user sees

Patterns C1–C6 of [02](02-patterns.md); line f of the tour; the scenario `two-callers`.

| Code | Before | TC5 |
|---|---|---|
| `function emit(w: Writer) { w.write(); }` (an interface) · `emit(new JsonWriter())` | unresolved (D43) | `emit` → `JsonWriter.write` |
| the same with `Writer` a class and `write` overridden | `emit` → `Writer.write` | `emit` → `JsonWriter.write` |
| `emit(this.json)`, the field declared `JsonWriter` | unresolved | `emit` → `JsonWriter.write` |
| `class Job { run() { this.step(); } }` · `new Upload().run()` | `Job.run` → `Job.step`, or unresolved if `step` is abstract | `Job.run` → `Upload.step` |
| `const w: Writer = new JsonWriter(); w.write();` | unresolved: the checker answers the declared type | `JsonWriter.write` |
| `function emit(w) { w.write(); }` in a `.js` file · `emit(new JsonWriter())` | `Unresolved(any_or_untyped)` | `emit` → `JsonWriter.write` |
| `emit(flag ? new JsonWriter() : new CsvWriter())` | unresolved | both, two nodes at one site |

## 2. `sem`

| New | What | From tsrs |
|---|---|---|
| `Expr::New(site)` | the value of a `new` expression: an instance of the class the site's **engine answer** constructs. A `new` whose class is a value (TC4's `Class`) gives an instance of that class | the site's own resolution |
| `Expr::This` | `this`, in a method, a constructor, an accessor, an object literal's method, and in the anonymous functions and arrow defs inside them | the syntax; `ast::get_this_container` |
| the **static class** of an expression | for each argument and each receiver of the table: the project class its static type is an instance of, if it is one class | `get_type_at_location`, `ObjectFlags::Class`, `Type::symbol` → the class's def |
| `Sem::member(class, name)`, asked through the `World` trait ([03](03-design.md) §15.4), so `eval` still needs no engine | what `name` is on an instance of `class`: a def with a body (the class's own, or the nearest base class's), a signature without a body, external, or nothing | `get_property_of_type_exported` on the class's instance type; the declaration → the def, as a callee is resolved today |

## 3. Values

`One::Instance { class, exact }`: an object of this class. No fields (TC8), no identity.

It has two sources, and they are not equally strong:

| Source | The claim | Rests on |
|---|---|---|
| `new C(…)` followed to here | it **is** a `C` | the value the layer followed |
| the static class of an expression the layer has **no** value for | the checker says it is a `C`; a subclass may be behind it | the type. The same strength as today's row `class-instance`, which already answers `w.write()` with `C`'s method when `w: C` |

The second is D27's idea from Python ("where the walk holds nothing, the engine's exact type"), used for an argument instead of a
callee. It is what lets `emit(this.json)` resolve inside `emit` before fields exist. Both give a complete value, **and the value
says which it is**: `exact: true` for the first row, `false` for the second. They are different values and different keys from the first
day, because adding the flag later would move every digest that holds an instance. Python keeps the same two apart
(`OriginKey::is_built_here`, the only ground for a `Basis::Path` edge).

On the edge: core's `Basis` is already per edge (`Path`, `Type`, `Name`, `Likely`). What cannot say it is the flat solver's `Target`, which
stamps the language's one `BASIS` on every edge. Letting a `Target` carry its basis is a small core change with a note of its own, the same
one Rust's plan lists for RC3 ([`04-rust/02-architecture.md`](../../04-rust/02-architecture.md) §9, row 3). Until it lands, every edge of
the layer has `Basis::Type`, which is true of the weaker source and not false of the stronger. When it lands, an edge that rests on an
exact instance is `Basis::Path`.

An interface, a type literal, a union of classes, `any`: no static class. (A union could give a set; not before a count asks.)

## 4. Dispatch

At a site whose callee is `Member(r, m)`:

```
v = eval(r)                                  or {Instance(static class of r)} when v has no known member
for each known member of v:
    Instance(C)   sem.member(C, m):   a def with a body     → Call(def, key with this = {Instance(C)})
                                      external               → External
                                      a signature, nothing   → the engine's answer for this site
    Object        TC4's member read
+ the engine's answer, if v is open
```

**This is the one place the layer replaces an answer the engine gave** ([03](03-design.md) §7): the engine says `Writer.write` because
`w` is declared `Writer`; the value says the object is a `JsonWriter`, whose class has its own `write`. The engine's edge is dropped
only when `v` is complete. "Never fewer edges" is checked accordingly: such a site must still have a target, and the target must be
the same method or an override of it in a subclass of the engine's class (a test asserts that relation for every replaced edge).

Never replaced: `super.m()`, a static method called on the class by name, a function called by name.

## 5. `this`

`Key.this` is the receiver's value. `Expr::This` evaluates to it.

| Where | `Key.this` |
|---|---|
| a method called on a tracked receiver (`r.m()`) | `eval(r)`, or its static class |
| a constructor reached by `new C()` or `<C />` | `{Instance(C)}`: the class being **built**, also in a base class's constructor reached through `super(…)`. So `this.setup()` in the base constructor finds the subclass's `setup`, as JavaScript does |
| a method of an object literal, called on the literal | the `Object` value (TC4) |
| an arrow def inside a method (`const later = () => this.flush()`) | captured like a variable: `this` is a member of `needs` (TC3) |
| a function called with no receiver, a static method, an entry | unknown |

`this` is in the key only when relevant, by TC2 §8's rule: the body calls a method on `this`, passes it, returns it, or a nested def
reads it. A method that never mentions `this` has one context for every receiver.

## 6. The shared scenario

`scenarios/two-callers` is the program of row 2 of §1. `expected.precise.tree` is **empty today** (its README: "The file is Python's to
write"). TC5 writes the tree the rules give, by hand, before running:

```
main
  read_json
    JsonWriter
    emit
      JsonWriter.write
  read_csv
    CsvWriter
    emit
      CsvWriter.write
```

`[check]` against Python's own answer for its program; the file is shared, so the commit says which language wrote it. TypeScript then
lists `two-callers` as precise (TC2's per-scenario level).

## 7. Fixtures and tests

`instance-interface` · `instance-two-callers` · `instance-base-class` (overridden) · `instance-inherited` (not overridden: the base's
method) · `instance-static-class` · `instance-declared-wider` · `instance-either` · `instance-abstract` · `instance-signature-only`
(a `declare class`: the engine's answer) · `instance-external-base` (a method inherited from a library class: `External`) ·
`instance-this-dispatch` · `instance-this-in-constructor` · `instance-this-in-arrow-def` · `instance-this-not-relevant` (one context) ·
`instance-literal-this` · `instance-class-value` (`new C()` with `C` a parameter, then a method) · `instance-javascript` ·
`instance-entry` (asked at `emit`: today's tree).
Added by the review: `instance-exact-and-static` (`emit(new Json())` and `emit(this.json)`: two keys, the same edges today, and the flag in
both digests).

| Test | |
|---|---|
| the three properties on every fixture | |
| every replaced edge is an override of the engine's target (§4) | rule 5, for the one case that removes an edge |
| the default policy rows `interface`, `class-instance`, `union` | unchanged |
| `scenarios/two-callers` at `Level::Precise` | the shared definition of done |
| the tour's line f | its final column |
| the four projects | the per-site measure; contexts per function (N6 again, with instances) |

## 8. Steps

| # | Do | Proof |
|---|---|---|
| 1 | `sem`: `New`, the static class, `Sem::member` | table snapshots; unit tests of `member` on the `extends` fixtures that exist |
| 2 | `Instance`; `eval`: `New`; the static-class fallback | `instance-interface`, `instance-static-class`, `instance-declared-wider` |
| 3 | dispatch at member-call sites; the replaced-edge test | `instance-base-class`, `instance-inherited`, `instance-external-base` |
| 4 | `Key.this`, `Expr::This`, the constructor's `this`, `this` in `needs` | `instance-this-*` |
| 5 | the precise tree of `two-callers` | the scenario passes |
| 6 | the four projects | `ENGINE.md` |
| 7 | (mine) `policy.md`, `ENGINE.md`; Q-TC4 put to you with the numbers | reviewed |

Next: [`10-tc6-return-values.md`](10-tc6-return-values.md)
