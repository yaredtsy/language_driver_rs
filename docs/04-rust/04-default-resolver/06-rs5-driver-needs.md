# 04.04.06 RS5: what the driver needs

```
RS5
├── 1. what comes from core, and what is Rust's
├── 2. the session
├── 3. edits: a file saved on disk
├── 4. cancel
├── 5. the memo, and one thing core's token must not assume
├── 6. reload
└── 7. steps, and the tag
```

The full design is TypeScript's ([`../../03-typescript/07-ts5-driver-needs.md`](../../03-typescript/07-ts5-driver-needs.md)); this file is
what differs.

## 1. What comes from core

| Need | Where | State |
|---|---|---|
| `Budget`, lazy render, the frontier (`more` tokens), paging | `analyzer-core` | TS5 steps 1–2. **Not built yet** (2026-11-12). If RS5 is reached first, those two steps are done here, in `core/`, against the toy language exactly as TS5 describes them; they need no engine |
| `Cancel` (a flag and a deadline) | `analyzer-core::solve` | built |
| stable ids, `environment_inputs` | RS3, RS4 | built by then |
| following the disk (`refresh`), reload | Rust | this phase. Text the editor has not saved is not read, for now (D42) |

## 2. The session

```rust
pub struct Session { /* a Project + a Solver<Rust> that outlives one request */ }

impl Session {
    pub fn open(root: &Path, file: &Path, opts: &LoadOptions) -> Result<Session, OpenError>;
    pub fn tree(&mut self, file: &str, entry: &Entry<'_>, budget: &Budget, cancel: &Cancel) -> Option<Tree>;
    pub fn expand(&mut self, token: &Token, budget: &Budget, cancel: &Cancel) -> Result<Tree, Stale>;
    pub fn refresh(&mut self) -> Refreshed;      // before each request: follows the disk (§3)
    pub fn reload(&mut self) -> Refreshed;       // loads the workspace again, whatever the disk says (§6)
}
```

The same methods as TypeScript's, so a driver's main loop is written once. `Refreshed { reopened, lost }` is TypeScript's
([`../../03-typescript/11-step6-text-and-disk.md`](../../03-typescript/11-step6-text-and-disk.md) §3). The first version had
`set_text(file, Option<&str>)` here; text the editor has not saved is **not read, for now (owner, 2026-11-17, D42)**.

**To align before RS5 starts:** the rest of this block was written before TS5 was built. TypeScript's session answers a `Graph`
(`graph`, not `tree`: rule 7), and `expand` and its tokens are not built (D31, TS5 row 2b). Rust follows what TS5 built.

> **🦀 Rust note: a struct that borrows from itself, and how to avoid it.** `Solver<Rust<'p>>` holds `Func<'p>` handles, which borrow the
> `Project`. A `Session` that owns *both* the `Project` and a `Solver` borrowing it would be a *self-referential struct*, which safe Rust
> does not allow (moving the `Session` would move the `Project` and leave the borrow pointing at the old place). TypeScript's plan meets
> the same wall. The clean way out here is that the solver's memo does not have to hold engine handles between requests: `Session`
> keeps the memo in an **owned** form, keyed by the stable identity (`StableDef` + key digest) with sites and targets as plain data, and
> each request builds a short-lived `Solver` over a fresh `Sem<'p>` that reads and fills that owned memo. The handles live for one
> request, as [`../02-architecture.md`](../02-architecture.md) §4 says they do. No `unsafe`, no self-reference, and the memo is already in
> the form a cache on disk would want.
>
> This asks one thing of core: a way to give the solver a memo to start from and take it back (`Solver::with_memo` / `into_memo`), with the
> conversion between a language's handles and stable ids supplied by the language. It is part of TS5 step 1's design, and TypeScript
> needs it for the same reason. **[check]** against whatever TS5 has built by then; if it kept handles alive another way, Rust follows it.

## 3. Edits: a file saved on disk

**Only saved text is read, for now (owner, 2026-11-17, D42).** The session follows the disk by itself: the driver calls `refresh` before
each request, as Python's and TypeScript's drivers do (D41), because the contract sends no change notification.

What differs from TypeScript is the cost. There a changed file is a whole program built again. Here the file's new text goes into the
database and salsa recomputes only what depended on it.

| What changed on disk | How `refresh` sees it | What it does |
|---|---|---|
| a `.rs` file of the project saved | the stamp (length, modified time) of every file the `Vfs` holds in a **local** source root (**[check]** `SourceRoot::is_library` at the pin tells a local root from a dependency's) | reads the text and applies it to the database (`RootDatabase::apply_change` with a `ChangeWithProcMacros` / `FileChange` **[check]** the type at the pin). Incremental by construction |
| a `.rs` file created or deleted | the stamp of each folder of a local source root (a folder's modified time moves when an entry is added or removed) | **[check]** at the pin whether a file can be added to the `Vfs` and the source roots set again without loading the workspace (rust-analyzer's own main loop does that); if it can, that; if not, a reload (§6) |
| a manifest, a lock, the toolchain and cargo config files, a `build.rs` when build scripts ran (`environment_inputs`) | their stamps | a reload (§6): seconds |
| a dependency's source (the registry, the sysroot) | not stamped | nothing. Cargo does not change the source of a published version, and a new version changes `Cargo.lock`, which is stamped. A registry file edited by hand needs `reload`: TypeScript's `node_modules` rule (D41 a) |
| text the editor has not saved | not read (D42) | when the contract adds the text: the same `apply_change` with the buffer's text, and `None` for "back to the disk". Cheap here, unlike TypeScript |
| the outline cache | per request (RS4 §5) | an edit cannot leave a stale one |

`refresh` takes `&mut self`, so the borrow checker guarantees no request is running on the old text (RS3 §5's note).

**What the review of TypeScript's step 6 found holds here too** (2026-11-17,
[`../../03-typescript/11-step6-text-and-disk.md`](../../03-typescript/11-step6-text-and-disk.md) §4, §5). Each is a test in step 3 or 4:

1. **The stamps live in the `Session`, beside the project**, not inside it, so a session whose reload failed still knows what to watch.
2. **A failed reload leaves the session with no project (`lost`). Every `refresh` says so while it lasts, and the session comes back by
   itself** when a stamp changes. Here that is rare: by D37 a missing or broken manifest is a warning on a load that still succeeds, so
   only `NotUnderRoot` (the opened file is gone) and `Load` lose a session. **[check]** that a `Cargo.toml` saved with a syntax error
   reloads with a warning, and that fixing it reloads clean.
3. **Stamp first, read second.** `refresh` takes a file's stamp and then reads its text. A file saved between the two has a newer stamp
   than the one kept, so the next `refresh` reads it again. The other order would keep the new stamp with the old text, and nothing would
   notice. For a reload, a path already known keeps the stamp taken before it.
4. **The old database is dropped before a reload builds the new one**, so the peak is one database and not two. **[measure]** the
   database's size on a real workspace in step 4.

**For the owner, at RS5's design:** point 2 is TypeScript's answer (D41 b) carried over. A reload is seconds here, 241 ms there; say if
Rust should keep its old database when a reload fails.

## 4. Cancel

| Who checks | When |
|---|---|
| core's solver | before each body |
| `walk::body` | before each site |
| the engine | **only if needed.** salsa cancels running queries when another thread starts a write; with `&mut self` there is no other thread. A long single query (type inference of one huge function) cannot be interrupted from our flag. **[measure]** the longest single `callee` call on a real project in RS4; if it is under ~100 ms, the flag between sites is enough and nothing engine-side is built |

If it is not enough: the engine's own way is a second thread that requests cancellation on the database while the worker runs inside
`Cancelled::catch` (S-RS2 finds the public form). `sem` would own that; nothing outside it changes. Not built until the measurement asks.

A cancelled request returns what it has, marked `Incomplete`; cut summaries are not memoized (the solver already does this).

## 5. The memo

- **Way 1, as TypeScript:** any change `refresh` applies (a file's text, a reload) drops the whole memo. A summary is one body's sites; recomputing is cheap next to what salsa
  keeps for us (the types). **[measure]** before anything smarter.
- **One thing core's token must not assume:** TS5 describes a token as "`ContextId` plus a child offset". A `ContextId` holds a
  *digest* of the key, and a digest cannot be turned back into a key. With `Key = ()` that does not matter. With layer 2 it does:
  `expand(token)` must find the `(function, key)` again. So the session resolves a token by **looking the id up** in the contexts it has
  listed (the owned memo of §2 has them), never by decoding it; an id it does not know is `Stale`. Building TS5 step 1 that way costs
  nothing now and is the difference between layer 2 working with `expand` and not.

## 6. Reload

`reload()` loads the workspace again (`cargo metadata` and all) and drops the memo. `refresh` does the same by itself when the stamp of
something in `environment_inputs()` changed; the driver calls `reload` only for what `refresh` cannot see (§3: a dependency's source
edited by hand). It is the slow path, seconds; a saved file applied by `refresh` is the fast one.

## 7. Steps

| # | Do | Proof |
|---|---|---|
| 1 | (if not built) core: `Budget`, lazy render, tokens by lookup, paging; the memo in and out of the solver | TS5's step-1 and step-2 tests on the toy language; every language green |
| 2 | `Session::open / tree / expand` | expanding under every cut node of a scenario gives the same tree as one unbounded request |
| 3 | `refresh`: a saved file applied to the database, the memo dropped (an old token is `Stale`, if core has tokens by then). No `set_text` (D42) | TypeScript step 6's cases: the callee's file rewritten (the tree follows; without `refresh` it does not, pinned); a file a `mod` names created; a callee's file deleted; nothing changed (the graph byte-identical); a file saved between the stamp and the read is read again by the next `refresh`. **[measure]** `refresh` with nothing changed on a real workspace: the files stamped and the time |
| 4 | `reload`, and `refresh` reloading when an environment input changed | add a dependency to a fixture's `Cargo.toml` between two requests: `refresh` reloads; a `Cargo.toml` saved with a syntax error, then fixed (a warning, then clean: D37); the opened file deleted, then written back (`lost`, then back by itself) |
| 5 | the cancel measurement of §4, written in `ENGINE.md`; engine-side cancel only if it asks | the number |
| 6 | `rust-cli calls --depth --max-nodes`, and an interactive `expand` | by hand |
| | **tag `rust-v0.3.0`** | tier-3 conformance in the driver (`vnoc`) |

Layer 1 ends here. Next: [`../05-context-layer/01-design.md`](../05-context-layer/01-design.md)
