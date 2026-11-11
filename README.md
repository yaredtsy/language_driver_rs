# language_driver_rs

One repo that holds the analyzer of every language V-NOC supports. One Cargo workspace per language, plus a small `core` workspace for what they share.
There is deliberately **no** `Cargo.toml` at the root. See `docs/` for the plan.

| Folder | Holds |
|---|---|
| `core/` | the shared answer types, renderer and testkit (no engine) |
| `languages/<lang>/` | one language: its crates, engine pin, lockfile, docs |
| `scenarios/` | the same small program in every language, with the tree it must give |
| `scripts/` | `new-language.sh`, `list-languages.sh`, `check-all.sh` |

Build one language: `cd languages/<lang> && cargo test`.

## Languages

| Language | Crates | State |
|---|---|---|
| Python | `python-syntax` (fast), `python-analysis`, `python-cli` | `python-syntax` is built: outline, markers, file header. The call graph is not moved yet |
| TypeScript, JavaScript | `typescript-syntax`, `typescript-analysis`, `typescript-cli` | outline and markers are built. The call graph is not |
| Rust | `rust-syntax`, `rust-analysis`, `rust-cli` | not started |

## Python

`python-syntax` answers "which functions and classes are in this file, what are they called, and where would a marker go" from the text alone.
It parses with the vendored ruff parser and does not compile the type checker. Build and test it with one command:

```sh
cd languages/python && cargo test -p python-syntax
```

```rust
use python_syntax::{MarkerStyle, Placement, outline, plan_marker};

const STYLE: MarkerStyle<'static> = MarkerStyle { label: "ID", placement: Placement::Docstring };

let text = "class Order:\n    def total(self):\n        return 1\n";
let outline = outline(text, STYLE);
for def in &outline.defs {
    println!("{:?} {}", def.kind, def.qualname);
}
// Class Order
// Method Order.total

// The edit that marks the method. It is planned, never applied: the caller applies it.
let edit = plan_marker(text, &outline.defs[1], "3RGJ8M2K", STYLE).unwrap();
// inserting it (`analyzer_core::edit::apply(text, &[edit])`) gives
//
//     class Order:
//         def total(self):
//             """ID: 3RGJ8M2K"""
//             return 1
```

The example is a test (`tests/readme.rs`), and the test fails if this section stops matching the code. The names follow Python's `__qualname__`
without the module, and are frozen once `python-v0.1.0` is tagged: see [`languages/python/docs/qualnames.md`](languages/python/docs/qualnames.md)
and [`markers.md`](languages/python/docs/markers.md). The engine pin and how to bump it are in
[`languages/python/ENGINE.md`](languages/python/ENGINE.md).
