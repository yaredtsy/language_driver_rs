# Engine: typescript

The TS0 spikes below were run on 2026-11-11 and their code deleted when TS0 closed; it is in git history at commit `147da9d`
(`languages/typescript/spikes/`).

- Upstream URL: https://github.com/oxc-project/tsrs.git
- Pinned commit / version: `13a31a4cac36b980db090a91ec12a3c3686de14d` (workspace version 0.24.0, follows TypeScript 7.1.0-beta)
- Engine crates used: `typescript-syntax`: `tsrs_parser`, `tsrs_ast`, `tsrs_core`, `tsrs_scanner`. `typescript-analysis`: those plus `tsrs_compiler`,
  `tsrs_tsoptions`, `tsrs_vfs`, `tsrs_checker`. **Not `tsrs_ls`** (see S-TS3: the resolver we wanted is private). Final list confirmed when TS1 and TS3 are built.
- Patches: none
- How to bump: an engine bump is a change of its own (only the pin, the lockfile, `sem.rs` and this file).
  1. Change the `rev` once, in `[workspace.dependencies]` of `languages/typescript/Cargo.toml` (every `tsrs_*` line shares it).
  2. Build: cargo re-resolves the git source on its own when the `rev` changes, and every `tsrs_*` package comes from that one source. Commit the new `Cargo.lock`.
  3. `cargo test`, then `cargo test -p typescript-syntax` alone: the fast crate must still not compile the checker.
  4. Re-check every item this file names (the calls in the spike sections, later the "items we call" list) against the new source; fix `sem.rs` only.
  5. Write the new commit, its workspace version and the TypeScript version it follows (`[workspace.metadata.typescript]` in tsrs's `Cargo.toml`) here.
- Licence notice: tsrs is Apache-2.0, Copyright 2026 Max Schwenk and the tsrs contributors. It is a derivative work of TypeScript,
  Copyright (c) Microsoft Corporation, Apache-2.0, and the binary embeds TypeScript's `lib.*.d.ts` files. A binary that links
  `typescript-analysis` ships tsrs's `NOTICE` (which carries TypeScript's third-party notices) and `LICENSE` unchanged.

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

Parsing one file: `tsrs_parser::parse_source_file_owned(SourceFileParseOptions { file_name, path: tspath::to_path(..), external_module_indicator_options: Default::default() }, text.to_string(), get_script_kind_from_file_name(name))`,
inside a region (S-TS2). This spike used `parse_source_file`, which is wrong for a long-running process: see S-TS2.

## Spike S-TS2: can a long-running process parse without growing?

Answer: **yes, inside a `Region`**. Measured on macOS, release build, with a synthetic file of 2,000 lines (40 KB: 500 functions, each with a JSDoc,
one parameter and a call). Each round: make a region, enter it, parse, walk every node, collect the function names into an owned `Vec<String>`,
leave the region.

| Mode | Round 1,000 | Round 10,000 | Parse time |
|---|---|---|---|
| in a region, **first version with `parse_source_file`** (10,000 rounds) | 4.5 MB | 4.9 MB (+16 KB per 1,000 rounds: the text table, see below) | 0.36 ms for 2,000 lines |
| in a region, 5,000 lines (1,000 rounds) | 5.6 MB | not run | 0.98 ms |
| **no region** (3,000 rounds) | 264 MB | 388 MB at round 3,000 | 0.40 ms |

- The target of the driver contract is "a few ms; under 50 ms for a 5,000-line file". Parsing and the walk are far inside it. This file is simple,
  so a real file will cost more; TS3 measures real ones.
- Without a region memory grows to hundreds of MB in a few thousand parses: that is the bug the rule protects against. (RSS stopped growing
  after round 2,000 in that run; macOS may compress or reclaim pages. The first 1,000 rounds alone cost 260 MB.)
- Constructor: `Region::new_scratch(64 * 1024)`, entered with `enter()`. tsrs's doc on `new_scratch`: an **unregistered** region (its chunks are not
  in the global registry, so creating or freeing it takes no global lock); "a request that owns all cached data may use `enter` directly", and
  `enter_scratch` is for "temporary data whose escaping caches must survive the region". A parse request owns everything it makes, so `enter` fits.
  `Region::new` (a registered region) gave no measurable difference (see the long run below). `enter_scratch` was not tried. The region is dropped
  at the end of each round; nothing from tsrs leaves the scope except owned `String`s.
- **Use `parse_source_file_owned`, not `parse_source_file`, inside a region.** tsrs keeps a global table of file texts with 1,048,576
  slots; every parse takes one and slots are never reused. `parse_source_file_owned` clears the slot when the region is freed (so the table
  does not point into freed memory). The first version of this spike used `parse_source_file`, and its "+16 KB per 1,000 rounds" drift was
  that table (16 bytes per slot).
- **Long run: 1,200,000 parses** (past the table's 1,048,576 slots), each in a fresh region, `parse_source_file_owned`, 2,000-line file:
  memory 5.1 MB at round 100,000, 5.0 MB at 1,000,000, 4.5 MB at 1,200,000: flat, no drift. Same for `Region::new`: 5.1 → 5.2 → 4.5 MB.
  Parse time per stretch of 100,000 rounds stayed 360–420 µs, with one exception: the last stretch of the `new_scratch` run took about 505 µs
  per parse, and the same stretch of the `Region::new` run took 372 µs, so it is noise, not an effect of the full table. The table-full path
  (identifiers store their own text) is therefore exercised and costs nothing visible here.
- Memory was read with `ps -o rss=` every 1,000 rounds. TS1's test uses the `memory-stats` crate (portable). **RSS is a noisy meter here:** a loop that leaks
  (no region entered) swings between 30 and 390 MB from one sample to the next, because the OS takes freed pages back whenever it likes; the loop that does not
  leak sits at about 8 MB. So read the **peak** over many samples, never one late value (the first S-TS2 table's "stopped growing" for the no-region run is this).
  When mutating code to check a test, restore the file with a command that updates its modification time (`touch`): `mv` of an older copy makes Cargo keep the
  mutated build, and the next runs measure the wrong code.

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
| overloaded function | **each overload signature** (a `FunctionDeclaration`) has its own block; the implementation has none | the def's marker goes on the **first overload signature**: the def's `range` starts there (`docs/03-typescript/04-ts2-markers.md` §3). A marker above the implementation is not read |
| interface method | `MethodSignature` | |

- **Range**: `jsdoc.end()` is the exact end of the comment, but **`jsdoc.pos()` is not the start of the comment**: it is the end of the
  previous node (it includes the whitespace before). Take the start from `get_leading_comment_ranges(text, node.pos())` filtered to comments
  starting with `/**`: in every case above it gave the exact `(start, end)` and the same `end` as the `JSDoc` node. So TS2 reads ranges from
  the scanner and uses `node.jsdoc()` only to know whether one exists.
- **Offsets**: `Node::pos()` returns a `TextPos` enum, not an integer. `pos().expect_source(msg)` or `source_offset()` gives the UTF-8 byte offset.
- **Two blocks above one def**: `node.jsdoc()` returns both, in order; the scanner lists both. The nearer one (the last) is the one that
  documents the def, and it is the one our rule uses.
- **Blank line between the block and the def**: tsrs **still attaches** it. So does a `//` comment between the block and the def. Our rule
  follows the engine (owner, 2026-11-11; `tsc` 5.1.6 agrees): neither detaches. The one block our rule keeps from the def is the file's
  opening comment (`docs/markers.md`).
- **Insert a line and reparse**: replacing `/** fn doc */` with a three-line block containing ` * ID: abc` gave 0 diagnostics, and the JSDoc was still
  attached to the function (range `(0, 28)`).
- **`.tsx` and `.js`** (the same fixture, with the TS-only syntax removed for `.js`): every row above is identical, byte for byte except for the
  offset shift from the removed annotations. In a `.js` file tsrs does not attach JSDoc differently, so the same rule serves all three.
- **A new block on an overload implementation** (inserted by hand above `function ov(a: any) {}`): 0 diagnostics, and the implementation now
  carries its own JSDoc while the two overload signatures keep theirs. In `.ts`, `.tsx` and `.js`.
- The fallback from the plan (walk up from the def's first byte over whitespace) was not needed.

## Spike S-TS3: load a real project and resolve its calls

Projects: the V-NOC frontend (`tsconfig.app.json`: 396 root files, `strict`, bundler resolution, JSX, `paths`; the program has 2,396 source files with
`node_modules` and the bundled libs) and, for plain JavaScript, `postcss/lib` from its `node_modules` (28 root `.js` files, `allowJs`, CommonJS;
117 source files). Measured on macOS, release build.

### 1. The resolver the plan relied on is private

`tsrs_ls::callhierarchy` is a **private module** (`mod callhierarchy;` in `tsrs_ls/src/lib.rs`), so `resolve_call_hierarchy_declaration` cannot be
called from outside; it is reachable only through `LanguageService::provide_*_call_hierarchy`, which returns LSP types. The plan said it was `pub`:
that was wrong (the function is `pub`, its module is not). So `typescript-analysis` does **not** depend on `tsrs_ls`, and the resolver is the plan's
"by hand" fallback, which uses only public checker items:

```rust
let mut checker = program.get_type_checker(&Context::default());   // CheckerHandle, derefs to tsrs_checker::Checker
let signature = checker.get_resolved_signature_exported(call);    // the matched signature of a CallExpression / NewExpression
if let Some(decl) = signature.declaration() { /* answer */ }
else if let Some(mut sym) = checker.get_symbol_at_location_exported(callee) {   // PropertyAccess callees work too
    if sym.flags().intersects(SymbolFlags::Alias) { sym = checker.get_aliased_symbol(sym); }
    let decl = sym.value_declaration();
}
```

The call-site walk (`collect_call_sites`) is private too; TS4 writes its own, following the list in `tsrs_ls/src/callhierarchy.rs` `CallSiteCollector::collect`:
`CallExpression`, `NewExpression`, `TaggedTemplateExpression`, `JsxOpeningElement`/`JsxSelfClosingElement`, `Decorator`, `PropertyAccessExpression`/`ElementAccessExpression`
(accessors), `ClassStaticBlockDeclaration`; it does not descend into ambient nodes, types, other call-hierarchy declarations, or the type side of `as`/`satisfies`.
The spike only visited `CallExpression` and `NewExpression`.

### 2. Cost

| | V-NOC frontend | postcss (JS) |
|---|---|---|
| load program (config + all files + bind) | 160 ms first run, 55-70 ms later runs (OS file cache) | 29 ms |
| memory after load | 105 MB | 18 MB |
| call sites resolved | 437 in 5 files (3,339 lines) in **57 ms** (includes the checker's first-use work) | 341 in 3 files in 15 ms |
| memory after resolving | 114 MB | 22 MB |

Both are far inside "load once per CLI run". Memory was read with `ps`; checker work is lazy, so these are for the files visited only.

### 3. Does freeing the program return the memory? Only inside regions

`free_unshared_program(program)` after use, **without** regions: 15 rounds of load, resolve, free grew RSS from 110 MB to **528 MB** (about 28 MB per round).
With the pattern `tsrs_lint::engine::run` uses it stays flat: **110 MB at round 1, 148 MB at round 15, 145-153 MB from round 10 to round 60**.

```rust
let region = Region::new_scratch(1 << 20);                  // one per request, entered for the whole load + resolve + free
let _scope = region.enter();
let loaders = Arc::new(tsrs_compiler::ProgramRegions::default());
// config as in S-TS1; then:
let mut options = ProgramOptions::new(P::new(config), host);
options.checker_regions = true;
options.regions = Some(Arc::clone(&loaders));
let program = tsrs_compiler::new_program(options);
// ... resolve with a checker; drop the checker handle ...
unsafe { tsrs_compiler::free_unshared_program(program) };   // locals then drop: loaders, scope, region
```

After the first free about 40 MB stays (110 MB after round 1, a plateau near 150 MB from round 10): stable, not growing, but not everything comes back.

Safety rule for `sem.rs`/`Project` (TS3): checker handles are dropped before the program is freed, and nothing from the program is used after `free`.
The spike did not try `free_program` (the variant that keeps data shared between program versions: for `Session` in TS5).

### 4. What the engine answers: first draft of the policy table

**V-NOC frontend, 437 sites: only 78 (18%) reach a project function with a body.** The engine returned *some* declaration for all 437
(none came back empty), but most are not defs. By what the declaration is and where it lives:

| Declaration the engine returns | project | `node_modules` | bundled libs | Meaning for us |
|---|---|---|---|---|
| `FunctionDeclaration` | 67 | 69 | 1 | a def with a body when in the project |
| `ArrowFunction` | 11 | 0 | 0 | a def (the arrow assigned to a `const`) |
| `FunctionType` | 36 | 102 | 2 | a **function-typed value** (React `setState`, store actions, props): the target is a type, not a def |
| `MethodSignature` | 0 | 17 | 104 | an interface or `lib.d.ts` method: no body |
| `CallSignature` / `ConstructSignature` | 0 | 12 | 16 | no body |

So 78 sites reach a project def; 323 are external (`node_modules` 200, bundled 123) and 36 more point at a
project type (an interface member or function-typed property). The 140 `FunctionType` sites (32%) are the policy decision TS4 must make: the engine
resolves them, but there is no def to link, so I propose `Unresolved` (calls through a function-typed value), never a guess.

**The hand-written seven cases** (`spikes/s-ts3/cases/a.ts`, deleted at TS0 close: `git show 147da9d:languages/typescript/spikes/s-ts3/cases/a.ts`; 0 diagnostics):

| Case | Engine answer | Path |
|---|---|---|
| method on an interface `s.area()` | `MethodSignature area` (no body) | signature |
| overloaded function, call matching overload 1 / 2 | `FunctionDeclaration` of **that overload signature** (line 6 / line 7), not the implementation | signature |
| imported function | `FunctionDeclaration` in `lib.ts` | signature |
| function from `node_modules` (`pkg`, `.d.ts`) | `FunctionDeclaration` in `node_modules/pkg/index.d.ts` | signature |
| class constructor `new K(1)` | `Constructor` node, which has no name | signature |
| method on an instance `new K(2).m()` | `MethodDeclaration m` | signature |
| arrow assigned to a `const` | `ArrowFunction` (nameless node; the name is on the `VariableDeclaration`) | signature |
| callback parameter `cb()` | `FunctionType` of the parameter's annotation | signature |
| callback argument `useCb(() => {})` | `FunctionDeclaration useCb` (the callee, as expected) | signature |
| callee of type `any` `anything()` | no signature; the symbol path returns the **`Parameter`** | symbol |
| `anything.foo.bar()` | nothing | none |
| lib method `[1].map`, `console.log` | `MethodSignature` in `bundled:///libs/lib.es5.d.ts` / `lib.dom.d.ts` | signature |

Rules these suggest for TS4 (to be confirmed there):
1. The signature path is the main one. The symbol path adds only noise: it returned `Parameter`, `VariableDeclaration`, `BindingElement` (a `require`d value or an `any` callee).
   Accept a declaration only if it is function-like **with a body**; everything else is `Unresolved`.
2. A matched **overload signature must map to its implementation** (the call hierarchy does it with `find_implementation`), because our identity is the implementation's qualname.
3. `Constructor` maps to its class; a class without an explicit constructor was **not tested**.
4. An `ArrowFunction` or `FunctionExpression` takes its name from the `VariableDeclaration`/`PropertyAssignment` it initialises, as the outline rule (TS1) does.
5. External = the declaration's file is under `/node_modules/`, ends in `.d.ts`, or starts with `bundled:///libs/`.

### 5. Plain JavaScript (postcss)

341 sites: **236 resolved, 105 unresolved (31%)**. Unresolved: 19 `require(...)` (this config had no Node types, so `require` is undeclared), 22 `this.*` calls, and the
rest are calls on values the checker types as `any` (`token[1].slice`, `tokens.pop`, `text.match`). Resolved targets: 110 `MethodDeclaration`, 27 `FunctionDeclaration`,
10 `Constructor`, 5 `ArrowFunction` in the package, 55 lib `MethodSignature`/`ConstructSignature`, and 22 `Parameter`/`BindingElement`/`VariableDeclaration`
(non-defs: rule 1 drops them). A package that ships `.d.ts` next to its `.js` resolves **to the `.d.ts`** (`this.sync().warnings` → `result.d.ts`), not to the implementation.
This is what the plan expected: JavaScript resolves less, and `Unresolved` says so.

### 6. Not done

- Both spikes report **1 program diagnostic** whose message text printed empty. Cause found in TS1: `Diagnostic::message_text()` is only filled for diagnostics that arrive
  already localized; ordinary ones carry a `Message` and arguments, and **`Diagnostic::localize()`** builds the text. The spikes' one diagnostic was not re-read with it: TS3 does.
- One checker (`program.get_type_checker`) answered for every file, with tsrs's default checker count. `tsrs_lint` asks `get_type_checker_for_file(ctx, file)`.
  The plan sets the count to 1 (`docs/03-typescript/05-ts3-project.md` §3), which makes both calls the same checker; TS3 checks that setting.
- Whether a call through a `FunctionType` is `Unresolved` or a distinct edge is a decision for the owner before TS4 (it bends "follow the engine").
- Not tried: object-literal methods, JSX elements, accessors, tagged templates, decorators as call sites; a class without a constructor; a project with project references; Linux and Windows.
- The resolve time includes the checker's lazy work for the visited files only, so the cost of the **first** query in a fresh program is in the 57 ms; a second query is cheaper.

## TS3: what `sem.rs` found when it loaded programs

Measured and read on 2026-11-11, macOS, `typescript-analysis` at the pin above.

### Freeing a program: the region alone is not enough

`tests/memory.rs` opens and drops a three-file project 100 times (RSS by `memory-stats`, peak over rounds 11–100 against round 10).

| `Sem::drop` | RSS at round 1 | round 10 | round 60 |
|---|---|---|---|
| region only, no `free_unshared_program` | 20 MB | 115 MB | 349 MB (the 100-round test: 115 → 440 MB) |
| region entered, then `free_unshared_program` | 12 MB | 19 MB | 24 MB |

Why, from the source (`tsrs_compiler/src/program.rs`): `new_program` does `Box::leak(Box::new(program))`, and the file loader leaks the processed files, the
project-reference mapper and the resolution hosts the same way. They sit on the **global heap**, so a region does not hold them. `free_program` drops the
`Program` and its resolution host, and dropping the `Program` drops its checker pool, which frees the leaked slice of checkers (`PoolState::drop`);
`free_unshared_program` also frees the processed files and the mapper (`SharedProgramData::free`). None of the `Drop` impls in `tsrs_compiler` closes a
file or stops a thread (read: `grep "impl.*Drop for"`), so the cost of skipping the call is memory. Which of those boxes holds most of the 5 MB per
open was not measured.

So the plan (`docs/03-typescript/05-ts3-project.md` §4) was right that `Drop` frees the program, and the lint was in its way: `free_unshared_program` is an
`unsafe fn`, `unsafe_code = "forbid"` cannot be lifted locally. The TypeScript workspace is `deny`, with one `#[allow(unsafe_code)]` on `Sem::drop`
(`scripts/check-all.sh` fails on that attribute anywhere but a `sem.rs`). tsrs has no safe wrapper; its own users (`tsrs_lint`, `tsrs_execute`,
`tsrs_project`, `tsrs_api`) call the `unsafe fn` the same way. The call is made with the program's region entered, as in S-TS3.

### Loading a tsconfig

- A `tsconfig.json` with a **syntax error still parses**: `get_parsed_command_line_of_config_file` returns no diagnostics for it. Its errors live on
  `config.config_file.source_file.diagnostics()`, which `tsrs_lint`'s `find_tsconfig` also checks. `Sem::open` reports both as `OpenError::ConfigError`.
- The upward search for `tsconfig.json` (then `jsconfig.json`, per directory) **stops at the root** the caller gave. `tsrs_lint`'s `find_tsconfig` goes to the
  filesystem root; ours must not pick up a stray config outside the repository, and `files()` is relative to the root.
- A program loaded from a tsconfig that does not include the file has no source file for it. `open` says so (`ConfigError`) rather than answering for a
  file the program does not hold. A solution config (`"files": []` plus `references`) is TS3 step 6.

### Program options `Sem::open` sets on purpose

| Option | Value | Why, and what was measured (2026-11-11) |
|---|---|---|
| `checkers` (`CompilerOptions`, set on the parsed config) | `Some(1)` | one checker sees every file, so no question of which checker owns a node, and the same project answers the same way every time. Checked by `sem::tests::the_program_has_one_checker` (`program.options().checkers`); `CheckerPoolImpl::new` reads it |
| `checker_regions` | `true` | the checkers' memory goes back with the program (S-TS3) |
| `regions` | one `ProgramRegions` per `Sem` | the loaders' memory goes back with the program (S-TS3) |
| `single_threaded` | **not set** | it implies one checker, but it also makes the file loader sequential (`fileloader.rs`). On a Next.js app (1,549 files in the program, `node_modules` included), warm: **about 45 ms to open by default, about 160 ms with `single_threaded`**. Parallel loading is kept; `sem::tests::the_program_lists_its_files_in_the_same_order_every_time` opens the fixture 31 times and checks the program's file order never changes |

The plan (`docs/03-typescript/05-ts3-project.md` §3) listed `single_threaded` and `checkers = 1` as two options and left "what the flag covers" as a
`[check]`: it covers file loading and the checker pool, and `checkers = 1` is the part we need. Cold opens vary a lot (the same program: 126 to 444 ms
on a first run), so quote warm numbers.

### The inferred project (no config above the file)

Built with `tsrs_tsoptions::new_parsed_command_line` and the file as the only root; its imports and `require` calls pull in the rest (fixtures `plain-js`,
`plain-js-require`: an unimported sibling is not in the program). Options, from `tsrs_lint`'s set for files no config claims (`engine.rs`, the `unmatched`
branch), with the plan's changes: `allow_js` on, `check_js` off, `no_emit` on, `module` ESNext, `module_resolution` bundler, `target` ES2022, `jsx` react-jsx
(so JSX elements are call sites in a `.jsx` file), `allow_importing_ts_extensions`, `es_module_interop`, `resolve_json_module`. Left out of `tsrs_lint`'s set:
`source_map` (nothing is emitted), `strict_null_checks` / `strict_function_types` (not set; whether they ever change which signature a call resolves to was not checked), and
`allow_non_ts_extensions`: without it a `.txt` root is ignored, so `open` says `NotInProject("(inferred project)")` instead of reading prose as TypeScript.
Not checked: that the options above change a resolved call target compared with `tsc`'s own inferred-project defaults (TS4's fixtures will show it).

### Which files are the project's, and what package a file belongs to

`Sem::is_project_code` is false for: a path in `node_modules` (or a file the program found by searching it: `is_source_file_from_external_library`), a
declaration file (`tspath::is_declaration_file_name`: `.d.ts`, `.d.mts`, `.d.cts`; this includes the project's own `src/types.d.ts`, as the plan said), and a
bundled library (`is_source_file_default_library`; these live outside the root, and there are 91 of them in a one-file program). `Project::files()` lists
project code only, relative to the root.

`Sem::package_of` names the package for an `External` edge: the first segment after the **last** `node_modules/` (pnpm nests them), `@scope/pkg` with
its scope, **`@types/x` as `x` and `@types/scope__x` as `@scope/x`** (the package a types package describes: a policy, not an engine fact), and
`typescript:lib` for every bundled library. `None` for the project's own files. Fixture `with-node-modules` covers a plain package, a scoped one and an
`@types` one; the pnpm and `__` cases are unit-tested on strings only, not on a real pnpm tree.

### The proof that a `File<'p>` cannot outlive its `Sem`

`File<'p>` is `P<SourceFile>` plus `PhantomData<&'p Sem>`; `Sem::drop`'s `unsafe` rests on no `File` outliving it. The proof is a `compile_fail` doctest
(`typescript_analysis::proof`, `#[doc(hidden)]`): `drop(sem)` while a `File` is used later. A doctest is a separate crate, so the real `Sem` and `File` are
re-exported there (a wrapper function would prove only its own signature).

- **A first version used a wrapper (`Project::handle_for_doctest`) and was too weak:** with `Sem::file` changed to `fn file<'p>(&self, ..) -> File<'p>` (an unbound
  lifetime, the realistic mistake) the wrapper's own `-> FileHandle<'_>` re-tied the borrow, and the doctest still passed. On the real types the same mutation
  makes it fail, which is the check that it protects something.
- **`compile_fail,E0505` is not enforced by stable rustdoc**: pinning `E0499` instead still passed. With `RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS=-Zunstable-options cargo test --doc`
  the code is checked (the error is `E0505`, "cannot move out of `sem` because it is borrowed"; `E0499` failed). On stable the guard against "fails for the wrong reason" is
  the **twin** doctest, which differs only in the order of the last two lines and must compile.
- A `PhantomData<&'p ()>` in place of `PhantomData<&'p Sem>` also passes: it binds the borrow just the same through `Sem::file`'s signature, so it is not a bug.
