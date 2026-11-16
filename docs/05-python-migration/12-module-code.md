# 05.12 A file's own code (module code)

```
module code
├── 0. what the code has today
├── 1. what is missing, with the Python shapes
├── 2. what TypeScript decided, and what Python takes from it
├── 3. the design here
├── 4. measure first: the coverage scan
├── 5. the steps
├── 6. what the owner decides
└── 7. to check while building
```

Written 2026-11-13 (reviewer), after TypeScript's D32 ([`../03-typescript/09-module-def.md`](../03-typescript/09-module-def.md)): a
file's own code becomes a root of its own. **Not built. Nothing here was run**; every `[check]` is a claim to verify first. Order: after
class summaries reach the owner's effects decision (CS7 done, CS4b), so the two do not move the same snapshots at once.

## 0. What the code has today

| | Today | Where |
|---|---|---|
| an entry | a def (`Entry { qualname, occurrence }`), at any depth, in any block (B5 is fixed) | `sem/defs.rs` `entry` |
| the code at the top of a file | **in no tree**, and not counted | — |
| a nested `def`'s decorators and defaults | calls of the def that runs the `def` statement | `interp/walk.rs` `Stmt::FunctionDef` |
| a `class` statement inside a def | its decorators and bases are evaluated (the `other` arm, `simple_stmt_exprs`); **its body's statements are not** `[check]` | `interp/walk.rs` |
| imports | bound, never walked: a module load is not a call | `Stmt::Import`, `Stmt::ImportFrom` |

## 1. What is missing, with the Python shapes

```python
app = Flask(__name__)                 # setup at the top of a file
router = APIRouter(prefix="/files")

@app.route("/upload")                 # a decorator call: runs when the module runs
def upload(): ...

class Config:
    DEBUG = env_flag("DEBUG")         # a class body runs when the class is defined

if __name__ == "__main__":            # the usual way a Python script starts
    main()
```

None of these calls is in any tree. For a script, the call that starts everything (`main()`) is one of them. How much of a real project
this is, is not known: Python has no coverage scan (§4). TypeScript's found 786 of 7657 call-like nodes missed on V-NOC, every one at the
top of a file.

## 2. What TypeScript decided, and what Python takes from it

| TypeScript (D32, `09-module-def.md`) | Python |
|---|---|
| core's `Entry` becomes `#[non_exhaustive] enum { Def { qualname, occurrence }, File }` | the same enum. Until this plan's step 2, Python answers `None` for `File` and its tests stay green (rule 5): TypeScript's core step makes that change |
| the root's row: `DefKind::Module`, `qname` the file's path, empty qualname, `occurrence` 0, the whole file as range, a path id | the same row, so the wire is one shape for every language |
| the wire: `target_kind: "Module"`, empty `target_qualname` and `target_id`; no new field, no version bump | the same: no change in `core/analyzer-core/src/wire.rs` |
| no new qualname, the outline unchanged (rule 10) | the same: `python_syntax::outline` prints what it prints |
| namespaces and class static blocks are their own callers (the engine's call hierarchy says so) | **a class body is not** a caller of its own here: §3 row 3, the owner's question 1 |
| the coverage scan (row 3c) is the acceptance test | the same kind of scan, built first (§4) |

## 3. The design here

| # | Rule | Why |
|---|---|---|
| 1 | `Entry::File` roots a walk of the module body: one context, no parameters (the module is run once, with nothing passed in). Its key is the function key of a def with no parameters | the solver needs nothing new: a module body is a body |
| 2 | Inside the module walk, a name bound at the top (`app = Flask(…)`) is a local of the module run, so `@app.route(…)` and `app.run()` read the walk's own value | the walk already keeps locals; the engine answers only what the walk does not hold |
| 3 | A `class` body's statements run as part of the run that executes the `class` statement (the module, or a def that defines a class): their calls are that run's calls, flat, as a nested `def`'s decorators are today (§6 question 1) | Python runs a class body exactly once, when the `class` statement runs, like a decorator. This also fills the same gap inside defs (§0, last-but-one row) |
| 4 | Imports stay module loads: `import x` draws nothing, and the module code of `x` is never in the tree | as TypeScript's policy; following imports would put every module's setup under every file |
| 5 | Calls inside a def's body stay that def's: the module walk does not enter `def` bodies (it binds the def, as today) | one call site, one owner |
| 6 | Functions reading a module global still get the engine's answer, not the module walk's value | the module walk is a root, not a prologue for every function. Globals in summaries are README §8 of class summaries ("Writes to module globals"), not this plan |
| 7 | `if __name__ == "__main__":` is walked like any `if`: both arms (the walk does not narrow) | `main()` is drawn; the `else` arm, if any, too |

## 4. Measure first: the coverage scan

As TypeScript's row 3c: an **independent** syntax scan (not the walker, or it agrees by construction) of every call-like node of a file,
compared **by span** with the sites the graphs draw, one way only, by **category** (the miss's syntactic position): `top-level`,
`class-body`, `decorator`, `default`, `lambda`, `comprehension`, and whatever else it finds. Every fixture program scanned, with a list of
known gaps (each with its reason); the three real projects (Jarvis, the Flask service, FastAPI) as an `#[ignore]` measure of misses by
category. It lives in the `sem` module as a `#[cfg(test)]` module (it may read the engine's syntax, rule 4; no new public API).

The scan answers whether this plan is worth building, and how big a module root gets (the owner's question 2).

## 5. The steps

| # | Do | Proof |
|---|---|---|
| PYM0 | the coverage scan (§4), with its known-gap list | the scan passes on every fixture with the list; the three projects' misses by category in `languages/python/ENGINE.md` |
| PYM1 | (comes with TypeScript's core step) `Entry` is the enum; Python's call sites use `Entry::Def`; `File` answers `None` | Python's tests green, no snapshot moved |
| PYM2 | `Entry::File`: the module root (§3 rows 1, 2, 4–7); `python-cli calls <dir> <file>` without `--entry` asks for it | a fixture per Python shape of §1 (one file per case); the scan's `top-level` misses go to 0 on the fixtures; the golden equal (Jarvis's entry is a def) |
| PYM3 | class bodies (§3 row 3), if the owner says so | the scan's `class-body` misses go to 0; every snapshot that moves is a def that defines a class, read and explained |

## 6. What the owner decides

| # | Question | Recommendation |
|---|---|---|
| 1 | Whose calls are a class body's: the run that executes the `class` statement (flat), or the class as its own root? | **the run that executes it**: the class body runs once, then, like a decorator; it has no name to open (a class entry is already the constructor, D25) |
| 2 | Flat calls under the module, or grouped by top-level statement? | **flat**, as TypeScript's note; decide again on PYM0's numbers if a module root is very large |
| 3 | Build PYM2 at all | decide on PYM0's numbers |
| 4 | The module root's `qname` on the wire: the file's path (TypeScript's choice, D32) or Python's dotted module name (`jarvis`, as Python's other qnames are written) | **the path, as every language**: the module is only ever a root, asked for by a reader that holds the file; one spelling across languages. Settle with the TypeScript session before PYM2 |

## 7. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | A `class` statement inside a def evaluates its decorators and bases, and not its body (`simple_stmt_exprs` does not enter nested statements) | PYM0 (the scan's `class-body` category shows it) |
| 2 | The solver can key and walk a module body with no `def` node (a `ModModule`'s body, not a `StmtFunctionDef`): what `Walk::new`, `bind_parameters` and `body_scope` need | PYM2 |
| 3 | The engine gives a module scope for name lookups (`global_scope` / the file's scope) the walk can use as `env.scope` | PYM2 |
| 4 | A module root never appears as a callee (nothing calls a module), so the wire needs no version bump | PYM2 |
