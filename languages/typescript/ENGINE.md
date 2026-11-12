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
project type (an interface member or function-typed property). The 140 `FunctionType` sites (32%) were a policy decision: the engine
resolves them, but there is no def to link, so they are `Unresolved` (calls through a function-typed value), never a guess, and counted in
`Stats.language["function_typed_value"]` (decided, D16 in `docs/07-decisions-and-questions.md`).

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
- A call through a `FunctionType` is `Unresolved` and counted, not a distinct edge (D16). TS4 builds it.
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

### Thread safety of the handles (checked 2026-11-11)

Before this step the compiler said: `File<'static>` is `Send` and `Sync`, and so is `Sem` (so `Project` is). Nothing was wrong yet, because no handle can be used
without a `&Sem` and no method touches the checker, but the design is one checker answering one question at a time and tsrs enters its regions per thread, so
"a handle may cross threads" was a claim nobody had made. `File` now carries a `PhantomData<*const ()>` (neither `Send` nor `Sync`), and two `compile_fail`
doctests with compiling twins pin it (`typescript_analysis::proof`, run with `--features __proof` by `scripts/check-proofs.sh`; they failed before the marker was added and
fail again with it removed). **`Sem` (so `Project`) is `Send` but not `Sync`: decided at the start of TS4 and pinned** (a `Cell` marker and a proof pair; moving a `Project` between threads stays allowed, `tests/threads.rs`). Asking a `Sem` questions runs the
checker, and a shared `&Sem` would let two threads call into one checker. TS5 revisits it only if the driver needs parallel questions.

The proof module is behind the feature `__proof` because a `compile_fail` doctest is a separate crate that can only name public items. With the feature off rustdoc
finds no proof doctest at all and reports success, hence the script and its count file. Cargo unifies features across a build, so a crate that enables `__proof`
exposes `Sem` to everything built with it.

### Which config owns a file: solution configs and references (TS3 step 6)

`find_owner` ports `tsrs_lint`'s `find_tsconfig` (`engine.rs`), bounded by the root: the nearest config, then the configs it references (`resolved_project_reference_paths`),
then the config above, and so on; the first whose file list holds the file wins (`!file_names().is_empty()`, `file_names_by_path` contains it; a `composite` config must also match
by its own patterns, `possibly_matches_file_name`). `disableSolutionSearching` on the starting config stops the climb. Nothing holds it: `NotInProject` naming the nearest config.

- **The compiler API `tsrs_lint` uses is enough**: no project-reference support had to be written. The `[check]` in the plan (does `tsrs_compiler` support references?) is answered:
  yes, through the parsed config's references plus `ProgramOptions::use_source_of_project_reference`.
- **`use_source_of_project_reference = true` for a config program** (as `tsrs_lint` sets it; off for an inferred one). Without it a referenced project's files drop out of the program (the
  `references` fixture's `packages/lib/src/greet.ts` is gone), because a referenced project is expected to be built and its `.d.ts` output read instead; that output is often not there.
  With it on, a call from `app` into `lib` should reach `lib`'s source (a def, not an `External`): TS4's fixtures must confirm the resolved target.
- **Error policy (mine, not tsrs's):** a syntax error in the **nearest** config is a `ConfigError`; one in a config only reached through a reference or a climb is skipped, as `tsrs_lint`
  does (it may have nothing to do with this file). Skipping can hide a broken referenced config that did own the file; the person then sees `NotInProject`.
- **Real layout checked** on a Vite project on this machine (`tsconfig.json` with `"files": []` and references to `tsconfig.app.json` and `tsconfig.node.json`):
  `src/main.tsx` and `src/app.tsx` load the app config's program (`src/vite-env.d.ts` is left out of `files()`: a declaration file), `vite.config.ts` loads the node config's, and `tsconfig.json`
  itself is `NotInProject`. The fixture `solution` has the same shape. Not checked: the V-NOC frontend itself (its path is not in this repo), a pnpm workspace, `extends` chains.

### What a reload must watch, and what moves across threads (TS3 steps 7–8)

- **`ParsedCommandLine::extended_source_files()` lists the config files an `extends` pulled in**, so `Project::environment_inputs` can name `config/shared.json` exactly. The fixed
  globs alone would miss it (it is not named `tsconfig*.json`). Every config the owner search read goes in too, a skipped broken one included: fixing it changes the answer.
  Not covered: a file a config reads in some other way (`tsrs` content mappers), and `package.json` fields other than through the glob (a `types` entry is covered by `package.json`).
- **A `Project` opened on one thread and dropped on another works** (`tests/threads.rs`: 20 move-and-drop rounds, four projects alive at once, four threads opening at once, 100 rounds
  of open-on-one drop-on-another with flat memory; the memory test fails, 116 to 282 MB, with the free removed). So the free does not depend on the thread that made the program.
  `Project` is `Send` and, since TS4, not `Sync` (pinned).
- **A skipped config is reported, not silent** (`OpenError::NotInProject { config, skipped }`). It is not reported when `open` succeeds: a skipped broken config that would also have
  held the file is invisible then, except through `environment_inputs`. If that matters, `Project` needs a `skipped_configs()`.
- **The V-NOC frontend, opened through `Project::open`** (2026-11-11, `src/frontend` of the V-NOC repo, read only; debug build): `tsconfig.json` has `"files": []`, references to
  `tsconfig.app.json` and `tsconfig.node.json`, and its own `paths`. `src/main.tsx` and every other file under `src` get `tsconfig.app.json` (`"include": ["src"]`); `vite.config.ts` gets
  `tsconfig.node.json`; `shiki.bundle.ts` and `tailwind.config.js` are in neither config, so `NotInProject`, which is what `tsc` says too. Open takes 0.15 s warm (the whole `typescript-cli files`
  run, `node_modules` included). The program holds 397 files: 395 TypeScript (+ `src/vite-env.d.ts`, the one declaration file) and **two imported `.json` fixtures, which `files()` wrongly
  listed as project code**: fixed, `is_project_code` now needs a TypeScript or JavaScript file (`Dialect::from_path`), fixture `json-import`.
- **Not checked, still:** a pnpm workspace, and Windows paths in `find_owner` (`relative_to` and the visited set compare engine strings, which are `/`-separated everywhere; the
  `canonicalize` of the root is not).

## TS4: reading a def's body

### A def and its node in the program's tree (step 2a)

The outline (`typescript_syntax::outline_with_nodes`) is made from the **text** of each program file, a second parse; the program has its own tree of the same text. A def is joined to its node by the node's **span**
(first token to end, trivia skipped: `tsrs_scanner::skip_trivia` from `pos`), which `typescript-syntax` reports with each def, plus a kind a def can name (`FunctionDeclaration`, `FunctionExpression`,
`ArrowFunction`, `ClassDeclaration`, `ClassExpression`, `MethodDeclaration`, `Constructor`, `GetAccessor`, `SetAccessor`, `InterfaceDeclaration`, `MethodSignature`, `EnumDeclaration`,
`TypeAliasDeclaration`, `ModuleDeclaration`). The two parses agree: the 48 files of the TS1 fixture suite (ts, tsx, js, jsx, including a `.d.ts`, a file with a syntax error and `.mjs` / `.cjs`) hold 334 defs, and
every one maps to exactly one node and back (checked 2026-11-12; at 44 files it was 292, and D21's four fixtures and the two rows of `object_literals.ts` are the other 42). Dropping `ArrowFunction` from the kinds gave 36
failures and not skipping trivia gave 314, both measured on the 292.

- **The node is not where `Def::range` starts.** An overloaded def's range runs from its first signature and its node is the implementation (what the checker returns too); `const f = () => {}` has the whole
  statement as its range and the arrow as its node; a modifier (`export`) is inside a node's span.
- **Why not `name_range.start`** (the review's suggestion): it is one token and unique, but from the name token to the node takes a rule per kind (a variable declaration's initializer, a property's, a
  constructor's keyword, `default`), which is the def rule written a second time. The span comes out of the walk that finds the defs.
- A def with no unique node is `None` in `FileDefs::nodes`; `describe` (step 3) turns that into an `Unresolved` edge and a count (`decl_not_in_outline`). None was found in the suite.

### The plain calls of a body (step 2b)

`Sem::call_sites` follows `tsrs_ls`'s `CallSiteCollector` (`callhierarchy.rs`): `Ambient` nodes are not descended, nor an identifier, an import or export declaration, an interface, a type alias, the type side
of `as` / `satisfies` / `<T>x`, the type of a variable or parameter, a call's type arguments, or any type node. The differences, all from the plan: it stops at a nested node that is a def **of the outline**
(`FileDefs::def_of`) and not at what tsrs's call hierarchy calls a declaration, so an anonymous function and an anonymous class's initializers belong to the def around them; a class's `extends`
expression, decorators (also those on its members), property initializers and static blocks are the class's body, and its methods, accessors and constructor are defs of their own (tsrs makes a static block its
own call-hierarchy item and walks the constructor in the class; we do not). A nested class still owns the calls in its computed member names, as in tsrs.

- **Only plain calls are sites so far**: `f(x)`, `a.b(x)`, `a?.b(x)`, `(g)(x)`, `a["k"]()`, `f()(x)`, `super(x)`. Not `import("./m")` (an `ImportKeyword` callee), not `require("./m")` (an identifier `require` with
  one string argument), and, until steps 5 and 6: `new`, a tagged template, a decorator's own site, a JSX element, a property read that is an accessor.
- **The site is the callee expression**, from its first token to its end, relative to the def's start (`Def::range.start`). `f()(g())` has the sites `f`, `f()` and `g`; sites are sorted by where the callee
  starts, then where it ends, so an argument's call comes after its callee's. That is source order of the resolved expression, not evaluation order; the order of siblings in the wire is the open question W3 Q4 and
  does not depend on this.
- The walk is checked on `tests/fixtures/sites/src/sites.ts` (28 defs, lists written from the rules before the run). Not counting the "nested def" rule gives `["f", "g", "a.b"]` for `nested` instead of `["f"]`;
  counting `import()` and `require()` gives two sites for `not_sites`.

### Resolving a site (TS4 step 3)

`tests/fixtures/resolve` (`callers`: 21 sites) is the contract; the expected answer for each site was written from the policy table and the engine's rule before the first run. 18 matched at once.
The three that did not, and what each showed:

- **An overloaded function.** The engine's call hierarchy names the **first signature** (no body). `find_implementation` looks for the implementation through `symbol.value_declaration()`, which for
  `function f(a: string): string; function f(a: any): any {}` is the first signature, so it finds nothing and `find_all_initial_declarations` keeps the first of the adjacent signatures. The outline's def
  is the implementation (TS1), so `sem` maps a bodiless function or method to the declaration of its symbol that has a body, in the same file. Without it: `decl_not_in_outline`.
- **`counter["bump"]()`.** The hierarchy's resolver finds nothing for an `ElementAccessExpression` callee (`get_symbol_at_location` is `None` there), but `get_resolved_signature_exported(call)` →
  `declaration()` is the method. So the signature is the second try. (The plan's §4 had it first; for a plain name the hierarchy answers, and the signature adds nothing.)
- **A union receiver (`w: JsonWriter | CsvWriter`).** The hierarchy gives no declaration, and the signature's `declaration()` is the **first member's** method only: taken as the answer it would be wrong and
  look right. But the checker does have the answer: the property of a union gets a synthetic symbol that holds the declarations of **every** member (`tsrs_checker/src/checker_11.rs`, the loop that
  appends each member's `declarations`), and its `value_declaration` is set only when the members share one. The hierarchy follows `value_declaration` alone, hence nothing. So `sem` asks that symbol
  (`get_symbol_at_location` of `a.b`, or of the literal in `a["b"]`) and the policy table's "one target per member" is the checker's answer, not an addition of ours. The first version of this step
  enumerated the members itself (`Type::types()`); review found it wrong for `a["b"]` (fell to the signature: first member only), for a member that holds an arrow (`write = () => {}`: dropped, so it
  became `union_receiver` inside a union and a target outside one), and for `A | B | undefined` (a spurious `Unknown`); all three go away when the checker's symbol is asked. A declaration that is a
  property of function type or of type `any` has nothing to enter and adds `Unknown(union_receiver)`. Not tried: an intersection, a generic method on a union.

Other facts: a call through a function type (`cb()`) has `get_type_at_location(callee)` with call signatures and no declaration; a value of type `any` has `TypeFlags::Any` (both are no declaration, told
apart by the type). A project `.d.ts` is `External` and named by its **relative path** (`src/types.d.ts`), since it has no package; `node_modules` packages are named by `package_of` (`pkg`, `@types/x` as
`x`), the bundled libraries `typescript:lib`, and the name is **qualified** by the classes, interfaces and namespaces around the declaration (`Math.max`, `Array.map`, `helper`), because the merged wire counts a repeated target as one and Python names an external by its qualname too. One checker handle is taken per `callee` and dropped before it returns
(one checker, so one handle at a time); that is correct, and it was not timed.

### The first graphs (TS4 step 4)

`typescript-cli calls` on the V-NOC frontend (`src/frontend`, release build, 2026-11-12, read only), entry `TaskDetailPanel` (the largest component, 427 lines):

| | |
|---|---|
| open the project | 63 ms warm, up to 230 ms cold |
| the graph (18 contexts, 173 targets) | 13 to 31 ms: one checker handle per `callee`, so it is not a cost to worry about (S-TS3 measured 437 sites in 57 ms) |
| edges | 32 call, 0 construct, 66 external, 75 unresolved |
| unresolved by reason | `function_typed_value` 75 |

- **The first run said otherwise, and the fixture reproduced it.** It reported `function_typed_value` 66, `union_receiver` 8 and `no_declaration` 1. The eight are react-query's `mutation.mutate(…)`: the
  receiver is a union of result types and `mutate` is, in every member, a **property of function type**; the one is an optional callback prop (`(id) => void | undefined`). Both are calls through a
  function-typed value (D16). `why_unknown` now asks whether the callee's type is callable (call signatures, or a union whose members other than `undefined` and `null` all have them) **before** it asks
  about a union receiver, and `union_receiver` is left for a union where a member has no declaration to enter beside members that do (a class method and a function-typed property). Fixture:
  `optional_callbacks` in `tests/fixtures/resolve`.
- **A script file merges with the lib's globals.** The scenario `straight-line` (`function fetch() {…}` with no `import` or `export`) gave an extra `External` for `fetch`: in a script the function and
  `lib.dom.d.ts`'s `fetch` are one symbol with two declarations, and the engine reports both. That is right, and real TypeScript files are modules, so the scenario programs start with `export {};`. An inferred
  project of a plain script (`jsconfig`-less JavaScript) has the same merging with whatever the DOM lib declares (`name`, `status`, `open`, `close`, `parse` is not one).
- **A class reached by a plain call** (`Foo()`, `super(x)` when the engine names the class) is a `Construct` with no `init` for now; `new` and the constructor are step 5.
- **The breakdown is the first thing to read** when a tree looks short: `edges` says how many calls were not followed and why. 66 externals in one component is the React and library surface; the share of
  `function_typed_value` is what context sensitivity (C3, unscheduled) would buy back.

### What the 75 were, and the choices among the engine's declarations (TS4 step 4, after review)

**`function_typed_value` is evidence only when the callee is declared as a value holder.** After the `any` check the first version asked whether the callee's *type* is callable, which is true of every call
that type-checks, so a function the resolver failed to enter would have been counted as a D16 case. Now the callee's symbol (through an import alias) must be declared as a parameter, property
(`PropertySignature`, `PropertyDeclaration`, `PropertyAssignment`), variable, binding element or shorthand property; a callee with no symbol (`makeHandler()()`, `(flag ? f : g)()`) is `no_declaration`.
`union_receiver` is left for a union where some members have a declaration to enter and some do not.

On the V-NOC component `TaskDetailPanel` the 75 were all value holders by declaration, but of four kinds (counted from the list of callee texts, not estimated):

| kind | calls | example | now |
|---|---|---|---|
| a value declared **in a package** with a function type | 18 | `useQueryClient` 8, `toast.error` 2, `mutation.mutate` 8 (react-query's result, a union whose members each hold a `mutate`) | `External` (a named call into the package): 84 external, 57 unresolved |
| a state setter from `useState` (a destructured binding) | 25 | `setAddingSubtask`, `setSubtaskQuery` | D16, as decided |
| a project hook or store built by a call | 11 | `useVersioningStore(…)` 6, `useProjectStore(…)` 4, `useTabStore(…)` 1 | D16 |
| an **arrow held by a property of an object literal in the project** | 20 | `tasksApi.updateTask(…)` and ten more, `queryKeys.tasks.anchorSummary(…)` and two more (9 in all) | D16 at the time, and **a coverage gap** (closed by D21, see "Arrows held by object-literal properties" below): these were project functions the outline did not treat as defs (a property holding an arrow is not a def, "for now"), and the engine's call hierarchy does not resolve them either (`is_assigned_expression` wants a `const` or a class property). Idiomatic code puts a whole API in such an object; whether `key: () => {}` should be a def is the owner's question |
| an optional callback prop of the project | 1 | `onNavigateToNode` | D16 |

25 + 11 + 20 + 1 = 57, the unresolved edges left after the 18 moved to external.

**A union of callables has no single declaration, and the checker hides that.** `(flag ? plain : second)()` resolved, through the resolved signature, to `plain` alone: for the branches of a conditional the checker
applies subtype reduction, and two alike function types become one type, so the signature names the first. The signature is therefore used only for a **name** (identifier, property access, element access) and
not when the name's type is a union of more than one type once `undefined` and `null` are set aside; for anything else (a conditional, a `||`, a call's result) the answer is `no_declaration`. Resolving each branch
of a conditional would be the next step; it was not done.

**D19, a script file's function that merges with a library global.** Pinned in `tests/fixtures/script-global/main.ts` (no `export`): `function fetch()` and `function open()` are each one symbol with `lib.dom.d.ts`'s,
and the engine lists both declarations (`main.ts fetch` and `typescript:lib fetch`). *Our choice among the engine's declarations*: when a declaration with a body is in project code and every other one is in a
declaration file, only the project's stay; `declare function alert(…)` in the same script, with no body, keeps both. An engine bump must re-run this fixture: it depends on the checker listing both declarations
of a merged symbol, in (file, offset) order.

**The seam** (`key.rs`, `env.rs`, `walk::resolve(site, &env)` with `resolve_from_env` returning `None`) exists as `docs/02-tree-rules/03-the-seam.md` §3 asks; `calls.rs` implements `Lang` and delegates to `walk::body`.
`def_without_node` is one `Unresolved` edge at the def's first byte and a count, the context `Finished`, and it is tested by a test hook that drops a def's node (nothing in the suite makes the two parses disagree).
A class reached by `super(x)` or `new` is a construction whose `init` is the constructor that runs (below).

### `new`, constructors and library globals (TS4 step 5)

Pinned in `tests/fixtures/construct/` (the graph of `build` and of `convert`, every edge written from the rules before the run) and in the scenarios `construct-marked-class` and `external-call`
in TypeScript. A `new` is a site on the class expression (`new a.B(x)` → `a.B`), found by the same `Collector` as a call; the engine resolves it like a call, through the symbol of the class.

- **`init` is the constructor that runs, found up the chain of base classes (`Sem::init_of`)**: the class's own, else the nearest base class's in project code (`Grandchild extends Child extends Parent`,
  only `Parent` has one: `Parent.constructor`). This was the plan's rule (an inherited constructor is the base class's), written for one level; the code follows the chain. A chain that reaches a base
  **outside the project** (`extends Error`, a package's class) or a base the checker cannot name as a class (`extends mixin(Base)`) ends with `init: None`, and that is a limit and not an answer, so
  it is counted: `Stats.language["inherited_constructor_external"]` and `["inherited_constructor_unknown"]`, listed by `Breakdown.notes`. They are not `Unresolved` edges: the class is known and the edge
  is a construction. The base's constructor is not an edge of its own (there is no site for it), so on a graph `new MyError()` shows no `Error` (open question Q-TS1 in `docs/07`; a core change). In the fixture: `FromPackage` and `MyError` (external),
  `Mixed` (unknown). **With a constructor of its own it is shown**: `class WithSuper extends Error { constructor() { super(); } }` has an `External` edge `typescript:lib Error` at the `super` site (fixture `WithSuper`, `PackageWithSuper`). The checker's symbol for the `super` keyword is the constructor's *type* (`ErrorConstructor`), which would have been a second name for `Error`: `external_declarations` reads the base from the class's `extends` expression instead, as `init_of` does.
- **An overloaded constructor is three defs in the outline** (two signatures, the implementation) and the first has no body. `Sem::constructor_of` takes the one with a body. The first version took the
  first def: the tests caught it (`Overloaded`'s `prepare()` was missing from the tree), the engine's own hierarchy has the same rule for a function (`find_implementation`).
- **A function reached by `new` is a call** (`new Legacy()`, an old-style constructor function): its body is the constructor, and it is a context like any function. The plan said "a class (or `new` was
  used) → `Construct`"; the graph's `defs` table describes a *class*, and a function has no class row, so the plan text was changed to match.
- **A class outside the project is an `External`, never a construction**, as the plan said: `new Map()` → `typescript:lib Map`, `new Boxed(1)` → the package, a class of the repo's own `.d.ts` → that file.
- **A library global declared as an interface and a `var` has no declaration with a body** (`Map`, `String`, `Date`, `Intl.DateTimeFormat`): the hierarchy gives nothing, and the first version of step 5 would
  have called every `new Map()` and `String(x)` `no_declaration`. `external_declarations` (the old `external_value_holder`, widened) takes the symbol's declarations when **all** are outside project code
  and calls them `External`; it applies to a call and to a `new`. *Found on the V-NOC frontend, not in the fixtures first:* `applyOps` had 4 unresolved edges and every one was `String(op.value)`; they
  are 0 now (21 → 25 external). *Our choice:* a declaration in the project in the same symbol keeps the old answer (a project value holder is D16's).
- **A class held by a value** (`declare const Ctor: new () => Plain`, a parameter of type `typeof Plain`) is `function_typed_value`, like a callback: the construct signature has no declaration to enter.
- **A `declare function` or `declare class` of a project file** is a def with no body, a leaf (`pick` in the fixture), as an interface method is.
- V-NOC `TaskDetailPanel`: 32 call, 0 construct, 88 external, 57 unresolved (84 external before this step). The 4 more are `new` of library classes: before, none of the panel's `new` was a site, and `no_declaration` was 0 both times, so the string-call fix adds nothing here. The 57 are unchanged.
  Project classes are rare there (two `new`, both at module level, in no def), so the project-class paths are covered by the fixture only.
- **A project file that augments a library global makes it the project's too.** `declare global { interface String { shout(): string } }` in a project file gives the symbol `String` a project declaration, so
  `external_declarations` (every declaration outside the project) says nothing and `String(1)` is `no_declaration` again in that project; `Number(…)` and `new Date()` stay `External` (checked on a scratch
  project, 2026-11-12). Left as it is: the rule is "all declarations outside", and narrowing it to the declarations that have a signature would be the next step if a real project shows it.

### Arrows held by object-literal properties (D21, TS4 after step 5)

The outline rule is D21 (`docs/07`); `typescript-syntax` tests pin it in four dialects (`object_literal_arrows.*`), the marker tests pin where a marker goes, and the 292 existing defs did not move (the existing
snapshots gained two rows, `api.arrowProp` and `api.functionProp` in `object_literals.ts`, and lost none). **Resolution**, pinned in `tests/fixtures/object-arrows/`: three places in `sem.rs` know that a property can hold the
function.

- `declarations_of_signature` takes an arrow or function expression that is the value of a property assignment (`api["load"]()` and the named `api.load()` both reach it through the checker's resolved signature).
- `callable_declarations` does the same for the members of a union receiver (`either.go()`: one target per member, as for methods).
- `classify` turns a property arrow that is **not** a def by D21's rule (a `let`, a default export, a class property's literal, an argument's literal) into `Unknown(function_typed_value)`: it is a holder of a function
  declared in the project, which is what D16 counts. Without this arm the outline and the engine disagree and the debug assertion for "not a def of the outline" fires (mutation-checked: the arm removed, the test panics there).
- An attempt that was **not needed**: a branch in `declarations_of`, the port of the engine's hierarchy, for a property assignment. The test passed with it removed (the signature fallback already finds the arrow), so it was
  taken out; the port is as it was.

An external property arrow (a `.js` file in a package) is named by its property in the `External`, as `own` in `classify`.

**On the V-NOC `TaskDetailPanel`** (the proof of D21, `typescript-cli calls … --entry TaskDetailPanel`):

| | before | after |
|---|---|---|
| edges | 32 call, 0 construct, 88 external, 57 unresolved | 81 call, 2 construct, 111 external, 38 unresolved |
| contexts | 18 | 41 |
| wire nodes | 41 | 190 |
| open / graph | 63–230 ms / 13–31 ms | 88–117 ms / 15–19 ms |

The 57 were 25 state setters, 11 project stores and hooks, 1 optional callback prop and **20 object-literal arrows**; the 20 are all calls now. The 38 are the same 37 (25 + 11 + 1) and **one new one**: the walk now enters
`readOnlyGate.ts`, which calls `useSidebarModalStore.getState().openReadOnlyModal`, a method of a zustand store, held by an object literal that is the return value of `create((set) => ({ … }))`: D21's rule 2, unresolved on purpose.
The two constructions are the first project classes constructed in a real run (`ApiError`, `ReadOnlyModeError`, each with its own constructor as `init`). The 11 stores (`useVersioningStore`, `useProjectStore`, `useTabStore`) stay
unresolved. Checked for `useVersioningStore` and `useSidebarModalStore`: both are `create<State>()(…)` results, so the arrows are inside an argument's literal (rule 2); `useProjectStore` and `useTabStore` were not opened.

**A def that has no frame.** An arrow property is a def, and what is inside its body keeps the names and parents it had before (`api.inner`, not `api.load.<locals>.inner`): `Def::parent` of such a def is the enclosing def, not the arrow.
The tree of the arrow shows its calls (it is a context), but a named function inside it is a sibling in the outline. This is the price of not renaming; if the owner decides a rename is acceptable at `typescript-v0.2.0`, the
frame is one line (`add` becomes `enter`).

**Calls leave the enclosing def, as they do for `const handler = () => {}`.** A `const` object literal **inside a function** (`const local = { l: () => other() }`) now owns the arrow's calls: `run` shows them only if something
calls `local.l` through a name the checker resolves (fixture: it does). Passed on without being called (`register(local)`), they are in no tree. Same trade as the named const arrow, which was already a def.

### Defs handed over and not called (a counter, TS4 after D21)

**The gap.** Three things the owner and the review found are one: a def that is *handed over* instead of called. (a) A D21 arrow in a local `const` literal that is passed on (`useEnhancedTreeLayout`'s
`onToggle: () => toggleNodeExpansion(nodeId)`); (b) a method of an object literal that is an argument (Q-TS2); (c) a function named in a value position, `onClick={handleClose}`. In each the def exists, nothing
calls it through a name the checker follows, so no edge reaches it and what it calls is in no tree. **This step only counts them; it adds no edge, node or context and changes no tree.** How such a def shows in the tree is the
owner's decision (Q-TS3 in `docs/07`).

**What is counted** (`Stats.language["def_written_not_called"]` and `["def_named_not_called"]`, `Breakdown.handed_written` / `handed_named`, one line in the breakdown when either is above 0). A *distinct def* of a
**walked body** (a context of the graph) that is a function with a body (a function declaration, a function expression, an arrow, a method; not a class, an accessor, a constructor or a signature) and that **no edge of
the graph reaches** (`Context`, or the `init` of a `Construct`):

- **written inside an expression**: the def is made where an expression is, with no name of its own to reach it by: a method or an arrow property of a literal (`run({ onSuccess() {} })`, `local = { traverse: () => {} }`), a
  named function expression (`xs.map(function step() {})`). Counted for being written there.
- **referenced by name**: a project function with a body that an identifier, a property access or a shorthand property names **without calling it**: `onClick={handleClose}`, `xs.map(parse)`, `register(api.load)`,
  `register(widget.method)`, `{ f }`, `{...{ onKeyDown: onKey }}`. Found through the checker's symbol for the name (and `callable_declarations`, the same mapping a call uses). A function of its own name that nothing names
  (`const unused = () => {}`) is dead code and is **not** counted (the first version counted it: the by-name rows passed for the wrong reason until a control row, `neverUsed`, and a mutation showed it).
- A def that is both (made in an expression and also named) is counted **once, as written**, even when the two happen in different bodies of the tree (`writtenHereNamedInAClosure`).

**Not counted, on purpose** (a callee, a tag and a decorator are *called*, not handed over): the names being called, a JSX element's tag and an attribute's name, a tagged template's tag, a decorator (step 6 makes the last three
sites). **Not looked at, so the count is a lower bound**: an element access (`api["load"]` as a value), `f.bind(…)` and `f.call(…)` (a callee's object is the thing called); a write to a function-valued property
(`a.b = handler` is read as a reference); and, **in a graph with one entry**, the defs *inside* a handed-over body, because that body is not walked (`handleBack` is handed over and calls `handleClose`: `handleClose` is counted
not called, and the defs `handleBack` itself hands over are not counted at all). A graph that has every def as an entry walks every body, so it has no such gap.

Pinned in `tests/fixtures/handed-over/`: 21 rows of `main.ts` (one function per shape, the expected pair `(written, named)` written from the rules before the run) and `panel.tsx` (JSX attribute, spread attribute, element
tag). **Mutation-checked**: the callee/tag/decorator suppression, the written rule, the shorthand record, "only written nested defs", "written wins", the subtraction of reached defs and the use of call edges, each makes a row
red. One was caught only by writing the control: a callee suppression test with a `declare function` passes whatever the code does, because a function with no body is never counted; the controls are functions with bodies.

**The numbers** (V-NOC frontend, 2026-11-12; `typescript-analysis` `measure_handed_over`, an ignored test driven by `TS_MEASURE_ROOT`, `TS_MEASURE_FILE`, `TS_MEASURE_ENTRY`, `TS_MEASURE_LIST`):

| graph | entries | graph time | edges | handed over, not called |
|---|---|---|---|---|
| `TaskDetailPanel` | 1 | 16–18 ms (unchanged by the counter) | 81 call, 2 construct, 111 external, 38 unresolved (unchanged) | **0 written, 7 by name** |
| the whole frontend, every function, method and class an entry | 1084 | 200 ms | 1185 call, 3 construct, 2399 external, 1009 unresolved | **3 written, 89 by name** |

The 7 of `TaskDetailPanel` are its handlers passed as props (`handleClose`, `handleBack`, `applySuggestions`, `commitDescription`, `navigateToTask`, `openSuggest`, `toggleSubtaskDone`); each was checked against the
source, and `handleClose` is also *called*, but only inside `handleBack`, which is handed over and not walked, so that call is in no tree. The 3 written of the frontend are `useEnhancedTreeLayout`'s
`onToggle` (the review's loss), `CreateProjectDialog.onSubmit`'s `onSuccess` (Q-TS2's shape) and `FlameGraph`'s `itemData.scale` (a property arrow in a local literal passed on). Of the first 40 of the 89 by name (listed in path order), the ones read are event handlers
(`onDrop`, `handleCopy`, `onAbort`) and React components used as values (`DynamicIcon`, `CalendarDayButton`, `IconComp`); the rest were not read. "Not called" there means *called by nothing in the project*, since every def is an entry.

### Passed edges, by name (D22 step 3)

A project function named where a value is **handed over** is now an edge: from the def that names it, at the name, to the def's context, **passed** (`SiteCalls::passed`, drawn `[Passed]`, `passed: true` on the wire).
The solver walks it like a call, so what it calls is in the tree. Pinned in `tests/fixtures/handed-over/` (`main.ts`, `panel.tsx`) and in `walk.rs` (the sites of a body, in source order).

**What is a hand-over is an allowlist, not "named and not called".** The counter before this step took every named, non-called reference, which was fine for a number and wrong for an edge: `if (handler)`, `typeof handler`,
`handler === other`, `!handler` and `handler.length` read a def, they do not give it to anything, and an edge would say they did. `is_handed_over` (`sem.rs`) says where a value is handed over: an argument of a call or
`new`, a property value (and `{ f }`), an array element, an initializer or the right of `=`, a return or an arrow's concise body, a JSX attribute value or child; through `(f)`, `f as T`, `f satisfies T`, `<T>f`, `f!`, a
conditional's branches and the right of `&&`, `||`, `??`. A callee, a tag, a decorator and a condition are not on it. This replaced the "walk the callee with a flag" suppression of the counter, which the allowlist makes
unnecessary. *Our choice, not the engine's;* the engine has no notion of it. It is narrow on purpose: a position not listed is not an edge, and shows nowhere (it is not counted either).

**What the checker says.** The name is resolved with the checker's symbol (`get_shorthand_assignment_value_symbol` for `{ f }`, whose symbol for the name is the property) and the same `callable_declarations` a call uses, then kept
if it is a **function with a body of the project**. Each place a def is named is its own edge (`namedTwice`: two). A def both called and named in one body has both edges (`constArrowCalled`).

**Mutation-checked**: every guard of the allowlist (a ternary's condition, either side of an operator, a comparison, a callee, any other position, the `!` wrapper, the concise arrow body, the shorthand), the source order of a
body's sites and "a hand-over is passed, not a call" each turns a named row red. Two were caught only after a row was added for them: a def on the **left** of `&&` (`handler && 1`), and the order of `Summary.sites`, which the graph
and the renderer both sort away, so only a test of `walk::body` itself sees it.

**The numbers** (V-NOC frontend, 2026-11-12):

| | before | after |
|---|---|---|
| `TaskDetailPanel`: edges | 81 call, 2 construct, 111 external, 38 unresolved | 84 call, 2 construct, 122 external, 48 unresolved, **15 passed** |
| `TaskDetailPanel`: constructions | 2 (`ApiError`, `ReadOnlyModeError`, each with its own constructor as `init`) | 2, the same |
| `TaskDetailPanel`: contexts | 41 | **50** |
| `TaskDetailPanel`: graph time | 16–18 ms | 16 ms (23 once in five runs) |
| `TaskDetailPanel`: handed over, not called | 0 written, 7 by name | 0 written, **0 by name** |
| whole frontend (1084 entries): edges | 1185 call, 3 construct, 2399 external, 1009 unresolved | the same, **143 passed** |
| whole frontend: handed over, not called | 3 written, 89 by name | 3 written, **0 by name** |
| whole frontend: graph time | 200 ms | 132 ms (a different run, not a speed-up: the same walks plus the edges) |

The 7 handlers of `TaskDetailPanel` are edges (8 passed nodes at the top, `navigateToTask` twice), and **`handleClose`'s call inside `handleBack` is in the tree**: `handleBack [Passed]` has `handleClose` under it, and
`handleClose [Passed]` is its own node, since `onClick={handleClose}` hands it over too. The extra 9 contexts are the 7 handlers plus `firstDoneColumn` and `firstWorkflowColumn` (the owner's review listed them; an earlier version of this text named `taskMutationError`, which was
already a context: `useTasks.ts` hands it to a mutation's `onError` seven times, and those are seven of the passed nodes); the extra 10 unresolved are all `function_typed_value`, state setters called inside those bodies, which were not walked before. The graph is bigger and no slower.

`def_named_not_called` is 0 **because every named hand-over now has an edge**; it is kept as a cross-check (a nonzero value would mean the edge builder and the counter disagree), not as evidence of coverage. What is
still counted is what has no edge: a def written inside an expression (`def_written_not_called`, step 4), and step 5 adds what no step covers (an element access, `f.bind(…)` and `obj.m.bind(obj)` handed over, `f.call(…)`, a class named as a value).

**A known over-report: a hook's dependency array.** `useEffect(fn, [a, b])` hands `a` and `b` over by the rule (an element of an array that is an argument), and they are passed edges, both, when they are project
functions. A dependency array lists what the hook watches; it does not give it to anything. The rule is **not changed** (the owner's decision): an array element is a hand-over, and this is the one place where that
says more than the code does. Pinned with its current answer in the fixture (`inDependencyArray`, 2 passed edges).

### Passed edges, written inside an expression (D22 step 4; closes Q-TS2)

A value that **holds defs** and is handed over hands them over with it. The def that hands it over gets an edge, **passed**, to each of them:

| What is handed over, and where | The edge | Site |
|---|---|---|
| an **object literal written in a hand-over position** (`register({ onSuccess() {} })`, `return { m() {} }`, `[{ m() {} }]`): its function members, through nested literals | to each member def | the member's **key** |
| a **name that holds a literal** (`register(local)` with `const local = { traverse: () => {} }`, `register(o.data)`, an import, a literal under `as const`): the literal's function members, through nested literals | to each member def | the **name** (all the members at one site) |
| a **named function expression** written where a value is handed over (`xs.map(function step() {})`) | to it | its **name** |

A member is a method, or an arrow or function expression held by a property, **that is a def** (a method always is; an arrow property is a def by D21, so in an argument's literal it is not, and nothing is handed over for it:
`register({ m() {}, a: () => {} })` hands `m` over). A getter, a setter, a shorthand property (a hand-over of its own where the literal is written) and a computed key are not looked at. Pinned in `handed-over/` (`argMethod`,
`localLiteral`, `inPlaceLiteral`, `literalByName`, `nestedLiteralByName`, `nestedLiteralByAccess`, `wrappedLiteralByName`, `importedLiteralByName`, `literalPassedInAClosure`, …).

**What is *not* handed over, on purpose** (rule 3 of D22: a def nothing names stays unreached and uncounted): a literal that is only **made** (the initializer of a variable, a property value, a default: the *name* that holds it hands
it over); a dead literal (`const dead = { m() {} }`, `deadLiteral`), a nested literal of one nobody hands (`storedLiteralNotHandedOver`), a `let` literal's arrow (not a def), a literal that is only read (`if (o)`, `typeof o`), a named
function expression that is bound (`const g = function step() {}`, `{ cb: function inner() {} }`). This changes what the old counter counted: **a def written in a literal that nothing hands over is no longer counted**
(the first counter took every unreached def written in a walked body).

*Our choice, and the one place it is broader than the counter was:* a name that holds a literal declared **anywhere in the project** hands over its members, not only a literal written in the same body (`register(api)` with `api` from
another file). Handing over an object hands over what it holds; if the receiver calls one member, the tree still shows all of them under the name. A literal handed over with many members shows many nodes.

**Q-TS2 is closed.** `run({ onSuccess() { f(); } })` was a def nobody called, so `f()` was in no tree: the tree of `argMethodCalls` is now `register`, then `argMethodCalls.<locals>.onSuccess [Passed]` with `sharedHelper` under it.

**Mutation-checked**: the in-place literal, "a bound literal is not handed over", nested literals, a name that holds a literal, a property that holds one (`o.data`), the wrapper over a held literal, a bound named function expression
and a handed one each turns a row red. `def_written_not_called` and `def_named_not_called` are **0 by construction** now (every hand-over has an edge, and a def nothing hands is not recorded); `calls.rs` tests the counting with a record
made by hand. They stay as a cross-check, not as evidence.

**The numbers** (V-NOC frontend, 2026-11-12). `TaskDetailPanel` is unchanged (84 call, 2 construct, 122 external, 48 unresolved, 15 passed, 50 contexts, graph 16 ms): it hands over no literal. The whole frontend (1084 entries): passed
edges **143 to 153**, call edges unchanged at 1185, graph 126 ms; **the 3 defs it counted as written are all passed nodes now**, each in the tree of its enclosing def: `FlameGraph.<locals>.itemData.scale` (under `FlameGraph`),
`CreateProjectDialog.<locals>.onSubmit.<locals>.onSuccess` (under `onSubmit`, with `idPrefixRemover` under it), and `useEnhancedTreeLayout.<locals>.traverse.<locals>.rfNode.data.onToggle` (under `traverse`). The call the review found
lost, `toggleNodeExpansion(nodeId)` in `onToggle`, **is an edge of the graph again** (bytes 16 to 35 of `onToggle`, 19 characters, the callee): `Unresolved(function_typed_value)`, since `toggleNodeExpansion` is a store action (D16),
and so not drawn in the tree, which hides unresolved edges unless asked. The other 7 of the 10 new passed edges were not listed.

### What no edge covers: counted, not drawn (D22 step 5)

Four shapes hand a def over, or call one, in a way no edge covers. Each is **counted per occurrence** in every walked body (`Stats.language["uncovered_<kind>"]`, `Breakdown.uncovered`, one line in the breakdown when any is above 0):

| Kind | What it is | Why there is no edge |
|---|---|---|
| `element_access` | a project function handed over by `a["k"]`, the key a **string literal** the checker can follow | resolving it is the checker's signature fallback for a call; for a value there is no site rule yet |
| `bind` | `f.bind(…)` or `obj.m.bind(obj)` **in a hand-over position**, `f` / `obj.m` a project function | the value handed over is a new function (the bound one), not the def |
| `call_apply` | `f.call(…)` / `f.apply(…)` with `f` a project function, **anywhere** | it is a call, and the call site is `f.call`, which resolves to the library's `Function.call`: an `External` edge, not an edge to `f` |
| `class` | a project class named where a value is handed over (`register(Widget)`, `{ Icon: Widget }`) | a passed edge points at a context, and a class has none (it is a `Construct` target, its constructor is `init`) |

Not counted, because it cannot be told from code that is not a def: an element access with a **computed key** (`a[key]`), a spread of an object that holds defs (`{ ...api }`). (A function named in `bind`'s own arguments is an ordinary argument: an edge, step 3.) A count is
a lower bound for what the checker can name.

Pinned in `handed-over/` (`elementAccessHandedOver`, `bindHandedOver`, `bindOfAMethod`, `callAndApply`, `classHandedOver`, `classInAProperty`, and the shapes that must **not** count: `elementAccessWithAComputedKey`,
`elementAccessKeyIsAFunction`, `elementAccessCalled`, `bindDiscarded`, `classNotHandedOver`, and the shapes the edges cover). **Mutation-checked**: a discarded `bind`, `apply`, a computed key, the class count and a called
access each turn a row red; one control was wrong the first time (a key wrapped in `as unknown as string` has no symbol, so the guard was never exercised) and was rewritten.

**On the V-NOC frontend all four are 0** (`TaskDetailPanel` and the whole frontend, 1084 entries; edges as in step 4, 153 passed), and that is the source, not the counter: there is no `.bind(` in it; its 3 `.call(` are on
`debounce(…)` results (`debouncedBasicInfoChange.call(data)`: a variable, not a project def, so correctly not counted); its 17 string-literal accesses are indexed **types** (`Task["subtasks"]`); its 5 classes are never named in a hand-over
position (the class counter reads 0; I did not check where each is used). The fixture is the evidence that the counters count; the frontend is the evidence that, for this project, the remainder is nothing.

### The remaining site kinds (TS4 step 6)

**What the engine does and what we do.** The engine's collector (`callhierarchy.rs`, `CallSiteCollector::collect`) records a site for a JSX element (the tag name), a tagged template (the tag), a decorator (its expression) and **every** property or element access, and resolves each by its symbol's value declaration. We follow it, with five differences, each found by writing the expected answer before the run:

| Difference | Why |
|---|---|
| an **intrinsic** JSX tag is no site | the engine resolves `div` to `JSX.IntrinsicElements.div`, which is nothing to enter: an `External` or `no_declaration` edge for every element of every component. TypeScript's own rule (`isIntrinsicJsxName`): a lower-case first letter or a dash, or a namespaced name |
| a **closing** tag is not visited | the engine records opening and self-closing elements only; the same here, and an access in a closing tag (`</props.Icon>`) is not an accessor site either (`is_callee_position`) |
| `@dec(1)`: the engine records the expression `dec(1)` and **finds nothing** (a call has no symbol) | we say what it is: the call through the result. The checker's resolved signature of the decorator names the function when it can be entered; otherwise `decorator_result` (a new reason), so the second call is a number and not a silent hole. The first call (`dec`) is an ordinary call site |
| **parameter decorators** are not collected by the engine (a `Parameter` is read for its name and initializer only) | the class's own body collects the decorators of the parameters of its constructor, methods and accessors |
| the engine records every property access, and a getter and a setter have **one** `value_declaration` | we pick the half by how the access is used (`ast::is_write_access`, `is_write_only_access`: `=` and loop and destructuring targets are writes; `+=`, `||=`, `++` read and write) and read the symbol's own declarations, as for a union |

**Cost of asking the checker about every property access** (V-NOC frontend, release build, the same machine, 6 runs each): the checker is asked once per body that has a candidate, for the access's symbol, and is a site only when a declaration is an accessor. `TaskDetailPanel`: 411 candidates in 90 bodies, 0 accessors; the **whole frontend** (1084 entries): 4198 candidates, **37 accessors**, 1148 JSX component sites, 2 tagged templates, 0 decorators. The 37 are DOM and library getters (`classList`, `style`, `textContent` written, `window.location`) and Tiptap's `editor.document`: `External` edges. Whole-frontend graph time **257 ms with the accessor query, 258–264 ms with it switched off** (3 runs each): no measurable cost. A probe that timed the query inside the walk read 86 ms in total, and that is the checker's lazy work for types, which the next question would have paid; so time the whole, not the query.

| `TaskDetailPanel` | step 1 (0d9d6d0) | step 6 |
|---|---|---|
| edges | 84 call, 2 construct, 122 external, 48 unresolved, 15 passed | 141 call, 2 construct, 235 external, 86 unresolved, 18 passed |
| contexts | 50 | 90 |
| graph time | 14–15 ms | 27–30 ms |
| unresolved by reason | `function_typed_value` 48 | `function_typed_value` 86 |

The growth is the JSX components that were never in the tree (35 JSX sites in 90 bodies): each is a context, its children are in the tree, and the time per context is the same (0.29 ms against 0.31 ms). Whole frontend: 1921 call, 3 construct, 2802 external, 1057 unresolved (`function_typed_value` 1034, `any_or_untyped` 17, `no_declaration` 6), 153 passed.

**Known and left**: a getter or setter in an **assignment** destructuring (`({ value } = box)`) is neither a site nor counted; an element access with a **computed** key is neither (as before); `styled(Button)`…`` is `no_declaration`, as `f()()` is; for an **external** decorator factory (`@Component({…})`, `@Input()`) the second call is `decorator_result`, one more unresolved edge for every use.

### What the tree still leaves out, counted (TS4 step 7, part A)

Step 6's review found six things that the tree does not draw and nothing counted. Each is now a number in `Stats.language["uncovered_<kind>"]` and in `Breakdown.uncovered`, in the same line as step 5's four, with no edge and no node added. The owner's decisions stand: decorators stay sites of the class body, a class component's `render` is not drawn (the analyzer holds no framework rule), an instance field initializer under a construction waits for a design note (`docs/02-tree-rules/07-what-a-construction-runs.md`).

| Kind | What is counted, and when | Why there is no edge |
|---|---|---|
| `rest_element` | `const { a, ...others } = api; use(others)`: **one for each place** the rest name is handed over, when the rest object holds a member that is a project function with a body. The members are read from the type of the rest name, so a member the pattern named is not in it | a rest element is not one member: `bound_member` follows a name to the member it names, and `others` names several |
| `assignment_destructuring` | `({ load } = api); use(load)`: one for each place the assigned variable is handed over, when the member the target takes is a project function (renamed targets and nested patterns too). Only a `=` with an object pattern on the left; the target inside the pattern is where the def is taken, not a hand-over | the variable's declaration is `let load`, with no initializer to read, and the assignment can be anywhere |
| `construct_initializer_sites` | one for each **site** in an instance field initializer (`x = make()`, a call or an accessor in it), for each construction (`new C()`, `<C />`, `super()`) of a project class, **and of every class the construction goes through without a constructor to enter** (`init_chain`: a derived class with no constructor runs its base's initializers). Not static initializers or static blocks (they run when the class is defined), not a parameter property's default (a site of the constructor, which is drawn) | the construction draws the constructor (`init`), and the initializers are the class's own body, which is walked only when the class is the entry |
| `local_class_decorator_sites` | one for each site that is in a decorator (the class's, a member's, a parameter's; `@factory(1)` is two) of a class **declared inside a walked body**; a class at the top of a file is not counted | the decorators run when the body does, and the body stops at a nested class (its own def) |
| `class_component_render` | one for each `<Cls />` where `Cls` is a project class with a `render` that is a project function (its own, inherited from a project class, or an arrow held by a field). A `render` declared by a package is nothing hidden: not counted; `new Cls()` is not an element | React calls `render`; nothing in the project does, and no framework rule is held |
| `getter_result_calls` | one for each plain call, `new`, tag or JSX tag whose callee is a **getter** (`b.fn()`): the one edge is to the getter (the engine's answer, as before step 6), and what the getter returns is called by a second call that has no edge | two calls on one span: a second edge on the same span would be merged with the first by the graph |

Counting rules, the same as step 5's: per occurrence in every **walked** body, once per walk, so a def reached from two places is counted once; a count is a lower bound for what the checker can name. `construct_initializer_sites` counts *sites*, not constructions, because a number of hidden edges is what the next step (an edge under the construct) would add; the others count the shape the owner named.

Pinned in `tests/fixtures/uncovered/` (`members.ts`, `classes.tsx`, `getters.ts`; the expected number of every row written before the first run, 48 rows in 5 tests of `tests/calls.rs`). The controls that read 0: a rest object that holds no def, or only what the pattern named; a rest object that is called and not handed over; an assignment target that is called, never used, not a def, or a `==` with an object on its left; an arrow held by a field (its own def: `WithArrowField.onClick`, checked by its own row), a static initializer, a parameter property, a class with no field sites; a decorated class at the top of the file; a function component, a class component whose `render` a package declares, `new Widget()`; a method, a plain read of a getter (an accessor site). **Mutation-checked**: every guard turned a row red (the rest guard and its `holds_defs`, the pattern self-exclusion, the assigned-variable test, the member-is-a-def test, the left-is-an-object-pattern test, the `=` test, nested patterns, the static filter, the field filter, the initializer range, the init chain, the decorator range and kind, the getter test, the JSX fact and the JSX guard). Five did not the first time. Four were guards no row told from "any site in the class": the `=` test (`({ load } == api)` has an object on its left too), the initializer range (a static initializer beside an instance field), and the decorator range and kind (a decorated local class with a static initializer); each got a row and is red now. The fifth, the check of `getter_result` in the fallback for an external callee, never fired (a getter of a class and of an anonymous type in a repo `.d.ts` both come from the hierarchy), so that line was removed. One guard is an **equivalent mutant**: `local_class_decorator_sites` only looks at nested defs that are classes, and `decorator_sites` of anything else is 0 anyway (a decorator is only in a class); the guard saves reading the body of every nested function.

**On the V-NOC frontend all six are 0** (`TaskDetailPanel`: 141 call, 2 construct, 235 external, 86 unresolved, 18 passed, 90 contexts, as before; the whole frontend, 1084 entries: 1921 / 3 / 2802 / 1057 / 153, as before), and the source says so, not only the counter:

| Kind | Source check |
|---|---|
| `rest_element` | 2 rest elements in a `const` pattern: `lib/api.ts` (`...customConfig` of `options`, request data) and `useMirrorStore.ts` (`...rest` of `state.docs`, a record of documents); 218 rest elements in parameter patterns (`{ className, ...props }: React.ComponentProps<"div">`, the UI kit). None of those types has a project function: the members are data or a package's types |
| `assignment_destructuring` | no object pattern on the left of a `=` in `src/` (searched for `({ … } = …)` and a bare `{ … } = …;`) |
| `construct_initializer_sites` | 5 classes. `ReplayRunner` has five fields, all initialized with a literal (`[]`, `0`, `"idle"`, `1`, `null`: no site); `ReadOnlyModeError` and `ApiError` have no field; the two services have none |
| `local_class_decorator_sites` | no decorator in `src/` (the step 6 count of decorators was 0 as well) |
| `class_component_render` | no class component: no `extends Component`; the one `render()` is a local function in `MermaidBlock.tsx` |
| `getter_result_calls` | no `get x()` or `set x()` in `src/` (classes or object literals); a getter of a package called as a function (`a.b()` with `b` a getter) would be counted, and there is none |

**Known and left**: the static initializers, static blocks and `extends` expression of a class declared inside a function are as hidden as its decorators, and are not counted (they are what the design note of part C asks about); `local_class_decorator_sites` counts decorators only. A rest element or an assignment target whose name is handed over in **another** body than the one that wrote the pattern (`let load; function init() { ({ load } = api); }` and `use(load)` elsewhere) is not counted: the assigned variable is looked up in the body of the assignment. An array pattern (`[a] = list`), an assignment target that is a property access (`({ load: this.load } = api)`) and a `for (… of …)` target are not followed. `getter_result_calls` counts a getter of a package like a project's; none exists on the V-NOC frontend, so whether that is noise is not measured.

### Three more counted (TS4 step 7c)

The review of step 7 asked for three more shapes that no edge covers, counted and not drawn (no tree change). Same line in the breakdown, same rules as step 7a (per occurrence in every walked body, once per walk; a lower bound):

| Kind | What is counted | Why there is no edge |
|---|---|---|
| `rest_element` (extended) | `({ load, ...o } = api); use(o)`: one for each place `o` is handed over, when the members of the value that the pattern did not name include a project function. Also a rest inside a `for … of` pattern (`for (const { ...o } of xs)`), which the existing branch already counted | the variable has no initializer to read, as for an assignment target (7a) |
| `for_of_destructuring` | `for (const { load } of xs) use(load)`: one for each place the name is handed over, when the member it stands for, looked up on the type of the pattern (the element type of `xs`), is a project function. Renames, nested patterns and `for await` too | a `for … of` pattern has no initializer, so `bound_member` follows nothing. **A call is not a hole:** `load()` resolves, the engine follows the binding to `api.load` (an edge) |
| `computed_member_name_sites` | one for each call, `new`, tag or JSX element in a computed member name (`[key()] = …`, `[make()]() {}`) of a class **at the top of a file**, counted where the class def is walked (as the entry). A class declared in a function has its names read by that function (an edge there); an accessor in a name is not asked | `body_of(class)` never reads the member names, and no def is around a top-level class to read them |

Pinned in `tests/fixtures/uncovered/` (`members.ts`, `classes.tsx`; 24 rows added to the 7a tests, the expected number written before the run) and in the policy rows `counter-rest` (two entries added), `counter-for-of`, `counter-computed-names`. **Mutation-checked.** The first round left seven survivors. Four guards were redundant with each other (the dedupe of the assigned variables, the `inside_pattern` test for a rest name, the `members.is_empty()` test before `for_of_member`, the initializer and list tests of `for_of_member`, which the `for … of` test covers) and were removed; two needed a row (a rest in a `for … of` pattern is the rest element's and is not counted twice; an array pattern named like a member is not followed). **One was a bug the V-NOC run found, not the fixtures:** the guard that asks a name for its expression only when it is computed was removed as "redundant" (no row survived without it), and the whole-frontend run panicked in the engine (`Unhandled case in Node.Expression: Identifier`) on the first class with an ordinary member name; the fixtures never walked a top-level class that had one. The row `ComputedAmongPlainNames` is the proof now, and the lesson stands: a mutant that survives may mean the fixtures do not reach the code, not that the code is redundant.

**On the V-NOC frontend all three are 0** (`TaskDetailPanel`: 141 call, 2 construct, 235 external, 86 unresolved, 18 passed, 90 contexts; the whole frontend, 1084 entries: 1921 / 3 / 2802 / 1057 / 153, no counter line in the breakdown), and the source says so: no assignment destructuring at all in `src/` (so none with a rest); 4 `for … of` with a destructuring pattern, all array patterns (`for (const [key, nested] of Object.entries(value))`), none an object pattern, which `for_of_destructuring` needs; the 5 classes have no computed member name.

**Known and left**: a `for … of` whose target is an assignment pattern (`for ({ load } of xs)`), an array pattern anywhere (`const [a] = list`), a `for … in`, and a `catch ({ load })` are not followed or counted; the computed names of a class counted only where the class def is walked, so a class reached only by a construction counts nothing (its definition runs once, elsewhere: D25).
