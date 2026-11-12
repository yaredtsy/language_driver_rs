# 04.04.06 RS5: what the driver needs

```
RS5
├── 1. what comes from core, and what is Rust's
├── 2. the session
├── 3. edits and unsaved text
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
| edits, unsaved text, reload | Rust | this phase |

## 2. The session

```rust
pub struct Session { /* a Project + a Solver<Rust> that outlives one request */ }

impl Session {
    pub fn open(root: &Path, file: &Path, opts: &LoadOptions) -> Result<Session, OpenError>;
    pub fn tree(&mut self, file: &str, entry: &Entry<'_>, budget: &Budget, cancel: &Cancel) -> Option<Tree>;
    pub fn expand(&mut self, token: &Token, budget: &Budget, cancel: &Cancel) -> Result<Tree, Stale>;
    pub fn set_text(&mut self, file: &str, text: Option<&str>);      // None: back to the disk content
    pub fn reload(&mut self) -> Result<(), OpenError>;
}
```

The same five methods as TypeScript's, so a driver's main loop is written once.

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

## 3. Edits and unsaved text

| | How |
|---|---|
| `set_text(file, Some(text))` | a change of that file's text applied to the database (`RootDatabase::apply_change` with a `ChangeWithProcMacros` / `FileChange` **[check]** the type at the pin). Incremental by construction: salsa recomputes only what depended on the file |
| `set_text(file, None)` | the same, with the text read from disk |
| a new file, a deleted file | a file the Vfs does not know needs the source roots updated; in RS5 that is `reload()`. Say so in the method's doc |
| the outline cache | per request (RS4 §5), so an edit cannot leave a stale one |

`set_text` takes `&mut self`, so the borrow checker guarantees no request is running on the old text (RS3 §5's note).

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

- **Way 1, as TypeScript:** any `set_text` drops the whole memo. A summary is one body's sites; recomputing is cheap next to what salsa
  keeps for us (the types). **[measure]** before anything smarter.
- **One thing core's token must not assume:** TS5 describes a token as "`ContextId` plus a child offset". A `ContextId` holds a
  *digest* of the key, and a digest cannot be turned back into a key. With `Key = ()` that does not matter. With layer 2 it does:
  `expand(token)` must find the `(function, key)` again. So the session resolves a token by **looking the id up** in the contexts it has
  listed (the owned memo of §2 has them), never by decoding it; an id it does not know is `Stale`. Building TS5 step 1 that way costs
  nothing now and is the difference between layer 2 working with `expand` and not.

## 6. Reload

`reload()` loads the workspace again (`cargo metadata` and all) and drops the memo. The driver calls it when something in
`environment_inputs()` changed. It is the slow path, seconds; `set_text` is the fast one.

## 7. Steps

| # | Do | Proof |
|---|---|---|
| 1 | (if not built) core: `Budget`, lazy render, tokens by lookup, paging; the memo in and out of the solver | TS5's step-1 and step-2 tests on the toy language; every language green |
| 2 | `Session::open / tree / expand` | expanding under every cut node of a scenario gives the same tree as one unbounded request |
| 3 | `set_text`, memo dropped; `Stale` tokens | rename a callee in an unsaved buffer: the tree follows; an old token is `Stale` |
| 4 | `reload` | add a dependency to a fixture's `Cargo.toml` between two requests |
| 5 | the cancel measurement of §4, written in `ENGINE.md`; engine-side cancel only if it asks | the number |
| 6 | `rust-cli calls --depth --max-nodes`, and an interactive `expand` | by hand |
| | **tag `rust-v0.3.0`** | tier-3 conformance in the driver (`vnoc`) |

Layer 1 ends here. Next: [`../05-context-layer/01-design.md`](../05-context-layer/01-design.md)
