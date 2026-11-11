# TypeScript and JavaScript qualnames

This is the naming rule of `typescript-syntax`. **It is frozen at the first tag (`typescript-v0.1.0`).** A qualname is stored in users' files:
adding a kind of def is safe, changing how an existing def is named is not. Every line below is pinned by a snapshot in
`crates/typescript-syntax/tests/snapshots/`; when this file and a snapshot disagree, the snapshot is what the code does.

## What a qualname is

```
qualname  =  the names of the enclosing names, joined, then the def's own name

joiner    "."            between a container and its member          C.m · N.f · api.load · Writer.write
          ".<locals>."   between a function and something declared   outer.<locals>.inner
                         inside its body
```

The qualname never contains the file path and never contains `export`. A name is **the source text as written**: quotes, brackets, `#` and
escapes are kept (`C."my-key"`, `C.[Symbol.iterator]`, `C.#secret`, `C.ascii`). One exception: **a string name that is a valid identifier loses
its quotes**, because `"load"() {}` and `load() {}` declare the same member. `"load"`, `'single'` and `"class"` (a reserved word is a valid member
name) become `load`, `single`, `class`; `"my-key"`, `"a.b"`, `"1st"`, `""` and a string with an escape in it (`"caf\u00e9"`) keep their quotes, so
a quoted name can never be mistaken for a path. The same holds wherever a string names something: a member, the key of a nested literal, a
`declare module`. `name_range` is always the whole token, quotes included.

**A qualname is not unique in a file.** A getter and its setter, a static and an instance member, overload signatures without an implementation,
`"load"` next to `load`, and methods of two anonymous literals all share one. The n-th def of a qualname in source order is its `occurrence`
(1-based), and the marker is what tells two of them apart for good. A tool that takes a qualname from a person lists every match and asks for
the marker when there is more than one; it never picks the first.

A **function** (function, method, constructor, accessor, arrow) opens a `.<locals>.` scope. A **container** (class, interface, namespace, object
literal that has a name) joins with `.`. A block (`if`, `try`, `for`, `switch`, `{ }`) adds nothing. An **anonymous** function, class or object
literal adds nothing: what is inside is named from what is around it.

## What is a def

| Syntax | Kind | Qualname | Fixture |
|---|---|---|---|
| `function f() {}` (also `async`, `*`, `export`, `declare`) | Function | `f` | `functions.ts` |
| `export default function () {}` | Function | `default` | `functions.ts` |
| `export default function f() {}`, `export default class Widget {}` (the export has a name) | Function / Class named by **its own** name, never `default` | `f`, `Widget`, `Widget.render` | `module.mjs`, `component.tsx` |
| `function f(a: string): …;` then `function f(a: number): …;` then `function f(a) {}` | **one** Function (the implementation); the range starts at the first signature | `f` | `functions.ts` |
| overload signatures with **no implementation** (`.d.ts`, `declare function`) | one Function per signature, same qualname | `f`, `f` | `declarations.d.ts` |
| `class C {}` (also `abstract`, `export`, decorated) | Class | `C` | `classes.ts` |
| `export default class {}` | Class | `default` | `classes.ts` |
| a method, `constructor`, `get x()`, `set x()`, `static m()`, `abstract m();` in a class | Method | `C.m`, `C.constructor`, `C.x` | `classes.ts` |
| getter and setter of one property | two Methods, both `C.x` | | `classes.ts` |
| a static and an instance member of one name | two Methods, both `C.m` | | `names.ts` |
| overloaded methods with an implementation | **one** Method | `C.m` | `classes.ts` |
| code inside `static { }` | what is inside is named under the class | `C.f` | `classes.ts` |
| `const f = () => {}`, `const f = function () {}` | Function | `f` | `arrows.ts` |
| `const C = class {}` | Class | `C` | `arrows.ts` |
| `const f = function g() {}` (the expression has its own name) | Function named by **its own** name | `g` | `arrows.ts` |
| a named function or class expression anywhere: `run(function step() {})` | Function / Class | `step` (`outer.<locals>.step` inside `outer`) | `arrows.ts` |
| `handle = () => {}`, `static make = function () {}`, `Inner = class {}` in a class | Method / Class | `C.handle`, `C.make`, `C.Inner` | `arrows.ts` |
| a method or accessor in an object literal | Method | `api.load` | `object_literals.ts` |
| a nested literal `{ nested: { deep() {} } }` | Method | `api.nested.deep` | `object_literals.ts` |
| `export default { m() {} }` (an anonymous default export) | Method | `default.m` | `object_literals.ts` |
| a string key: `"load"() {}`, `"my-key"() {}`, `"nested": { deep() {} }` | Method | `api.load`, `api."my-key"`, `api.nested.deep` | `names.ts` |
| an interface, and each method signature in it | Interface, Method | `Writer`, `Writer.write` | `types.ts` |
| overload signatures in an interface | one Method per signature, same qualname | `Overloaded.parse` ×2 | `types.ts` |
| `enum E {}`, `const enum E {}` | Enum | `E` | `types.ts` |
| `type T = …` | Type | `T` | `types.ts` |
| `namespace N {}` / `module N {}` | Namespace | `N`, and `N.f` inside | `types.ts` |
| `namespace A.B.C {}` | three Namespaces | `A`, `A.B`, `A.B.C` | `types.ts` |
| `declare module "lodash" {}` (the name is a valid identifier) | Namespace, without the quotes | `lodash`, and `lodash.chunk` inside | `names.ts` |
| `declare module "@scope/pkg" {}`, `declare module "external-package" {}` | Namespace, name keeps its quotes | `"@scope/pkg"`, and `"@scope/pkg".f` inside | `names.ts`, `types.ts` |
| `declare global {}` | Namespace | `global` | `types.ts` |

## What is not a def

| Syntax | Why |
|---|---|
| `let f = () => {}` / `var f = function () {}` | the engine's rule names only `const`: a `let` can be reassigned, so its name is not stable |
| `const f = (() => {})` | the engine does not see through parentheses |
| `const { a } = …` (a destructured name) | no single name |
| an anonymous function passed as an argument: `xs.map(x => …)` | no name. Its calls belong to the enclosing def |
| a property holding an arrow or function in an object literal: `{ load: () => {} }` | the engine's rule does not accept it. Adding it later is safe |
| a class property with a private or computed name holding an arrow: `#h = () => {}`, `["k"] = () => {}` | the engine's rule accepts only a plain identifier |
| a field without a function, a parameter, a local variable | |
| `module.exports = function () {}`, `exports.f = function () {}`, `Foo.prototype.m = function () {}` | no name the engine accepts (`exports.f = function f() {}` is named `f` by its own name) |
| members of an enum, properties and call signatures of an interface | |
| the file itself | the file is already a node in V-NOC |

Removing a kind later orphans docs; adding one is safe. Where the answer was unsure, it is "no for now".

## Ranges, params, errors

| Field | Rule |
|---|---|
| `range` | from the first byte of the def's first decorator, modifier (`export`, `async`, `static`, `declare`) or keyword, to its last byte. The JSDoc and every other comment above it are **not** in it. `const f = () => {}`: the whole statement when it declares one variable, otherwise the declarator. An overloaded def: from its first signature. A class property: the property including its `;` |
| `name_range` | the name token; for an unnamed `export default` the `default` keyword; for a constructor the word `constructor` |
| `params` | the parameter names as written, on one line (`{ a, b }`, `[h, ...t]`); a rest parameter without its `...` |
| `syntax_error` | true when a parse error overlaps the def's range. An empty error range counts as one byte: when unsure, flag more. The def right after an error is often flagged too, because the parser reports the error at its first byte |
| offsets | UTF-8 bytes into the text as given (a byte-order mark counts; CRLF is two bytes) |

## Decided by the owner before the tag (2026-11-11)

- Methods of an **anonymous** object literal or class have no container name: `registerPlugin({ m() {} })` gives `m`, and a class returned from a method gives
  `Outer.method.<locals>.inner`. Two such defs can share a qualname (see "not unique" above).
- A default export with a name uses the name; `default` is used only when it has none.
- A string name that is a valid identifier loses its quotes; any other keeps them.

## Open

- An `export default () => {}` arrow is not a def (the engine does not accept it).
- A named expression inside a **parameter default value** or a **decorator argument** is not searched, so it is not a def (the body of a `static { }` block is searched).
