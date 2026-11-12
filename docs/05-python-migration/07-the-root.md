# 05.07 The root is the folder that was asked for

```
the root
├── 1. what is wrong, and how it was seen
├── 2. the decision (D26)
├── 3. the design
├── 4. the fixture
├── 5. the steps, each with its failing tests and its proof
├── 6. what else the two projects showed (not scheduled)
└── 7. to check while building
```

Written on 2026-11-12. §1 and §6 were **run** (the ignored helper `dump_graph_of_a_real_project`, on two projects of the owner). §3 to §5
are from reading the code: **not built**. Every `[check]` is a claim to verify before depending on it.

## 1. What is wrong

`Project::open(dir)` says "opens the project rooted at `dir`". It does not.

`open_project` (`python-analysis/src/sem.rs`) calls the engine's `ProjectMetadata::discover(dir, …)`. That function walks **up** from `dir`
through every parent folder (`ty_project/src/metadata.rs`, `discover_with_uv_workspace`), and takes as the root, in this order: the closest
folder with a `ty.toml` or a `pyproject.toml` that has a `[tool.ty]` table; else the closest folder with any `pyproject.toml`; else `dir`.
So a config file in a parent folder moves the root.

Seen on a real project: the folder `…/playground/parser/minicart` was opened, and `…/playground/pyproject.toml` exists. The helper's
graph named every file `parser/minicart/app/checkout.py` and every module `parser.minicart.app.checkout`: the root was `playground`.

| What breaks | Why |
|---|---|
| every request answers `None` (the V-NOC driver says `no function … in …`) | `Project::file("app/checkout.py")` joins the path onto the wrong root, and there is no such file |
| `DefInfo::file`, `target_file`, `call_site_file` | relative to the parent folder, not to the folder the caller opened |
| `target_qname` | carries the parent folders as module names |
| what "project code" is | `is_project` asks the engine what the project includes: with the parent as the root, the sibling folders are project code too |

The analysis itself is right once the file is found: the helper opens the file by its full path, and the tree of `checkout_order` was
complete (9 functions). No fixture has a config file above its root, so no test saw this.

## 2. The decision (D26)

**The root is the folder the caller passes. Parent folders are never searched for it.**

- A config file **in** the folder (`ty.toml`, `pyproject.toml`) is still read: that is the engine's answer for that folder (follow the engine).
- A config file **above** the folder is ignored. Its options are written against another root (`src` roots, `extra-paths`, an environment
  path), and V-NOC names files and keys from the opened folder.
- Rejected: keep the engine's root and translate paths. Module names and import resolution would still follow the parent, so the answer
  for one folder would change with what happens to be above it.

What is lost: a virtual environment that lives in a parent folder (`<repo>/.venv`, with `services/api` opened) is no longer found by the
"`<root>/.venv`" rule. Library types then do not resolve, which costs precision (§6). Step R1b gives it back.

## 3. The design

All of it is in `open_project`, in `sem.rs` (rule 4).

```
metadata = ProjectMetadata::discover(dir)
    Ok,  metadata.root() == dir   → use it                      a config in dir, or no config anywhere
    Ok,  the root is a parent     → ProjectMetadata::new(name of dir, dir)      the engine's own "no config" project
    Err                           → ProjectMetadata::new(…, dir)                as today (a broken config never fails the open)
```

- `ProjectMetadata::new` is what `discover` itself returns when no folder has a config ("a virtual project"), so the fallback is an
  answer the engine already gives, not a new one.
- The comparison is between two absolute paths with no `\\?\` prefix (`Project::open` runs `plain` before `open_project`). `[check]`
  that `metadata.root()` and the `dir` passed in are spelled the same way on every system (case on Windows and macOS, a trailing
  separator, a symlink): if they can differ, compare what `canonicalize` gives for both.
- One case is knowingly given up: `dir` has a `pyproject.toml` **without** `[tool.ty]`, and a parent has a `ty.toml`. The engine prefers
  the parent, we fall back to the default project, and `dir`'s `requires-python` is not read. The engine has no public function that
  reads one folder only (`discover_in` is private). It is rare; write it in `ENGINE.md` as a known limit.
- The engine does not run `uv` here unless the environment variable `TY_UV` is set (`ty_project/src/uv.rs`, `UseUv::from_system`). Read,
  not run: write it in `ENGINE.md`, because the V-NOC driver opens the project on every request.

**Rust note.** `match` on a `Result` with a guard (`Ok(m) if m.root() == dir => …`) lets one arm take the good case and the next arm take
"good, but not what we want". The arms are tried in order, so the guarded one must come first.

## 4. The fixture

`languages/python/crates/python-analysis/tests/fixtures/09-root/`. No `# expect:` blocks: read by `tests/api.rs`.

```
09-root/
├── above-pyproject/
│   ├── pyproject.toml            [project] name = "outer"            (no [tool.ty])
│   ├── sibling.py                def outside(): pass
│   └── inner/
│       ├── app.py                from helper import work  ·  def main(): work()
│       └── helper.py             def work(): pass
├── above-ty/
│   ├── ty.toml                   an empty file is a valid config `[check]`
│   └── inner/                    the same two files
└── own-config/
    ├── pyproject.toml            [project] name = "outer"
    └── inner/
        ├── pyproject.toml        [project] name = "inner"            (no [tool.ty])
        └── the same two files
```

`[check]` that a `pyproject.toml` under `tests/fixtures/` does not change any existing test: it is above no other fixture.

## 5. The steps

Each step is one commit. Tests first, seen red for the stated reason.

### R1. The root

| | |
|---|---|
| Change | §3, in `open_project`. Nothing else in the crate changes |
| Failing tests | `tests/api.rs`, for each of the three folders, with `Project::open("<…>/inner")`: (a) `call_wire(&project, "app.py", main)` is `Some` (**red today: `None`**, the file is looked for under the parent). (b) the root's `target_file` is `app.py`, its child's is `helper.py`, and the child's `call_site_file` is `app.py`: no `inner/` in any of them. (c) the root's `target_qname` is `app.main`: no parent folder in the module name. (d) for `above-pyproject`: the graph's `defs` has no row whose file is `sibling.py` or starts with `..`, and opening `above-pyproject` itself (the parent) with `inner/app.py` still works and gives `inner/app.py` (a folder with its own config is still its own root) |
| Docs | D26 in `docs/07-decisions-and-questions.md`; `languages/python/ENGINE.md`: `ProjectMetadata::discover` walks up, what we do about it, the known limit of §3, and the `TY_UV` note; the doc comment of `Project::open` says a config in the folder is read and one above it is not |
| Proof | `cargo test` in `languages/python`: the new tests, the 129 fixture entries with unchanged snapshots; `cargo clippy -p python-analysis --all-targets -- -D warnings`. Then, by hand, the project of §1: the helper on the opened folder gives files without the parent's folders (`PYTHON_DUMP_PROJECT=<the opened folder> PYTHON_DUMP_FILE=app/checkout.py PYTHON_DUMP_ENTRY=checkout_order cargo test -p python-analysis --lib dump_graph -- --ignored --nocapture`); the owner gives the folder, it is not written in the repo |

After R1 is pushed and CI is green, the V-NOC driver takes the new `rev` (all three crates, one hash). That is a step of `vnoc`, not of this file.

### R1b. The environment above the root

Do it only after R1, and only if claim 3 of §7 holds.

| | |
|---|---|
| Change | When the folder has no `.venv` of its own and no environment is configured, use the closest `.venv` in a parent folder as the Python environment. Only the environment: the root does not move |
| Failing tests | a fixture with `outer/.venv/` holding one tiny package and `outer/inner/app.py` that calls a function of it through a project function: opened at `inner`, the import resolves. `[check]` what the smallest folder is that the engine accepts as an environment (`pyvenv.cfg`, `lib/python3.x/site-packages`); if it cannot be made small and the same on three systems, stop and report |
| Proof | the test; the 129 entries unchanged |

## 6. What else the two projects showed (measured, not scheduled)

On a Flask service of the owner (1082 Python files, its own `pyproject.toml`, **no virtual environment found**), four entries of one file:
two gave 33 and 34 contexts; one gave nothing in the project although it calls a project classmethod. The unresolved sites, by what the
engine said there:

| Pattern | Example | What the engine says | Today |
|---|---|---|---|
| a method found after a base class that is not resolvable | `File.get_multiple_by_owner(…)`, `class File(SAModel, OwnedResourceMixin)`, the method is the mixin's | `(bound method <class 'File'>.get_multiple_by_owner…) & Unknown` | `Unresolved` ("a type that names no def") |
| a call through what a decorator returns | `require_session(self._post_common_logic)(…)`, `def require_session(func: F) -> F` | the bound method, a def of the project | `Unresolved` |
| a def under `functools.wraps` | `File.acreate(…)` | `_Wrapped[…]` | `Unresolved` |
| a method of an untyped value | `row.split(…)` | `Unknown` | `Unresolved`; a library call, not in the tree anyway |

The first three lose project calls. Each needs a decision before a step: the first one asks whether an answer the engine gives **with**
`Unknown` beside it may be drawn (rule 8 says an unknown target is never guessed). They are recorded here so they are not found twice.
The owner confirmed the first one as a failure he saw (`get_files` shows no calls). The base that is not resolvable there is
`SA_MANAGER.Model`, a class made at run time (`class SAModel(SA_MANAGER.Model, CRUDMixin)`), so installing the libraries may not make it
resolvable `[check]`: the fix is in what the analyzer does with the engine's answer, not in the environment.

## 7. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | `metadata.root()` equals the `dir` given to `open_project`, spelled the same, when the config is in `dir` or nowhere | R1 tests (a) on `own-config`, and every existing test (their folders have no config) |
| 2 | An empty `ty.toml` is a valid config and makes its folder the engine's root | R1, `above-ty`: red before the fix |
| 3 | The engine has a public way to set the environment's path on a `ProjectMetadata` made with `new` (an option, or `apply_override_options`) | R1b; if not, stop and report |
| 4 | A file given by an absolute path that is under the opened folder is still found after R1 (`a_file_may_be_given_by_its_absolute_path`) | R1, the existing test |

Next: [`../06-roadmap.md`](../06-roadmap.md)
