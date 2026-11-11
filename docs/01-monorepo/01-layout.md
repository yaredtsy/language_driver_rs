# 01.01 The layout

```
layout
├── 1. the tree
├── 2. the four kinds of folder
├── 3. inside one language
├── 4. the root files
└── 5. adding and removing a language
```

## 1. The tree

```
language_driver_rs/
├── README.md                     what it is · the languages and their state · build one language in one command
├── CONTRIBUTING.md               setup · sparse checkout · the test loop · add a fixture · bump an engine
├── LICENSE                       Apache-2.0 (your answer to Q-R1)
├── NOTICE                        the engines' notices: ruff (MIT), tsrs (Apache-2.0, derived from TypeScript), rust-analyzer (MIT OR Apache-2.0)
├── rust-toolchain.toml           ONE toolchain for every workspace (02-workspaces-and-toolchain.md §3)
├── rustfmt.toml  clippy.toml     one style for every workspace
├── .gitattributes                LF everywhere; fixtures and snapshots never converted
├── .gitignore                    target/ · out/ · .DS_Store
│
├── core/                         WORKSPACE 0. No engine. Depends on serde and a hash crate only.
│   ├── Cargo.toml  Cargo.lock
│   ├── analyzer-core/            the shared answer: outline types · graph · renderer · the flat solver · text · edits
│   └── analyzer-testkit/         the shared tests: fixture runner · round trip · never panics · scenario runner
│
├── languages/
│   ├── python/                   WORKSPACE. Engine: ruff + ty, vendored.
│   │   ├── Cargo.toml  Cargo.lock
│   │   ├── ENGINE.md             the pin, the patch, how to bump (was PATCHES.md)
│   │   ├── crates/
│   │   │   ├── python-syntax/    FAST: outline · markers · header
│   │   │   ├── python-analysis/  SLOW: ty · the context-sensitive solver
│   │   │   └── python-cli/       the developer tool
│   │   ├── vendor/ruff/          the trimmed ruff + ty crates
│   │   └── docs/                 the solver's design (from pylspt/plan/03-call-tree)
│   │
│   ├── typescript/               WORKSPACE. Engine: tsrs, a git dependency pinned by commit.
│   │   ├── Cargo.toml  Cargo.lock
│   │   ├── ENGINE.md
│   │   ├── crates/
│   │   │   ├── typescript-syntax/
│   │   │   ├── typescript-analysis/
│   │   │   └── typescript-cli/
│   │   └── docs/
│   │
│   ├── rust/                     WORKSPACE. Engine: rust-analyzer's crates from crates.io (ra_ap_*).
│   │   └── … the same shape
│   │
│   └── _template/                copied by scripts/new-language.sh
│
├── scenarios/                    the SAME small program in every language, with the tree it must give
│   ├── two-callers/
│   │   ├── python/main.py
│   │   ├── typescript/main.ts
│   │   ├── rust/src/main.rs
│   │   ├── expected.default.tree     what a default resolver must give
│   │   └── expected.precise.tree     what a context-sensitive solver must give
│   └── …
│
├── docs/                         this plan now; the repo's docs later
├── scripts/                      new-language.sh · check-all.sh · list-languages.mjs
└── .github/workflows/            ci.yml · nightly.yml
```

## 2. The four kinds of folder

| Folder | Holds | Who changes it | Rule |
|---|---|---|---|
| `core/` | what every language returns, and the code that needs no engine | rarely, and with care: a change here touches every language | no engine crate, ever. `cargo tree` in `core/` must not show `salsa`, `ruff_*`, `tsrs_*` or `ra_ap_*` |
| `languages/<lang>/` | one language, complete: its crates, its engine pin, its lockfile, its docs | whoever works on that language | depends on `core/` and its engine. **Never on another language** |
| `scenarios/` | the cross-language tests | whoever adds a rule to [`02-tree-rules/02`](../02-tree-rules/02-the-rules.md) | a rule without a scenario is not a rule yet |
| `docs/`, `scripts/`, `.github/` | repo-level things | — | — |

The second row is the one that keeps the repo cheap: a language folder can be added, removed, or moved to another repo without touching the
others.

## 3. Inside one language

The same shape for every language, so nobody learns a new layout:

```
languages/typescript/
├── Cargo.toml                 [workspace] members = ["crates/*"] · the engine pin, written ONCE in [workspace.dependencies]
├── Cargo.lock                 committed
├── ENGINE.md                  upstream URL · pinned commit · which engine crates we use · patches (none, if none) · how to bump
├── crates/
│   ├── typescript-syntax/
│   │   ├── src/
│   │   │   ├── lib.rs         the facade: `pub use` by name, modules private
│   │   │   ├── sem.rs         the ONLY file that imports the engine (rule 7)
│   │   │   ├── outline.rs     what a def is, qualnames
│   │   │   ├── marker.rs      reading a marker, planning an edit
│   │   │   └── header.rs      where a file-header comment may go
│   │   └── tests/
│   │       ├── fixtures/      *.ts *.tsx *.js: one file per case
│   │       ├── snapshots/     written by the test, reviewed in the pull request
│   │       └── syntax.rs      three tests from the testkit: outline · round trip · never panics
│   ├── typescript-analysis/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── sem.rs         the only file that imports the checker
│   │   │   ├── project.rs     open a folder → a loaded project
│   │   │   ├── walk.rs        one body → its call sites
│   │   │   ├── key.rs         a context: function + key. Phase 1: the key is empty
│   │   │   └── calls.rs       implements the core's `Lang` trait; the public `call_graph` function
│   │   └── tests/             the scenarios, and language-only fixtures
│   └── typescript-cli/
│       └── src/main.rs        typescript-cli outline <file> · typescript-cli calls <dir> <file> --entry <name>
└── docs/                      decisions that are about this language only
```

`walk.rs` and `key.rs` have the same names and the same jobs as in pylspt's `interp/`. That is deliberate: when TypeScript becomes
context-sensitive, the work is to fill in those modules, not to add new ones
([`02-tree-rules/03-the-seam.md`](../02-tree-rules/03-the-seam.md)).

## 4. The root files

**`.gitattributes`**: without it, a Windows checkout turns `\n` into `\r\n` in fixtures, and every byte offset in a snapshot is wrong.

```
* text=auto eol=lf
**/tests/fixtures/**   -text
**/tests/snapshots/**  -text
scenarios/**           -text
languages/python/vendor/**  -text
```

**`.gitignore`**: `target/` (there will be several: one per workspace), `out/`, `.DS_Store`.

**`NOTICE`**: each engine has its own licence, and a driver binary built from this repo contains the engine. tsrs is Apache-2.0 and is itself a
derivative of TypeScript (its `NOTICE` says so), so its notice must travel with any binary. One file at the root lists all three, and each
`languages/<lang>/ENGINE.md` repeats its own.

**There is no `Cargo.toml` at the root.** That is the subject of the next file.

## 5. Adding and removing a language

| Job | Steps |
|---|---|
| **add** | `scripts/new-language.sh go` copies `languages/_template/` to `languages/go/`, renames the three crates, and adds the language to the CI matrix file. Then fill in `sem.rs`. The checklist is the phase list of [`03-typescript/README.md`](../03-typescript/README.md), which is the same for every language |
| **remove** | delete `languages/<lang>/` and its folders under `scenarios/*/`. Nothing else refers to it |
| **move out** | `git filter-repo --subdirectory-filter languages/<lang>`; in the new repo, change the `analyzer-core` path dependency to a git dependency on this repo |
| **bump an engine** | one pull request that changes only `languages/<lang>/` (the pin, `Cargo.lock`, `sem.rs` if an API moved). [`04-engines.md`](04-engines.md) §5 |

`scripts/list-languages.mjs` prints the folder names under `languages/` (minus `_template`). CI and the scripts read the list from there, so the
folder is the registry; there is no second list to keep in step. This is the same idea as row R11 in the V-NOC release plan (`drivers/*/` is the
list of languages).

Next: [`02-workspaces-and-toolchain.md`](02-workspaces-and-toolchain.md)
