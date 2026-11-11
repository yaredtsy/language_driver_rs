# 03.01 What tsrs gives, and what needs our work

```
what tsrs gives
├── 1. what tsrs is
├── 2. the crates we use
├── 3. exported: use as it is
├── 4. needs our work
├── 5. four facts about tsrs that shape the design
└── 6. risks
```

Read from `../language_clone/tsrs` at commit `13a31a4` (workspace version 0.24.0; follows TypeScript `7.1.0-beta`).

## 1. What tsrs is

A Rust port of the TypeScript 7 compiler and language server (`oxc-project/tsrs`, Apache-2.0). It is a **mechanical port of the Go compiler**:
the same file names, function names and algorithms, with the Go line each function came from in a comment. 38 crates, about 600,000 lines.

It is built as an application (`tsrs`, `tsrs-lint`, a language server), not as a library. Nothing promises a stable API, and upstream does not
take pull requests. But almost everything is `pub`, because a port keeps the original's visibility, and one of its own programs, `tsrs_lint`, uses
the checker exactly the way we want to: load a program, get a checker, ask about nodes. **`crates/tsrs_lint/src/engine.rs` is the best example to
read before TS3.**

## 2. The crates we use

```
typescript-syntax    tsrs_parser ── tsrs_ast ── tsrs_scanner ── tsrs_core (+ tsrs_diagnostics, tsrs_spanmap)
                     about 140,000 lines; no checker

typescript-analysis  + tsrs_tsoptions   read tsconfig.json
                     + tsrs_vfs         the file system, and the bundled lib.*.d.ts files
                     + tsrs_compiler    Program: the set of files, module resolution
                     + tsrs_checker     types, symbols, resolved signatures      (110,000 lines in one crate)
                     + tsrs_ls          only for one function: resolve_call_hierarchy_declaration
                     (TS5 only) tsrs_project   sessions, unsaved-file overlays, snapshots
```

## 3. Exported: use as it is

Every item below is `pub` at the pinned commit. The left column is the question we ask; `sem.rs` wraps each one.

### For `typescript-syntax`

| We need | tsrs gives | Notes |
|---|---|---|
| parse one file | `tsrs_parser::parse_source_file(opts, text, script_kind) -> P<SourceFile>` | error-tolerant: always returns a tree |
| which dialect | `tsrs_core::get_script_kind_from_file_name` → `ScriptKind::{JS, JSX, TS, TSX, JSON}` | |
| syntax errors | `SourceFile::diagnostics()` | ranges + messages |
| walk the tree | `Node::for_each_child`, `Node::kind()`, `ast::is_function_declaration(node)` and one `is_*` per kind, `node.name()`, `node.parameters()`, `node.members()`, `node.parent()` | generated accessors; `docs/AST.md` is the contract |
| a node's range | `node.pos()` / `node.end()`, in **UTF-8 byte offsets**; `tsrs_scanner::skip_trivia(text, pos)` to step over leading comments | `pos` includes leading trivia, as in TypeScript |
| comments above a node | `tsrs_scanner::get_leading_comment_ranges(text, pos)`; JSDoc: `node.jsdoc(file)` | |
| shebang | `tsrs_scanner::get_shebang(text)` | for the file header slot |
| line and UTF-16 column | `tsrs_scanner::get_ecma_line_and_utf16_character_of_position` | we use `analyzer-core`'s `LineIndex` instead, so the driver has one rule for all languages |

### For `typescript-analysis`

| We need | tsrs gives | Notes |
|---|---|---|
| find and read `tsconfig.json` | `tsrs_tsoptions::get_parsed_command_line_of_config_file`; `tsrs_lint::engine::find_tsconfig` shows the upward search | |
| a project with no tsconfig | `tsrs_tsoptions::new_parsed_command_line` with our own options (`allowJs`) | the "inferred project" |
| the standard library types | `tsrs_vfs::bundled::default_lib_path()` | **bundled in the binary**: the user needs no `typescript` package installed |
| load the program | `tsrs_compiler::new_compiler_host(cwd, fs, lib_path, …)` → `ProgramOptions::new(config, host)` → `new_program(options) -> &'static Program` | module resolution, `node_modules`, project references included |
| the files | `Program::get_source_files()`, `Program::get_source_file(name)` | |
| a checker | `Program::get_type_checker(&ctx) -> CheckerHandle` | |
| **what a call refers to** | `tsrs_ls::callhierarchy::resolve_call_hierarchy_declaration(program, node) -> Option<CallHierarchyDeclarations>` | **the default resolver.** Returns the declaration node(s) "Show Call Hierarchy" would show |
| the same, by hand | `Checker::get_resolved_signature_exported(call)` → `Signature::declaration()`; `Checker::get_symbol_at_location_exported(expr)`; `Checker::skip_alias` / `get_aliased_symbol` | the fallback if we need to differ from the call hierarchy (for example: pick the overload that matched) |
| free the memory | `tsrs_compiler::free_program`; `tsrs_core::arena::Region` | §5 |
| cancel | `Checker::was_canceled()`; a `tsrs_core::context::Context` is passed to every entry point | [check] how a context is cancelled from outside |
| later: edits and unsaved files | `tsrs_project::Session`: `did_open_file`, `did_change_file`, `snapshot()`, `get_language_service` | the language server's own project layer |

## 4. Needs our work

| # | Work | Why tsrs does not do it for us | Phase |
|---|---|---|---|
| 1 | **the outline rule**: which nodes are defs, and each one's qualname | tsrs has a document-symbols feature (`tsrs_ls/src/symbols.rs`), but it is shaped for the editor.s outline view (LSP types, display names). We need qualified names under a rule we freeze ourselves | TS1 |
| 2 | **reading and placing a marker comment** | product-shaped; no compiler has it | TS2 |
| 3 | **the body walk**: the call sites of one function, in source order, with one resolver call each | tsrs's `collect_call_sites` does this but is private, and its public wrapper returns LSP types, grouped by callee and sorted by file. We need per-site, in order. It is about 150 lines to write ourselves, following the same list of node kinds | TS4 |
| 4 | **declaration node → our identity** (`file`, qualname, nth → `StableDef`) | the bridge between the checker's world and the outline's. Must agree with #1 exactly | TS4 |
| 5 | **project code or external** | a path test on the declaration's file (`node_modules`, `.d.ts`, the bundled libs) | TS4 |
| 6 | **the memory life cycle** in a long-running process | tsrs is built for run-once commands; see §5 | TS0, TS3 |
| 7 | **one program per tsconfig**, and which one owns a file | `tsrs_lint` shows how; a monorepo with ten tsconfigs needs a rule | TS3 |
| 8 | **budget, expand, cancel, cache** | driver-shaped | TS5 |
| 9 | **the facade** (`sem.rs`) and its contract tests | our protection against upstream change | every phase |

Nothing in this list is a parser or a type checker. The largest item is #3 + #4, and it is a tree walk.

## 5. Four facts about tsrs that shape the design

### a) Everything lives in arenas, and pointers are `'static`

```rust
pub fn parse_source_file(opts: SourceFileParseOptions, source_text: &str, script_kind: ScriptKind) -> P<SourceFile>
pub fn new_program(opts: ProgramOptions) -> &'static Program
```

`P<T>` is tsrs's arena pointer: `Copy`, compared by address, valid "forever". Nodes are never freed one by one. By default they are **never freed
at all**: the docs say a parsed file's text "like the thread arena, is never freed". That is fine for `tsrs --noEmit`, which exits. A driver that
answers `outline` ten thousand times would grow without limit.

The answer exists in tsrs: a **region**.

```rust
let region = tsrs_core::arena::Region::new_scratch(64 * 1024);
{
    let _scope = region.enter();                    // allocations on this thread now go to the region
    let file = tsrs_parser::parse_source_file(opts, text, kind);
    let outline = walk(file);                       // build OUR owned data (Strings, Vecs)…
    outline                                         // …and let nothing from tsrs escape the scope
}
// when the last handle to `region` is dropped, all of its memory is returned at once
```

> **🦀 Rust note: `'static` here does not mean "safe forever".** In ordinary Rust, `&'static T` is a promise the compiler checks: the value
> really lives for the whole program. tsrs uses `'static` as a convention inside its own arenas, with `unsafe` underneath, because the Go original
> has a garbage collector and the port needed pointers that can go anywhere. The compiler therefore **cannot** stop you from keeping a `P<Node>`
> after its region was freed; using it then is undefined behaviour. This is the single most important reason for rule 7: `sem.rs` is the only
> module that ever holds a `P<…>`, it converts everything to owned `analyzer-core` data before returning, and its handle types (`Func`) are only
> valid while the `Project` that made them is alive. TS0 checks this with a spike; TS3 designs `Project` around it.

### b) Positions are UTF-8 bytes

Good: the same as ruff and rust-analyzer, so `analyzer_core::text::Span` is a direct copy. TypeScript itself (the JavaScript version) counts in
UTF-16; tsrs converts at its language-server edge, and so do we, in the driver.

### c) The checker is parallel and pooled; we use it from one thread

`Program::get_type_checker` hands out a checker from a pool. Our walk is single-threaded and deterministic (rule 11): one checker, asked one
question at a time, in source order.

### d) It needs Rust 1.99 and wants mimalloc

`rust-version = "1.99"` sets the repo's toolchain ([`01-monorepo/02`](../01-monorepo/02-workspaces-and-toolchain.md) §3). tsrs's binaries call
`tsrs_core::allocator::init_mimalloc`; its notes measure large wins from it. A library must not choose the global allocator, so that choice is
the **driver's** (and `typescript-cli`'s): `ENGINE.md` says "binaries that link `typescript-analysis` should install mimalloc as tsrs's own do".
**[check]** in TS0 whether anything breaks without it.

## 6. Risks

| Risk | How likely | What limits the damage |
|---|---|---|
| an upstream change breaks an item we call | certain, eventually | the `rev` pin; `sem.rs`; `ENGINE.md`'s list; contract tests |
| upstream stops, or changes licence | low | Apache-2.0 at our pin is ours to keep; a fork is always possible |
| a `P<…>` escapes a region | medium while learning | fact (a): the rule, the spike, and one test that parses in a loop and watches memory |
| the checker is slow or large on a big project in a long-lived process | unknown | TS0 measures it; TS5 moves to `tsrs_project::Session`, which is what the language server uses for exactly this |
| build time and binary size of the driver | certain | the fast/slow crate split: tier 1–2 never links the checker. V-NOC already plans one small extension per language (rows R14–R16), so the size is paid only by TypeScript users |
| plain JavaScript resolves poorly (everything is `any`) | certain | reported honestly as `Unresolved` (rule 6). It is the strongest argument for context sensitivity later, and for the "local values only" step ([`02-tree-rules/03`](../02-tree-rules/03-the-seam.md) §7) |
| tsrs does not match TypeScript in some case | low for resolution (it tracks conformance closely) | we show what the engine says; `ENGINE.md` names the TypeScript version |

Next: [`02-ts0-spikes.md`](02-ts0-spikes.md)
