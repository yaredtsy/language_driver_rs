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
- Constructor: `Region::new_scratch(64 * 1024)`. I also ran `Region::new(64 * 1024)`: no measurable difference (see the long run below), so
  `new_scratch` stays, as the docs suggest for temporary data whose results do not escape. The region is dropped at the end of each round;
  nothing from tsrs leaves the scope except owned `String`s.
- **Use `parse_source_file_owned`, not `parse_source_file`, inside a region.** tsrs keeps a global table of file texts with 1,048,576
  slots; every parse takes one and slots are never reused. `parse_source_file_owned` clears the slot when the region is freed (so the table
  does not point into freed memory). The first version of this spike used `parse_source_file`, and its "+16 KB per 1,000 rounds" drift was
  that table (16 bytes per slot).
- **Long run: 1,200,000 parses** (past the table's 1,048,576 slots), each in a fresh region, `parse_source_file_owned`, 2,000-line file:
  memory 5.1 MB at round 100,000, 5.0 MB at 1,000,000, 4.5 MB at 1,200,000: flat, no drift. Same for `Region::new`: 5.1 → 5.2 → 4.5 MB.
  Parse time per stretch of 100,000 rounds stayed 360–420 µs, with one exception: the last stretch of the `new_scratch` run took about 505 µs
  per parse, and the same stretch of the `Region::new` run took 372 µs, so it is noise, not an effect of the full table. The table-full path
  (identifiers store their own text) is therefore exercised and costs nothing visible here.
- Memory was read with `ps -o rss=` every 1,000 rounds. A portable crate (`memory-stats`) is for TS3.

The pattern to copy into `sem.rs`:

```rust
let region = tsrs_core::arena::Region::new_scratch(64 * 1024);
let _scope = region.enter();
let file = tsrs_parser::parse_source_file_owned(opts, text.to_string(), kind);
let mut names = Vec::new();
collect(file.as_node(), &mut names);   // owned data only
names                                  // `_scope`, then `region`, drop here
```

Node walk used: `node.for_each_child(&mut |c| { ...; false })` (return `true` to stop), `tsrs_ast::is_function_declaration(node)`,
`node.name()` is an `Option<P<Node>>`, `name_node.text()` is the identifier text.

## Spike S-TS4: can we find and edit the JSDoc of every kind of def?

Answer: **yes**. Fixture: one file with 21 candidate nodes (see the table). tsrs reported 0 syntax diagnostics. For each node I printed
`node.jsdoc(None)` (the `JSDoc` nodes) and the `/**` comments from `tsrs_scanner::get_leading_comment_ranges(text, node.pos())`.

| Kind of def | Node that carries the JSDoc | Notes |
|---|---|---|
| function declaration, exported or not | `FunctionDeclaration` | |
| class | `ClassDeclaration` | |
| decorated class | `ClassDeclaration` | the block above the decorator is the one attached |
| method, getter | `MethodDeclaration`, `GetAccessor` | |
| class property holding an arrow | `PropertyDeclaration` | |
| `export const f = () => {}` | the **`VariableStatement`**, not the `VariableDeclaration` | the declarator has no JSDoc |
| object-literal method `om() {}` | `MethodDeclaration` | |
| object-literal `pf: function () {}` | `PropertyAssignment` | |
| overloaded function | **each overload signature** (a `FunctionDeclaration`) has its own block; the implementation has none | the def's marker goes on the implementation: it has no JSDoc, so TS2 must insert one |
| interface method | `MethodSignature` | |

- **Range**: `jsdoc.end()` is the exact end of the comment, but **`jsdoc.pos()` is not the start of the comment**: it is the end of the
  previous node (it includes the whitespace before). Take the start from `get_leading_comment_ranges(text, node.pos())` filtered to comments
  starting with `/**`: in every case above it gave the exact `(start, end)` and the same `end` as the `JSDoc` node. So TS2 reads ranges from
  the scanner and uses `node.jsdoc()` only to know whether one exists.
- **Offsets**: `Node::pos()` returns a `TextPos` enum, not an integer. `pos().expect_source(msg)` or `source_offset()` gives the UTF-8 byte offset.
- **Two blocks above one def**: `node.jsdoc()` returns both, in order; the scanner lists both. The nearer one (the last) is the one that
  documents the def, and it is the one our rule uses.
- **Blank line between the block and the def**: tsrs **still attaches** it. So does a `//` comment between the block and the def. Our rule says a
  blank line detaches, so TS2 applies its own blank-line check on the text.
- **Insert a line and reparse**: replacing `/** fn doc */` with a three-line block containing ` * ID: abc` gave 0 diagnostics, and the JSDoc was still
  attached to the function (range `(0, 28)`).
- **`.tsx` and `.js`** (the same fixture, with the TS-only syntax removed for `.js`): every row above is identical, byte for byte except for the
  offset shift from the removed annotations. In a `.js` file tsrs does not attach JSDoc differently, so the same rule serves all three.
- **A new block on an overload implementation** (inserted by hand above `function ov(a: any) {}`): 0 diagnostics, and the implementation now
  carries its own JSDoc while the two overload signatures keep theirs. In `.ts`, `.tsx` and `.js`.
- The fallback from the plan (walk up from the def's first byte over whitespace) was not needed.
