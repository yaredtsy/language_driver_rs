# 05.06 `python-cli`: PY7

```
python-cli
├── 1. why now, and what the older rows got wrong
├── 2. the commands
├── 3. the rules of `calls`
├── 4. the script and the golden run
├── 5. the steps, each with its failing tests and its proof
├── 6. what is not in this file
└── 7. to check while building
```

Written on 2026-11-12 from reading the code at `7348b1a`. **Nothing here was built or run when it was written.** Every `[check]` is a
claim to verify before depending on it.

## 1. Why now

`python-cli` is `fn main() {}`. Two things wait for it:

- The `pylspt` folder cannot be deleted: `pylspt-dev calls` and `run-jarvis.sh` have no replacement (`vnoc/docs/15-leave-pylspt.md` §5).
- PY8 (the tag `python-v0.2.0`, archiving pylspt) comes after PY7.

The rows that describe PY7 were written before D17 and D20. Where they differ from this file, this file wins, and step C4 corrects them:

| Row | Says | Now |
|---|---|---|
| [`03-steps.md`](03-steps.md) PY7 | "`calls --graph` on the Jarvis project equals PY0's golden graph; `calls` equals the golden tree" | no golden graph and no golden tree were ever made. The golden is the **wire** (`out/golden/wire.json`, `wire-merged.json`), as D17 says |
| [`02-what-moves-where.md`](02-what-moves-where.md) §5 | "`--merged` is dropped" | kept (D17) |
| [`04-the-wire.md`](04-the-wire.md) W5 | "`calls --wire` is byte-equal to the regenerated golden" | equal **as JSON** (§4): the golden was written with its keys in alphabetical order, the CLI writes them in the struct's order |
| [`05-entries-and-targets.md`](05-entries-and-targets.md) §6 | "`--occurrence`, default 0" | no default when the name matches several defs (§3.1) |

## 2. The commands

```
python-cli outline <file> [--label <label>]
python-cli calls <dir> <file> --entry <qualname> [--occurrence <n> | --id <marker>]
                 [--graph | --wire [--merged] [--function-prefix <p>] [--class-prefix <p>]]
                 [--only-marked] [--timing] [--label <label>]
```

The shape is `typescript-cli`'s (`languages/typescript/crates/typescript-cli/src/main.rs`): the same subcommand names, the same flags,
the same exit codes, hand-written argument parsing with no new dependency. Read it first and copy its structure. `files` is not copied:
it is about TypeScript configs.

| | |
|---|---|
| `<dir>` | the project root, as `Project::open` takes it |
| `<file>` | relative to `<dir>`, or absolute |
| `--label` | the marker label, default `ID` (rule 1: it is a parameter) |
| exit code | 0 done · 1 the thing asked for is not there (file, def, directory) · 2 the command line is wrong, or the entry is ambiguous |
| stdout | the answer and nothing else, so it can be redirected to a file. There is no `-o`: the shell does that |
| stderr | messages, the counters when stdout is JSON, `--timing` |

`outline` prints one line per def, indented by depth, as `typescript-cli` does:
`<Kind> <qualname>  <line>:<col>-<line>:<col>[  <label>: <marker>][  SYNTAX-ERROR]`, then one `syntax error <line>:<col>: <message>` line
per syntax error. Lines and columns are 1-based, columns in UTF-16 units (`analyzer_core::text::LineIndex`).

`calls` has three outputs:

| Flags | stdout | Library call |
|---|---|---|
| none | the tree (`to_text(&render(&graph, &graph.entries[0], &options.render_options()))`), a blank line, the counters | `call_graph` |
| `--graph` | the graph as pretty JSON (`Graph` is `Serialize`); the counters go to stderr | `call_graph` |
| `--wire` | the wire as pretty JSON, alone; with `--merged`, merged | `call_wire` |

- **The counters** are `Graph::stats`, as text: `contexts`, `walks`, `reuse hits`, `unresolved` on one line, then one line per cut and one
  per language counter, in the maps' own order (they are `BTreeMap`s, so the order is fixed: rule 9). The exact text is the builder's;
  a test pins it.
- **`--wire` prints no counters.** `call_wire` returns the frame only, and asking for the graph too would solve twice. Not worth a new
  library function now.
- `--function-prefix` and `--class-prefix` go into `Options::wire` (`WireStyle`); without them the ids have no prefix. No prefix is
  spelled in the crate or in its tests (rule 1): the tests use `F/` and `C/`, as `tests/api.rs` does.
- `--only-marked` sets `Options::only_marked` (pylspt's `--ids`).
- `--timing` prints `open <n> ms, solve <n> ms` to stderr.
- `--graph` with `--wire`, or `--merged` or a prefix without `--wire`, is a wrong command line: exit 2.

**Rust note.** `std::process::ExitCode` is what `main` returns to say how the program ended; `ExitCode::from(2)` is the number a shell
script sees in `$?`. Returning it (instead of calling `std::process::exit`) lets every value be dropped normally first.

## 3. The rules of `calls`

### 3.1 Which def is the entry

A qualname is not unique in a file (twins under `if`/`else`, a property's getter and setter). The CLI finds the def itself, from the
outline, and then asks the library by `(qualname, occurrence)` (D20):

1. Read the file's text. `python_syntax::outline(text, style)`.
2. The defs whose `qualname` equals `--entry`, in the outline's order. The `n`th of them has occurrence `n` (0-based).
3. Then:

| Matches | Flags | Result |
|---|---|---|
| none | | `no def named …`, exit 1 |
| one | none | that def |
| several | none | list them on stderr (`#<occurrence>  <Kind>  <line>:<col>  <label>: <marker>` or `no marker`) and exit 2. **The first is never picked** (D15, the rule `typescript-cli` has) |
| any | `--occurrence n` | the `n`th; exit 1 when there are fewer |
| any | `--id m` | the one whose marker is `m`; exit 1 when none has it |
| | both `--occurrence` and `--id` | exit 2 |

No function is added to `python-analysis` for this: the outline is public in `python-syntax`, and the count is three lines.

### 3.2 A class as the entry

Nothing special: `call_graph` and `call_wire` take a class (E5). The tree's root is the class.

### 3.3 The same input gives the same output

`calls` run twice on one project prints the same bytes (rule 9). A test does it for the three outputs.

## 4. The script and the golden run

`scripts/run-on-project.sh`, for a person, not for CI:

```
scripts/run-on-project.sh <lang> <dir> <file> <entry> [flags passed on to `calls`]
```

- Builds `<lang>-cli` in `languages/<lang>`, runs `calls … --wire` and `calls … --wire --merged`, and writes `out/run/wire.json` and
  `out/run/wire-merged.json` (`out/` is git-ignored).
- With `GOLDEN=<folder>` set, compares each with the file of the same name in that folder **as JSON** (`python3`, `json.load` of both,
  `==`), prints `wire.json: equal` or `differs`, and exits 1 on a difference.
- No path of a person and no prefix is written in it (rules 1 and 6): the project and the prefixes are arguments.

**Why "as JSON" and not byte-equal.** `out/golden/wire*.json` were written by the ignored test `golden_wire_of_a_real_project`, through
`serde_json::Value`, which sorts an object's keys. The CLI serializes the `Frame` itself, in the order of its fields, as `typescript-cli`
does. Comparing parsed JSON ignores the order of keys and nothing else: arrays (the children, in source order) are compared in order. So
one golden folder serves both the test and the CLI, and nothing is regenerated.

**The golden run** (the proof of C4, on the owner's machine: the project is not in the repo):

```
GOLDEN=out/golden scripts/run-on-project.sh python <the Jarvis project> jarvis.py CallGraphGenerator.do_pass \
    --function-prefix <p> --class-prefix <p>
```

with the two prefixes read from a `target_id` in `out/golden/wire.json`. Expected: both files equal. `out/golden/README.txt` names the
project folder. If a file differs, **stop and report**: do not regenerate the golden. The golden was last made after E2; E3–E5 and D22
should not have moved it `[check]`.

## 5. The steps

Each step is one commit, with explicit paths. In each: write the tests first, see them fail for the stated reason, then build. The tests
are in `languages/python/crates/python-cli/tests/cli.rs` and run the built binary (`env!("CARGO_BIN_EXE_python-cli")`), on the fixture
projects of `python-analysis` (`../python-analysis/tests/fixtures/…`), the way `typescript-cli/tests/files.rs` does.

**Rust note.** `CARGO_BIN_EXE_<name>` is set by Cargo when it builds an integration test of a crate that has a binary: it is the path of
that binary, already built. So the test runs the real program, with real arguments, and reads its stdout, stderr and exit code.

Dependencies of `python-cli` after C3: `python-syntax`, `python-analysis`, `analyzer-core` (for `LineIndex`) and `serde_json`, all
already in the workspace. `Cargo.lock` may gain lines for `python-cli` only. Never run `cargo update`.

### C1. `outline`, and the command line

| | |
|---|---|
| Change | `main` with the usage text, `take_value` / `take_flag`, and `outline` (§2) |
| Failing tests | (a) `outline` on `08-entries/python/twins.py`: the exact text, with the two `f`, the two `Impl` and their `run` methods indented under them. (b) `outline --label Other` on `06-wire/python/ids_and_classes.py` shows no marker; without the flag it shows `ID: 06000000-…`. (c) a file that is not there: exit 1, a message on stderr, nothing on stdout. (d) no arguments, and an unknown subcommand: exit 2 and the usage on stderr |
| Proof | `cargo test -p python-cli`; `cargo clippy -p python-cli --all-targets -- -D warnings` |

### C2. `calls`: the tree

| | |
|---|---|
| Change | §3.1, the tree output and the counters, `--only-marked`, `--timing` |
| Failing tests | on `06-wire/python`, file `ids_and_classes.py`: (a) `--entry main`: the tree is the text `tests/api.rs` pins for the same entry, then the counters. (b) `--only-marked`: the tree differs from (a) and is the one the library gives with `Options::only_marked` for this entry (§7, claim 6). On `08-entries/python`: (c) `twins.py --entry f`: exit 2, stderr lists `#0` and `#1`, stdout is empty. (d) `--entry f --occurrence 1`: the tree has `b` and not `a`. (e) `--occurrence 2`: exit 1. (f) `inherit_a.py --entry Child`: the root is the class and `helper` is under it. (g) `nested.py --entry "top.<locals>.inner"`: `leaf` is under it. (h) `--entry no_such`: exit 1. (i) a directory that is not there: exit 1. (j) `--occurrence 0 --id x`: exit 2. (k) `--id` on `ids_and_classes.py`: `--entry marked --id 06000000-0002-0000-0000-000000000000` works, and a wrong id is exit 1. (l) `--timing` writes one line with `open` and `solve` to stderr and does not change stdout |
| Proof | as C1 |

### C3. `calls --graph` and `calls --wire`

| | |
|---|---|
| Change | the two JSON outputs, `--merged`, the prefixes, the flag rules |
| Failing tests | on `06-wire/python`, entry `main`: (a) `--wire --function-prefix F/ --class-prefix C/`: stdout parses as JSON; the root's `target_id` is `F/06000000-0003-…`, the first child's is `C/06000000-0001-…`, four children; stderr has no counters. (b) with `--merged`: three children, and `unmarked` has `call_count` 1 (the numbers `tests/api.rs` pins). (c) `--wire` with no prefix: the ids have none. (d) `--graph`: stdout parses as JSON and has `entries`, `defs`, `contexts`, `edges`; the counters are on stderr. (e) `--graph --wire`, `--merged` alone, and `--function-prefix F/` alone: exit 2. (f) each of the three outputs, run twice: the same bytes (§3.3). (g) `08-entries/python`, `inherit_a.py --entry Child --wire`: the child's `call_site_file` is `inherit_b.py` |
| Proof | as C1; plus `cargo test` in `languages/python` (the library's tests still pass, no snapshot changed) |

### C4. The script, the golden run, the docs

| | |
|---|---|
| Change | `scripts/run-on-project.sh` (§4). The doc corrections of §1's table. In `04-the-wire.md`: W5 marked done with what was run, and the sentence under the title deleted. `README.md`: the Python row says what `calls --wire` gives. `vnoc` is not touched |
| Failing tests | none in git: the script is for a person. Run it on a fixture as a smoke test: `scripts/run-on-project.sh python languages/python/crates/python-analysis/tests/fixtures/06-wire/python ids_and_classes.py main` writes the two files, and with `GOLDEN=out/run` right after, both are `equal` |
| Proof | the golden run of §4: both files equal. `grep -rn "not moved" docs` finds no line about the wire. `scripts/check-all.sh --static` passes (it checks personal paths in `scripts/` and every link in the docs). Then `scripts/check-all.sh` |

## 6. What is not in this file

| Thing | Why not | Where it goes |
|---|---|---|
| `python-cli mro`, `attrs`, `ast` (pylspt's other dev binaries; D11 kept `ast`) | each needs a new public function in a library crate (`classes` is private; an AST dump must live in `sem.rs`, rule 4). Not needed to retire `pylspt-dev calls` | their own step, when the owner asks for one |
| The edge breakdown `typescript-cli` prints (edges by kind, unresolved by reason) | `breakdown` lives in `typescript-analysis`. Python would be its second user, so it would move to core (rule 5), which touches TypeScript files while that session is working | after TS4 is finished: move `breakdown` to core, then both CLIs print it |
| Which Python environment ty resolved (it changes the answer on a real project) | the helper that prints it is in a patch outside the tree | with the constructor-effects work |
| `--depth`, `--deadline` | no user yet | when someone needs them |
| Deleting the `pylspt` folder, the tag, the archive | PY8 | [`03-steps.md`](03-steps.md) |

## 7. To check while building

| # | Claim | Checked in |
|---|---|---|
| 1 | `Graph` serializes with `serde_json::to_string_pretty` without an error (every key of a map is a string) | C3 test (d). **Holds.** `graph_is_json_alone_and_the_counters_go_to_stderr` parses `--graph` output with `serde_json` and finds `entries`, `defs`, `contexts`, `edges` as arrays. The only maps in `Graph` are `Stats.cuts` (`BTreeMap<Cut, u64>`, a unit-variant key: written as a string) and `Stats.language` (`BTreeMap<String, u64>`) |
| 2 | `Options::render_options()` is what the tree needs: the tree of `main` equals the one `tests/api.rs` pins | C2 test (a) |
| 3 | The fixture folders are reachable from the CLI's tests on Windows too (`CARGO_MANIFEST_DIR` plus a relative path) | CI, the `analysis` job for `python` on `windows-latest` |
| 4 | The golden still holds at HEAD: E3–E5 and D22 did not move the Jarvis wire | C4, the golden run |
| 5 | `python-analysis/src/lib.rs` line 81 says "the tree V-NOC stores" in a doc comment. Rule 1 forbids product words in the crates; the static check does not catch this spelling. Reword it in C3 (`the stored tree`), and say whether other such comments exist | C3. **Done in `python-analysis`; two such comments remain in core.** Reworded `lib.rs` (`call_wire`: "the stored tree") and `interp/fixture.rs` ("prefixes are the caller's"). `grep -rni "vnoc\|v-noc" languages/python/crates core` now finds only `core/analyzer-core/src/wire.rs:1` ("the call tree as V-NOC stores it") and `:298` ("V-NOC's merge"), both doc comments, which C3 does not touch (core, shared with TypeScript) |
| 6 | What `Options::only_marked` shows on `ids_and_classes.py`, entry `main`: this file does not say, because it was not run. Find the library test that pins the filter, write the expected tree from the rule, and say in the report what it is | C2 test (b) |

Next: [`07-the-root.md`](07-the-root.md)
