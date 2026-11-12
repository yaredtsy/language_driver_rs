# 01.04 The engines

```
engines
├── 1. the three engines side by side
├── 2. Python: ruff + ty, vendored (unchanged from the PA plan)
├── 3. TypeScript: tsrs, a git dependency pinned by commit
├── 4. Rust: rust-analyzer's crates from crates.io
├── 5. ENGINE.md, and bumping an engine
└── 6. the facade rule: `sem.rs`
```

An **engine** is the compiler front end a language's analyzer sits on. We do not write parsers or type checkers. We write the thin layer that asks
an engine questions and turns the answers into `analyzer-core` types.

## 1. Side by side

| | Python | TypeScript / JavaScript | Rust |
|---|---|---|---|
| engine | ruff (parser) + ty (types) | tsrs (a Rust port of the TypeScript 7 compiler) | rust-analyzer |
| upstream | `astral-sh/ruff` | `oxc-project/tsrs` | `rust-lang/rust-analyzer` |
| licence | MIT | Apache-2.0 (derivative of TypeScript) | MIT OR Apache-2.0 |
| published to crates.io | no | no (`publish = false`) | **yes**, weekly, as `ra_ap_*` |
| meant to be used as a library | no (internals private) | no (an application; pull requests are closed) | partly (`ide` is a designed API; it still breaks weekly) |
| do we need a patch | **yes**: one file of `pub` wrappers | **not for phase 1** [check in TS0] | no |
| how we carry it | folder in the repo (`vendor/ruff`) | git dependency, `rev = …` | crates.io, exact version |
| size on disk | 42 MB in our repo | 0 in our repo; Cargo fetches about 80 MB once | 0 in our repo |
| incremental (salsa) | yes | no; it has its own snapshot layer (`tsrs_project`) | yes |

All three are **unstable as libraries**. That is the main engineering fact of this repo, and §6 is the answer to it.

## 2. Python: ruff + ty, vendored

Nothing changes from the PA plan (`02-target/03-vendored-ruff.md`): the folder is copied as is, the patch file is renamed from `vnoc.rs` to
`analyzer_ext.rs`, and the patch is documented. Two details change because of the monorepo:

- the folder is `languages/python/vendor/ruff/`, and `PATCHES.md` becomes `languages/python/ENGINE.md` so that every language has a file with the
  same name;
- `vendor/ruff/Cargo.toml` is ruff's own workspace root (that is how every `workspace = true` line inside the vendored crates resolves, as
  pylspt's `Cargo.toml` explains). So `languages/python/Cargo.toml` must say `exclude = ["vendor"]`, or Cargo will complain that the vendored
  crates sit inside our workspace's folder without being members of it. **[check]** in PY1.

## 3. TypeScript: tsrs, a git dependency

tsrs's own crates are well split, and what we need is already `pub` (the full list is in
[`03-typescript/01-what-tsrs-gives.md`](../03-typescript/01-what-tsrs-gives.md)). So there is no reason to copy 51 MB of crates into the repo.

```toml
tsrs_parser = { git = "https://github.com/oxc-project/tsrs", rev = "<full commit hash>" }
```

Rules:

- **Pin by `rev`, never by branch.** A branch moves; a build must not.
- **Every `tsrs_*` line uses the same `rev`.** They are one program cut into crates; mixing commits will not compile.
- **Record the TypeScript version the commit follows** in `ENGINE.md` (tsrs keeps it in `[workspace.metadata.typescript]`; today `7.1.0-beta`).
  It tells a user which TypeScript behaviour our call graph follows.

When a patch is needed one day (a private function we must call), the order of preference is:

1. write the few lines ourselves on top of `pub` API (often possible: tsrs is a port, and most helpers are small);
2. a fork on GitHub with **one commit** on top of upstream, used as the git dependency. The patch is then a real commit that can be rebased;
3. vendoring, as for ruff. Last, because it is the only one that grows our repo.

> **🦀 Rust note: what Cargo does with a git dependency on a big workspace.** Cargo clones the repo once into `~/.cargo/git/`, checks out the
> commit, and finds the package you named by searching for its `Cargo.toml`. It compiles **only** that package and what it depends on. tsrs has 38
> crates; `typescript-syntax` will compile six of them (`tsrs_core`, `tsrs_diagnostics`, `tsrs_spanmap`, `tsrs_ast`, `tsrs_scanner`,
> `tsrs_parser`). The upstream workspace's `[profile]` and `[workspace.lints]` tables do not apply to you: profiles come from *your* workspace root,
> and lints are capped for dependencies. So tsrs's strict Clippy rules and its fat-LTO release profile never reach our build.

## 4. Rust: rust-analyzer's crates from crates.io

rust-analyzer publishes its internal crates automatically, renamed with an `ra_ap_` prefix: `ra_ap_syntax`, `ra_ap_ide`, `ra_ap_hir`,
`ra_ap_load-cargo`, and so on. This is the supported way to use it as a library.

```toml
ra_ap_syntax = "=0.0.<N>"          # the fast crate needs only this
ra_ap_ide = "=0.0.<N>"             # the slow crate
ra_ap_load-cargo = "=0.0.<N>"
ra_ap_project_model = "=0.0.<N>"
```

Rules:

- **`=` pins, and one number for all of them.** A new version comes every week and may break any API.
- **[check]** in RS0 which `0.0.<N>` matches the commit in `../language_clone/rust-analyzer`, so the code you read is the code you build.
- A crate name with a prefix is awkward in code; rename at the dependency line: `syntax = { package = "ra_ap_syntax", version = "=0.0.<N>" }`,
  then `use syntax::ast;` reads like rust-analyzer's own source.

## 5. ENGINE.md, and bumping an engine

One file per language, the same headings in each, so that someone else can update an engine without asking you:

```markdown
# Engine: tsrs

Upstream:  https://github.com/oxc-project/tsrs
Pinned:    13a31a4…            (2026-11-11)     follows TypeScript 7.1.0-beta
Toolchain: needs Rust ≥ 1.99

## Crates we depend on
typescript-syntax:    tsrs_parser, tsrs_ast, tsrs_scanner, tsrs_core
typescript-analysis:  + tsrs_compiler, tsrs_checker, tsrs_tsoptions, tsrs_vfs

## Every engine item we call          ← generated: `grep -h "tsrs_" crates/*/src/sem.rs | sort -u`
tsrs_parser::parse_source_file_owned
tsrs_checker::Checker::get_resolved_signature_exported
…

## Patches
none

## Tests that guard the engine
crates/typescript-analysis/tests/sem_contract.rs: one test per `sem` function.

## How to bump
1. change `rev` in languages/typescript/Cargo.toml
2. cargo update -p tsrs_parser   (moves every tsrs crate; they share the rev)
3. cargo test; fix sem.rs only
4. update "Pinned" above
```

Bumping rules (the same for every engine; the PA plan's rules for ruff, generalised):

- **In a pull request that changes nothing else.** Then a red test points at the engine.
- **Not more than once per release** of that language, unless a bug forces it. All three engines move weekly; chasing them costs more than it
  gives.
- **The "every engine item we call" list is the review.** If the diff of `sem.rs` is empty, the bump is safe to approve on green tests.

## 6. The facade rule: `sem.rs`

Rule 7 from [`00-start-here/01`](../00-start-here/01-what-this-repo-is.md) §3: in each crate, **one module imports the engine**. pylspt already
works this way (`sem.rs`: "the only code that imports the vendored ty crates"), and it is why moving pylspt between ruff versions has been
survivable.

```rust
// typescript-analysis/src/sem.rs — sketch
use tsrs_checker::Checker;            // engine imports appear in this file and nowhere else
use tsrs_compiler::Program;

pub(crate) struct Sem { program: &'static Program, /* … */ }

/// A function-like declaration in the loaded program. Opaque outside this module.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Func(/* an engine node pointer */);

impl Sem {
    pub(crate) fn entry(&self, file: &str, qualname: &str) -> Option<Func> { … }
    pub(crate) fn body_sites(&self, f: Func) -> Vec<RawSite> { … }       // call expressions, in source order
    pub(crate) fn callee(&self, site: &RawSite) -> Callee { … }          // THE default resolver
    pub(crate) fn qualname(&self, f: Func) -> String { … }
    pub(crate) fn span(&self, f: Func) -> (String, Span) { … }           // file + range
    pub(crate) fn is_project_code(&self, f: Func) -> bool { … }
}
```

> **🦀 Rust note: a newtype as a wall.** `Func` wraps an engine type in a struct with a private field. Outside the `sem` module (`sem.rs`, or a `sem/` folder), code can copy a `Func`,
> compare two, and put one in a `HashMap`, because of the derives. It cannot look inside, because the field is private to the module. So the rest
> of the crate *cannot* call an engine method on it, even by accident: the compiler enforces rule 7. `scripts/check-all.sh` adds a belt to the
> braces: `grep -rl "tsrs_" crates/*/src` must print only `sem.rs` or files under `sem/`.

The cost of the facade is a little duplication (a `sem` function that forwards to one engine call). The gain is that an engine bump is a diff of
one file per crate, and that the context-sensitive upgrade later has one clear place where "ask the engine" lives
([`02-tree-rules/03-the-seam.md`](../02-tree-rules/03-the-seam.md) §3).

Next: [`05-ci.md`](05-ci.md)
