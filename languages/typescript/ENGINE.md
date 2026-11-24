# Engine: typescript

The TS0 spikes below were run on 2026-11-11 and their code deleted when TS0 closed; it is in git history at commit `147da9d`
(`languages/typescript/spikes/`).

- Upstream URL: https://github.com/oxc-project/tsrs.git
- Pinned commit / version: `13a31a4cac36b980db090a91ec12a3c3686de14d` (workspace version 0.24.0, follows TypeScript 7.1.0-beta)
- Engine crates used: `typescript-syntax`: `tsrs_parser`, `tsrs_ast`, `tsrs_core`, `tsrs_scanner`. `typescript-analysis`: those plus `tsrs_compiler`,
  `tsrs_tsoptions`, `tsrs_vfs`, `tsrs_checker`. **Not `tsrs_ls`** (see S-TS3: the resolver we wanted is private). Final list confirmed when TS1 and TS3 are built.
- Patches: none
- How to bump: an engine bump is a change of its own (only the pin, the lockfile, the `sem` module and this file).
  1. Change the `rev` once, in `[workspace.dependencies]` of `languages/typescript/Cargo.toml` (every `tsrs_*` line shares it).
  2. Build: cargo re-resolves the git source on its own when the `rev` changes, and every `tsrs_*` package comes from that one source. Commit the new `Cargo.lock`.
  3. `cargo test`, then `cargo test -p typescript-syntax` alone: the fast crate must still not compile the checker.
  4. Re-check every item this file and `engine/` name (the calls in the spike sections, later the "items we call" list) against the new source; fix `sem.rs` only.
     Then the two guards of H14 and H17: run the totality walk (`sem/view/tests.rs`) first, and regenerate the list of the engine's downcasts in
     `crates/typescript-analysis/clippy.toml` with the command written above it (183 at `13a31a4`, plus two of `ast.rs` named there); a downcast the
     new engine adds is unbanned until then.
  5. Write the new commit, its workspace version and the TypeScript version it follows (`[workspace.metadata.typescript]` in tsrs's `Cargo.toml`) here.
- Licence notice: tsrs is Apache-2.0, Copyright 2026 Max Schwenk and the tsrs contributors. It is a derivative work of TypeScript,
  Copyright (c) Microsoft Corporation, Apache-2.0, and the binary embeds TypeScript's `lib.*.d.ts` files. A binary that links
  `typescript-analysis` ships tsrs's `NOTICE` (which carries TypeScript's third-party notices) and `LICENSE` unchanged.

## Where the rest is

What each phase found in the engine, as built and measured, is in `engine/`, one file per phase, in the order the phases ran. A pointer
elsewhere that says "`ENGINE.md`, section X" means the section X of one of these files: the table lists every section. A `###` subsection (`S-TC1`, `N5`, …) is in the file of
the `##` section above it; `grep -rn '<its title>' languages/typescript/engine` finds any. A new phase writes a new file here and adds its row.

| File | Sections |
|---|---|
| [`engine/01-ts0-spikes.md`](engine/01-ts0-spikes.md) | Spike S-TS1: does it build as a dependency?; Spike S-TS2: can a long-running process parse without growing?; Spike S-TS4: can we find and edit the JSDoc of every kind of def?; Spike S-TS3: load a real project and resolve its calls |
| [`engine/02-ts3-loading-programs.md`](engine/02-ts3-loading-programs.md) | TS3: what `sem.rs` found when it loaded programs |
| [`engine/03-ts4-reading-bodies.md`](engine/03-ts4-reading-bodies.md) | TS4: reading a def's body |
| [`engine/04-ts5.md`](engine/04-ts5.md) | TS5 step 0: what the code already had, and two answers; TS5 step 3: a class is a construction; TS5 step 5: cancel at every check, and the steps that cannot be stopped; TS5 row 3c: a coverage test for rule 8; TS5 step H: the hardening batch (H1–H10, 2026-11-17); TS5 step 6 design: what the engine gives for following the disk (2026-11-17); TS5 step 6, commit 1 as built: the tracker and the stamps (`d3b394e`, reviewed 2026-11-19); TS5 step 6 as measured, and the sweep on four projects (reviewer, 2026-11-19) |
| [`engine/05-h14-to-h17.md`](engine/05-h14-to-h17.md) | H14, H15, H16 as built and measured (733e26c, ae59938; reviewed 2026-11-19); H17 as built and measured (477c4bd, 5362028; reviewed 2026-11-23) |
| [`engine/06-tc0-counts.md`](engine/06-tc0-counts.md) | TC0: what was counted |
| [`engine/07-tcl.md`](engine/07-tcl.md) | TCL as built and measured (`aa32042`, fix `fbee1b4`; reviewed 2026-11-24) |
| [`engine/08-tc1.md`](engine/08-tc1.md) | TC1 step 1 as built: the switch and the interned key (`c2452bb`; reviewed 2026-11-24); TC1 step 2 as built: a body as plain data, and the gate (`bac93f9`; reviewed 2026-11-24); TC1 step 3 as built: the hand-overs in the table (`6b7c2ef`; reviewed 2026-11-24); TC1 step 4a as built: values and their laws, the key's parameters (`7b680b7`; reviewed 2026-11-24); TC1 step 4b as built: the layer runs, and knows nothing (`9530a7d`; reviewed 2026-11-24) |
