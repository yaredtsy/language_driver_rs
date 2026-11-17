# two-callers

Pins **rule 2**: the difference between a default resolver and a context-sensitive solver.

`emit(w: Writer)` calls `w.write()`. `read_json` passes it a `JsonWriter`, `read_csv` a `CsvWriter`.

- **Default** (`expected.default.tree`, TypeScript today): the call is resolved from the declared type of `w`, so both callers show `Writer.write`.
  Each caller also shows the class it constructs (a construction with no constructor of its own is a leaf).
- **Precise** (`expected.precise.tree`, Python): the argument's type is followed into `emit`, so `read_json` shows `JsonWriter.write` and `read_csv` `CsvWriter.write`.
  The file is Python's to write; it is empty until then.

**Rust** (`rust/`, RS4 step 9): `emit<W: Writer>(w: &W)` is generic, so the default resolver answers the trait's declaration,
`Writer::write`, counted `open_type_param`. Rust's own tree is `expected.default.rust.tree`: the same tree with Rust's names
(`Writer::write`, D30). Rust's precise level comes with layer 2 (RC1 follows the type argument into `emit`).
