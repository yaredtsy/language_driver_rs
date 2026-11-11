# 03.02 TS0: spikes

```
TS0
├── 1. why spikes first
├── 2. the four spikes
│   ├── S-TS1  does it build as a dependency?
│   ├── S-TS2  can a long-running process parse without growing?
│   ├── S-TS3  load a real project and resolve its calls
│   └── S-TS4  can we find and edit the JSDoc of every kind of def?
├── 3. where the spike code lives
└── 4. what TS0 hands to the next phases
```

## 1. Why spikes first

Everything in [`01-what-tsrs-gives.md`](01-what-tsrs-gives.md) was read, not run. Four of its claims decide the design of TS1 to TS4. If one is
wrong, it is far cheaper to learn that from 50 lines of throwaway code than from a half-built crate. Your V-NOC plan used spikes the same way
(S2, S3 for the Python driver).

A spike is done when its question has an answer **written down**. Its code is not kept.

## 2. The four spikes

### S-TS1: does it build as a dependency?

| | |
|---|---|
| Question | Can a crate outside tsrs depend on `tsrs_parser` (and then on `tsrs_checker`, `tsrs_compiler`, `tsrs_ls`) by `git` + `rev` and compile on 1.99.1, on macOS, Linux and Windows? |
| Do | a new crate with one `main.rs` that parses a string and prints the number of statements. Then add the heavy crates and call `new_program` on a folder with one file |
| Write down | cold build time of the parser-only crate and of the full one **[measure]**; release binary size of each **[measure]**; any feature flag or environment variable the build needed; whether it runs without mimalloc installed as the global allocator |
| If the answer is no | the usual cause is something the upstream workspace sets that a dependent does not get (a `[patch]`, a `build.rs` that reads a workspace file). Then: fork with one commit, or vendor ([`01-monorepo/04`](../01-monorepo/04-engines.md) §3) |

### S-TS2: can a long-running process parse without growing?

| | |
|---|---|
| Question | Does parsing inside a `Region` really give the memory back? Is a scratch region the right kind? |
| Do | loop 10,000 times: make a region, enter it, parse a 2,000-line file, walk every node, build an owned `Vec<String>` of function names, leave. Print the process's memory every 1,000 rounds |
| Write down | memory at round 1,000 and round 10,000 (they must be about equal); the time per parse **[measure]**: the driver contract's target is "a few ms; under 50 ms for a 5,000-line file"; which constructor was used (`Region::new`, `new_scratch`) and why |
| Also try | parse **without** a region and watch memory grow. That is the bug the rule protects against; seeing it once makes the rule real |
| If the answer is no | parse in a short-lived child process per batch (ugly, safe), or ask upstream. Unlikely: tsrs's own watch mode and its Node API depend on regions |

> **🦀 Rust note: reading a process's memory in a spike.** No crate is needed for a rough number. On macOS and Linux:
> ```rust
> fn rss_kb() -> u64 {
>     let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().unwrap();
>     String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
> }
> ```
> It starts a process, so call it every 1,000 rounds, not every round. For the real test in TS3 use a proper crate (`memory-stats`) so that it
> also runs on Windows.

### S-TS3: load a real project and resolve its calls

| | |
|---|---|
| Question | Is "program + checker + `resolve_call_hierarchy_declaration`" enough to get per-site callees, and what does it cost? |
| Do | pick one real TypeScript project of medium size (a few hundred files, with `node_modules` installed) and one plain-JavaScript project. Load the program the way `tsrs_lint/src/engine.rs` does. For one chosen function: visit every `CallExpression` and `NewExpression` in its body, call the resolver on the callee expression, print `site line → declaration file:line` |
| Write down | time to load the program; time to resolve one function's calls; peak memory **[measure all three]**; how many sites resolved, how many did not, and *why* for five of the unresolved ones; what the resolver returns for: a method on an interface, an overloaded function, an imported function, a function from `node_modules`, a class constructor, an arrow function assigned to a `const`, a callback parameter; whether dropping the program (`free_program`, with `checker_regions`) returns the memory |
| If the answer is "too slow to load per request" | expected for big projects. It does not block TS4 (the CLI loads once). It sets the priority of the session work in TS5 |

This spike's printed output is the first draft of the policy table in TS4.

### S-TS4: can we find and edit the JSDoc of every kind of def?

The marker goes inside the JSDoc block (decision D6, [`04-ts2-markers.md`](04-ts2-markers.md)). So TS2 depends on getting the exact byte range of
the JSDoc that belongs to a def.

| | |
|---|---|
| Question | For each kind of def, where does tsrs attach the JSDoc, and can we get its range in the source text? |
| Do | parse a file with a JSDoc above: a function, a class, a method, a decorated class (JSDoc above the decorator), `export const f = () => {}`, an object-literal method, an overloaded function, an interface method. For each, print `node.jsdoc(file)` (is it empty? which node carries it: the declarator or the statement?) and the comment's start and end offsets. Then insert a ` * ID: abc` line by hand, parse again, and check the JSDoc is still attached and the file has no new diagnostics |
| Write down | per kind: which node has the JSDoc, and how its range is obtained (`node.jsdoc(file)` versus `get_leading_comment_ranges` filtered to `/**`); what tsrs does with two JSDoc blocks above one def; whether a blank line between the block and the def detaches it in tsrs (our rule says it does: if tsrs still attaches it, we apply our own blank-line check on the text) |
| If a kind gives no usable range | fall back to the text: from the def's first byte, walk upward over whitespace and take the comment that ends there, if it starts with `/**`. That needs no JSDoc support from the engine at all |

Optional, only for the later comment-above option ([`04-ts2-markers.md`](04-ts2-markers.md) §8): does a `//` line between a JSDoc block and a
declaration detach the JSDoc, in tsrs and in VS Code? Not needed for TS2.

## 3. Where the spike code lives

`languages/typescript/spikes/`, one folder per spike, each its own tiny workspace (`[workspace]` in its `Cargo.toml`) so that it never joins the
real build. The folder is deleted when TS0 closes; the answers move to `languages/typescript/ENGINE.md` and `docs/`. CI never builds it.

## 4. What TS0 hands to the next phases

| To | What |
|---|---|
| `ENGINE.md` | the pinned `rev`; the exact crate list; build notes; the allocator note |
| TS1 | the parse time; the region pattern that worked, as ten lines to copy into `sem.rs` |
| TS2 | how to get the JSDoc range for each kind of def |
| TS3 | load time and memory for a real project; whether memory comes back |
| TS4 | the first policy table, from real output |
| [`../06-roadmap.md`](../06-roadmap.md) | real sizes for TS1–TS4 in place of my S/M/L guesses |

Next: [`03-ts1-outline.md`](03-ts1-outline.md)
