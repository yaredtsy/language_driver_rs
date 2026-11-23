# two-callers

Pins **rule 2**: the difference between a default resolver and a context-sensitive solver.

`emit(w: Writer)` calls `w.write()`. `read_json` passes it a `JsonWriter`, `read_csv` a `CsvWriter`.

- **Default** (`expected.default.tree`, TypeScript today): the call is resolved from the declared type of `w`, so both callers show `Writer.write`.
  Each caller also shows the class it constructs (a construction with no constructor of its own is a leaf).
- **Precise** (`expected.precise.tree`, Python): the argument's type is followed into `emit`, so `read_json` shows `JsonWriter.write` and `read_csv` `CsvWriter.write`.
  The file is Python's to write; it is empty until then.

**Rust** (`rust/`, RS4 step 9): `emit<W: Writer>(w: &W)` is generic, and `write` is a **required** trait method (no body). Since
D44 (RS4 step 9b) the call is `Unresolved(function_typed_value)`, still counted `open_type_param`, so Rust's own tree,
`expected.default.rust.tree`, shows `emit` as a leaf under each caller: the default level draws nothing where the type does not say
which `write` runs. The TypeScript program differs on purpose: its `Writer` is a class whose `write` has a body, so its default tree
still shows `Writer.write`. Rust's precise level comes with layer 2 (RC1 follows the type argument into `emit`, and draws
`<JsonWriter as Writer>::write` under `read_json`).

**Bend** (`bend/`, BD4 step 4) has no receiver to dispatch on, so the nearest program is a **template parameter**: `emit(~write)` calls
whatever `~write` its caller hands it, `read_json` passes `~json_write` and `read_csv` `~csv_write`. At the default level
(`expected.default.bend.tree`) each reader shows `emit` and the def it hands over as a passed edge (`json_write [Passed]`); `emit`'s own
call through `~write` is `Unresolved(function_typed_value)`, which the tree does not draw. The precise level (BC1, the first phase of Bend's
context layer) resolves `write` per call site, as Bend's compiler itself does for a template, and draws `json_write` under `read_json`'s
`emit`.
