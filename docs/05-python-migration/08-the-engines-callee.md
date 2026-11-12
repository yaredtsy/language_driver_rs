# 05.08 Where the walk cannot call, the engine's exact answer is used

```
the engine's callee
├── 1. what is wrong, with the line that shows it
├── 2. the rule, and where it stops (D16, R2)
├── 3. the design
├── 4. the fixtures
├── 5. the step, with its failing tests and its proof
├── 6. what is not in this file
└── 7. to check while building
```

Written on 2026-11-12. §1 was **run** (the ignored helper `dump_graph_of_a_real_project`, on a Flask service of the owner, at `cd3c502`).
§3 to §5 are from reading the code: **not built**. Every `[check]` is a claim to verify before depending on it.

## 1. What is wrong

The line, in a method `post` of the owner's project:

```python
ret = run_coroutine_in_sync(
    require_session(self._post_common_logic)(user, filename, stream),
)
```

and the decorator, in another file:

```python
F = TypeVar("F", bound=Callable[..., Awaitable[Any]])

def require_session(func: F) -> F:
    @functools.wraps(func)
    async def wrapper(*args, **kwargs):
        ...
        return await func(*args, **kwargs)
    return cast(F, wrapper)
```

What the helper printed for the entry `post`:

```
  @455-495 -> core.sqlalchemy.session.require_session#… Name        the inner call: found
  @455-519 -> ? unknown_callee                                       the outer call: lost
…:368 require_session(self._post_common_logic)(user, filename, str <- ty: a def of the project
      [bound method Self@post._post_common_logic(user: …, filename: str, stream: BytesIO, session: Unknown) -> CoroutineType[…]]
```

So `_post_common_logic` and everything under it is missing from the tree, and the engine names it exactly.

**Why.** A call is evaluated in two halves (`interp/call.rs`, `eval_call`): the callee expression is evaluated to *values*, then the
values are called. Here the callee expression is itself a call, so its value is what the walk of `require_session` returned:

| Step | Where | What comes out |
|---|---|---|
| `return cast(F, wrapper)` | `call_values`: `cast` is a library function, so the engine is asked for the type of the call | the type `F`, a type variable |
| `lift` of that type | `interp/value.rs`: `Shape::Other` | `AbstractValue::Ty(F)` |
| back in `post` | `call_project` → `resolve` | the callee value is `Ty(F)`: the type variable **of the generic body**, not solved for this call |
| the outer call | `call_values`: `callee_definition(F)` is `None` | `unresolved = true` |

The walk carried a type out of a generic function, where it means nothing. The engine, asked at the call site, has solved `F` for this
call: it is the bound method. The walk never asks it there.

The same happens with the result in a variable (`g = require_session(self.work)` and later `g(…)`): the variable holds `Ty(F)`.

## 2. The rule

**At a call site, when the walk holds a callee value it cannot call, and the engine's type of the callee expression names defs and nothing
else, the engine's defs are the callees.**

It is "follow the engine" (CLAUDE.md), in the one place the walk had stopped following it. It is not a guess: rule 8 forbids naming a
target nobody knows, and here the engine knows it with no `Unknown` in the answer.

- **The walk first.** A value the walk can call (a function, a closure, a bound method, a class) is kept: the walk knows *which* closure or
  object it is, which the engine's type does not say. The engine is asked only for the values the walk cannot call.
- **By value, not by syntax.** The rule does not look at whether the callee is a name, an attribute or a call. It looks at what the walk
  holds. So `passthrough(f)()` and `g = passthrough(f)` … `g()` are one case.

**Where the line is.** The engine's type of the callee expression:

| The engine says | Example | Result |
|---|---|---|
| a `def` itself, a method of a `def` bound to an object, or a `class` itself (ty: `FunctionLiteral`, `BoundMethod`, `ClassLiteral` / `GenericAlias`), or a union of only these | `require_session(self.work)`: `F` solved to the bound method | **an edge** (this file) |
| a signature and no def: `Callable[…]`, a type variable that is not solved, a protocol, an object with `__call__` | a parameter `cb: Callable[[], None]`; `func(*args)` inside `wrapper` | `Unresolved`. This is D16's "function-typed value": a type, not a def |
| a def with `Unknown` beside it: a union with `Unknown`, an intersection `… & Unknown` | a method found after a base class that is not resolvable | `Unresolved`, until R2 is decided |
| `Unknown` | an untyped value | `Unresolved` |

So a type variable is on both sides: **solved at the call site** it is the def's own type and gives an edge; **inside the generic body**
it is a function-typed value and stays `Unresolved`. D16's counter (`function_typed_value`) is TypeScript's; Python has one reason,
`unknown_callee`, and this file adds no counter: nothing is cut and nothing is doubtful.

Rejected:

- **Model `cast` and `functools.wraps` in the walk**, so the value of `require_session(f)` is the closure `wrapper` holding `f`. It is what
  runs, and it would put `wrapper`'s own calls in the tree. But it special-cases two library functions by name, and every project has its own
  spelling (`cast`, `# type: ignore`, `wraps`, `update_wrapper`, `ParamSpec`). The engine's signature is the one statement that covers them.
- **Replace the returned `Ty(F)` wherever it flows**, not only where it is called. Values go into context keys, and a key is an identity
  outside the process: a wide change for one lost call. The callee position is the only place the value is used to find a def.

**What the tree then shows, and what it does not.** `post` gets two children: `require_session`, and `_post_common_logic` with its
calls. The body of `wrapper` (the session it opens) is **not** in the tree: nothing the walk holds calls `wrapper`. That is the
`functools.wraps` pattern of [`07-the-root.md`](07-the-root.md) §6, not scheduled.

## 3. The design

All of it is in `interp/call.rs`. `sem.rs` does not change: `shape`, `type_of` and `callee_definition` are there.

```rust
// in eval_call, right after the callee is evaluated
let callees = self.eval(sol, &call.func, env);
let callees = self.or_engine_callee(sol, &call.func, callees);
```

`or_engine_callee(sol, callee: &Expr, walked: Values) -> Values`:

1. A value **cannot be called** when it is `AbstractValue::Unknown`, or `AbstractValue::Ty(t)` with `callee_definition(t, file)` `None`.
   These are the two arms of `call_values` that set `unresolved`. Nothing else counts: not `Pending` (the fixpoint's), not an `Instance`
   or a `Module` (the engine's type for those is never a def), and not an empty list (a call Python never makes).
2. If no value of `walked` is one of those: return `walked` unchanged. The engine is not asked. This is every call site of today's
   fixtures that resolves.
3. `t = type_of(callee)`. It is **exact** when `shape(t)` is `Function`, `BoundMethod` or `Class`, or a `Union` whose members are all one
   of those. If it is not exact: return `walked` unchanged (the site stays `Unresolved`, as today).
4. Else: `walked` without the values of step 1, plus `lift(t)`, then `dedup`.

- `call_values` is not touched (it carries "moved from pylspt unchanged"). The values `lift` makes from an exact type are the ones it
  already takes: a project function, a project bound method, a class, or `Ty` of a library def (an `External` edge).
- `named` (what makes `Basis::Name`) is computed as today, by `names_a_def` on the same expression. So a function reached this way is
  `Name` and a bound method is `Type`, the same as when the walk finds them `[check]`.
- The bound method's receiver is the engine's (`lift` of the type it is bound to), not the walk's object. The edge is right; the context
  of the method may be a different one from a direct `self.work(…)` (§7, claim 7).
- Both modes go through `eval_call`, so inline mode and summary mode change together and the fixture cross-check between them holds.

**Rust note.** A closure that reads `sol` (`|v| … sol.sem.callee_definition(…)`) borrows `sol` for as long as the closure is alive.
`self.lift(sol, t)` needs `sol` mutably, and Rust refuses a mutable borrow while a shared one is alive. So finish with the closure first:
collect the values to keep into a new `Values`, let the closure go out of use, then call `lift`. If the compiler still complains, make the
test a small method (`fn cannot_call(&self, sol: &Solver<'db>, v: &AbstractValue<'db>) -> bool`) instead of a closure.

## 4. The fixtures

`languages/python/crates/python-analysis/tests/fixtures/10-engine-callee/python/`. No `# expect:` blocks: read by `tests/api.rs`, so
`every_entry` skips them and the `(35, 129)` count does not move.

`wrap.py` (shared by the cases):

```python
import functools
from typing import Any, Callable, TypeVar, cast

F = TypeVar("F", bound=Callable[..., Any])


def passthrough(func: F) -> F:
    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        return func(*args, **kwargs)

    return cast(F, wrapper)


def leaf():
    pass
```

One file per case. Each starts with `from wrap import leaf, passthrough` (only the names it uses).

| File | The program | Entry |
|---|---|---|
| `method.py` | `class Service:` with `def work(self, n): leaf()` and `def run(self): passthrough(self.work)(1)` | `Service.run` |
| `function.py` | `def helper(): leaf()` · `def run(): passthrough(helper)()` | `run` |
| `variable.py` | `def helper(): leaf()` · `def run():` `g = passthrough(helper)` then `g()` | `run` |
| `klass.py` | `class Thing:` with `def __init__(self): leaf()` · `def run(): passthrough(Thing)()` | `run` |
| `either.py` | `def a(): leaf()` · `def b(): pass` · `def run(flag): passthrough(a if flag else b)()` | `run` |
| `callback.py` | `from typing import Callable` · `def run(cb: Callable[[], None]): cb()` | `run` |
| `unknown_beside.py` | `def helper(): leaf()` · `def run(flag): passthrough(helper if flag else missing)()`, where `missing` is defined nowhere | `run` |

## 5. The step

One commit. Tests first, seen red for the stated reason.

### R3. The engine's callee

| | |
|---|---|
| Change | §3, in `interp/call.rs`. Nothing else in the crate changes |
| Failing tests | `tests/api.rs`, project `10-engine-callee`, with `call_graph`. **Red today, each because the outer call is an `Unresolved` edge:** (a) `method.py`: `stats.unresolved` is 0; `run` has an edge to a context of `Service.work`, basis `Type`; the tree is `run` → `passthrough`, `Service.work` → `leaf`. (b) `function.py`: an edge to `helper`; say in the report which basis it has. (c) `variable.py`: the call `g()` has an edge to `helper`. (d) `klass.py`: a `Construct` edge of `Thing`, with `leaf` under its `init`. (e) `either.py`: the one site has two edges, to `a` and to `b`, and no `Unresolved`. **Green before and after (they pin where the rule stops):** (f) `callback.py`: exactly one `Unresolved` edge, and no edge to a context. (g) `unknown_beside.py`: the outer call is `Unresolved`, and no edge goes to `helper` from that site |
| Docs | D27 in `docs/07-decisions-and-questions.md`: the rule of §2 and its table, in three lines, and that D16's line for Python is "the engine's type is a def itself, or it is a signature". [`07-the-root.md`](07-the-root.md) §6: the second row says "R3, [`08-the-engines-callee.md`](08-the-engines-callee.md)". `docs/README.md`: the line for this file. The doc comment of `names_a_def` and of the new function say which shapes each accepts |
| Proof | `cargo test` in `languages/python`: the new tests, and the 129 fixture entries with **unchanged snapshots** (if one moves, stop and report which site and what it became: do not accept it). `cargo clippy -p python-analysis --all-targets -- -D warnings`. Then, by hand, the project of §1 (the owner gives the folder; it is not written in the repo): `PYTHON_DUMP_PROJECT=<dir> PYTHON_DUMP_FILE=app/api/file.py PYTHON_DUMP_ENTRY=FilesResource.post PYTHON_DUMP_UNRESOLVED=1 cargo test -p python-analysis --lib dump_graph -- --ignored --nocapture`: line 368 is no longer in the unresolved list, the site `@455-519` goes to a context of `_post_common_logic`, and the graph has more than the 7 contexts it had |

After R3 is pushed and CI is green, the V-NOC driver takes the new `rev`. That is a step of `vnoc`.

## 6. What is not in this file

| Thing | Why not | Where it goes |
|---|---|---|
| The body of the wrapper (`wrapper`'s own calls) | the walk holds nothing that calls it; needs the `functools.wraps` pattern | [`07-the-root.md`](07-the-root.md) §6, not scheduled |
| A def with `Unknown` beside it (`(bound method …) & Unknown`) | it relaxes rule 8; the owner decides | R2 |
| A callee that is `Pending` when the walk meets it, and resolves in the fixpoint to a value that cannot be called | the replay has no expression to ask the engine about (`DeferredOp::Call`, `call: None`). The fix would be to keep the engine's exact callee in the `DeferredOp` | only if claim 6 of §7 shows a real site |
| A counter | nothing is cut and the answer has no doubt in it; before and after is read with the helper's tally | — |
| The walk's own object as the receiver of a method found this way | the engine's type does not say which object | with constructor effects, if a real project needs it |

## 7. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | In a fixture project, the engine types `passthrough(self.work)` as the bound method `work` (the type variable solved to the def's own type, not widened to its bound) | R3, done, test (a): yes. `passthrough(self.work)(1)` is the bound method `Service.work`; the edge is `Type`, and `Service.work` → `leaf` is in the tree |
| 2 | For `passthrough(helper)` it is the function itself, and the edge's basis is `Name` | R3, done, test (b): yes. The edge is to `helper` with basis `Name`: `names_a_def` asks ty for the callee expression, which is the function itself |
| 3 | For `passthrough(Thing)` it is the class itself (`Shape::Class`), not `type[Thing]` | R3, done, test (d): yes. A `Construct` edge of `Thing` with `Thing.__init__` as `init`, basis `Name`, so the engine's type was the class itself |
| 4 | For `passthrough(a if flag else b)` it is a union of the two functions | R3, done, test (e): yes. Two edges at the one site, to `a` and to `b`, both `Name`, and no `Unresolved` |
| 5 | With an undefined name in one branch the type has `Unknown` in it and is not exact | R3, done, test (g): the outcome holds. The outer call stays `Unresolved` and nothing goes to `helper` from that site (green before and after). The engine's type was not printed, so "has `Unknown` in it" is inferred from the type not being exact |
| 6 | No existing snapshot moves: no site of the 129 entries has a value the walk cannot call where the engine names a def exactly | R3, done: no snapshot moved and `(29, 6)`, `(35, 129)` hold. A probe (not committed) showed the engine step is reached 410 times in the whole test run and replaces something only in the five new tests. 50 calls reach `or_engine_callee` with a `Pending` among the walked values: fixtures do have a callee that is `Pending` at `eval_call`, and none of them changed (a `Pending` is not a value that cannot be called) |
| 7 | The context of `Service.work` reached through `passthrough` and the one reached by a direct `self.work(1)`: the same or two? | R3, done (scratch copy, not committed): **the same one.** `passthrough(self.work)(1)` and a direct `self.work(1)` in one method give one `Service.work` context (one digest), both edges `Type` |
| 8 | `callee_definition` of a parameter typed `Callable[[], None]` is `None`, so `callback.py` is `Unresolved` today | R3, done, test (f): yes. One `Unresolved` edge, green before and after |

Next: [`../06-roadmap.md`](../06-roadmap.md)
