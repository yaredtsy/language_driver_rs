# Engine: rust

The RS0 spikes below were run on 2026-11-16 (macOS, Darwin 23.6.0, Rust 1.99.1) and their code deleted when RS0 closed; it is in git history at commit `31864ad`
(`languages/rust/spikes/`: `s1-*` S-RS1/S-RS2 loaders, `s2-policy`, `s2-oracle`, `s2-cancel`, `s4-docs`, `fixtures/policy`).

- Upstream URL: https://github.com/rust-lang/rust-analyzer (read from `../language_clone/rust-analyzer` at `2a362a4`; **built from crates.io**, not git)
- Pinned commit / version: `ra_ap_* =0.0.358` for **every** `ra_ap_*` crate we name (published 2026-10-12), written once in `languages/rust/Cargo.toml` (RS1 step 1). See S-RS1 for why one number and why not newer.
- Engine crates used: `rust-syntax`: `ra_ap_syntax` (its `ra_ap_parser`, `rowan` come with it). `rust-analysis`: + `ra_ap_hir`, `ra_ap_ide_db`,
  `ra_ap_load-cargo`, `ra_ap_project_model`, `ra_ap_vfs`. Dev only: `ra_ap_ide` (the oracle). Final list confirmed when RS3 and RS4 are built.
- Patches: none
- How to bump: an engine bump is a change of its own (only the pins, the lockfile, the `sem` module and this file).
  1. Look for a newer set: `cargo info ra_ap_hir`, `ra_ap_ide`, `ra_ap_ide_db`, `ra_ap_load-cargo`, `ra_ap_project_model` (these five were the ones that stopped at
     0.0.358). Move **every** `ra_ap_*` line of `[workspace.dependencies]` in `languages/rust/Cargo.toml` to one `=` version that all of them have.
  2. `cargo update`, then check the pair that broke a fresh lock (S-RS1): `grep -A1 'name = "unicode-' Cargo.lock`. `ra_ap_hir_def` pins `unicode-ident` exactly; if
     `unicode-properties` is a different Unicode version, `ra-ap-rustc_lexer` does not compile: `cargo update -p unicode-properties --precise <the matching one>`. Commit the lockfile.
  3. `cargo test`, then `cargo test -p rust-syntax` alone, and `scripts/check-all.sh --static` (its `cargo tree` line: `rust-syntax` has no `salsa`, `ra_ap_hir` or `ra_ap_ide`).
  4. Re-check every item this file names (the S-RS2 policy table, the `attach_db` rule, `Sysroot::discover`'s `rustup` call, the `set_test` default, the doc-comment node shapes
     of S-RS4) against the new source; fix `sem` only.
  5. Write the new version and the date here.
- Licence notice: rust-analyzer is MIT OR Apache-2.0. (Notice text to be copied from the published crates when RS3 ships a binary.)

## S-RS1: does it build as a dependency?

### Which version (2026-11-16)

There is **no published `ra_ap_*` set at the clone's commit `2a362a4`**, and no complete set after 0.0.358. `cargo info` on 2026-11-16:

| Crate | Latest published |
|---|---|
| `ra_ap_hir`, `ra_ap_ide`, `ra_ap_ide_db`, `ra_ap_load-cargo`, `ra_ap_project_model` | **0.0.358** (2026-10-12) |
| `ra_ap_syntax`, `ra_ap_parser`, `ra_ap_vfs` | 0.0.363 |

The publishing job stopped for the `hir`-family crates after 0.0.358 (guess: the new `hir-ide` crate is not published; not verified). Pin
**`=0.0.358` for every `ra_ap_*` crate**, `ra_ap_syntax` included: `rust-syntax` and `rust-analysis` share one lockfile, and mixing 0.0.363 with
0.0.358 would put two `rowan`-compatible but different `ra_ap_syntax` into one tree. Read the engine source from the registry copy
(`~/.cargo/registry/src/*/ra_ap_*-0.0.358/`), not from the clone: they differ (below, and S-RS4).

`rust-version` of every crate is 1.98, edition 2024; the toolchain 1.99.1 builds them with no flag and no patch.

### The lockfile trap: `unicode-properties` must be 0.1.4

A cold resolve **fails to compile** (`ra-ap-rustc_lexer`, `E0080`: "unicode-properties and unicode-ident must use the same Unicode version").
`ra_ap_hir_def 0.0.358` pins `unicode-ident = "=1.0.24"` (Unicode 17); the newest `unicode-properties` is 0.1.5 (Unicode 18); the lexer
checks that the two agree at compile time. Cargo picks 0.1.5, so a fresh `Cargo.lock` is broken. The fix is one line in the lockfile:
`cargo update -p unicode-properties --precise 0.1.4` (Unicode 17). Therefore **`languages/rust/Cargo.lock` is committed and a bump re-checks this pair.**
A crate with only `ra_ap_syntax` has no `hir_def` pin in its tree, so alone it would resolve `unicode-ident` 1.0.27 and 0.1.5 (Unicode 18, which agree);
inside our workspace the lock decides.

### Build cost, measured

`cargo build --release`, `debug = 0`, `CARGO_INCREMENTAL=0`, warm registry, empty target directory for the first two rows:

| Crate set | Cold build | Binary: names the API only | Binary: really loads a workspace and resolves calls |
|---|---|---|---|
| `ra_ap_syntax` (parse a string) | **9.5 s** | 1.0 MB | 1.0 MB |
| + `hir`, `ide_db`, `load-cargo`, `project_model`, `vfs` | **2 m 05 s** | 17.5 MB | **21.95 MB** |
| + `ra_ap_ide` (`AnalysisHost`, `outgoing_calls`) | +2 m 39 s (so ≈ 4 m 45 s cold) | 17.5 MB | **22.45 MB** |

- "names the API only" is a trap: the linker drops what is not called, so adding `ide` looked free. The right column is a program that loads a
  Cargo workspace, parses every file, resolves every call, and (for `ide`) calls `outgoing_calls`. **`ide` costs about 0.5 MB in the binary and
  2.5 minutes of cold release build.** Keeping it as a dev-dependency (the oracle) is worth it for the build time, not the size.
- `cargo tree`: the syntax-only crate has **no `salsa`** (0 matches). The `hir` set has `salsa 0.28.5`. Rule 4's "fast crate must not compile the
  engine's analysis" is therefore real: `cargo test -p rust-syntax` compiles 9.5 s of engine, not 2 minutes.
- Target directory after all three builds: about 0.9 GB (`debug = 0`). Without `debug = 0` expect several times that: the owner has ~12 GB free.
- 120 `ra_ap_*` lines in `cargo tree` for the `hir` set, 154 with `ide`.
- Not checked: Linux and Windows. **Open:** needs CI (the repo has `.github/workflows/ci.yml`, RS1 adds the job).

### The edition type

`ra_ap_syntax::Edition` (a re-export of `ra_ap_parser::Edition`, which is `ra_ap_edition::Edition`). Variants `Edition2015`, `2018`, `2021`, `2024`;
`Edition::CURRENT` and `LATEST` are 2024, **`Edition::DEFAULT` is 2015** (do not use it for a file whose edition is unknown: `rustc` defaults to
2015 only without `--edition`; Cargo always passes one). It implements `FromStr` (`"2024"`). `SourceFile::parse(text, edition) -> Parse<SourceFile>`.

## S-RS4: can we find and edit the doc comment of every kind of def?

Answer: **yes**, with five corrections to the plan. Run on `ra_ap_syntax =0.0.358`, `Edition::Edition2024`, with `languages/rust/spikes/s4-docs`
(about 45 cases; the parse of each had no errors).

**What the tree looks like (this differs from the plan).**

- A doc line is **a node**, not a token: `DOC_COMMENT` wrapping one token `OUTER_DOC_COMMENT` (`///`, `/** */`) or `INNER_DOC_COMMENT` (`//!`, `/*! */`).
  An attribute is the node `ATTR`. Both are direct children of the item's node, **interleaved in source order**
  (`/// a` · `#[inline]` · `/// b` · `#[must_use]` gives `DOC_COMMENT ATTR DOC_COMMENT ATTR`).
- **`HasDocComments` does not exist in 0.0.358** (the plan named it). Doc lines come from `ast::HasAttrs`: `doc_comments() -> AstChildren<ast::DocComment>`
  (outer **and** inner, in order), `attrs() -> AstChildren<ast::Attr>`, `attrs_with_doc() -> AstChildren<ast::AnyAttr>` (both kinds, source order: this
  is the one RS2 needs for "after the last doc comment, wherever it stands"). `ast::DocComment` has `.kind()` (`AttrKind::Outer | Inner`), `.token()`,
  `.text_with_markers()`, `.text()` (markers stripped).
- `//!` inside a body is a `DOC_COMMENT` child of the body's `STMT_LIST`, **not** of the `FN` node. For a `mod m { //! … }` it is under the module's `ITEM_LIST`.
  So the item's own `doc_comments()` never contains an inner doc line.

**The kinds of def.** For every kind below, a `///` above the item is a child of the item's own node, first in the node, and `doc_comments()` returns it:
`fn`, a method in an inherent `impl`, a method in a trait `impl`, a trait method with and without a body, `struct`, `enum`, `union`, `trait`,
inline `mod`, `type`, `const`, `static`, `macro_rules!` (also with `#[macro_export]` between the doc and the macro: the order is kept), a `fn` in a `fn`,
a foreign `fn` in an `unsafe extern "C" { }` block, and the `impl` block itself. An `impl` has no name (`name()` is `None`), as the plan said.
(Enum variants and record fields were not asked of RS1's table; the parser also attaches comments to them, `n_attached_trivias` names `VARIANT`,
`RECORD_FIELD`, `TUPLE_FIELD`.)

**Blank lines (the plan's "unknown").** Two mechanisms, and the first reading of this section mixed them up (corrected 2026-11-16, after a review
case that the first reading could not explain):

1. **Doc comments are not trivia in 0.0.358.** `SyntaxKind::is_trivia` is only `WHITESPACE | COMMENT`. `OUTER_DOC_COMMENT` and `INNER_DOC_COMMENT` are real
   tokens, and the grammar eats them like `#[…]`: `outer_attrs` in `ra_ap_parser/src/grammar/attributes.rs` wraps each in a `DOC_COMMENT` node as a child of
   the item. So **whatever stands between two doc lines (a blank line, a plain `//`) is trivia *inside* the item's node, and every outer doc line above an
   item is a child of it.** Nothing about blank lines is ever asked of a doc comment.
2. **Only plain `//` and `/* */` comments** go through `n_attached_trivias` (`ra_ap_parser/src/shortcuts.rs`): they are leading trivia of the item, and
   they are pulled into its node only while no blank line (`"\n\n"`) stands between them and the item. That function also has a branch for "the next comment
   up is an outer doc comment", but it matches the lexer's `COMMENT` kind, so for a doc line it never fires (a dead branch in this version).

The cases (all run on the real parser):

| Source above an item | In the item's node? |
|---|---|
| `/// a`, blank line, item | **yes**: a blank line does not detach an outer doc comment (as in `rustc`) |
| `/// a`, blank line, `/// b`, item | both, and the blank line between them |
| `/// a`, two blank lines, item | yes |
| `/// a`, blank line, `#[inline]`, item | yes |
| `// plain`, item (no blank) | **yes** (so `range` must skip it by hand) |
| `// plain`, blank line, item | **no** (LF files) |
| `// plain`, `/// d`, item | both |
| `/// d`, `// plain`, item | both; `doc_comments()` is just `/// d` |
| `/// d`, blank, `// plain`, item | all three |
| `// plain`, blank, `/// d`, item | only `/// d` and after: the plain comment is outside |
| `// header`, blank, `/// d`, item (file start) | only `/// d` |
| `//! crate doc`, blank, `/// d`, item | only `/// d`: an inner doc comment breaks attachment |
| `/// a`, `// plain`, blank, `/// b`, item (the review case) | **everything**: `/// a` is a `DOC_COMMENT` child of `f`, the plain comment a `COMMENT` child, the blank line the whitespace after it, then `/// b`. Nothing is left in the parent, so there is no stray token. `doc_comments()` is `["/// a", "/// b"]`, as `rustc` counts them. CRLF: the same |
| `/// a`, `// plain`, blank, item | `/// a`, the plain comment and the blank line are all in the node; `doc_comments()` is `["/// a"]` |
| `/// a`, `// plain`, blank, `#[inline]`, item | all in the node |
| `fn f() {} // c` then `/// d` `fn g() {}` | **`g`'s node starts at `// c`**, the trailing comment of the previous item's line |

So **an outer `///` or `/** */` line above an item is always a child of that item's node, however it is spaced or what stands between it and the item.**
A plain `//` before the *first* doc line or attribute is the item's only when it is directly above the item, or when it ends the run of comments that reaches the item
without a blank line. One caveat: a doc line the grammar cannot attach to an item (one before a `}`, say) is a parse error, which `SyntaxError` already covers. The consequence for RS1's `range` ("from the first doc comment or attribute"): do not use `text_range().start()`.
Use the first `DOC_COMMENT` or `ATTR` child, else the first child that is not a `COMMENT` or `WHITESPACE`. The trailing-comment case is the one the
plan missed: without the rule a def's range would begin on the previous def's last line.

**CRLF is a different tree.** A `\r\n` file keeps the `\r` **inside** the comment token (`"/// a\r"`; `.text()` ends in `\r`), and the whitespace
between two lines is `"\n\r\n"`, which does not contain `"\n\n"`. Therefore in a CRLF file **a plain `//` comment above a blank line stays attached**
(in an LF file it is detached). Doc lines are unaffected (they attach either way). RS2: strip a trailing `\r` when reading a doc line, copy the
file's line ending when writing (already in the plan).

**The file header (RS2).** The header is a plain `// <label>: <text>` comment "before the first item". Because of the rules above, **it can be a child of the first
item's node**, so RS2 must compare against RS1's range start (the first `DOC_COMMENT` or `ATTR` child, else the first child that is not `COMMENT` or `WHITESPACE`),
**never the item node's start**. Cases, with the node start and the range start (byte offsets):

| File | Where the header comment is | node start | range start |
|---|---|---|---|
| `// ID: x` directly above `fn f() {}` (LF) | a `COMMENT` child of `FN` | 0 | 9 |
| `// ID: x`, blank, `fn f() {}` (LF) | a `COMMENT` child of `SOURCE_FILE`, before the `FN` | 10 | 10 |
| `// ID: x`, blank, `fn f() {}` (**CRLF**) | a `COMMENT` child of **`FN`** (the CRLF rule above) | **0** | 12 |
| `// ID: x` directly above `fn f() {}` (CRLF) | child of `FN` | 0 | 10 |
| `// ID: x`, blank, `/// d`, `fn f() {}` (CRLF) | child of `FN`, before the `DOC_COMMENT` | 0 | 12 (the doc line) |
| `// ID: x`, blank, `#[inline]`, `fn f() {}` (CRLF) | child of `FN`, before the `ATTR` | 0 | 12 (the attribute) |
| `#!/usr/bin/env rust-script`, `// ID: x`, blank, `fn f() {}` | `SHEBANG` then a `COMMENT`, both children of `SOURCE_FILE` | 37 | 37 |
| `// ID: x`, blank, `//! crate`, blank, `fn f() {}` | child of `SOURCE_FILE`; the inner doc line is a `DOC_COMMENT` child of `SOURCE_FILE` too | 21 | 21 |

So in a CRLF file with a blank line between header and item, `node start` is the start of the file while the header is "before the first item": the rule to
write is "the header is the comment run that ends before the first item's **range start**, found by scanning tokens from the start of the file, whichever node
holds them", not "a child of `SOURCE_FILE`".

**The other rows of the table.**

| Source | Result |
|---|---|
| `/** d */` and a multi-line `/**\n * d\n * ID: x\n */` | one `DOC_COMMENT` each; `.text_with_markers()` is the whole block |
| `#[doc = "d"]` | an `ATTR`, **not** a `DOC_COMMENT`: `doc_comments()` is empty, `attrs()` has it. It is not a place for the marker |
| `////` (four slashes), `/*** */` | a plain `COMMENT`, as the plan said |
| `//!` in a body | see above |

**`rustfmt`** (`rustfmt 1.10.0-stable` of 1.99.1), a file with a `/// ID: …` line, a `/** … ID: … */` block and `#[doc]` attributes:
default config: **file unchanged**. With `wrap_comments = true` and `normalize_doc_attributes = true` (both took effect on this stable rustfmt, no
`unstable_features` needed): a 140-column summary line is re-wrapped, `#[doc = "from attribute"]` became `///from attribute`, and **every `ID:` line, the
`ID:` line inside the `/** */` block, and the odd ones (`///ID: …`, `///   ID: …`) were byte-identical.** `rustfmt` does not touch a short `/// ID: <id>` line.
Caveat for RS2's test: `normalize_doc_attributes` makes `#[doc]` into a doc *line*, so a marker placed after such a line goes after it, harmlessly.

## S-RS3: what does the user's machine need? (partly run, 2026-11-16)

Run on `core/` (and on scratch packages) with `load_workspace_at`, build scripts off, proc-macro server `None`, `RustLibSource::Discover`.

| Case | What happens | Where the text is |
|---|---|---|
| **no `rust-src`** | the load **succeeds**. No error, no progress message. `Sysroot::error()` says `can't load standard library from sysroot\n<sysroot>\n(discovered via `rustc --print sysroot`)\ntry installing `rust-src` the same way you installed `rustc`\n`; `is_rust_lib_src_empty()` is `true`; `num_packages()` is 0. Calls then resolve **silently half**: on `core/` 634 of 978 calls and **74 of 914 method calls** (with `std`: 978/978 and 914/914) | `ProjectWorkspace.sysroot`, which `load_workspace_at` does not return: RS3 must load through `ProjectWorkspace::load` and `load_workspace` (see below) |
| **no `cargo`** (`HOME` empty, minimal `PATH`) | `Err`: `Failed to load the project at <manifest>: Failed to read Cargo metadata from Cargo.toml file <manifest>, None: Failed to run `cd "<dir>" && RUSTUP_AUTO_INSTALL="0" "cargo" "metadata" "--format-version" "1" "--manifest-path" "<manifest>"`: No such file or directory (os error 2)`. **With only a minimal `PATH` it still works**: the engine falls back to `~/.cargo/bin/cargo` | the returned `Err` |
| **`rust-toolchain.toml` names a toolchain that is not installed** | `Err`: `… `cargo metadata` exited with an error: error: toolchain '1.50.0-aarch64-apple-darwin' is not installed\nhelp: run `rustup toolchain install` to install it\n`. The engine sets `RUSTUP_AUTO_INSTALL=0` itself, so it never downloads a toolchain | the returned `Err` |
| **offline, a dependency not downloaded** | the load **succeeds**. `cargo metadata` fails, the engine retries with `--no-deps`, and calls into the dependency are unresolved (0 of 1 calls, 0 of 2 method calls on a one-line fixture). The cause is in `ProjectWorkspace.kind`: `Cargo { error: Some(..) }`, text `` `cargo metadata` exited with an error: error: no matching package named `itoa` found\nlocation searched: crates.io index\nrequired by package `withdep v0.1.0 (<dir>)`\nAs a reminder, you're using offline mode (--offline) … `` | `ProjectWorkspaceKind::Cargo { error }` (not returned by `load_workspace_at`) |

Two of the four are a **quiet half answer** (rule 8): a missing `rust-src` and an unreachable dependency. Both are visible only on the
`ProjectWorkspace`. RS3's rule: load through `ProjectWorkspace::load`, read `sysroot.error()`, `sysroot.is_rust_lib_src_empty()` and `kind.Cargo.error`,
turn them into `OpenError` variants (or a loud counter in `Stats`) and only then call `load_workspace`.

**`Sysroot::discover` installs `rust-src` by itself.** `RustLibSource::Discover` (with no `sysroot_src`) goes through
`discover_rust_lib_src_dir_or_add_component`, which runs **`rustup component add rust-src`** when the source is missing. My first load on this machine did
exactly that (5.4 s instead of 0.7 s, and `rustlib/src` appeared). A library that changes the user's toolchain breaks "text in, data out". RS3 must not call
`Sysroot::discover`; it finds the sysroot with `rustc --print sysroot`, checks `lib/rustlib/src/rust/library` itself, and passes `sysroot_src` so that
`discover_with_src_override` is used (no `rustup`).
(To reproduce the clean no-`rust-src` case I removed the component the engine had added, ran the cases above with `RUSTUP_DIST_SERVER` pointing at a
closed port, and added it back for the rest of RS0.)

**Which toolchain's `std`?** The sysroot is whatever `rustc --print sysroot` prints **in the project's folder**. In this repo `rust-toolchain.toml` selects 1.99.1;
a fixture outside the repo gets the default `stable`. Scenarios live inside the repo, so they get 1.99.1; a user's project gets its own.

## S-RS2: load a real workspace and resolve its calls (run 2026-11-16)

The loader is `languages/rust/spikes/s1-hir`; the policy table is `s2-policy` over `spikes/fixtures/policy` (one function per row); the oracle is `s2-oracle`;
cancellation is `s2-cancel`. All on `=0.0.358`, release build, `LoadCargoConfig { load_out_dirs_from_check: false, with_proc_macro_server: None,
prefill_caches: false, num_worker_threads: 1, proc_macro_processes: 0 }`, `RustLibSource::Discover` with `rust-src` installed (see S-RS3 for why RS3 does not use
`Discover`), `set_test: true`.

### Rules the engine imposes (each is a panic or a silent miss if forgotten)

- **`hir::attach_db` is required.** `Semantics::resolve_method_call_fallback` outside `hir::attach_db(&db, || …)` **panics**: `Tried to use attached db, but
  no db is attached` (`ra_ap_hir_ty-0.0.358/src/next_solver/interner.rs:2491`). `ide::Analysis` does it itself (`attach_db_allow_change` inside `Cancelled::catch`).
  `sem.rs` has one entry that attaches, and every walk happens inside it. A `Semantics` created and used outside it is a bug.
- **`set_test: true` is needed.** `CargoConfig::set_test` ("Gate `#[test]` behind `#[cfg(test)]`") defaults to `false`: `cfg(test)` is off, the
  `#[cfg(test)]` modules are inactive, and **not one call inside them resolves**. On `core/`: 151 of 953 calls and 490 of 863 method calls without it,
  **978/978 and 914/914 with it**. Calls in code that a `cfg` switches off are always `Unresolved`; RS4 counts them.
- **A call inside a macro's arguments is not in the file's tree.** `Semantics::parse` gives the written file; `println!("{}", f())` holds `f()` only as tokens.
  The call is found by `sema.expand_macro_call(&MacroCall) -> Option<InFile<SyntaxNode>>` and walking the expansion (recursively: a nested macro call is a
  `MacroCall` node in the expansion). `Analysis::outgoing_calls` does **not** do this (below), so the plan's "built into `outgoing_calls`" was wrong.

### The policy table, observed (first draft of RS4's table)

`R` = `resolve_path_with_subst` / `resolve_method_call_fallback`, `C` = `type_of_expr(callee).original.as_callable(db).kind()`. "container" is
`Function::as_assoc_item(db)?.container(db)`. "subst" is whether the second element of the pair is `Some`.

| Shape | What came back | The plan assumed | Verdict |
|---|---|---|---|
| `json.write()`, `json: JsonWriter`, method from `impl Writer for JsonWriter` | the **impl's** method, subst `Some` | the impl's method + subst | as planned |
| `w.write()`, `w: &W` or `w: W`, `W: Writer` | the **trait's** declaration, subst `Some` | the trait's | as planned |
| `w.write()`, `w: &dyn Writer`; `w: Box<dyn Writer>` | the trait's declaration | the trait's | as planned |
| `w.write()`, `w: impl Writer` (argument) | the trait's declaration | the trait's | as planned |
| `make().write()`, `fn make() -> impl Writer` | the **impl's** method (`impl Writer for JsonWriter`), not the trait's: the engine sees through a return-position `impl Trait` | the trait's | **differs**: an opaque return type is resolved to its hidden type |
| `self.step()` in a trait's default method | the trait's declaration of `step`, subst `Some` | the trait's | as planned |
| `Foo::new()` (inherent) | `R`: the function, container "inherent impl", subst `Some`; `C`: `Function` | the function | as planned |
| `W::make()`, `W: Make` | `R`: the **trait's** declaration; `C`: the same | the trait's | as planned |
| `<JsonWriter as Make>::make()` | `R`: the **impl's** `make`; `C`: the **trait's** `make` | (not stated) | **`R` and `C` disagree**: `R` is the more specific |
| `<Bar as Default>::default()` and `Default::default()` with `let b: Bar = …` (hand-written `impl Default for Bar`) | `R`: the impl's `default`, subst `Some`; `C`: the trait's `Default::default` | "unknown: write down what comes back" | **answer**: with the type known or inferred, `R` gives the impl's method; `C` gives the trait's. The inferred case is resolved |
| the same on `#[derive(Default)] struct Foo` | `R` and `C`: a `Function` whose container is an `impl Default for _` but whose `HasSource` is **the trait's declaration in `core`** (not a macro file), subst `None` | (not stated) | **a derived impl has no source of its own**: `source()` points at the trait item. A call to a derived method is External or "derived"; do not identify the callee by `source()` |
| `Pair(1, 2)` | `R`: `Adt Pair`; `C`: `TupleStruct(Pair)` | `TupleStruct` | as planned |
| `E::V(1)` | `R`: `Variant V`; `C`: `TupleEnumVariant(V)` | `TupleEnumVariant` | as planned |
| `Self(1, 2)` in `impl Pair` | `R`: **`SelfType`** (not the struct); `C`: `TupleStruct(Pair)` | the struct | **differs** for `R`; use `C` |
| `Rec { a: 1 }`, `E::R { a: 1 }`, `Self { a: 1 }` | `resolve_path`: `Adt Rec`, `Variant R`, **`SelfType`**; **`resolve_variant(RecordExpr)`**: `Variant Rec`, `Variant R`, `Variant Rec` (`hir::Variant` covers a struct too) | `resolve_path` on the path | **use `resolve_variant`**: it also answers `Self { .. }` |
| `f(1)`, `let f = \|x\| x` | `R`: `Local`; `C`: `Closure` | `Closure` | as planned |
| `(h.cb)(1)`, `cb: fn(u8) -> u8` field | no path (a `ParenExpr`); `C`: `FnPtr` | `FnPtr` | as planned |
| `f()`, `f: fn()` | `R`: `Local`; `C`: `FnPtr` | `FnPtr` | as planned |
| `f()`, `f: F`, `F: Fn()` | `C`: `FnImpl(Fn)` | `FnImpl` | as planned |
| `f(1)`, `f: &dyn Fn(u8) -> u8` | `C`: `FnImpl(Fn)` | (not stated) | `FnImpl`, not a different kind |
| `xs.into_iter().map(parse)`, `spawn(worker)` | `resolve_path` on the path expression: `Function` (free) | `Def(Function)` | as planned |
| `.map(Self::helper)`, `.map(Foo::helper)` | `resolve_path`: `Function`, container "inherent impl" | `Def(Function)` | as planned |
| `.map(Wrapper)` (tuple struct as a function) | `resolve_path`: `Adt Wrapper` | the struct | as planned |
| `println!("{}", f())` | `MacroCall` expands (56 bytes). In the expansion: `$crate::io::_print(…)` (the macro's own call, **`original_range_opt` = `None`**), `format_args_nl!` (expands again), and `f()` with `original_range_opt` = exactly `f()` | the call maps to the `f()` written in the file | as planned |
| `vec![g()]` | the expansion holds three calls of `vec!`'s own (`box_assume_init_into_vec_unsafe`, `write_box_via_move`, `Box::new_uninit`, all `None`) and `g()` (exact) | the same | as planned |
| a call in the **body** of a project `macro_rules!` (`inner_call()`) | the call resolves; **`original_range_opt` = `None`** | "maps to nothing smaller than the macro call" | **answer to RS4 §3**: a call written in a macro body has no original range. The call site to report is the **macro call's** range (the `body_call!()` in the file) |
| a call to a function a `macro_rules!` generated (`generated()`) | `R` and `C`: `Function`, `HasSource::source` has `file_id.is_macro() == true` | a macro file | as planned |
| a function under `#[tokio::main]`, proc macros off | the body is the original body: `work()` resolves, `original_range_opt` exact | found, from the original body | as planned |
| `a + b` (a user `Add`), `x?`, `*p`, `for y in it`, `.await` | **no** `CallExpr` and no `MethodCallExpr` node for any of them | none is a call | as planned |

The rule these rows give RS4: **a site's range is `original_range_opt` of the callee expression (call) or of the name (method call); `Some` means written in the
file; `None` means the call comes from a macro's own body, and the macro call's range is the site.** For a path call prefer `resolve_path_with_subst`; when it is
`None` or returns a local or a closure, use `as_callable().kind()`; for a record use `resolve_variant`.

### The oracle: `Analysis::outgoing_calls`

Called on eight functions of the fixture (a concrete method, an impl-return chain, an inherent associated call, a tuple struct, an iterator chain with a
closure, `println!`, a macro-generated callee, a `#[tokio::main]` body). **Its ranges are a subset of ours on all eight**, and they are comparable: the oracle's
range is the **name** of a method call and the **whole callee expression** of a call (`Foo::new`, not just `new`), the same two ranges the table above uses.
Where the sets differ: the oracle never reports `f()` inside `println!(…)` (ours does), because it walks `fn_body.syntax().descendants()` and never expands macros.
It also drops a call it cannot turn into a navigation target (no target, no entry). So the subset check is the right oracle test, and "the oracle finds
everything" is not.

### Cancellation (RS5 depends on this)

A query running in another thread is cancelled by unwinding with a `salsa::Cancelled` payload. `ra_ap_ide_db::base_db::salsa::Cancelled` is public; its
`Cancelled::catch(f)` takes an `UnwindSafe` closure (use `AssertUnwindSafe`) and returns `Result<T, Cancelled>`. `Cancelled` is `Local`, `PendingWrite` or
`PropagatedPanic`. Two ways to cause it, measured on a cold query (the first walk, about 2.5 s) cancelled 400 ms in:

| How | What the reader gets | Time from request to return | The same handle afterwards |
|---|---|---|---|
| `db.cancellation_token()` (per handle, `Clone + Send`), then `token.cancel()` from any thread | `Err(Local)` | **0.9 ms** | **usable**: a second walk on it returned `Ok` (the token is reset when the next top-level query starts) |
| a writer: `db.trigger_cancellation()` (or `apply_change`) with `&mut RootDatabase`; it blocks until every other handle is dropped | `Err(PendingWrite)` | 1.6 ms | **not usable**: the second walk failed with `PendingWrite` again; the handle must be dropped and a new one cloned |
| neither | `Ok` after 2.35 s | | |

RS5's per-request handle is `db.clone()` (`RootDatabase: Clone`, a cheap storage handle) plus its token, and a cancelled request ends in `Err(Local)` with the handle
still good. A text change is a writer: the old handles die with `PendingWrite`. Everything outside `ide` is `attach_db` + `Cancelled::catch` around the walk.

### Load numbers (three projects, release, `core/`-style loads)

| Project | Packages in the graph (workspace + sysroot) | Load | First walk (all project files) | Second walk, a new `Semantics` on the same database | Peak memory |
|---|---|---|---|---|---|
| this repo's `core/` (13 files walked) | 39 (26 are the sysroot) | 0.72 s | 3.3 s | **8.7 ms** | 474 MB |
| `languages/typescript/` (37 files, a git dependency) | 113 | 0.87 s | 6.6 s | 22.8 ms | 767 MB |
| `languages/rust/spikes/` (9 files; the outside project, 232 packages) | **232** | 0.94 s | 7.3 s | 4.7 ms | 840 MB |

(Peak memory is `/usr/bin/time -l`; `ps` RSS fell during the third run because macOS compresses memory, so it is not used.) A second `Semantics` on a loaded
database costs milliseconds and no memory: the work is in the database, not in `Semantics`. **Everything 100%-resolved**: 978/978 calls and 914/914 method calls on `core/`,
1762/1762 and 2997/2997 on `typescript/`, 140/140 and 307/307 on `spikes/` (calls that are written outside macros).

### Scenario cost (decides how RS4's scenarios load)

Thirteen one-package programs, each with a `Box<dyn Writer>`, a `vec!` and a `println!`:

| Option | Time for 13 | Resolves `std`? |
|---|---|---|
| accept: one load per scenario | **39 s** (3.0 s each: 0.3 s load + 2.5 s first query, which is `std` being analysed) | yes |
| **one workspace for all** (a generated `Cargo.toml` with `members = ["s*"]`) | **3.0 to 3.5 s** in total, 445 MB | yes |
| a sysroot-less load, one per scenario | 3.0 s | **no**: 0 of 2 method calls resolve (`Box`, `Vec`, `String` are unknown) |

The `std` cost is paid once per process, not once per package. Choose **one shared workspace**, built in a temporary folder by the test harness (it copies the
scenario folders in and writes the `[workspace]` manifest there). The sysroot-less option is a trap and is not an option. The repo currently has six scenarios; the
numbers scale with the process, not with the count.

### `Cargo.lock`: what a load writes, and the three checks of RS3 §5

Confirmed on `ra_ap_project_model 0.0.358` with `toolchain 1.99.1`, 1.98.0, 1.94.1 and 1.74.1 (the rows are one-package fixtures with an `itoa` dependency):

| Fixture | Toolchain (decided by `cargo --version` run in the project's folder) | Result |
|---|---|---|
| `core/` (lock committed) | 1.99.1 | `Cargo.lock` byte-identical, mtime unchanged, `git status` clean |
| a one-package fixture, lock from an earlier load | 1.99.1 | byte-identical |
| **stale** lock (the manifest has `itoa`, the lock does not) | 1.99.1 | **byte-identical**, and `itoa` still resolves (the engine worked on a copy) |
| stale lock | 1.94.1 (≥ 1.82, < 1.95: `--lockfile-path`) | byte-identical |
| stale lock | **1.74.1 (< 1.82)** | **rewritten in the project**: `itoa` entries added. No copy |
| **no lock** | 1.99.1 | `Cargo.lock` **written into the project** (365 bytes with `itoa`) |

So: a project **with** a lock is never touched by a toolchain ≥ 1.82; a project **without** one gets a file, whatever the toolchain.

The three checks of RS3 §5, with `CARGO_RESOLVER_LOCKFILE_PATH=<temp>/Cargo.lock` put in `CargoConfig::extra_env`:

1. **The sysroot still loads, and a project with a lock still uses the engine's copy.** With the variable set on 1.99.1: `sysroot error: None`, 26 sysroot packages, calls resolve
   (`itoa` too). With a stale lock the project's lock stayed byte-identical **and the redirect path stayed empty**: the engine sets its own value for the variable after `extra_env`, so
   its copy wins.
2. **Cargo creates the lock at the path when no file is there.** Yes: with no lock, on 1.99.1 and on 1.98.0, `<temp>/Cargo.lock` was created (with the `itoa` entries) and **the
   project had no `Cargo.lock` afterwards.** The control run without the variable wrote the file into the project.
3. **Which toolchain version decides: the project's.** The engine runs `cargo --version` with the manifest's folder as the working directory
   (`toolchain_info/version.rs`), so a `rust-toolchain.toml` in the folder counts. Observed: **1.98.0 honours the variable** (file created at the path, none in the project);
   **1.94.1 ignores it** (the lock was written into the project), also when `rust-toolchain.toml` in the project selected 1.94.1 while the loader ran under another toolchain.
   1.97 itself (the plan's boundary, from `make_lockfile_copy`) is not installed here: only "1.98.0 yes, 1.94.1 no" is observed. So option B works from 1.97 (observed ≥ 1.98), and below it the fallback is the
   rule-2 exception reported by `wrote_lockfile()`.

### Smaller facts

- **Load writes nothing else** into a project that has a lock (checked by `git status` and hashes). The sysroot's own metadata uses `--lockfile-path` (cargo's deprecation warning for the flag shows up in the sysroot's `metadata_error`
  when offline).
- **API names that exist in 0.0.358** (checked by compiling and running): `Semantics::new`, `parse`, `attach_first_edition_opt`, `resolve_expr_as_callable`,
  `resolve_method_call`, `resolve_method_call_fallback`, `resolve_path`, `resolve_path_with_subst`, `resolve_variant(RecordExpr) -> Option<Variant>`, `type_of_expr`,
  `expand_macro_call`, `original_range`, `original_range_opt`; `hir::CallableKind::{Function, TupleStruct, TupleEnumVariant, Closure, FnPtr, FnImpl}` (note `TupleEnumVariant`
  holds an `EnumVariant`); `AnalysisHost::with_database`, `Analysis::outgoing_calls(&CallHierarchyConfig, FilePosition)`. **`resolve_record_literal` does not exist.**
