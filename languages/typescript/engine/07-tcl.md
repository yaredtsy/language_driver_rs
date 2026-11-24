# TCL: a function a library made

Part of `languages/typescript/ENGINE.md` (split out on 2026-11-24, text unchanged). Sections in the order they were written.

## TCL as built and measured (`aa32042`, fix `fbee1b4`; reviewed 2026-11-24)

**What it is** (D56, `docs/03-typescript/12-context-layer/11-later.md` §4.1, policy row `library-made`). A `const` name bound straight
from a call whose resolved signature is declared in a package is called as an `External` named after the call as written and the path
to the name: `react` · `useState()[1]`, `zustand` · `create()()`. `sem` answers its own variant `Resolved::LibraryMade { package, maker,
path }` (`Sem::library_made`, seven conditions); `walk::target` is the one function that draws it, today as `External`, and counts
`library_made_function`. The breakdown prints the count when it is above 0.

**What the engine answered, checked at review:**

- The maker's declaration is `get_resolved_signature_exported(call).declaration()`; its written return type is `Node::type_node()`, an
  accessor clippy's downcast ban allows. A type reference is read through the new view `TypeReference` (`type_name`), the seventeenth view
  in the totality walk. `get_symbol_at_location_exported` answers on the type name of a type reference in a `.d.ts`, and a type parameter's
  symbol has `SymbolFlags::TypeParameter`: that is condition 7.
- `f.bind(x)` never reaches the rule: with `strict`, the engine answers `const b = f.bind(x); b()` as a `Call` of `f` itself.
- TypeScript's bundled library hands back what the project stored (`Map.get`, `Array.shift`: `V | undefined`, a union condition 7 does
  not see). It is left out by condition 5 (`package == "typescript:lib"`). At `aa32042` it made all 6 of the private sample's
  library-made calls (`x.get()`, `queue.shift().resolve`), each a wrong answer.
- Spellings: the maker is its callee's text with `()`; a property of a package's anonymous type read through a variable is the existing
  named `External` with the receiver in front (`store.getState`); `handlers.get("a")` is `typescript:lib` · `Map.get`.

**Fixture** `tests/policy/src/library_made.tsx` with its own package `policy/node_modules/hooks`: 22 edges, `library_made_function` 5,
`function_typed_value` 7 (all `_from_call_result`): a `let`, a nested pattern, a project maker, `useCallback`, a selector, `getState`, and
`Map.get` stay unresolved. Reviewer: 383 pass, 11 ignored; breaking condition 5 and, separately, condition 7 each fails the three tests of
`tests/library_made.rs`.

**Before and after, run by the reviewer** (depth 1, `TS_SWEEP_FILE_CODE=1`; before = release sweep of `9717270`, after = dev sweep of
`fbee1b4`; dumps compared by hash, one toolchain):

| Project | Graphs | Graphs that differ | `function_typed_value` (of it `_from_call_result`) | `library_made_function` | unresolved |
|---|---|---|---|---|---|
| V-NOC frontend | 1,871 | 192 | 1,097 (824) → 476 (203) | 0 → 621 | 1,130 → 509 |
| the private codebase | 15,575 | 0 | 4,525 (1,376), unchanged | 0 | 27,039, unchanged |
| TypeScript 5.9.3 `src/compiler` (`c63de15`) | 12,136 | 0 | 7,301 (1,632), unchanged | 0 | 7,465, unchanged |
| vscode `src` | not run (owed with TC0's rows) | | | | |

On V-NOC the drop in unresolved is exactly the 621 named; the other origins (196 parameter, 70 other, 7 `this`) do not move. TCL names
75% of V-NOC's call-result column and 57% of its function-typed calls; outside React it names nothing.

**V-NOC's three sample entries** (whole graphs, dev CLI `calls … --graph`): library-made 620 at `aa32042` → 490 at `fbee1b4` (condition 7
removed the selectors, `useCallback`, `useEffectEvent`). By name: `create()()` 237, `useState()[1]` 193 + `React.useState()[1]` 11,
`useForm().…` 21, `cva()` 7, `memo()` / `forwardRef()` 6, `create()` 3, `useNavigate()` 3, `useSearchParams()[1]` 2, `useNodesState()[1]` /
`useEdgesState()[1]` 2, `createExtension()` 1, and the 4 store hooks without a selector that condition 7 misses (`(): ExtractState<S>`,
accepted in 11 §4.1). **`TaskDetailPanel`**: 141 call, 2 construct, 235 → 291 external, 86 → 30 unresolved (19 parameter, 8 call result,
3 other), 56 library-made, 18 passed, 90 contexts: the new standing numbers.
