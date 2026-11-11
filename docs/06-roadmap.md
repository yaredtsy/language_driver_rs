# 06. The roadmap

```
roadmap
├── 1. the picture: one bootstrap, three tracks
├── 2. B0: the bootstrap, step by step
├── 3. what blocks what
├── 4. a suggested order for one person
├── 5. what V-NOC gets at each tag
└── 6. what is deliberately not on the roadmap
```

## 1. The picture

```
B0  bootstrap ── repo skeleton · analyzer-core · analyzer-testkit · CI · the first scenarios
 │
 ├── PYTHON      PY0 freeze ─ PY1 skeleton ─ PY2 syntax ─▶ python-v0.1.0 ─ PY4 vnoc switches
 │               └─ PY5 analysis ─ PY6 core renderer ─ PY7 cli + golden ─▶ python-v0.2.0 ─ PY8 archive
 │
 ├── TYPESCRIPT  TS0 spikes ─ TS1 outline ─ TS2 markers ─▶ typescript-v0.1.0
 │               └─ TS3 project ─ TS4 call graph ─▶ typescript-v0.2.0 ─ TS5 driver needs ─▶ typescript-v0.3.0
 │
 └── RUST        RS0 spikes ─ RS1 outline ─ RS2 markers ─▶ rust-v0.1.0
                 └─ RS3 project ─ RS4 call graph ─▶ rust-v0.2.0 ─ RS5 driver needs ─▶ rust-v0.3.0

later, not scheduled:   C1–C6 context sensitivity for TypeScript (03-typescript/08) · C / C++ · the solver moving into core
```

After B0 the three tracks touch each other only through `core/` and `scenarios/`. That is what the layout is for.

## 2. B0: the bootstrap

The goal of B0 is a repo where "add a language" is a known, small job, and where the shared contract exists as code with tests.

| # | Do | Proof |
|---|---|---|
| 1 | `git init` in `language_driver_rs/`; `LICENSE` (Apache-2.0), `NOTICE`, `.gitattributes`, `.gitignore`, `rust-toolchain.toml` (1.99.1), `rustfmt.toml`, `clippy.toml`; a README that says what the repo is in its first line ([`00-start-here/02`](00-start-here/02-the-name.md) §2) | the first commit |
| 2 | `core/` workspace with `analyzer-core`: `text` (from pylspt `lines` + `positions`), `outline` (types), `edit` (`Edit`, `apply`, `MarkerStyle`) | `cd core && cargo test` |
| 3 | `analyzer-core::graph` and `render`, moved from pylspt ([`05-python-migration/02`](05-python-migration/02-what-moves-where.md) §2). Render tests on **hand-written graphs**: one test per rule 7, 8, 9, and one per mark | the rules of [`02-tree-rules/02`](02-tree-rules/02-the-rules.md) §3 exist as tests |
| 4 | `analyzer-core::solve`: the `Lang` trait, the flat solver, `Limits`. Tested with a **toy language**: a `Lang` whose "program" is a `HashMap<&str, Vec<&str>>` | the solver builds the right `Graph` for a straight line, a diamond, direct and mutual recursion, a cut by `max_contexts` |
| 5 | `analyzer-testkit`: `syntax::*`, `scenarios::run`, `graph::check_invariants` | used by the toy language's own tests |
| 6 | `scenarios/`: the first four folders (`straight-line`, `two-callers`, `recursion-direct`, `external-call`), with the **Python** source and both expected trees. The trees come from pylspt today (run `pylspt-dev calls` on them); the `default` trees are written by hand | each folder has a README naming the rule it pins |
| 7 | `languages/_template/` and `scripts/new-language.sh`; `scripts/check-all.sh --static` | running the script makes a folder that builds |
| 8 | `.github/workflows/ci.yml` with `changes`, `core`, `repo`, `ci-ok`; the language matrix is empty but wired | green on an almost empty repo. This is why CI comes early: Windows line-ending and path problems are cheap to fix now |
| 9 | the two **[check]** experiments of [`01-monorepo/02`](01-monorepo/02-workspaces-and-toolchain.md) §2: a core member inheriting a workspace dependency, built from a language workspace; and a crate outside the repo depending on a template crate by `git` + `rev` | both build. If either fails, the layout changes **now**, while it holds nothing |
| 10 | `CONTRIBUTING.md`: setup, sparse checkout, the test loop, how to recreate `../language_clone/` | a second machine can follow it |

> **🦀 Rust note: testing a generic solver with a toy language.** `Solver<L: Lang>` cannot be tested without some `L`. Do not wait for
> TypeScript: write the smallest possible one inside the core's tests.
> ```rust
> struct Toy(HashMap<&'static str, Vec<&'static str>>);       // function name → the names it calls, in order
>
> impl Lang for Toy {
>     type Func = &'static str;                                // &'static str is Copy + Eq + Hash: a fine handle
>     type Key = ();
>     fn digest(&self, _: &()) -> [u8; 16] { [0; 16] }
>     fn describe(&self, f: &'static str) -> FuncInfo { FuncInfo::for_test(f) }
>     fn walk(&mut self, f: &'static str, _: &(), _: &Cancel) -> Summary<Self> {
>         let sites = self.0.get(f).into_iter().flatten().enumerate()
>             .map(|(i, callee)| SiteCalls { site: Site::at(i as u32), targets: vec![match self.0.contains_key(callee) {
>                 true => Target::Call(*callee, ()),
>                 false => Target::Unresolved,
>             }] })
>             .collect();
>         Summary { sites, complete: true }
>     }
> }
> ```
> Twenty lines, no engine, runs in milliseconds, and it is the proof that the trait is implementable before two real languages depend on it.
> It also gives a second toy with `type Key = u8` for free: a fake "context-sensitive" language that checks the solver really keys its memo by
> `(Func, Key)`.

## 3. What blocks what

| This | Needs first | Why |
|---|---|---|
| any language's phase 1 (outline) | B0 steps 1, 2, 5, 7, 8 | the types, the testkit, the template, CI |
| any language's call graph | B0 steps 3, 4, 6 | the graph, the solver, the scenarios |
| PY1 | PY0 | the frozen baseline |
| PY4 (vnoc switches) | `python-v0.1.0` | |
| **TS4** | B0; **ideally PY6** | PY6 runs the shared renderer against 41 real fixtures. Without it, TypeScript is the renderer's first real user, and a renderer bug looks like a TypeScript bug. Not a hard block: B0's hand-written tests cover the rules |
| TS5 steps 1–2 (budget, cancel in core) | B0 step 4 | they are core work; can be done any time after B0 |
| RS4 | TS4 | Rust is the second user of the flat solver: that is when the rule of two tests the core's shape |
| RS5 | TS5 steps 1–2 | Rust reuses them |
| V-NOC's TypeScript driver | `typescript-v0.1.0` **and** the V-NOC-side items for a second language: drivers chosen by file extension, an open `DefKind`, the comment `style` (`06-drivers.md` §3) | those are `vnoc` work, not this repo's |
| context sensitivity (C3+) | TS5, numbers from real projects, and a decision | [`03-typescript/08`](03-typescript/08-later-context-sensitive.md) §5 |

## 4. A suggested order for one person

You said to start with the monorepo, then TypeScript, and to write the Python migration. Python's first slice is small, is what V-NOC 0.1
already waits for, and is the first real test of the core. So my suggested interleave is:

```
 1.  B0                         the repo exists; the contract is code
 2.  PY0 · PY1 · PY2 · PY3      python-syntax: the first real user of core's outline types and the testkit        → python-v0.1.0
 3.  PY4                        vnoc's Python driver switches                                                     (V-NOC 0.1 unblocked)
 4.  TS0                        the four spikes: real numbers replace my guesses
 5.  TS1 · TS2                  typescript-syntax                                                                 → typescript-v0.1.0
 6.  PY5 · PY6                  the solver moves; the core renderer is proven on 41 fixtures
 7.  TS3 · TS4                  the TypeScript call graph, on a proven renderer                                   → typescript-v0.2.0
 8.  PY7 · PY8                  Python finished; pylspt archived                                                  → python-v0.2.0
 9.  TS5                        budget, expand, cancel, cache                                                     → typescript-v0.3.0
10.  RS0 … RS5                  Rust, start to end, mostly reuse
```

If you would rather see TypeScript first, swap 2–3 with 4–5; nothing breaks, and B0's toy language stands in for Python as the core's first
user. That choice is **Q-M2**. What I would not do is start TS4 before either PY6 or a careful review of B0 step 3.

Sizes, relative: B0 is M. PY2 is M (the PA plan calls it "the real work"). PY5 is M–L, mostly moving files. PY6 is S–M. TS0 is S. TS1 is M.
TS2 is S. TS3 is M. TS4 is L. TS5 is M. Rust as a whole is about TS1–TS4 at two thirds the size. **[measure]**: replace these with real numbers
after TS0 and PY2.

## 5. What V-NOC gets at each tag

| Tag | V-NOC can |
|---|---|
| `python-v0.1.0` | drop its direct ruff dependencies; the Python driver is a thin adapter (V-NOC 0.1) |
| `typescript-v0.1.0` | show `.ts` / `.js` files in the tree and canvas; attach docs; keep them across renames |
| `python-v0.2.0` | Python call trees from the polished repo (V-NOC 0.2) |
| `typescript-v0.2.0` | TypeScript call trees, whole |
| `typescript-v0.3.0` | call trees that open level by level, cancel, and reuse work |
| `rust-v0.1.0` … | the same three steps for Rust |

And, because every language returns the same core types, `vnoc` can write its conversion code once
([`01-monorepo/06`](01-monorepo/06-versions-and-vnoc.md) §4): the second and third Rust-written drivers are mostly a `main.rs`.

## 6. What is deliberately not on the roadmap

| Not planned | Why |
|---|---|
| context sensitivity for TypeScript or Rust | measure first ([`03-typescript/08`](03-typescript/08-later-context-sensitive.md) §5) |
| moving Python's solver into core | only when a second language needs it (rule of two) |
| incoming calls ("who calls this?") | the V-NOC contract does not ask for it. Both engines have it; the graph type already supports it (edges can be read backwards) |
| C and C++ | no engine chosen; the PA plan's direction (clangd as a child process) would not use this repo's library shape at all |
| a single CLI for all languages | it would link three engines into one binary, against [`01-monorepo/02`](01-monorepo/02-workspaces-and-toolchain.md) §1 |
| publishing to crates.io | the engines under Python and TypeScript are not published |
| release automation | by hand until a second person tags releases (row R13) |

Next: [`07-decisions-and-questions.md`](07-decisions-and-questions.md)
