# 00.02 The name, and the two folders

```
the name
├── 1. the name: language_driver_rs (yours)
├── 2. one thing to know about the word "driver"
├── 3. where the name appears (almost nowhere)
└── 4. the two folders on your machine
```

## 1. The name

The repo is **`language_driver_rs`**. That is your choice, and it does what a name here must do: it says nothing about V-NOC (rule 1: an analyzer
knows no product) and nothing about one language.

## 2. One thing to know about the word "driver"

The V-NOC plan already uses "driver" for something specific: the small program in `vnoc/drivers/<lang>/` that speaks V-NOC's JSON-RPC wire. This
repo holds the layer **under** that: the analyzers.

```
vnoc/drivers/typescript/                     "the driver"     speaks the wire; about 500 lines; stays in vnoc
language_driver_rs/languages/typescript/     "the analyzer"   knows TypeScript; what this plan builds
```

So in these files:

- **driver** always means the program in `vnoc/drivers/<lang>/`;
- **analyzer** means a language's crates in this repo;
- **`language_driver_rs`** is only ever the repo's name.

The name is still fine: from outside, this repo is "what the language drivers are made of". The README's first line should say exactly that, so
a reader who knows the V-NOC plan is not confused. If you later want the drivers themselves to live here too, that is a real decision with a
cost (the driver changes with the protocol, the analyzer with the language), listed as **Q-M7** in
[`../07-decisions-and-questions.md`](../07-decisions-and-questions.md). This plan assumes they stay in `vnoc` (row L1, unchanged).

## 3. Where the name appears

Almost nowhere, on purpose (row M4):

| Thing | Name | Has the repo name in it? |
|---|---|---|
| language crates | `python-syntax`, `python-analysis`, `typescript-syntax`, `typescript-analysis`, `rust-syntax`, `rust-analysis` | no |
| developer CLIs | `python-cli`, `typescript-cli`, `rust-cli` | no |
| shared crates | `analyzer-core`, `analyzer-testkit` | no |
| tags | `python-v0.1.0`, `typescript-v0.1.0` | no |
| the git URL in a driver | `https://github.com/<owner>/language_driver_rs` | **yes, the only place** |

The crate names follow the pattern the PA plan chose (row PA3): `<lang>-syntax` and `<lang>-analysis`. None of these crates is published to
crates.io (the engines under them are not), so a name being taken there does not matter. Every crate has `publish = false`.

Because no crate carries the repo name, renaming the repo later costs one line per driver, and GitHub redirects the old URL.

## 4. The two folders on your machine

```
rust/
├── language_driver_rs/        THE REPO. `git init` here. Holds this plan in docs/, then the layout of 01-monorepo/01-layout.md
│   └── docs/
│
└── language_clone/            RESOURCES. Not a repo of yours, never committed anywhere.
    ├── tsrs/                  upstream clone, for reading       (oxc-project/tsrs at 13a31a4)
    └── rust-analyzer/         upstream clone, for reading       (rust-lang/rust-analyzer at 2a362a4)
```

- The clones are **for reading only**. `grep` in `../language_clone/tsrs` answers most "how does the engine do this" questions. Nothing is ever
  built from them: the build gets tsrs from a pinned git commit and rust-analyzer from crates.io
  ([`../01-monorepo/04-engines.md`](../01-monorepo/04-engines.md)).
- Keeping them **outside** the repo is the simplest arrangement: nothing to add to `.gitignore`, and no risk of committing hundreds of MB by
  accident.
- Keep each clone checked out at the commit the repo pins (`languages/<lang>/ENGINE.md` records it). Then the code you read is the code you
  build. When an engine is bumped, `git checkout` the same commit in the clone.
- Paths like `../language_clone/tsrs/crates/tsrs_lint/src/engine.rs` in these files are relative to the repo root and only work on a machine
  with this arrangement. They are reading hints, not links; `CONTRIBUTING.md` should say how to recreate the folder
  (`git clone` + `git checkout <rev>`).

Next: [`../01-monorepo/01-layout.md`](../01-monorepo/01-layout.md)
