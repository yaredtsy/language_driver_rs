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
