//! `rust-cli calls … --depth N --max-contexts N --max-nodes N` (RS5 step 6): a budget on the request (the solver's) and a limit on the drawing (the
//! renderer's), on the shared scenarios. The same cases as `typescript-cli`'s (`tests/budget.rs` there, TS5 row 8), with Rust's programs.
//!
//! The expected output of each case was written before the run. `tests/snapshots/no-budget/` is the output of this program at `b78733f`, the commit
//! before the flags: a request with no flag must still print exactly that.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;

/// A copy of `scenarios/<name>/rust` under this crate's scratch folder, with a `[workspace]` of its own (a package inside `target/` would otherwise be
/// looked for in the workspace above it), made once per scenario. A load may write into a project, and the repo is not written.
fn scenario(name: &str) -> PathBuf {
    static MADE: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let to = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cli_budget_{name}"));
    let mut made = MADE.lock().unwrap_or_else(|e| e.into_inner());
    if !made.iter().any(|done| done == name) {
        let from = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../scenarios")
            .join(name)
            .join("rust");
        let _ = std::fs::remove_dir_all(&to);
        copy(&from, &to);
        let manifest = to.join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(&manifest, format!("{text}\n[workspace]\n")).unwrap();
        made.push(name.to_owned());
    }
    to
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn snapshot(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots/no-budget")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `rust-cli calls <scenario> src/main.rs --entry main <flags>`.
fn calls(scenario_name: &str, flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-cli"))
        .arg("calls")
        .arg(scenario(scenario_name))
        .args(["src/main.rs", "--entry", "main"])
        .args(flags)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

/// What the tree mode prints for a program with no unresolved edge and nothing open: the tree, a blank line, and the breakdown, whose last line says what
/// cut the graph: nothing, or the budget's frontier.
fn tree_output(
    tree: &str,
    calls: u32,
    constructs: u32,
    contexts: u32,
    walked: u32,
    frontier: u32,
) -> String {
    let cuts = if frontier == 0 {
        "none".to_owned()
    } else {
        format!("the budget stopped at {frontier} (listed, not walked)")
    };
    format!(
        "{tree}\nbreakdown:\n  edges: {calls} call, {constructs} construct, 0 external, 0 unresolved\n  contexts {contexts}, bodies walked {walked}, summaries reused 0\n  cuts in the graph: {cuts}\n"
    )
}

const STRAIGHT_LINE: &str = "main\n  fetch\n    parse\n  transform\n  store\n";

// --- the tree: straight-line is main -> fetch -> parse, main -> transform, main -> store (3 deep) ---

#[test]
fn depth_0_is_the_entry_alone_on_the_frontier() {
    let out = calls("straight-line", &["--depth", "0"]);
    assert_eq!(stdout(&out), tree_output("main [Cut]\n", 0, 0, 1, 0, 1));
}

#[test]
fn depth_1_walks_the_entry_and_cuts_its_callees() {
    let out = calls("straight-line", &["--depth", "1"]);
    assert_eq!(
        stdout(&out),
        tree_output(
            "main\n  fetch [Cut]\n  transform [Cut]\n  store [Cut]\n",
            3,
            0,
            4,
            1,
            3
        )
    );
}

#[test]
fn depth_2_cuts_only_what_is_two_steps_down() {
    let out = calls("straight-line", &["--depth", "2"]);
    assert_eq!(
        stdout(&out),
        tree_output(
            "main\n  fetch\n    parse [Cut]\n  transform\n  store\n",
            4,
            0,
            5,
            4,
            1
        )
    );
}

#[test]
fn a_depth_that_reaches_nothing_is_the_whole_tree_and_has_no_frontier_line() {
    let out = calls("straight-line", &["--depth", "3"]);
    assert_eq!(stdout(&out), snapshot("straight-line.tree.stdout"));
    assert_eq!(stdout(&out), tree_output(STRAIGHT_LINE, 4, 0, 5, 5, 0));
}

#[test]
fn max_contexts_1_reads_the_entry_and_lists_the_rest() {
    let out = calls("straight-line", &["--max-contexts", "1"]);
    assert_eq!(
        stdout(&out),
        tree_output(
            "main\n  fetch [Cut]\n  transform [Cut]\n  store [Cut]\n",
            3,
            0,
            4,
            1,
            3
        )
    );
}

#[test]
fn max_contexts_0_reads_nothing() {
    let out = calls("straight-line", &["--max-contexts", "0"]);
    assert_eq!(stdout(&out), tree_output("main [Cut]\n", 0, 0, 1, 0, 1));
}

#[test]
fn one_frontier_context_is_a_cut_node_under_each_caller() {
    // `emit` is one context and two nodes: the frontier counts contexts. Its body (a call to a trait's required method, an unresolved edge) is not read
    let out = calls("two-callers", &["--depth", "2"]);
    assert_eq!(
        stdout(&out),
        tree_output(
            "main\n  read_json\n    JsonWriter\n    emit [Cut]\n  read_csv\n    CsvWriter\n    emit [Cut]\n",
            4,
            2,
            4,
            3,
            1
        )
    );
}

// --- no flag: what the program printed before the flags ---

#[test]
fn with_no_flag_every_output_is_what_it_was() {
    for (scenario_name, mode, flag) in [
        ("straight-line", "tree", None),
        ("straight-line", "wire", Some("--wire")),
        ("straight-line", "graph", Some("--graph")),
        ("two-callers", "tree", None),
        ("two-callers", "wire", Some("--wire")),
    ] {
        let out = calls(scenario_name, flag.as_slice());
        assert_eq!(
            stdout(&out),
            snapshot(&format!("{scenario_name}.{mode}.stdout")),
            "{scenario_name} {mode}: stdout"
        );
        assert_eq!(
            stderr(&out),
            snapshot(&format!("{scenario_name}.{mode}.stderr")),
            "{scenario_name} {mode}: stderr"
        );
    }
}

// --- --wire and --graph honour the budget ---

fn marks_of(frame: &serde_json::Value) -> Vec<&str> {
    frame["marks"]
        .as_array()
        .map(|marks| marks.iter().map(|m| m.as_str().unwrap()).collect())
        .unwrap_or_default()
}

#[test]
fn the_wire_marks_a_frontier_node_cut_and_gives_it_no_children() {
    let out = calls("straight-line", &["--wire", "--depth", "1"]);
    let frame: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(marks_of(&frame).is_empty());
    let children = frame["children"].as_array().unwrap();
    let names: Vec<_> = children
        .iter()
        .map(|c| c["target_qname"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["fetch", "transform", "store"]);
    for child in children {
        assert_eq!(marks_of(child), ["cut"], "{}", child["target_qname"]);
        assert!(child["children"].as_array().unwrap().is_empty());
    }
    assert!(
        stderr(&out)
            .ends_with("  cuts in the graph: the budget stopped at 3 (listed, not walked)\n")
    );

    let out = calls("straight-line", &["--wire", "--depth", "0"]);
    let frame: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(marks_of(&frame), ["cut"]);
    assert!(frame["children"].as_array().unwrap().is_empty());
}

#[test]
fn the_graph_counts_its_frontier_and_lists_the_contexts_as_frontier() {
    let out = calls("straight-line", &["--graph", "--depth", "1"]);
    let graph = stdout(&out);
    assert!(graph.contains("frontier: 3,"), "{graph}");
    assert_eq!(graph.matches("Frontier,").count(), 3, "{graph}");
    assert!(
        stderr(&out)
            .ends_with("  cuts in the graph: the budget stopped at 3 (listed, not walked)\n")
    );
}

#[test]
fn timing_says_the_frontier() {
    let out = calls("straight-line", &["--timing", "--depth", "1"]);
    assert!(out.status.success());
    let first = stderr(&out).lines().next().unwrap().to_owned();
    assert!(first.ends_with(", frontier 3"), "{first}");
    let out = calls("straight-line", &["--timing"]);
    let first = stderr(&out).lines().next().unwrap().to_owned();
    assert!(!first.contains("frontier"), "{first}");
}

// --- usage errors ---

fn usage_error(flags: &[&str], message: &str) {
    let out = calls("straight-line", flags);
    let text = stderr(&out);
    assert_eq!(out.status.code(), Some(2), "{flags:?}: {text}");
    assert!(out.stdout.is_empty(), "{flags:?}");
    assert!(text.starts_with(message), "{flags:?}: {text}");
    assert!(text.contains("usage: rust-cli"), "{flags:?}: {text}");
}

#[test]
fn a_depth_that_is_not_a_whole_number_from_zero_is_a_usage_error() {
    let hint = "--depth needs a whole number, 0 or more";
    usage_error(&["--depth", "-1"], &format!("{hint}, not `-1`\n"));
    usage_error(&["--depth", "x"], &format!("{hint}, not `x`\n"));
    usage_error(&["--depth", "1.5"], &format!("{hint}, not `1.5`\n"));
    usage_error(
        &["--depth", "99999999999"],
        &format!("{hint}, not `99999999999`\n"),
    );
    // the next word is a flag the program knows: taken first, so `--depth` is left with no value
    usage_error(&["--depth", "--graph"], &format!("{hint}\n"));
    // the next word is a flag it does not know: not a number
    usage_error(&["--depth", "--bogus"], &format!("{hint}, not `--bogus`\n"));
    // nothing after it
    usage_error(&["--depth"], &format!("{hint}\n"));
}

#[test]
fn a_max_contexts_that_is_not_a_whole_number_from_zero_is_a_usage_error() {
    let hint = "--max-contexts needs a whole number, 0 or more";
    usage_error(&["--max-contexts", "-1"], &format!("{hint}, not `-1`\n"));
    usage_error(
        &["--max-contexts", "many"],
        &format!("{hint}, not `many`\n"),
    );
    usage_error(&["--max-contexts"], &format!("{hint}\n"));
}

// --- --max-nodes: the renderer's limit, on a complete graph ---

/// The line after the breakdown's `cuts in the graph` line when the renderer cut.
fn renderer_line(nodes: u32, graph: &str) -> String {
    format!("  cuts in the tree: at {nodes} nodes (--max-nodes); the graph {graph}\n")
}

#[test]
fn a_tree_wider_than_max_nodes_is_cut_there_and_a_line_says_so() {
    // main, fetch, parse: three nodes; transform and store do not fit, so `main` is the cut node
    let out = calls("straight-line", &["--max-nodes", "3"]);
    assert_eq!(
        stdout(&out),
        tree_output("main [Cut]\n  fetch\n    parse\n", 4, 0, 5, 5, 0)
            + &renderer_line(3, "is complete")
    );
    let out = calls("straight-line", &["--max-nodes", "4"]);
    assert_eq!(
        stdout(&out),
        tree_output(
            "main [Cut]\n  fetch\n    parse\n  transform\n",
            4,
            0,
            5,
            5,
            0
        ) + &renderer_line(4, "is complete")
    );
    let out = calls("straight-line", &["--max-nodes", "1"]);
    assert_eq!(
        stdout(&out),
        tree_output("main [Cut]\n", 4, 0, 5, 5, 0) + &renderer_line(1, "is complete")
    );
}

#[test]
fn a_tree_that_exactly_fits_max_nodes_is_not_cut_and_says_nothing() {
    // five nodes in all
    let out = calls("straight-line", &["--max-nodes", "5"]);
    assert_eq!(stdout(&out), snapshot("straight-line.tree.stdout"));
    let out = calls("straight-line", &["--max-nodes", "500"]);
    assert_eq!(stdout(&out), snapshot("straight-line.tree.stdout"));
}

#[test]
fn the_renderer_line_says_when_the_graph_has_its_own_cuts_too() {
    // the graph has a frontier (`parse`), and the renderer cuts at 2 nodes: `main` and `fetch` are both cut, the first by the renderer
    let out = calls("straight-line", &["--depth", "2", "--max-nodes", "2"]);
    assert_eq!(
        stdout(&out),
        tree_output("main [Cut]\n  fetch [Cut]\n", 4, 0, 5, 4, 1)
            + &renderer_line(2, "has its own cuts, above")
    );
}

#[test]
fn the_wire_honours_max_nodes_merged_or_not() {
    for flags in [
        &["--wire", "--max-nodes", "3"][..],
        &["--wire", "--merged", "--max-nodes", "3"][..],
    ] {
        let out = calls("straight-line", flags);
        let frame: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(marks_of(&frame), ["cut"], "{flags:?}");
        let children = frame["children"].as_array().unwrap();
        assert_eq!(children.len(), 1, "{flags:?}");
        assert_eq!(children[0]["target_qname"], "fetch");
        assert_eq!(children[0]["children"].as_array().unwrap().len(), 1);
        assert!(
            stderr(&out).ends_with(&renderer_line(3, "is complete")),
            "{flags:?}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn timing_says_the_renderer_cut() {
    let out = calls("straight-line", &["--timing", "--max-nodes", "3"]);
    let first = stderr(&out).lines().next().unwrap().to_owned();
    assert!(first.ends_with(", tree cut at 3 nodes"), "{first}");
}

#[test]
fn max_nodes_must_be_a_whole_number_from_one() {
    let hint = "--max-nodes needs a whole number, 1 or more";
    usage_error(&["--max-nodes", "0"], &format!("{hint}, not `0`\n"));
    usage_error(&["--max-nodes", "-1"], &format!("{hint}, not `-1`\n"));
    usage_error(&["--max-nodes", "x"], &format!("{hint}, not `x`\n"));
    usage_error(&["--max-nodes"], &format!("{hint}\n"));
    // the graph is not drawn, so there is nothing for it to limit
    usage_error(
        &["--graph", "--max-nodes", "3"],
        "--max-nodes limits the tree and the wire",
    );
}

#[test]
fn the_help_text_names_the_flags_and_says_which_limit_is_whose() {
    let out = Command::new(env!("CARGO_BIN_EXE_rust-cli"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let text = stderr(&out);
    assert!(
        text.contains("[--depth <n>] [--max-contexts <n>] [--max-nodes <n>]"),
        "{text}"
    );
    assert!(
        text.contains("--depth and --max-contexts are the solver's budget"),
        "{text}"
    );
    assert!(
        text.contains("--max-nodes is the renderer's limit"),
        "{text}"
    );
}
