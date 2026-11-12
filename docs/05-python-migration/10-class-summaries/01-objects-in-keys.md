# 05.10.01 Objects in keys (CS1): built

```
objects in keys
├── 1. what it does
├── 2. pylspt's decisions, and where each one is in the code
├── 3. what this repo changed
├── 4. the fixture and its test
├── 5. what it does not do
└── 6. to measure
```

From pylspt's `04-class-summaries/01-objects-in-keys.md`. **This step is built**: it came over with the solver (PY5). Nothing to build
here. This file records what is there, so the later steps can rely on it. Checked against the code at `e55ab37`.

## 1. What it does

A method called on an object goes through a summary. The object goes into the callee's key as **its class plus its attributes'
fingerprints**, to `fp_depth` (2) steps. The callee's walk starts from an object rebuilt from the key, named by its place among the
inputs. Constructors work in summary mode, without `__init__`'s writes (those are effects, step 3).

```python
class Box:                       # no __init__: the caller writes the state
    def run(self): self.handler.process()

def json_box(): b = Box(); b.handler = JsonHandler(); b.run()   # Box.run > JsonHandler.process
def csv_box():  b = Box(); b.handler = CsvHandler();  b.run()   # Box.run > CsvHandler.process
```

`json_box` keys `run` with `self = Instance { Box, [handler: Instance { JsonHandler, [] }] }`, `csv_box` with `CsvHandler`: two keys, two
walks, two different children.

## 2. pylspt's decisions, and where each one is in the code

| # | Decision | Code |
|---|---|---|
| 1 | An object in a key is its class plus its attributes, sorted by name. Fingerprinting reads the caller's env at the call | `Fp::Instance { class, attrs }` (`key.rs`); `Solver::key(…, env: &Env)`; `Env::attrs_of` (a scan of the flat map, sorted) |
| 2 | Two steps deep, then the class only. An object with no state is always `Opaque(class)` | `fp_one`: `!origin.holds_state() \|\| depth == fp_depth` → `Fp::Opaque`. `Limits::fp_depth` (`mod.rs`), its comment as pylspt asked: "steps from a parameter (attribute, receiver, free value, default), in keys and in summaries" |
| 3 | Where an object was built is not in the key: two equal objects share one summary | the key has no origin. `p04_01_in_keys` asserts `shared` gives 3 walks and 1 reuse |
| 4 | The callee's walk rebuilds input objects from the key, named by path | `OriginKey::Entry { act, path }` (`value.rs`), `Path { root: Param \| Free \| Default, steps: Attr \| Free \| Default \| Recv }`; `Walk::materialize` / `from_fp` (`key.rs`). The annotation fallback in `bind.rs` makes `Entry` origins too, `act` kept so inline mode's runs stay apart (`chains.py` `separate`) |
| 5 | A method call goes through a summary: the receiver is slot 0 | `call_project(…, receiver: Option<AbstractValue>, …)` hands it to `bind_for_callee` (`call.rs`) |
| 6 | A constructor makes a `New { site, act }` origin and keys `__init__` with it as `self` | `Walk::construct_summary` (`call.rs`); `Target::Construct { class, init, basis }` |
| 7 | Render draws a construct as its class | **superseded**: the tree is core's `render` over `Graph`; a construct is `EdgeTo::Construct` (`core_graph.rs`) |
| 8 | A pending value inside an object defers the call (two halves: look as deep as the key before keying; a replay keys with this round's values) | `Walk::reaches_pending`, `needs_defer` (`call.rs`); `Walk.now` (`walk.rs`), read by `fp_one`'s `Pending` arm, with the `debug_assert` pylspt asked for |

## 3. What this repo changed

| pylspt | Here | Why |
|---|---|---|
| `from_fp` writes a key's attributes with a weak `bind_attr` | `Env::join_attr`: joined, **no `Unknown` added** (`11e20bb`) | since `570ab69` a weak write onto an absent attribute adds `Unknown`. Rebuilding from a key is not a write: nothing was missed |
| `Target::Construct { class, init }` | `… basis }`, and `Target::Call(ctx, basis)`, `Target::External`, `Target::Unresolved` | D28 and the graph's edges: every target says what it rests on |
| render (decision 7) | core's renderer | PY6 |
| a classmethod's receiver is `Class(c)` | the same; and the engine's `type[X]` lifts to `Class(X)` (`Shape::SubclassOf`, counted `lifted_subclass_of`) | the engine's answer, D7 |

## 4. The fixture and its test

`tests/fixtures/04-class-summaries/python/in_keys.py`, the same file as pylspt's. `p04_01_in_keys` (`interp/fixture.rs`) runs it with
`check_both` (inline, summary, summary without reuse) and asserts `shared`'s walks and reuses. Its mode-specific blocks pin three
differences, all expected:

| Entry | Inline | Summary | Why |
|---|---|---|---|
| `deep` | `JsonHandler.process` | nothing | the read is three steps from `self`; the key stops at two (§5) |
| `again` | nothing | `log` | summary mode keys `Runner.run` with the round's value of `r.cb` (decision 8); inline mode stops at the recursion guard |
| `again_closure` | nothing | `log` | the same, for a closure's free value |

## 5. What it does not do

- **Reads past `fp_depth`.** `self.app.config.handler` is three steps from `self`: the key holds `self.app.config` as `Opaque(Config)`,
  the walk finds no `handler` in the state, and the class side through the engine knows only the declared type. Inline mode finds it.
  Writes that deep reach the caller by path (step 5); reads need the key.
- **Identity.** Two equal objects share one summary: the callee cannot tell them apart. Step 2 gives identity back through paths, without
  the key.
- **A union at one path** shares one origin (`from_fp` on `Fp::Union`); step 6's split removes most of it.

## 6. To measure

| # | Measure | When |
|---|---|---|
| 1 | `fp_depth` 2 against 3, on the fixtures and the three real projects: edges found, contexts, walks, time. pylspt's plan (`15` § "`fp_depth = 2` loses reads at depth 3") says to set the default from these numbers; it was never done | after CS4, when objects cross calls in both directions (before it, a deeper key finds little a summary can carry back) |

Next: [`02-objects-coming-back.md`](02-objects-coming-back.md)
