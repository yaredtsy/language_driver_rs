# two-callers

Pins **rule 2**: the difference between a default resolver and a context-sensitive solver.

`emit(w: Writer)` calls `w.write()`. `read_json` passes it a `JsonWriter`, `read_csv` a `CsvWriter`.

- **Default** (`expected.default.tree`, TypeScript today): the call is resolved from the declared type of `w`, so both callers show `Writer.write`.
  Each caller also shows the class it constructs (a construction with no constructor of its own is a leaf).
- **Precise** (`expected.precise.tree`, Python): the argument's type is followed into `emit`, so `read_json` shows `JsonWriter.write` and `read_csv` `CsvWriter.write`.
  The file is Python's to write; it is empty until then.

The Rust program (`rust/`) is not written yet.
