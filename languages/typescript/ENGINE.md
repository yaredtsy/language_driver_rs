# Engine: typescript

- Upstream URL: https://github.com/oxc-project/tsrs.git
- Pinned commit / version: `13a31a4cac36b980db090a91ec12a3c3686de14d` (workspace version 0.24.0, follows TypeScript 7.1.0-beta)
- Engine crates used: see "Spike S-TS1" below (the crates proven to build); the final list is written when TS1 and TS3 finish
- Patches: none
- How to bump:
- Licence notice:

## Spike S-TS1: does it build as a dependency?

Answer: **yes**, by `git` + `rev`, with no patch, no feature flag and no environment variable.

Measured on macOS (Darwin 23.6.0), Rust 1.99.1, `cargo build --release`, cold target directory, warm registry:

| Crates | Cold build | Binary |
|---|---|---|
| `tsrs_parser` + `tsrs_ast` + `tsrs_core` (parse a string, count statements) | 20 s | 2.1 MB |
| + `tsrs_compiler` + `tsrs_tsoptions` + `tsrs_vfs` + `tsrs_checker` + `tsrs_ls` (load a one-file program) | 1 m 33 s | 16.5 MB |

- The parser-only crate printed `statements: 2` for a two-function file.
- The full crate loaded a folder with one `a.ts`: `files: 91` (the file plus the bundled `lib.*.d.ts`), `global diagnostics: 0`. The user needs no
  `typescript` package: the libs are bundled (`tsrs_vfs::bundled`).
- **No mimalloc** was installed as the global allocator in either spike, and nothing broke. Speed with and without it was not measured.
- Cargo resolved 83 packages for the full crate; the lockfile is the spike's, not ours.
- Not yet checked: Linux and Windows builds (CI does that once a real crate depends on the engine).
- `Context::default()` was enough to call `get_global_diagnostics`; how to cancel a context from outside is still open (TS5).

How the program was loaded (the shape `sem.rs` will wrap):

```rust
let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(cachedvfs::from(osvfs::fs())));
let config = tsrs_tsoptions::new_parsed_command_line(P::new(CompilerOptions::default()), vec![file], vec![], compare_paths_options);
let host = tsrs_compiler::new_compiler_host(&cwd, fs, &bundled::default_lib_path(), None, None, None);
let program = tsrs_compiler::new_program(ProgramOptions::new(P::new(config), host));
```

Parsing one file: `tsrs_parser::parse_source_file(SourceFileParseOptions { file_name, path: tspath::to_path(..), external_module_indicator_options: Default::default() }, text, get_script_kind_from_file_name(name))`.

## Spike S-TS2: can a long-running process parse without growing?

Answer: **yes, inside a `Region`**. Measured on macOS, release build, with a synthetic file of 2,000 lines (40 KB: 500 functions, each with a JSDoc,
one parameter and a call). Each round: make a region, enter it, parse, walk every node, collect the function names into an owned `Vec<String>`,
leave the region.

| Mode | Round 1,000 | Round 10,000 | Parse time |
|---|---|---|---|
| in a region (10,000 rounds) | 4.5 MB | 4.9 MB (+16 KB per 1,000 rounds, flat to the eye) | 0.36 ms for 2,000 lines |
| in a region, 5,000 lines (1,000 rounds) | 5.6 MB | not run | 0.98 ms |
| **no region** (3,000 rounds) | 264 MB | 388 MB at round 3,000 | 0.40 ms |

- The target of the driver contract is "a few ms; under 50 ms for a 5,000-line file". Parsing and the walk are far inside it. This file is simple,
  so a real file will cost more; TS3 measures real ones.
- Without a region memory grows to hundreds of MB in a few thousand parses: that is the bug the rule protects against. (RSS stopped growing
  after round 2,000 in that run; macOS may compress or reclaim pages. The first 1,000 rounds alone cost 260 MB.)
- Constructor used: `Region::new_scratch(64 * 1024)`, as the docs suggest for temporary data whose results do not escape. The region was
  dropped at the end of each round; nothing from tsrs left the scope except owned `String`s.
- The small drift (+16 KB per 1,000 rounds) is probably the fixed-capacity source-text slot table (`parse_source_file_owned` says slots are never
  reused); not chased. TS1 should keep a memory test that parses in a loop and checks the growth stays small.
- Memory was read with `ps -o rss=` every 1,000 rounds. A portable crate (`memory-stats`) is for TS3.

The pattern to copy into `sem.rs`:

```rust
let region = tsrs_core::arena::Region::new_scratch(64 * 1024);
let _scope = region.enter();
let file = tsrs_parser::parse_source_file(opts, text, kind);
let mut names = Vec::new();
collect(file.as_node(), &mut names);   // owned data only
names                                  // `_scope`, then `region`, drop here
```

Node walk used: `node.for_each_child(&mut |c| { ...; false })` (return `true` to stop), `tsrs_ast::is_function_declaration(node)`,
`node.name()` is an `Option<P<Node>>`, `name_node.text()` is the identifier text.
