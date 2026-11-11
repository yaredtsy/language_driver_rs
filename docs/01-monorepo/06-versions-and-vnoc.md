# 01.06 Versions, and how vnoc uses this repo

```
versions and vnoc
├── 1. git workflow (the same as vnoc's)
├── 2. tags: one line of versions per language
├── 3. what a version promises
├── 4. how a driver pins an analyzer
└── 5. a change that touches both repos
```

## 1. Git workflow

The same as `vnoc` (V-NOC release plan, row R1): one `main` branch, short-lived branches, squash merges, conventional pull-request titles. The
scope in the title is the language or `core`:

```
feat(typescript): outline lists object-literal methods
fix(core): renderer marks a cut parent
chore(python): bump ruff to <commit>
```

That scope is what makes a per-language changelog possible later, without any tooling today.

## 2. Tags

A git tag names a commit of the whole repo, but each language moves at its own speed. So the tag carries the language:

```
python-v0.1.0        typescript-v0.1.0        rust-v0.1.0
python-v0.2.0        typescript-v0.2.0
```

- `core/` gets **no tags**. Nobody depends on it directly; it reaches a driver through a language crate, at whatever commit that language's tag
  points to.
- No version numbers are edited in `Cargo.toml` files. Every crate says `version = "0.0.0"` and the tag is the version, as row R10 decided for
  `vnoc`.
- No release automation yet (row R13: "no release-please in the analyzer yet"). A tag is made by hand when a driver needs a stable point.

## 3. What a version promises

What can break a driver is small, because a driver uses only `lib.rs` of two crates and the `analyzer-core` types.

| Change | Version step |
|---|---|
| a new function, a new field on a `#[non_exhaustive]` struct, a new `DefKind` | minor: `0.1.0 → 0.2.0` |
| a bug fix; an engine bump with the same output on the scenarios | patch: `0.1.0 → 0.1.1` |
| **the qualname rule of a language changes** | this is the one that hurts users: qualnames are stored in `.vnoc/` and are part of how a node is found again (V-NOC plan `09/02` §2). Treat it as a breaking change of the *product*, not of the crate: it needs a migration note in `vnoc`, whatever the number says |
| the policy table of a language changes (a rule in [`02-tree-rules/02`](../02-tree-rules/02-the-rules.md)) | minor, with the scenario files changed in the same pull request. Call trees are recomputable, so no stored data breaks |

Before 1.0 every minor step may break the Rust API. That is acceptable while the only users are your own drivers, pinned by tag.

## 4. How a driver pins an analyzer

```toml
# vnoc/drivers/typescript/Cargo.toml
[workspace]                                   # its own workspace: its own lockfile and toolchain file (as drivers/python today)

[dependencies]
typescript-syntax   = { git = "https://github.com/<owner>/language_driver_rs", tag = "typescript-v0.1.0" }
# added when the driver reaches tier 3:
typescript-analysis = { git = "https://github.com/<owner>/language_driver_rs", tag = "typescript-v0.2.0" }
vnoc-driver-protocol = { path = "../../crates/vnoc-driver-protocol" }
```

- **A tag at release, a `rev` between releases** (row R13, unchanged).
- Both lines of one driver must point at the **same** commit, or Cargo builds `analyzer-core` twice and the types do not match.
- The driver names **no engine crate**. Today's Python driver names three `ruff_*` crates because pylspt has no facade; that goes away with
  the migration ([`05-python-migration/03-steps.md`](../05-python-migration/03-steps.md), PY4).
- `drivers/typescript/rust-toolchain.toml` pins 1.99.1.

What the driver does with the analyzer's answer is conversion only:

```
analyzer_core::outline::Def     →  vnoc_driver_protocol::Def         (marker → id · Span → UTF-16 Range via core's LineIndex)
analyzer_core::edit::Edit       →  vnoc_driver_protocol::TextEdit
analyzer_core::graph::Graph     →  the callTree wire shape           (decided with V-NOC 0.2; the old FrameJson lives here if it lives at all)
```

Because every language returns the same core types, **those conversions can be one shared module in `vnoc`** (for example
`vnoc/drivers/common/`), used by every Rust-written driver. A new language's driver is then the JSON-RPC loop plus three function calls. That
module belongs to `vnoc`, not here: it knows the wire.

## 5. A change that touches both repos

The order is always analyzer first:

```
1. language_driver_rs:  change + tests → merge to main
2. vnoc:   point the driver at the new `rev` (local work: [patch] to your language_driver_rs folder, uncommitted)
3. vnoc:   driver change + conformance test → merge
4. language_driver_rs:  tag when vnoc releases
```

`vnoc`'s nightly builds every driver from source (row R8), so a break between the repos shows up within a day in one place.

Next: [`../02-tree-rules/01-graph-and-tree.md`](../02-tree-rules/01-graph-and-tree.md)
