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
