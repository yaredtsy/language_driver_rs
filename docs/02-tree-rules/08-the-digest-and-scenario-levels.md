# Note: one digest function in core, and the scenario level per scenario

Status: **decided** (D57 for the digest; [`03-typescript/12-context-layer/06-tc2-functions-through-parameters.md`](../03-typescript/12-context-layer/06-tc2-functions-through-parameters.md)
§7 and [`04-rust/02-architecture.md`](../04-rust/02-architecture.md) §9 row 2 for the level). Shape written by the reviewer on 2026-11-24;
**built as written by TypeScript's TC2 step 1 (`d36d7b9`, reviewed 2026-11-24;
[`languages/typescript/engine/09-tc2.md`](../../languages/typescript/engine/09-tc2.md)).** Written from reading `core/analyzer-core/src/graph.rs`, `core/analyzer-testkit/src/scenarios.rs`,
Python's `interp/canon.rs` and `stable.rs`, and Bend's `key.rs`. The pinned bytes of §1 were computed by the reviewer before the build
(**[ran]**: a throwaway crate, `xxhash-rust` 0.8.19 and 0.8.21, `serde_json` 1.0.151 and 1.0.154, all four the same bytes).

```
the digest and the scenario level
├── 1. the digest: what core fixes and what stays the language's
├── 2. the scenario level per scenario
├── 3. what does not change
└── 4. proofs
```

## 1. The digest

A context's identity outside the process is `ContextId { func, digest }`. Python (`interp/canon.rs` `stable_id`) and Bend (`key.rs`
`digest`) each make the 16 bytes the same way: the key written in a canonical form, serialized with `serde_json::to_vec`, hashed with
`xxhash_rust::xxh3::xxh3_128`, written little-endian. TypeScript's TC2 is the third user, so by the rule of two the function moves to core
(D57):

```rust
// core/analyzer-core/src/graph.rs, next to `ContextId`
pub fn digest<T: Serialize + ?Sized>(canonical: &T) -> [u8; 16]
```

**What core fixes**, for every language that calls it: the bytes hashed are `serde_json::to_vec(canonical)` (compact JSON, no spaces), the
hash is XXH3-128 with seed 0, and the result is `u128::to_le_bytes`. It panics when `canonical` cannot be written as JSON (a map whose keys
are not strings, a `Serialize` that fails), as Python's and Bend's copies do: a canonical key holds neither, and a failure there is a bug in
the language, not an input.

**What stays the language's:**

- **The canonical form.** Its types, its names, its order. Python writes a marked def as `{"Node":"…"}` (`stable::hashed_def`, pylspt's
  bytes); core's `StableDef` derive writes `{"Marked":"…"}`, which Bend uses and TypeScript will. Both are fine: the function hashes what it
  is given. Switching a language to core's function changes none of its digests (D57), because the bytes hashed and the hash are the ones it
  already uses.
- **The open key's zeros.** "Whatever was passed" is `[0; 16]` by each language's rule, never `digest(&nothing)`. The function never
  returns zeros on purpose.
- **Growth.** A field a later phase adds is skipped when empty (`#[serde(skip_serializing_if = "Vec::is_empty")]`), so a key that does not
  use it keeps its digest. Core's test shows the pattern on a test-local struct; each language pins its own variants. When an existing
  digest must change all the same, the language bumps its own `CONTEXT_ID_VERSION`.

**Pinned bytes** (core's test, on CI's Linux, macOS and Windows). With `StableDef` as core derives it, and a test-local canonical form
`enum Canon { Unknown, Fn(StableDef), Set { known: Vec<Canon>, open: bool } }`, `struct Key { params: Vec<Canon> }`:

| Key | JSON hashed | `digest` |
|---|---|---|
| `params: [Fn(Path{"src/a.ts","handler",0}), Unknown, Set{[Fn(Marked("abc"))], open}]` | `{"params":[{"Fn":{"Path":{"module":"src/a.ts","qname":"handler","nth":0}}},"Unknown",{"Set":{"known":[{"Fn":{"Marked":"abc"}}],"open":true}}]}` | `[222, 81, 217, 167, 216, 56, 119, 221, 205, 106, 67, 102, 68, 16, 248, 74]` |
| `params: []` | `{"params":[]}` | `[122, 58, 146, 60, 100, 231, 89, 134, 25, 59, 41, 249, 84, 241, 149, 151]` |
| the first, as a struct with a second field `captured: []` skipped when empty | the same as the first | the same as the first |
| the first, with `captured: [Unknown]` | the first's JSON with `,"captured":["Unknown"]` before the closing brace | `[197, 253, 104, 34, 64, 12, 233, 198, 225, 138, 19, 150, 9, 170, 124, 236]` |

And the algorithm itself: `xxh3_128(b"")` is `0x99aa06d3014798d86001c324468d497f`, the published XXH3-128 test vector. A change to
`StableDef`'s serialized form changes the first row: that is the point, every stored id would move.

**Dependencies.** `analyzer-core` takes `serde_json` as a normal dependency (it was a dev-dependency) and `xxhash-rust = { version = "0.8",
features = ["xxh3"] }`, both in core's `[workspace.dependencies]`. In the languages' lockfiles: Python, TypeScript and Bend already have
both (0.8.19, 0.8.19, 0.8.21), so only `analyzer-core`'s dependency list changes; Rust gains one package, `xxhash-rust`, which has no
dependencies of its own. Nothing else may move.

## 2. The scenario level per scenario

`analyzer_testkit::scenarios::run(subject, level)` holds every scenario of a language to one level, so a language can switch to its precise
trees only all at once, at the end of its layer. A layer that becomes precise one scenario at a time (TypeScript from TC2, Rust from RC1)
needs the level per scenario:

```rust
// core/analyzer-testkit/src/scenarios.rs
pub enum Levels {
    /// Every scenario at one level: what `run(subject, Level::Default)` has always meant.
    All(Level),
    /// The scenarios named here at `Level::Precise`, every other at `Level::Default`.
    PreciseFor(&'static [&'static str]),
}
impl From<Level> for Levels { … }                // `Levels::All`

pub fn run(subject: &Subject, levels: impl Into<Levels>) -> usize
```

Every existing call (`run(&BEND, Level::Default)`, …) compiles and behaves as before. A name in `PreciseFor` that is not a scenario with a
program in the subject's folder is a failure, after the scenarios ran: a stale or misspelt list would otherwise hold nothing to the precise
tree and pass. The subject's `graph` function is the language's: a language that runs its layer passes a subject whose `graph` turns the
layer on, so the default trees of the scenarios it is not precise for are checked **with the layer on**, which is the "never fewer edges,
nothing changed where the layer knows nothing" property on the shared programs.

## 3. What does not change

No graph, tree, wire or digest of any language: nothing calls `digest` yet (TypeScript's TC2 step 4 is its first caller; Python and Bend
switch in a session of their own, D57), and `Levels::All` is today's behaviour.

## 4. Proofs

Core's tests (the pinned bytes, the levels); TypeScript's and Bend's tests; every lockfile current (`cargo tree --locked` in each
workspace, which `scripts/check-all.sh --static` runs for core, Rust and Bend). Python's and Rust's tests run on CI when the branch is
pushed: on 2026-11-24 neither had a build directory on the reviewer's machine and the disk could not hold one (5 GB free). Their code
does not call anything this note adds.
