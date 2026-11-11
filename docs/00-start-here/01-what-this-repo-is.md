# 00.01 What this repo is

```
what this repo is
├── 1. the four layers, and which one this is
├── 2. what an analyzer offers
├── 3. what an analyzer never does
├── 4. what "phase 1" means for TypeScript and Rust
└── 5. why one repo now (the earlier plan said no)
```

## 1. The four layers

This is the picture from the PA plan, with one change: the bottom layer is now one repo for all languages.

```
extension    knows VS Code                                   vnoc/extension
core         knows V-NOC (IDs, .vnoc/)                       vnoc/crates
driver       knows V-NOC's wire + calls an analyzer          vnoc/drivers/<lang>      ← stays in vnoc
analyzer     knows one language. Nothing else.               language_driver_rs/languages/<lang>   ← this repo
```

The driver stays in `vnoc` (row L1 of the V-NOC plan, unchanged). It changes when the protocol changes. The analyzer changes when the language or
its engine changes. Those are different speeds, so they are different repos.

## 2. What an analyzer offers

Every language offers the same three things, in this order of cost:

| | Input | Output | V-NOC tier | Needs |
|---|---|---|---|---|
| **outline** | the text of one file | every definition: name, qualified name, kind, range, the marker text if one is written | 1 | a parser |
| **markers** | text + a def + the marker text | one edit: where the marker line goes | 2 | a parser |
| **call graph** | a project folder + an entry function | a graph of contexts and call edges, and the tree rendered from it | 3 | a type checker |

The first two are fast (milliseconds, one file, no project). The third is slow (it loads a project). So each language has **two library crates**,
split by cost, exactly as the PA plan split Python:

```
<lang>-syntax       parser only                        builds in seconds
      ▲
<lang>-analysis     + the type checker / project        builds in minutes
```

## 3. What an analyzer never does

These are the six rules from the PA plan (`01-why/01-playground-to-product.md` §3). They apply to every language here.

| # | Rule |
|---|---|
| 1 | It knows no product: no `vnoc`, no `FileID`, no ID format. The marker's label (`ID`) is a parameter. |
| 2 | It never writes a file and never invents an ID. Text in, data out. |
| 3 | Its public API is chosen: modules are private, `lib.rs` exports by name. |
| 4 | It has tests that run in CI on Linux, macOS and Windows. |
| 5 | No personal paths, no commented-out code, no output files in git. |
| 6 | A stranger can build it from the README in one command. |

One more, new with this repo:

| 7 | **Only one module per crate imports the engine.** It is called `sem` (as in pylspt). Everything else in the crate uses `sem`'s functions. When tsrs or rust-analyzer changes an API, one file changes. |

## 4. What "phase 1" means for TypeScript and Rust

pylspt's call tree is **context-sensitive**: the same function can appear with different callees depending on what was passed to it. That took a
solver of about 4,600 lines on top of ty.

For TypeScript and Rust the first version does **not** do that. It asks the engine "what does this call expression refer to?", the same question
"Go to Definition" and "Show Call Hierarchy" ask, and takes the answer. I call this the **default resolver**.

```
def emit(w):            # Python, context-sensitive (pylspt today)
    w.write()           #   called with JsonWriter → JsonWriter.write
                        #   called with CsvWriter  → CsvWriter.write

function emit(w: Writer) {     // TypeScript, default resolver (phase 1)
    w.write();                 //   → Writer.write, whoever called emit
}
```

The default resolver is still useful: most calls in typed code name one function. And it is cheap: the engines already do it.

The plan's main design job is to make sure phase 1 is a **smaller case of the same design**, not a different design. That is
[`02-tree-rules/03-the-seam.md`](../02-tree-rules/03-the-seam.md).

## 5. Why one repo now

The PA plan compared three arrangements and called "one `analyzers` repo for all languages" the worst (row PA8). Its two reasons were:

1. the analyzers share almost no code, so a shared repo gives nothing back;
2. one lockfile and one CI would have to satisfy ruff, rust-analyzer and a TypeScript engine at once.

You have now chosen one repo. I think that is right **today**, because both reasons can be removed, and something changed:

| Reason then | Now |
|---|---|
| nothing to share | there **is** something to share: you want every language to build the tree by pylspt's rules. The graph type, the renderer, the marks, the limits and the test scenarios are one thing, used three times. In three repos they would be three copies that drift |
| one lockfile for all | one **workspace per language**: each has its own `Cargo.lock` and engine pins ([`01-monorepo/02`](../01-monorepo/02-workspaces-and-toolchain.md)) |
| one CI for all | jobs run **by path**: a TypeScript change never builds ty ([`01-monorepo/05`](../01-monorepo/05-ci.md)) |

What the monorepo still costs, honestly:

| Cost | How big | What limits it |
|---|---|---|
| a Rust contributor clones Python's vendored ruff (42 MB) | one time per clone | `git sparse-checkout` is documented in CONTRIBUTING; the clone is still far smaller than rust-analyzer's own |
| `vnoc`'s driver build downloads the whole repo at the pinned commit | once per pin, then cached by Cargo | same as today for pylspt |
| a tag is for the whole repo | naming only | tags carry the language: `typescript-v0.1.0` |
| all languages must use one Rust toolchain | real | the newest one any engine needs; see [`01-monorepo/02`](../01-monorepo/02-workspaces-and-toolchain.md) §3 |

If one language ever needs to leave (its own licence, its own team), a workspace folder is easy to lift out with `git filter-repo`, because it
depends on nothing but `core/`.

Next: [`02-the-name.md`](02-the-name.md)
