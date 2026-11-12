// src/interp/fixture.rs
use crate::interp::bind::param_names;
use crate::interp::relevant::relevant_params;
use crate::interp::summary::{PlainVal, SVal};
use crate::interp::{Limits, Mode, Options, Stats, call_graph, call_tree, solve, to_text};
use crate::sem::Sem;
use crate::sem::{ProjectDatabase, open, open_project};
use ruff_db::system::SystemPath;
use ruff_text_size::TextRange;
use std::collections::BTreeSet;

pub(super) struct Expect {
    pub entry: String,
    pub tree: String,
    pub mode: Option<Mode>, // None: `# expect:`, every mode
}
/// Every `# expect:` block in `source`.
pub(super) fn expectations(source: &str) -> Vec<Expect> {
    let mut out = Vec::new();
    let mut lines = source.lines().peekable();
    while let Some(line) = lines.next() {
        let mode = match line.trim_end() {
            "# expect:" => None,
            "# expect inline:" => Some(Mode::Inline),
            "# expect summary:" => Some(Mode::Summary),
            _ => continue,
        };
        let mut tree = String::new();
        while let Some(rest) = lines.peek().and_then(|l| l.strip_prefix("# ")) {
            if rest.starts_with("expect") {
                break; // the next block
            }
            tree.push_str(rest);
            tree.push('\n');
            lines.next();
        }
        let entry = tree
            .lines()
            .next()
            .unwrap_or("")
            .split(" [")
            .next()
            .unwrap()
            .to_string();
        out.push(Expect { entry, tree, mode });
    }
    out
}

/// The functions `func` can return (qualified names), over all its contexts,
/// solved in summary mode from `entry`.
pub(super) fn returns_of(fixture: &str, entry: &str, func: &str) -> BTreeSet<String> {
    let path = std::fs::canonicalize(format!("tests/fixtures/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let module = path.file_stem().unwrap().to_str().unwrap();

    let (s, _) = solve(&db, file, entry, &summary()).unwrap();
    let want = format!("{module}.{func}");
    let mut out = BTreeSet::new();
    for (ctx, c) in s.contexts.iter() {
        if s.sem.qname(c.func) != want {
            continue;
        }
        let Some(res) = &s.slots[ctx].result else {
            continue;
        };
        for v in &res.returns {
            if let SVal::Plain(PlainVal::Function(d)) = v {
                out.insert(s.sem.qname(*d));
            }
        }
    }
    out
}

/// How many contexts of `func` summary mode makes, solving from `entry`.
pub(super) fn contexts_of(fixture: &str, entry: &str, func: &str) -> usize {
    let path = std::fs::canonicalize(format!("tests/fixtures/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let module = path.file_stem().unwrap().to_str().unwrap();
    let (s, _) = solve(&db, file, entry, &summary()).unwrap();
    let want = format!("{module}.{func}");
    s.contexts
        .iter()
        .filter(|(_, c)| s.sem.qname(c.func) == want)
        .count()
}

pub(super) fn relevant_names(fixture: &str, func: &str) -> Vec<String> {
    let path = std::fs::canonicalize(format!("tests/fixtures/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let sem = Sem::new(&db, Options::default().marker);
    let def = sem.entry(file, func).unwrap();
    let parsed = sem.parsed(def);
    let node = sem.function_node(def, &parsed).unwrap();
    param_names(node)
        .zip(relevant_params(&db, def))
        .filter(|(_, r)| **r)
        .map(|(n, _)| n.to_string())
        .collect()
}
/// The stats of one entry in `fixture`, opened the way `check` does.
pub(super) fn stats_for(fixture: &str, entry: &str, opts: &Options) -> Stats {
    let path = std::fs::canonicalize(format!("tests/fixtures/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    call_tree(&db, file, entry, opts).unwrap().1
}

/// The context graph of one entry in `fixture`, in summary mode.
pub(super) fn graph_for(fixture: &str, entry: &str) -> analyzer_core::graph::Graph {
    let path = std::fs::canonicalize(format!("tests/fixtures/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    call_graph(&db, file, entry, &summary()).unwrap()
}

pub(super) fn inline() -> Options {
    Options {
        mode: Mode::Inline,
        ..Options::default()
    }
}
pub(super) fn summary() -> Options {
    Options {
        mode: Mode::Summary,
        ..Options::default()
    }
}

/// Inline mode only: for a fixture summary mode can't run yet.
pub(super) fn check(fixture: &str) {
    check_with(fixture, &[inline()]);
}

/// The `# limits: k=v,…` line of a fixture: limits for summary mode, and `ids=on`
/// (the ID filter, every mode).
pub(super) fn fixture_limits(source: &str) -> Option<(Limits, bool)> {
    let line = source.lines().find_map(|l| l.strip_prefix("# limits:"))?;
    let mut limits = Limits::default();
    let mut ids = false;
    for kv in line.split(',') {
        let (k, v) = kv.trim().split_once('=').expect("limits are `key=value`");
        let (k, v) = (k.trim(), v.trim());
        if k == "ids" {
            ids = v == "on";
            continue;
        }
        let n: u64 = v.parse().expect("limits are numbers");
        match k {
            "max_rounds" => limits.max_rounds = n as u32,
            "fp_depth" => limits.fp_depth = n as u8,
            "max_stack" => limits.max_stack = n as usize,
            "max_contexts" => limits.max_contexts = n as u32,
            "contexts_per_func" => limits.contexts_per_func = n as u32,
            "render_nodes" => limits.render_nodes = n as u32,
            "render_depth" => limits.render_depth = Some(n as u32),
            other => panic!("unknown limit `{other}`"),
        }
    }
    Some((limits, ids))
}

/// Every mode. The same expected trees must come out of each. A fixture with
/// limits skips the no-reuse run: that run compares outputs with no cuts at all.
pub(super) fn check_both(fixture: &str) {
    let path = format!("tests/fixtures/{fixture}");
    let found = fixture_limits(&std::fs::read_to_string(path).unwrap());
    let ids = found.as_ref().is_some_and(|(_, ids)| *ids);
    let mut runs = vec![inline()];
    match found {
        Some((limits, _)) => runs.push(Options {
            limits,
            ..summary()
        }),
        None => {
            runs.push(summary());
            runs.push(Options {
                limits: Limits {
                    reuse: false,
                    ..Limits::default()
                },
                ..summary()
            });
        }
    }
    for r in &mut runs {
        r.only_marked = ids;
    }
    check_with(fixture, &runs);
}

/// One run per `Options`. Every failing block of every run goes in one report.
fn check_with(fixture: &str, runs: &[Options]) {
    let report: String = runs.iter().map(|opts| failures(fixture, opts)).collect();
    assert!(report.is_empty(), "\n{report}");
}

/// A tree as text (one line per node, two spaces per level) with every node's children in the order of their own text: what two
/// renderers that agree on content and disagree on order both reduce to.
fn sorted_siblings(text: &str) -> String {
    struct N {
        line: String,
        kids: Vec<N>,
    }
    fn render(n: &N, depth: usize, out: &mut Vec<String>) {
        out.push(format!("{}{}", "  ".repeat(depth), n.line));
        let mut kids: Vec<Vec<String>> = n
            .kids
            .iter()
            .map(|k| {
                let mut v = Vec::new();
                render(k, depth + 1, &mut v);
                v
            })
            .collect();
        kids.sort();
        out.extend(kids.into_iter().flatten());
    }
    let mut roots: Vec<(usize, N)> = Vec::new(); // the path of open nodes, with their depth
    let mut top: Vec<N> = Vec::new();
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        let depth = (l.len() - l.trim_start().len()) / 2;
        while roots.last().is_some_and(|(d, _)| *d >= depth) {
            let (_, done) = roots.pop().unwrap();
            match roots.last_mut() {
                Some((_, parent)) => parent.kids.push(done),
                None => top.push(done),
            }
        }
        roots.push((
            depth,
            N {
                line: l.trim().to_owned(),
                kids: Vec::new(),
            },
        ));
    }
    while let Some((_, done)) = roots.pop() {
        match roots.last_mut() {
            Some((_, parent)) => parent.kids.push(done),
            None => top.push(done),
        }
    }
    let mut out = Vec::new();
    top.iter().for_each(|n| render(n, 0, &mut out));
    out.join("\n")
}

#[test]
fn sorted_siblings_reduces_two_orders_to_one_text() {
    let a = "main\n  b\n    y\n    x\n  a\n";
    let b = "main\n  a\n  b\n    x\n    y\n";
    assert_eq!(sorted_siblings(a), sorted_siblings(b));
    assert_ne!(
        sorted_siblings(a),
        sorted_siblings("main\n  a\n  b\n    x\n")
    );
    // the shape still matters: the same lines at other depths are another tree
    assert_ne!(
        sorted_siblings(a),
        sorted_siblings("main\n  b\n  y\n  x\n  a\n")
    );
}

fn failures(fixture: &str, opts: &Options) -> String {
    let path = std::fs::canonicalize(format!("tests/fixtures/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let source = std::fs::read_to_string(&path).unwrap();
    let module = path.file_stem().unwrap().to_str().unwrap();

    let blocks = expectations(&source);
    assert!(!blocks.is_empty(), "{fixture}: no `# expect:` block");
    let mut report = String::new();
    for e in blocks
        .iter()
        .filter(|e| e.mode.is_none_or(|m| m == opts.mode))
    {
        let got = call_tree(&db, file, &e.entry, opts)
            .map(|(t, _)| to_text(&t, module))
            .unwrap_or_else(|| format!("(no function `{}`)\n", e.entry));
        println!("--- {fixture}: {} ({:?})\n{got}", e.entry, opts.mode); // --show-output
        // Inline mode lists a call in the order it is evaluated, which is how the blocks are written. Summary mode draws the graph with
        // core's renderer, which lists siblings in source order (D17, decision A): the same tree, compared with siblings in any order.
        let same = match opts.mode {
            Mode::Inline => got == e.tree,
            Mode::Summary => sorted_siblings(&got) == sorted_siblings(&e.tree),
        };
        if !same {
            report += &format!(
                "--- {fixture}: {} {opts:?}\nexpected:\n{}got:\n{}\n",
                e.entry, e.tree, got
            );
        }
    }
    report
}

#[test]
fn reads_blocks() {
    let src = "def f(): pass\n# expect:\n# main\n#   a [recursive]\n# expect:\n# other\n";
    let b = expectations(src);
    assert_eq!(b.len(), 2);
    assert_eq!(b[0].entry, "main");
    assert_eq!(b[0].tree, "main\n  a [recursive]\n");
    assert_eq!(b[1].entry, "other");

    let b = expectations("# expect inline:\n# f\n#   f [recursive]\n# expect summary:\n# f\n");
    assert_eq!(
        (b[0].mode, b[1].mode),
        (Some(Mode::Inline), Some(Mode::Summary))
    );
    assert_eq!(b[1].entry, "f");
}

#[test]
fn p01_01_harness() {
    check_both("01-functions/python/harness.py");
}

#[test]
fn p01_02_one_call() {
    check_both("01-functions/python/one_call.py");
}

#[test]
fn p01_03_locals() {
    check_both("01-functions/python/locals.py");
}

#[test]
fn p01_04_imports() {
    check_both("01-functions/python/imports.py");
    check_both("01-functions/python/imports_partial.py");
}

#[test]
fn p01_05_follow() {
    check_both("01-functions/python/follow.py");
}

#[test]
fn p01_06_recursion() {
    check_both("01-functions/python/recursion.py");
}

#[test]
fn p01_07_arguments() {
    check_both("01-functions/python/arguments.py");
}

#[test]
fn p01_08_varargs() {
    check_both("01-functions/python/varargs.py");
}

#[test]
fn p01_09_branches() {
    check_both("01-functions/python/branches.py");
}

#[test]
fn p01_10_returns() {
    check_both("01-functions/python/returns.py");
}

#[test]
fn p01_11_closures() {
    check_both("01-functions/python/closures.py");
}

#[test]
fn p01_12_callbacks() {
    check_both("01-functions/python/callbacks.py");
}
#[test]
fn p01_13_wall() {
    check_both("01-functions/python/wall.py");
    let stats = stats_for("01-functions/python/wall.py", "main", &Options::default());
    assert_eq!(stats.walks, 15);
    assert_eq!(stats.nodes, 14);
    assert_eq!(stats.contexts, 7);
}

#[test]
fn p02_01_keys() {
    check_both("02-function-summaries/python/keys.py");
    let stats = stats_for(
        "02-function-summaries/python/keys.py",
        "main",
        &Options::default(),
    );
    assert_eq!(stats.walks, 19);
    assert_eq!(stats.contexts, 11); // 12 before step 02.04: `note`'s `msg` is irrelevant
}

#[test]
fn p02_02_summaries() {
    check_both("02-function-summaries/python/summaries.py");
}

#[test]
fn p02_03_solver() {
    check_both("02-function-summaries/python/graph.py");
    let st = stats_for("02-function-summaries/python/graph.py", "main", &summary());
    assert_eq!((st.walks, st.reuse_hits), (8, 2));

    let st = stats_for("02-function-summaries/python/keys.py", "main", &summary());
    assert_eq!((st.walks, st.reuse_hits), (11, 6)); // (12, 5) before step 02.04

    let st = stats_for("01-functions/python/wall.py", "main", &summary());
    assert_eq!(st.walks, 7);
}

#[test]
fn p02_04_relevant() {
    check_both("02-function-summaries/python/relevant.py");
    let st = stats_for(
        "02-function-summaries/python/relevant.py",
        "main",
        &summary(),
    );
    assert_eq!(st.contexts, 9);
}

#[test]
fn p02_04_relevance_cases() {
    let fixture = "02-function-summaries/python/relevance_cases.py";
    let cases: &[(&str, &[&str])] = &[
        ("cond_only", &["b"]),
        ("bare_condition", &[]),
        ("attr_in_condition", &["cb"]),
        ("loop_order", &["cb"]),
        ("aug", &["cb"]),
        ("attr_store", &["cb", "obj"]),
        ("matched", &["cb"]),
        ("nested_class", &["cb"]),
        ("global_write", &["cb"]),
        ("splats", &["args"]),
    ];
    let mut wrong = String::new();
    for (func, want) in cases {
        let got = relevant_names(fixture, func);
        if got != *want {
            wrong += &format!("{func}: expected {want:?}, got {got:?}\n");
        }
    }
    assert!(wrong.is_empty(), "\n{wrong}");
}

#[test]
fn p02_05_pending() {
    check_both("02-function-summaries/python/pending.py");
}

#[test]
fn p02_06_groups() {
    check_both("02-function-summaries/python/groups.py");
}

#[test]
fn p02_06_entry_independence() {
    // The same contexts, solved from two entries: the group's members are
    // walked in a different order, and must settle on the same returns.
    let f = "02-function-summaries/python/groups.py";
    let from_expr = returns_of(f, "parse_expr", "parse_expr");
    assert_eq!(
        from_expr,
        BTreeSet::from(["groups.binop", "groups.num"].map(String::from))
    );
    for func in ["parse_expr", "parse_atom", "combine"] {
        assert_eq!(
            returns_of(f, "parse_expr", func),
            returns_of(f, "parse_atom", func),
            "{func}"
        );
    }
}

#[test]
fn p02_06_capped_group_is_incomplete() {
    let opts = Options {
        limits: Limits {
            max_rounds: 1,
            ..Limits::default()
        },
        ..summary()
    };
    let st = stats_for(
        "02-function-summaries/python/groups.py",
        "ping_entry",
        &opts,
    );
    assert_eq!(st.rounds_capped, 1);
    let st = stats_for(
        "02-function-summaries/python/groups.py",
        "ping_entry",
        &summary(),
    );
    assert_eq!(st.rounds_capped, 0);
}

#[test]
fn p02_07_closures() {
    check_both("02-function-summaries/python/closures_across.py");
    let f = "02-function-summaries/python/closures_across.py";
    let st = stats_for(f, "outer", &summary());
    assert_eq!(st.contexts, 2); // outer, and one context of walk
    let st = stats_for(f, "chained", &summary());
    assert_eq!(st.rounds_capped, 0); // the depth limit, not the round cap, ends chain's fixpoint
    assert_eq!(contexts_of(f, "chained", "chain.g"), 3);
    assert_eq!(contexts_of(f, "use_walker", "make_walker.walk"), 1);
    assert_eq!(contexts_of(f, "use_default", "with_default.walk"), 1); // Context.defaults
}

#[test]
fn p02_08_cuts() {
    check_both("02-function-summaries/python/cuts.py");
    let opts = Options {
        limits: Limits {
            max_stack: 4,
            ..Limits::default()
        },
        ..summary()
    };
    let st = stats_for("02-function-summaries/python/cuts.py", "main", &opts);
    assert_eq!(st.cuts.stack, 1); // mid under main > a > b > top
}

#[test]
fn p03_01_construct() {
    check_both("03-classes/python/construct.py");
}

#[test]
fn p03_02_methods() {
    check_both("03-classes/python/methods.py");
}
#[test]
fn p03_03_state() {
    check("03-classes/python/state.py");
}

#[test]
fn p03_04_isolation() {
    check("03-classes/python/isolation.py");
}

/// `subclass_of`'s tree would also come out right from a plain class literal.
/// This checks the `type[X]` arm itself ran.
#[test]
fn p03_04_subclass_of_arm_runs() {
    let st = stats_for("03-classes/python/isolation.py", "subclass_of", &inline());
    assert!(
        st.lifted_subclass_of > 0,
        "ty gave something other than type[Logger]"
    );
}
#[test]
fn p03_05_writes() {
    check("03-classes/python/writes.py");
}
#[test]
fn p03_06_special() {
    check("03-classes/python/special.py");
}

#[test]
fn p03_07_fanout_counted() {
    let st = stats_for("03-classes/python/chains.py", "fanout", &inline());
    assert_eq!(st.chain_fanout_hits, 1); // `b.h` is cut once; `.handle` on 8 values is under the cap
}

#[test]
fn p03_07_chains() {
    check("03-classes/python/chains.py");
}

/// Remove the `ignore` when step 03.08 is built. Covers methods, classmethods,
/// staticmethods, properties, closures at any depth, classes in functions and
/// methods, and the end-of-run rule for an escaped closure (`fire`).
#[test]
fn p03_08_entries() {
    check("03-classes/python/entries.py");
}

#[test]
fn p03_08_prologue_runs_the_parents() {
    let f = "03-classes/python/entries.py";
    let walks = |entry| stats_for(f, entry, &inline()).prologue_walks;
    assert_eq!(walks("process"), 0); // a module function: nothing to set up
    assert_eq!(walks("Reg.create"), 0); // a classmethod: no object
    assert_eq!(walks("outer.inner"), 1); // outer ran
    assert_eq!(walks("with_param.inner"), 1); // with_param ran, h from its annotation
    assert_eq!(walks("with_default.inner"), 1);
    assert_eq!(walks("siblings.inner"), 1); // helper is not run: it is only defined
    assert_eq!(walks("branches.inner"), 1);
    assert_eq!(walks("recursive.walk"), 1);
    assert_eq!(walks("a.b.c"), 2); // a, then b
    assert_eq!(walks("Svc.run"), 1); // Svc.__init__
    assert_eq!(walks("Svc.build.step"), 2); // Svc.__init__, then build
    assert_eq!(walks("Host.meth.Local.m"), 2); // Host.__init__, then meth (Local has no __init__)
}
#[test]
fn p04_01_in_keys() {
    check_both("04-class-summaries/python/in_keys.py");
    let st = stats_for("04-class-summaries/python/in_keys.py", "shared", &summary());
    assert_eq!((st.walks, st.reuse_hits), (3, 1));
}

#[test]
fn p07_accessors_run_in_summary_mode_as_they_do_inline() {
    check_both("07-accessors/python/accessors.py");
}

#[test]
fn p05_01_graph() {
    let g = graph_for("02-function-summaries/python/graph.py", "main");
    assert_eq!((g.contexts.len(), g.edges.len()), (8, 9));
}

#[test]
fn p05_01_stable_ids_ignore_discovery_order() {
    use analyzer_core::graph::ContextId;
    use std::collections::BTreeMap;
    // main calls read_a then read_b; main2 calls them in the other order,
    // so their ContextIds differ. Their stable ids must not.
    check_both("05-output/python/stable_ids.py");
    let ids = |entry: &str| -> BTreeMap<String, ContextId> {
        let g = graph_for("05-output/python/stable_ids.py", entry);
        g.contexts
            .iter()
            .map(|c| (g.def(&c.id.func).unwrap().qname.clone(), c.id.clone()))
            .filter(|(qname, _)| *qname != format!("stable_ids.{entry}")) // the entries themselves differ
            .collect()
    };
    let (one, two) = (ids("main"), ids("main2"));
    assert_eq!(one.len(), 3); // read_a, read_b, done
    assert_eq!(one, two);
}

#[test]
fn p05_02_wire_tree() {
    check_both("05-output/python/ids.py");
    check_both("05-output/python/render_cut.py");
}

#[test]
fn p05_02_merge_joins_the_calls_of_one_site_and_keeps_the_unmerged_form() {
    // `twice` calls `run` twice from one function: two nodes on the wire, one after the merge
    let (db, file) = open_fixture("02-function-summaries/python/graph.py");
    let twice = |t: &crate::wire::Frame| {
        t.children
            .iter()
            .find(|c| c.target_qname == "graph.twice")
            .unwrap()
            .clone()
    };
    let before = twice(&frame_of(&db, file, &summary(), false));
    assert_eq!(before.children.len(), 2);
    assert!(
        before
            .children
            .iter()
            .all(|c| c.call_count == 0 && c.call_site.is_some())
    );
    let after = twice(&frame_of(&db, file, &summary(), true));
    assert_eq!(after.children.len(), 1);
    assert_eq!(after.children[0].call_count, 1);
}

/// The wire of `main`, made the way a request makes it.
fn frame_of(
    db: &ProjectDatabase,
    file: ruff_db::files::File,
    opts: &Options,
    merged: bool,
) -> crate::wire::Frame {
    crate::interp::call_frame(db, file, "main", opts, merged)
        .unwrap()
        .0
}

fn styled() -> Options {
    use crate::interp::WireStyle;
    Options {
        wire: WireStyle {
            function_prefix: "F/".into(),
            class_prefix: "C/".into(),
        },
        ..summary()
    }
}

/// `(qname, target_id)` of every node, depth first.
fn ids(f: &crate::wire::Frame, out: &mut Vec<(String, String)>) {
    out.push((f.target_qname.clone(), f.target_id.clone()));
    f.children.iter().for_each(|c| ids(c, out));
}

#[test]
fn wire_ids_use_the_style_by_kind_and_are_empty_without_an_id() {
    let (db, file) = open_fixture("06-wire/python/ids_and_classes.py");
    let mut got = Vec::new();
    ids(&frame_of(&db, file, &styled(), false), &mut got);
    let want = |q: &str, id: &str| (format!("ids_and_classes.{q}"), id.to_owned());
    assert_eq!(
        got,
        [
            want("main", "F/06000000-0003-0000-0000-000000000000"),
            want("Report", "C/06000000-0001-0000-0000-000000000000"),
            want("Report.prepare", ""),
            want("marked", "F/06000000-0002-0000-0000-000000000000"),
            want("unmarked", ""),
            want("unmarked", ""),
            want("unmarked", ""),
        ]
    );
}

#[test]
fn wire_ids_are_bare_when_no_prefix_is_given() {
    let (db, file) = open_fixture("06-wire/python/ids_and_classes.py");
    let wire = frame_of(&db, file, &summary(), false);
    assert_eq!(wire.target_id, "06000000-0003-0000-0000-000000000000");
}

#[test]
fn the_marker_label_is_the_callers() {
    use analyzer_core::edit::MarkerStyle;
    let (db, file) = open_fixture("06-wire/python/ids_and_classes.py");
    // the program marks its defs `ID: …`; read under another label it has no markers, so no ids
    let other = Options {
        marker: MarkerStyle::docstring("Other"),
        ..summary()
    };
    let wire = frame_of(&db, file, &other, false);
    let mut got = Vec::new();
    ids(&wire, &mut got);
    assert!(got.iter().all(|(_, id)| id.is_empty()), "{got:?}");
    let default = frame_of(&db, file, &summary(), false);
    assert_eq!(default.target_id, "06000000-0003-0000-0000-000000000000");
}

#[test]
fn merging_joins_siblings_of_one_target_and_is_idempotent() {
    let (db, file) = open_fixture("06-wire/python/ids_and_classes.py");
    // `main` calls `unmarked` twice itself: two nodes before the merge, one after, counted
    let unmarked = |f: &crate::wire::Frame| {
        f.children
            .iter()
            .filter(|c| c.target_qname == "ids_and_classes.unmarked")
            .map(|c| c.call_count)
            .collect::<Vec<_>>()
    };
    assert_eq!(unmarked(&frame_of(&db, file, &styled(), false)), [0, 0]);
    let merged = frame_of(&db, file, &styled(), true);
    assert_eq!(unmarked(&merged), [1]);
    assert_eq!(crate::wire::merge(merged.clone()), merged);
    // the id travels with the merged node
    let node = merged
        .children
        .iter()
        .find(|c| c.target_qname == "ids_and_classes.marked")
        .unwrap();
    assert_eq!(node.target_id, "F/06000000-0002-0000-0000-000000000000");
}

/// Every `# expect:` entry of every fixture program, with the options the fixture asks for: `(fixture path under tests/fixtures, entry,
/// options)`. Summary mode, the mode the wire is made in.
fn every_entry() -> Vec<(String, String, Options)> {
    let mut out = Vec::new();
    let mut groups: Vec<_> = std::fs::read_dir("tests/fixtures")
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    groups.sort();
    for group in groups {
        let dir = group.join("python");
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<_> = files.map(|e| e.unwrap().path()).collect();
        files.sort();
        for path in files
            .into_iter()
            .filter(|p| p.extension().is_some_and(|x| x == "py"))
        {
            let source = std::fs::read_to_string(&path).unwrap();
            let blocks = expectations(&source);
            if blocks.is_empty() {
                continue;
            }
            let name = format!(
                "{}/python/{}",
                group.file_name().unwrap().to_str().unwrap(),
                path.file_name().unwrap().to_str().unwrap()
            );
            // a program no test names was never run (`deep_write_capped.py` asks for a limit that does not exist)
            if !include_str!("fixture.rs").contains(&format!("\"{name}\"")) {
                continue;
            }
            let mut opts = summary();
            if let Some((limits, ids)) = fixture_limits(&source) {
                opts.limits = limits;
                opts.only_marked = ids;
            }
            for e in blocks
                .into_iter()
                .filter(|e| e.mode.is_none_or(|m| m == Mode::Summary))
            {
                out.push((name.clone(), e.entry, opts.clone()));
            }
        }
    }
    out
}

fn open_fixture(fixture: &str) -> (ProjectDatabase, ruff_db::files::File) {
    let path = std::fs::canonicalize(format!("tests/fixtures/{fixture}")).unwrap();
    let db = open_project(SystemPath::from_std_path(path.parent().unwrap()).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    (db, file)
}

#[test]
fn the_core_graph_of_every_fixture_keeps_core_s_invariants() {
    use crate::interp::call_graph;
    let all = every_entry();
    assert!(all.len() > 40, "found only {} entries", all.len());
    for (fixture, entry, opts) in all {
        let (db, file) = open_fixture(&fixture);
        let Some(graph) = call_graph(&db, file, &entry, &opts) else {
            continue; // an entry some expectation names on purpose to be missing
        };
        let bad = analyzer_testkit::graph::violations(&graph);
        assert!(bad.is_empty(), "{fixture} {entry}:\n  {}", bad.join("\n  "));
    }
}

/// What an edge rests on. `Name`: the callee expression names a def, a class or a closure (ty types it as the function or the class itself),
/// project code or library. `Path`: a method on an object the walk built **in that walk**, and a closure called through a variable. `Type`:
/// everything the engine answers through a type: a method on an object only ty typed, a method on an input object of the context (its
/// `self`, an argument rebuilt from the key), a def that arrived as data (a parameter) and a decorated def. An `Unresolved` edge has a
/// reason, and the counter of that name counts the edges.
///
/// KNOWN LIMIT: an object a caller built and passed down is an input of the callee, so a method called on it there is `Type`, not `Path`
/// (`Pipeline.run` calling `self.handler.process()` on a handler `outside` wrote; `with_default.inner` calling `x.go()` on a default `A()`
/// built by `outer`). A key holds an object's class and attributes, not whether the walk of a caller built it. Keeping `Path` through the key
/// needs that bit in `Fp::Instance`: digests change and contexts split. Decided not now; revisit with part 04 (returned objects).
#[test]
fn edges_say_what_they_rest_on() {
    use crate::interp::call_graph;
    use analyzer_core::graph::EdgeTo;
    let edges = |fixture: &str, entry: &str| {
        let (db, file) = open_fixture(fixture);
        let g = call_graph(&db, file, entry, &summary()).unwrap();
        let from = g.entries[0].context().unwrap().clone();
        let named = |f: &analyzer_core::graph::StableDef| g.def(f).unwrap().qname.clone();
        let rows: Vec<String> = g
            .edges_from(&from)
            .iter()
            .map(|e| match &e.to {
                EdgeTo::Context(c, b) => format!("{} {b:?}", named(&c.func)),
                EdgeTo::Construct { class, basis, .. } => format!("new {} {basis:?}", named(class)),
                EdgeTo::External(x, b) => format!("{} {b:?}", x.name),
                EdgeTo::Unresolved(r) => format!("? {}", r.as_str()),
                _ => unreachable!(),
            })
            .collect();
        (rows, g.stats)
    };
    let (rows, _) = edges("03-classes/python/state.py", "read_json");
    assert_eq!(
        rows,
        [
            "new state.Pipeline Name",
            "state.Pipeline.run Path",
            "new state.JsonHandler Name"
        ]
    );
    // a def named in the code is `Name`; a decorated def, whose name ty does not type as the def, is the engine's answer
    let (rows, _) = edges("01-functions/python/callbacks.py", "main");
    assert_eq!(
        rows,
        ["callbacks.handle_ok Name", "callbacks.handle_err Name"]
    );
    let (rows, _) = edges("01-functions/python/callbacks.py", "uses_decorated");
    assert_eq!(rows, ["callbacks.flaky Type", "callbacks.flaky_with Type"]);
    // a library function named by an import is `Name` as well
    let (rows, _) = edges("01-functions/python/callbacks.py", "uses_stdlib");
    assert_eq!(rows[0], "partial Name");
    // a classmethod entry holds its class in `cls`: `cls()` is not an unknown callee, but `cls` is a `type[Self]`, not a class the code names
    let (rows, _) = edges("03-classes/python/entries.py", "Reg.create");
    assert_eq!(rows, ["new entries.Reg Type"]);
    let (rows, _) = edges("03-classes/python/chains.py", "annotated");
    assert_eq!(rows, ["chains.Handler.handle Type"]);
    let (rows, stats) = edges("03-classes/python/chains.py", "unknown_mid_chain");
    assert_eq!(rows, ["? unknown_callee"]);
    assert_eq!(stats.language["unknown_callee"], stats.unresolved);
}

/// A weak write marks what it may have dropped, and only that. `outside` writes one object (a strong write) and its answer is complete:
/// no `Unresolved` edge anywhere, in `Pipeline.run` either, where the object's attributes come back from its fingerprint (that is not a
/// write, and adds no `Unknown`). `two_candidates` writes one of two objects onto an attribute this path holds nothing for: its read is
/// an `Unresolved` site next to the written handler.
#[test]
fn a_weak_write_marks_what_it_may_have_dropped_and_a_strong_one_marks_nothing() {
    use crate::interp::call_graph;
    let unresolved = |entry: &str| {
        let (db, file) = open_fixture("03-classes/python/writes.py");
        let g = call_graph(&db, file, entry, &summary()).unwrap();
        g.edges
            .iter()
            .filter(|e| matches!(e.to, analyzer_core::graph::EdgeTo::Unresolved(_)))
            .count()
    };
    assert_eq!(unresolved("outside"), 0);
    assert!(unresolved("two_candidates") > 0);
}

/// The golden wire of a real project. Not run by default: it needs a project that is not in the repo and the files in `out/golden/`, and the
/// prefixes are V-NOC's, which no crate may spell. Run it by hand:
///
/// `PYTHON_GOLDEN_PROJECT=<dir> PYTHON_GOLDEN_FILE=<file in it> PYTHON_GOLDEN_ENTRY=<entry> PYTHON_GOLDEN_DIR=<out/golden>
///  PYTHON_GOLDEN_PREFIXES=<function prefix>,<class prefix> cargo test -p python-analysis golden -- --ignored --nocapture`
///
/// Two comparisons, for the unmerged wire and the merged one. `pylspt-wire*.json` is what pylspt gave at the pin: the new wire must have
/// its content, with siblings in any order (decision A), the one rename of the plan (`Node` to `Marked`), and **except the accessor nodes**:
/// the calls to a property's getter or setter that summary mode makes now and pylspt's did not (inline mode always made them). The
/// accessors are named by `PYTHON_GOLDEN_ACCESSORS=<qname>,<qname>`, and the nodes removed are listed (parent, node, call site) and must
/// equal `accessors.txt` / `accessors-merged.txt`, so a new accessor node, or a missing one, is a diff. `wire*.json` is this crate's own
/// wire, generated from itself: the new wire must equal it exactly, so a change is noticed to the byte. With `PYTHON_GOLDEN_EMIT=1` the
/// test prints the wire and the lists between `=====BEGIN <file>` and `=====END` lines instead of comparing them with the own files (a
/// library never writes a file: redirect and split the output to regenerate them).
#[test]
#[ignore = "needs a real project and out/golden/: see the comment"]
fn golden_wire_of_a_real_project() {
    use crate::interp::{WireStyle, call_frame};
    use serde_json::Value;
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
    let (project, file, entry, dir, prefixes) = (
        var("PYTHON_GOLDEN_PROJECT"),
        var("PYTHON_GOLDEN_FILE"),
        var("PYTHON_GOLDEN_ENTRY"),
        var("PYTHON_GOLDEN_DIR"),
        var("PYTHON_GOLDEN_PREFIXES"),
    );
    let emit = std::env::var("PYTHON_GOLDEN_EMIT").is_ok_and(|v| v == "1");
    let (function_prefix, class_prefix) = prefixes.split_once(',').expect("two prefixes");
    let db = open_project(SystemPath::new(&project)).unwrap();
    let (file, _) = open(&db, &SystemPath::new(&project).join(file)).unwrap();

    // the one rename of the plan: the key of a marked identity is `Marked`, not `Node`
    fn renamed(v: Value) -> Value {
        match v {
            Value::Object(m) => Value::Object(
                m.into_iter()
                    .map(|(k, v)| {
                        (
                            if k == "Node" { "Marked".to_owned() } else { k },
                            renamed(v),
                        )
                    })
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.into_iter().map(renamed).collect()),
            other => other,
        }
    }
    // children in any order, contexts sorted: what is left is the content
    fn canon(v: &mut Value) {
        if let Some(kids) = v.get_mut("children").and_then(Value::as_array_mut) {
            kids.iter_mut().for_each(canon);
            kids.sort_by_key(|k| k.to_string());
        }
        if let Some(c) = v.get_mut("contexts").and_then(Value::as_array_mut) {
            c.sort_by_key(|k| k.to_string());
        }
    }
    fn nodes(v: &Value) -> usize {
        1 + v["children"]
            .as_array()
            .map_or(0, |k| k.iter().map(nodes).sum())
    }
    let read = |name: &str| -> Value {
        serde_json::from_str(&std::fs::read_to_string(format!("{dir}/{name}")).unwrap()).unwrap()
    };
    let accessors_var = std::env::var("PYTHON_GOLDEN_ACCESSORS").unwrap_or_default();
    let accessors: Vec<&str> = accessors_var.split(',').filter(|a| !a.is_empty()).collect();
    // take the accessor nodes out, listing each as `parent -> node @line:column`; an accessor with calls under it is not handled here
    fn strip(v: &mut Value, accessors: &[&str], removed: &mut Vec<String>) {
        let parent = v["target_qname"].as_str().unwrap_or("").to_owned();
        let Some(kids) = v.get_mut("children").and_then(Value::as_array_mut) else {
            return;
        };
        kids.retain(|k| {
            let name = k["target_qname"].as_str().unwrap_or("");
            if !accessors.contains(&name) {
                return true;
            }
            assert!(
                k["children"].as_array().is_none_or(|c| c.is_empty()),
                "the accessor {name} has calls under it"
            );
            let at = &k["call_site"]["start"];
            removed.push(format!("{parent} -> {name} @{}:{}", at[0], at[1]));
            false
        });
        kids.iter_mut().for_each(|k| strip(k, accessors, removed));
    }

    for (merged, name) in [(false, "wire.json"), (true, "wire-merged.json")] {
        let mut opts = summary();
        opts.wire = WireStyle {
            function_prefix: function_prefix.into(),
            class_prefix: class_prefix.into(),
        };
        let (new, _) = call_frame(&db, file, &entry, &opts, merged).unwrap();
        let new = serde_json::to_value(&new).unwrap();

        let reference = renamed(read(&format!("pylspt-{name}")));
        // pylspt's wire has not the fields that name a target by file, qualname, occurrence and kind (D20): this crate's own golden has them,
        // and the comparison with pylspt is on what both have
        fn without_target_names(v: &mut Value) {
            if let Some(o) = v.as_object_mut() {
                for key in [
                    "target_file",
                    "target_qualname",
                    "target_occurrence",
                    "target_kind",
                    "call_site_file",
                ] {
                    o.remove(key);
                }
            }
            if let Some(kids) = v.get_mut("children").and_then(Value::as_array_mut) {
                kids.iter_mut().for_each(without_target_names);
            }
        }
        let mut stripped = new.clone();
        without_target_names(&mut stripped);
        let mut removed = Vec::new();
        strip(&mut stripped, &accessors, &mut removed);
        let (mut a, mut b) = (reference.clone(), stripped);
        canon(&mut a);
        canon(&mut b);
        println!(
            "{name}: {} nodes from pylspt, {} nodes new, {} of them accessor nodes | same content in any sibling order, accessors aside: {}",
            nodes(&reference),
            nodes(&new),
            removed.len(),
            a == b
        );
        assert!(
            a == b,
            "{name}: differs from pylspt's beyond sibling order and the accessor nodes"
        );

        let list_file = if merged {
            "accessors-merged.txt"
        } else {
            "accessors.txt"
        };
        let list = removed.join("\n") + "\n";
        if emit {
            println!(
                "=====BEGIN {name}\n{}\n=====END\n=====BEGIN {list_file}\n{list}=====END",
                serde_json::to_string_pretty(&new).unwrap()
            );
        } else {
            let own = read(name);
            println!("{name}: equal to this crate's own golden: {}", own == new);
            assert!(own == new, "{name}: differs from this crate's own golden");
            let listed = std::fs::read_to_string(format!("{dir}/{list_file}")).unwrap();
            assert_eq!(listed, list, "{list_file}: the accessor nodes changed");
        }
    }
}

/// A wire as reviewed text: one line per node, two spaces per level: the name, `[marks]`, the id when there is one, `xN` for a merged
/// node (N calls joined), and where the call is as `@line:column`. No digests: the golden of a real project pins those.
fn wire_text(frame: &crate::wire::Frame) -> String {
    let mut out = String::new();
    let mut stack = vec![(frame, 0usize)];
    while let Some((f, depth)) = stack.pop() {
        out.push_str(&"  ".repeat(depth));
        out.push_str(&f.target_qname);
        // what the frame says its target is: (file, outline qualname, occurrence) and the kind; nothing for an external or an unresolved leaf
        if let Some(file) = &f.target_file {
            out.push_str(&format!(
                " <{file}:{}#{} {:?}>",
                f.target_qualname,
                f.target_occurrence,
                f.target_kind.as_ref().expect("a row has a kind")
            ));
        }
        for m in &f.marks {
            out.push_str(&format!(" [{m}]"));
        }
        if !f.target_id.is_empty() {
            out.push_str(&format!(" id={}", f.target_id));
        }
        if f.call_count > 0 {
            out.push_str(&format!(" x{}", f.call_count + 1));
        }
        if let Some(site) = f.call_site {
            out.push_str(&format!(" @{}:{}", site.start.0, site.start.1));
        }
        if let Some(file) = &f.call_site_file {
            out.push_str(&format!(" in {file}"));
        }
        out.push('\n');
        stack.extend(f.children.iter().rev().map(|c| (c, depth + 1)));
    }
    out
}

/// The wire of every `# expect:` entry of every fixture a test runs, unmerged and merged, equals its snapshot. One file per program,
/// `tests/snapshots/<fixture>.wire`; `ANALYZER_BLESS=1` rewrites them, and the diff is what you review.
///
/// A program the suite checks in inline mode only (`check`, "until part 04") is one summary mode cannot answer completely yet, and the wire
/// is made in summary mode. Its snapshot is in `tests/snapshots/unverified/`, headed as such: it detects a change, it does not bless an
/// answer. Moving a program to `check_both` and re-blessing moves its snapshot out of that folder.
#[test]
fn the_wire_of_every_fixture_equals_its_snapshot() {
    use crate::interp::call_frame;
    let this_file = include_str!("fixture.rs");
    // (the program, whether summary mode answers it completely, the text)
    let mut by_fixture: Vec<(String, bool, String)> = Vec::new();
    for (fixture, entry, opts) in every_entry() {
        let verified = !this_file.contains(&format!("check(\"{fixture}\")"));
        let (db, file) = open_fixture(&fixture);
        let Some((plain, _)) = call_frame(&db, file, &entry, &opts, false) else {
            continue;
        };
        let (merged, _) = call_frame(&db, file, &entry, &opts, true).unwrap();
        let text = format!(
            "== {entry}\n{}\n== {entry} (merged)\n{}\n",
            wire_text(&plain),
            wire_text(&merged)
        );
        match by_fixture.last_mut() {
            Some((name, _, all)) if *name == fixture => all.push_str(&text),
            _ => by_fixture.push((fixture, verified, text)),
        }
    }
    // 29 programs are verified in both modes and 6 only inline; 35 are run at all; 43 have `# expect:` blocks (eight were never run,
    // even at the pin: `every_entry` skips them)
    let verified = by_fixture.iter().filter(|(_, v, _)| *v).count();
    assert_eq!(
        (verified, by_fixture.len() - verified),
        (29, 6),
        "a program was added or lost"
    );
    for (fixture, verified, text) in by_fixture {
        let dir = if verified {
            "tests/snapshots".to_owned()
        } else {
            "tests/snapshots/unverified".to_owned()
        };
        let text = if verified {
            text
        } else {
            format!(
                "# UNVERIFIED: summary mode does not answer this program completely yet (class summaries, part 04).\n\
                 # This pins what it says today so a change is noticed; it is not a right answer.\n\n{text}"
            )
        };
        let path = std::path::Path::new(&dir).join(format!("{fixture}.wire"));
        analyzer_testkit::expect_file(&path, &text);
    }
}

/// A graph as reviewed text: every def row with the outline's name for it (D20), then each context with its digest, and under it every edge
/// with what it rests on. The wire and the tree leave out
/// what 570ab69 got wrong (a spurious `Unresolved` edge: the tree does not draw one), so this is the snapshot that sees edges, bases, reasons
/// and the contexts' digests. A digest is shown by its first 8 bytes.
fn graph_text(g: &analyzer_core::graph::Graph) -> String {
    use analyzer_core::graph::{ContextId, EdgeTo, Root, StableDef};
    use std::fmt::Write;
    let name = |f: &StableDef| g.def(f).map_or_else(|| "?".to_owned(), |d| d.qname.clone());
    let ctx = |c: &ContextId| {
        let digest: String = c.digest[..8].iter().map(|b| format!("{b:02x}")).collect();
        format!("{}#{digest}", name(&c.func))
    };
    let mut out = String::new();
    let entries: Vec<String> = g
        .entries
        .iter()
        .map(|root| match root {
            Root::Context(id) => ctx(id),
            Root::Construct { class, .. } => format!("new {}", name(class)),
            _ => "?".to_owned(),
        })
        .collect();
    writeln!(out, "entries: {}", entries.join(", ")).unwrap();
    let s = &g.stats;
    writeln!(
        out,
        "stats: walks {}, contexts {}, reuse_hits {}, unresolved {}, cuts {:?}, language {:?}",
        s.walks, s.contexts, s.reuse_hits, s.unresolved, s.cuts, s.language
    )
    .unwrap();
    for d in &g.defs {
        writeln!(
            out,
            "def {} = {}#{} {:?} {}",
            d.qname, d.qualname, d.occurrence, d.kind, d.file
        )
        .unwrap();
    }
    for c in &g.contexts {
        writeln!(out, "{} {:?}", ctx(&c.id), c.status).unwrap();
        for e in g.edges_from(&c.id) {
            let to = match &e.to {
                EdgeTo::Context(to, b) => format!("{} {b:?}", ctx(to)),
                EdgeTo::Construct { class, init, basis } => format!(
                    "new {} init {} {basis:?}",
                    name(class),
                    init.as_ref().map_or_else(|| "-".to_owned(), ctx)
                ),
                EdgeTo::External(x, b) => format!("external {}.{} {b:?}", x.package, x.name),
                EdgeTo::Unresolved(r) => format!("? {}", r.as_str()),
                _ => unreachable!(),
            };
            // a def handed over, not called (D22): said only when it is, so a snapshot of a graph with none is what it was
            let passed = if e.passed { " passed" } else { "" };
            writeln!(out, "  @{}-{} -> {to}{passed}", e.site.start, e.site.end).unwrap();
        }
    }
    out
}

/// The graph of one entry of a real project, as `graph_text` writes it, on stdout between `=====BEGIN graph` and `=====END` lines. For
/// reading what summary mode says about code that is not in the repo (Jarvis, FastAPI); nothing is compared:
/// `PYTHON_DUMP_PROJECT=<dir> PYTHON_DUMP_FILE=<file in it> PYTHON_DUMP_ENTRY=<entry> cargo test -p python-analysis dump_graph -- --ignored --nocapture`
#[test]
#[ignore = "needs a real project: see the comment"]
fn dump_graph_of_a_real_project() {
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
    let (project, file, entry) = (
        var("PYTHON_DUMP_PROJECT"),
        var("PYTHON_DUMP_FILE"),
        var("PYTHON_DUMP_ENTRY"),
    );
    let db = open_project(SystemPath::new(&project)).unwrap();
    let (file, _) = open(&db, &SystemPath::new(&project).join(file)).unwrap();
    // `PYTHON_DUMP_EFFECTS=1`: with the constructor effects of part 04, step 3 (the spike)
    let opts = Options {
        constructor_effects: std::env::var("PYTHON_DUMP_EFFECTS").is_ok(),
        ..summary()
    };
    let graph = call_graph(&db, file, &entry, &opts).unwrap();
    println!("=====BEGIN graph\n{}=====END", graph_text(&graph));
    if std::env::var("PYTHON_DUMP_UNRESOLVED").is_ok() {
        dump_unresolved_sites(&db, &project, &graph);
    }
    if std::env::var("PYTHON_DUMP_GAP").is_err() {
        return;
    }
    // with `PYTHON_DUMP_GAP=1`: inline mode's tree against summary mode's, and the topmost missing nodes at no `Unresolved` site, each with
    // the source line and what ty says the receiver is there (the type of `x` in `x.m(...)` and `x.attr`, else of the callee)
    use analyzer_core::render::{RenderOptions, render};
    use ruff_python_ast::{AnyNodeRef, Expr, ExprRef, find_node::covering_node};
    let inline_opts = Options {
        mode: Mode::Inline,
        ..summary()
    };
    let inline = call_tree(&db, file, &entry, &inline_opts).unwrap().0;
    let tree = render(&graph, &graph.entries[0], &RenderOptions::default());
    let mut gap = Gap::default();
    compare_with_inline(&entry, &inline, &tree, &graph, &mut gap);
    fn size(n: &analyzer_core::render::Node) -> usize {
        1 + n.children.iter().map(size).sum::<usize>()
    }
    fn inline_size(n: &crate::interp::tree::Node) -> usize {
        1 + n.children.iter().map(inline_size).sum::<usize>()
    }
    println!(
        "=====BEGIN gap\nnodes: summary {}, inline {}",
        size(&tree),
        inline_size(&inline)
    );
    println!(
        "{} nodes missing, {} topmost ({} at an Unresolved site, {} silent), {} extra",
        gap.missing,
        gap.highest_missing,
        gap.at_unresolved,
        gap.silent.len(),
        gap.extra
    );
    let sem = Sem::new(&db, Options::default().marker);
    for (rel, range, qname) in &gap.silent {
        let path = SystemPath::new(&project).join(rel);
        let (file, program_file) = open(&db, &path).unwrap();
        let text = ruff_db::source::source_text(&db, file);
        let line = text.as_str()[..usize::from(range.start())]
            .matches('\n')
            .count()
            + 1;
        let parsed = ruff_db::parsed::parsed_module(&db, program_file.python_file(&db)).load(&db);
        let receiver = match covering_node(AnyNodeRef::from(parsed.syntax()), *range).node() {
            AnyNodeRef::ExprCall(c) => match &*c.func {
                Expr::Attribute(a) => Some(&*a.value),
                f => Some(f),
            },
            AnyNodeRef::ExprAttribute(a) => Some(&*a.value),
            _ => None,
        };
        // a class by its qualified name and whether it is the project's: `ModelField` of pydantic and of the project are two classes
        let shown = |t| {
            crate::sem::Type::display(
                t,
                &db,
                &crate::sem::ProgramEnvironment::from_file(program_file),
            )
            .to_string()
        };
        let describe = |t| {
            let class = |d| {
                let whose = if sem.is_project(d) {
                    "project"
                } else {
                    "library"
                };
                format!("{} ({whose})", sem.qname(d))
            };
            match sem.shape(t, program_file) {
                crate::sem::Shape::Instance(d) => class(d),
                crate::sem::Shape::Union(members) => members
                    .into_iter()
                    .map(|m| match sem.shape(m, program_file) {
                        crate::sem::Shape::Instance(d) => class(d),
                        _ => shown(m),
                    })
                    .collect::<Vec<_>>()
                    .join(" | "),
                other => {
                    let kind = match other {
                        crate::sem::Shape::Unknown => "Unknown",
                        crate::sem::Shape::Class(_) => "Class",
                        crate::sem::Shape::SubclassOf(_) => "SubclassOf",
                        crate::sem::Shape::Alias(_) => "Alias",
                        crate::sem::Shape::Module => "Module",
                        crate::sem::Shape::Function(_) => "Function",
                        crate::sem::Shape::BoundMethod { .. } => "BoundMethod",
                        _ => "Other",
                    };
                    format!("{} [shape {kind}]", shown(t))
                }
            }
        };
        let ty = receiver.map_or_else(
            || "?".to_owned(),
            |e| describe(sem.type_of(&sem.model(program_file), ExprRef::from(e))),
        );
        println!("{rel}:{line} {qname} <- receiver: {ty}");
    }
    println!("=====END");
}

/// With `PYTHON_DUMP_UNRESOLVED=1`: every `Unresolved` edge of `graph` with what ty says the callee expression is, so that "ty could have
/// answered here" is a count and not a guess. One line per edge: `file:line callee-source <- ty: <what ty makes of the callee>`, and a
/// tally of the answers: a def of the project, a def of a library, a type that names no def, and nothing (`Unknown`).
fn dump_unresolved_sites(db: &ProjectDatabase, project: &str, graph: &analyzer_core::graph::Graph) {
    use analyzer_core::graph::EdgeTo;
    use ruff_python_ast::{AnyNodeRef, Expr, ExprRef, find_node::covering_node};
    let sem = Sem::new(db, Options::default().marker);
    let mut tally: std::collections::BTreeMap<String, usize> = Default::default();
    println!("=====BEGIN unresolved");
    for e in graph
        .edges
        .iter()
        .filter(|e| matches!(e.to, EdgeTo::Unresolved(_)))
    {
        let Some(def) = graph.def(&e.from.func) else {
            continue;
        };
        let start = def.range.start + e.site.start;
        let end = def.range.start + e.site.end;
        let range = TextRange::new(start.into(), end.into());
        let (file, program_file) = open(db, &SystemPath::new(project).join(&def.file)).unwrap();
        let text = ruff_db::source::source_text(db, file);
        let line = text.as_str()[..usize::from(range.start())]
            .matches('\n')
            .count()
            + 1;
        let parsed = ruff_db::parsed::parsed_module(db, program_file.python_file(db)).load(db);
        let callee: Option<&Expr> =
            match covering_node(AnyNodeRef::from(parsed.syntax()), range).node() {
                AnyNodeRef::ExprCall(c) => Some(&*c.func),
                _ => None,
            };
        let (answer, shown) = match callee {
            None => ("not a call".to_owned(), String::new()),
            Some(expr) => {
                let t = sem.type_of(&sem.model(program_file), ExprRef::from(expr));
                let members = match sem.shape(t, program_file) {
                    crate::sem::Shape::Union(m) => m,
                    _ => vec![t],
                };
                let kinds: std::collections::BTreeSet<&str> = members
                    .iter()
                    .map(|&m| match sem.shape(m, program_file) {
                        crate::sem::Shape::Unknown => "nothing (Unknown)",
                        _ => match sem.callee_definition(m, program_file) {
                            Some(d) if sem.is_project(d) => "a def of the project",
                            Some(_) => "a def of a library",
                            None => "a type that names no def",
                        },
                    })
                    .collect();
                let shown = crate::sem::Type::display(
                    t,
                    db,
                    &crate::sem::ProgramEnvironment::from_file(program_file),
                )
                .to_string();
                (kinds.into_iter().collect::<Vec<_>>().join(" + "), shown)
            }
        };
        *tally.entry(answer.clone()).or_default() += 1;
        let source = &text.as_str()[usize::from(range.start())..usize::from(range.end())];
        let source = source
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(60)
            .collect::<String>();
        println!("{}:{line} {source} <- ty: {answer} [{shown}]", def.file);
    }
    for (answer, n) in &tally {
        println!("TALLY {n:4} {answer}");
    }
    println!("=====END");
}

/// E1: every row of `graph.defs` names a def the outline of its file has: the `occurrence`th def with that `qualname`, and it is the same
/// def (the same kind, as the outline says it, and the same range). This is the one naming rule a request and an answer share (D20), so a row that is wrong is the bug.
#[test]
fn every_def_row_of_every_graph_names_a_def_of_its_files_outline() {
    use crate::interp::call_graph;
    let mut rows = 0;
    for (fixture, entry, opts) in every_entry() {
        let (db, file) = open_fixture(&fixture);
        let Some(graph) = call_graph(&db, file, &entry, &opts) else {
            continue;
        };
        let root = std::fs::canonicalize(format!("tests/fixtures/{fixture}")).unwrap();
        let root = root.parent().unwrap();
        for d in &graph.defs {
            let text = std::fs::read_to_string(root.join(&d.file))
                .unwrap_or_else(|_| panic!("{fixture} {entry}: no file {}", d.file));
            let outline = python_syntax::outline(&text, opts.marker);
            let same_name: Vec<_> = outline
                .defs
                .iter()
                .filter(|o| o.qualname == d.qualname)
                .collect();
            let o = same_name.get(usize::from(d.occurrence)).unwrap_or_else(|| {
                panic!(
                    "{fixture} {entry}: `{}` occurrence {} is not in the outline of {} ({} defs of that name)",
                    d.qualname,
                    d.occurrence,
                    d.file,
                    same_name.len()
                )
            });
            assert_eq!(
                (o.kind.clone(), o.range),
                (d.kind.clone(), d.range),
                "{fixture} {entry}: `{}` occurrence {} is another def than the row says",
                d.qualname,
                d.occurrence
            );
            rows += 1;
        }
    }
    assert!(rows > 300, "only {rows} rows were checked");
}

/// E1: `occurrence` counts like the outline, `StableDef::Path::nth` does not. A function `top` with a nested `inner` and a class `top` with a
/// method `inner`: the outline calls them `top.<locals>.inner` and `top.inner`, occurrence 0 for both, and `nth_of` counts both under
/// `top.inner`, so their `nth` is 0 and 1. `nth` is hashed into every context digest (an identity outside the process): it is not touched, and
/// this test pins the difference D18 had called "the same number".
#[test]
fn occurrence_is_the_outlines_count_and_nth_is_the_older_count_by_the_name_without_locals() {
    use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal, walk_body};
    use ruff_python_ast::{AnyNodeRef, StmtFunctionDef};
    struct Inners<'a>(Vec<&'a StmtFunctionDef>);
    impl<'a> SourceOrderVisitor<'a> for Inners<'a> {
        fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
            if let AnyNodeRef::StmtFunctionDef(f) = node
                && f.name.as_str() == "inner"
            {
                self.0.push(f);
            }
            TraversalSignal::Traverse
        }
    }
    let (db, file) = open_fixture("08-entries/python/nth.py");
    use ty_python_semantic::Db as _;
    let program_file = db.program_file(file);
    let parsed = ruff_db::parsed::parsed_module(&db, program_file.python_file(&db)).load(&db);
    let mut inners = Inners(Vec::new());
    walk_body(&mut inners, &parsed.syntax().body);
    assert_eq!(inners.0.len(), 2);
    let sem = Sem::new(&db, Options::default().marker);
    let model = sem.model(program_file);
    let rows: Vec<_> = inners
        .0
        .iter()
        .map(|f| {
            let def = sem.definition_of(&model, f);
            let nth = match sem.stable_def(def) {
                analyzer_core::graph::StableDef::Path { nth, .. } => nth,
                other => panic!("no marker in the fixture, got {other:?}"),
            };
            let named = sem.outline_name(def).unwrap();
            ((named.qualname, named.occurrence), nth)
        })
        .collect();
    assert_eq!(
        rows,
        [
            (("top.<locals>.inner".to_owned(), 0), 0),
            (("top.inner".to_owned(), 0), 1)
        ]
    );
}

/// The graph of every `# expect:` entry of every fixture a test runs, in summary mode, equals its snapshot: `tests/snapshots/<fixture>.graph`,
/// blessed with `ANALYZER_BLESS=1` like the wire's. This is the net under a change to the solver or to the rule that gives an edge its basis:
/// the diff of a re-bless is every edge, basis, reason and digest the change moved.
#[test]
fn the_graph_of_every_fixture_equals_its_snapshot() {
    use crate::interp::call_graph;
    let mut by_fixture: Vec<(String, String)> = Vec::new();
    let mut entries = 0;
    for (fixture, entry, opts) in every_entry() {
        let (db, file) = open_fixture(&fixture);
        let Some(graph) = call_graph(&db, file, &entry, &opts) else {
            continue;
        };
        entries += 1;
        let text = format!("== {entry}\n{}\n", graph_text(&graph));
        match by_fixture.last_mut() {
            Some((name, all)) if *name == fixture => all.push_str(&text),
            _ => by_fixture.push((fixture, text)),
        }
    }
    assert_eq!(
        (by_fixture.len(), entries),
        (35, 129),
        "an entry was added or lost"
    );
    for (fixture, text) in by_fixture {
        let path = std::path::Path::new("tests/snapshots").join(format!("{fixture}.graph"));
        analyzer_testkit::expect_file(&path, &text);
    }
}

/// Where summary mode misses what inline mode finds, and what the graph says at that place. Inline mode's tree is the oracle (it is what the
/// `# expect:` blocks of these programs are), and it knows the call site of every node.
#[derive(Default)]
struct Gap {
    /// nodes inline finds and summary does not, counting everything under a missing node
    missing: usize,
    /// the topmost missing nodes: the ones whose own call is not in the summary tree
    highest_missing: usize,
    /// of those, how many sit at a call site that is an `Unresolved` edge in the summary graph
    at_unresolved: usize,
    /// the missing nodes (a topmost one and what is under it) split by where the topmost one sits
    nodes_at_unresolved: usize,
    nodes_elsewhere: usize,
    /// the ones that do not, as `entry: path -> node`
    not_at_unresolved: Vec<String>,
    /// the same nodes with where they are: the file (relative to the project), the call expression's range, the node's name
    silent: Vec<(String, TextRange, String)>,
    /// nodes summary has and inline does not, counting what is under them: wrong edges, and nothing marks them
    extra: usize,
    extras: Vec<String>,
}

fn compare_with_inline(
    path: &str,
    inline: &crate::interp::tree::Node,
    summary: &analyzer_core::render::Node,
    graph: &analyzer_core::graph::Graph,
    gap: &mut Gap,
) {
    use analyzer_core::graph::{EdgeTo, Site};
    fn inline_size(n: &crate::interp::tree::Node) -> usize {
        1 + n.children.iter().map(inline_size).sum::<usize>()
    }
    fn summary_size(n: &analyzer_core::render::Node) -> usize {
        1 + n.children.iter().map(summary_size).sum::<usize>()
    }
    // the call sites under this node were made in the function its contexts are of
    let def_start = summary
        .contexts
        .first()
        .and_then(|c| graph.def(&c.func))
        .map(|d| d.range.start);
    let mut taken = vec![false; summary.children.len()];
    for child in &inline.children {
        let found = summary
            .children
            .iter()
            .enumerate()
            .position(|(k, s)| !taken[k] && s.qname == child.qname);
        match found {
            Some(k) => {
                taken[k] = true;
                compare_with_inline(
                    &format!("{path}/{}", child.qname),
                    child,
                    &summary.children[k],
                    graph,
                    gap,
                );
            }
            None => {
                gap.missing += inline_size(child);
                gap.highest_missing += 1;
                let site = (|| {
                    let range = child.site?;
                    let start = def_start?;
                    Some(Site::new(
                        u32::from(range.start()).checked_sub(start)?,
                        u32::from(range.end()).checked_sub(start)?,
                    ))
                })();
                let unresolved = site.is_some_and(|site| {
                    graph.edges.iter().any(|e| {
                        summary.contexts.contains(&e.from)
                            && e.site == site
                            && matches!(e.to, EdgeTo::Unresolved(_))
                    })
                });
                if unresolved {
                    gap.at_unresolved += 1;
                    gap.nodes_at_unresolved += inline_size(child);
                } else {
                    gap.nodes_elsewhere += inline_size(child);
                    gap.not_at_unresolved
                        .push(format!("{path} -> {}", child.qname));
                    if let (Some(range), Some(def)) = (
                        child.site,
                        summary.contexts.first().and_then(|c| graph.def(&c.func)),
                    ) {
                        gap.silent
                            .push((def.file.clone(), range, child.qname.clone()));
                    }
                }
            }
        }
    }
    for (k, s) in summary.children.iter().enumerate() {
        if !taken[k] {
            gap.extra += summary_size(s);
            gap.extras.push(format!("{path} -> {}", s.qname));
        }
    }
}

/// The spike of part 04, step 3 (`Options::constructor_effects`, off by default). With it on: (1) every program verified in both modes still
/// gives its tree (the safety net), (2) the class programs' gaps against inline mode, entry by entry, with it off and on, and what moved.
#[test]
fn constructor_effects_spike() {
    use crate::interp::call_graph;
    use analyzer_core::render::{RenderOptions, render};
    let this_file = include_str!("fixture.rs");
    let with_effects = |o: &Options| Options {
        constructor_effects: true,
        ..o.clone()
    };

    // 1. programs verified in both modes: the trees do not change
    let mut seen = BTreeSet::new();
    for (fixture, _, opts) in every_entry() {
        if this_file.contains(&format!("check(\"{fixture}\")")) || !seen.insert(fixture.clone()) {
            continue;
        }
        let report = failures(&fixture, &with_effects(&opts));
        assert!(
            report.is_empty(),
            "effects broke a verified program:\n{report}"
        );
    }

    // 2. the class programs: what is missing against inline mode, per entry
    let gap_of = |effects: bool| {
        let mut rows: Vec<(String, usize, usize, Gap)> = Vec::new();
        for (fixture, entry, opts) in every_entry() {
            if !this_file.contains(&format!("check(\"{fixture}\")")) {
                continue;
            }
            let (db, file) = open_fixture(&fixture);
            let inline_opts = Options {
                mode: Mode::Inline,
                ..opts.clone()
            };
            let opts = if effects { with_effects(&opts) } else { opts };
            let inline = call_tree(&db, file, &entry, &inline_opts).unwrap().0;
            let graph = call_graph(&db, file, &entry, &opts).unwrap();
            let summary = render(&graph, &graph.entries[0], &RenderOptions::default());
            let mut gap = Gap::default();
            compare_with_inline(&entry, &inline, &summary, &graph, &mut gap);
            let applied = graph
                .stats
                .language
                .get("effects_applied")
                .copied()
                .unwrap_or(0) as usize;
            let dropped = graph
                .stats
                .language
                .get("effects_dropped")
                .copied()
                .unwrap_or(0) as usize;
            rows.push((format!("{fixture} {entry}"), applied, dropped, gap));
        }
        rows
    };
    let (off, on) = (gap_of(false), gap_of(true));
    let sum = |rows: &[(String, usize, usize, Gap)]| {
        (
            rows.iter()
                .filter(|r| r.3.missing > 0 || r.3.extra > 0)
                .count(),
            rows.iter().map(|r| r.3.missing).sum::<usize>(),
            rows.iter().map(|r| r.3.highest_missing).sum::<usize>(),
            rows.iter().map(|r| r.3.extra).sum::<usize>(),
        )
    };
    println!(
        "EFFECTS off: (failing entries, missing nodes, topmost, extra) = {:?}",
        sum(&off)
    );
    println!(
        "EFFECTS on:  (failing entries, missing nodes, topmost, extra) = {:?}",
        sum(&on)
    );
    for ((name, _, _, before), (_, applied, dropped, after)) in off.iter().zip(&on) {
        let (b, a) = (before.missing + before.extra, after.missing + after.extra);
        if (before.missing, before.extra) != (after.missing, after.extra) {
            println!(
                "EFFECTS moved {name}: missing {} -> {}, extra {} -> {} ({applied} effects applied, {dropped} dropped){}",
                before.missing,
                after.missing,
                before.extra,
                after.extra,
                if a > b { "  <-- WORSE" } else { "" }
            );
        }
    }
    let two_alive = on
        .iter()
        .find(|r| r.0 == "03-classes/python/isolation.py two_alive")
        .expect("isolation.two_alive is run");
    assert_eq!(
        (two_alive.3.missing, two_alive.3.extra),
        (0, 0),
        "isolation.two_alive is green with effects on"
    );
}

/// The six programs summary mode cannot answer completely (class summaries, part 04 of pylspt: steps 2 to 8 are not built). For every
/// entry of them, against inline mode: what is missing, whether each topmost missing node sits at a call site that the summary graph marks
/// `Unresolved`, and what summary mode has that inline mode does not. The numbers are pinned: a fix moves them, and the diff is the review.
#[test]
fn what_summary_mode_misses_on_the_class_programs_and_what_the_graph_says_there() {
    use crate::interp::call_graph;
    use analyzer_core::render::{RenderOptions, render};
    let this_file = include_str!("fixture.rs");
    let (mut entries, mut failing, mut total) = (0, 0, Gap::default());
    for (fixture, entry, opts) in every_entry() {
        if !this_file.contains(&format!("check(\"{fixture}\")")) {
            continue; // verified in both modes: the snapshots are its guard
        }
        entries += 1;
        let (db, file) = open_fixture(&fixture);
        let inline_opts = Options {
            mode: Mode::Inline,
            ..opts.clone()
        };
        let inline = call_tree(&db, file, &entry, &inline_opts).unwrap().0;
        let graph = call_graph(&db, file, &entry, &opts).unwrap();
        let summary = render(&graph, &graph.entries[0], &RenderOptions::default());
        let mut gap = Gap::default();
        compare_with_inline(&entry, &inline, &summary, &graph, &mut gap);
        failing += usize::from(gap.missing > 0 || gap.extra > 0);
        total.missing += gap.missing;
        total.highest_missing += gap.highest_missing;
        total.at_unresolved += gap.at_unresolved;
        total.nodes_at_unresolved += gap.nodes_at_unresolved;
        total.nodes_elsewhere += gap.nodes_elsewhere;
        total.extra += gap.extra;
        for line in gap.not_at_unresolved {
            total.not_at_unresolved.push(format!("{fixture} {line}"));
        }
        for line in gap.extras {
            total.extras.push(format!("{fixture} {line}"));
        }
    }
    println!(
        "{entries} entries, {failing} fail: {} nodes missing ({} under an Unresolved call site, {} elsewhere), {} topmost, {} of those at an Unresolved call site; {} extra",
        total.missing,
        total.nodes_at_unresolved,
        total.nodes_elsewhere,
        total.highest_missing,
        total.at_unresolved,
        total.extra
    );
    assert_eq!((entries, failing), (48, 21));
    assert_eq!(total.missing, 44, "nodes inline finds and summary does not");
    assert_eq!(total.highest_missing, 36, "the topmost of them");
    assert_eq!(
        total.at_unresolved, 33,
        "topmost missing nodes at a call site the graph marks Unresolved"
    );
    assert_eq!(
        (total.nodes_at_unresolved, total.nodes_elsewhere),
        (40, 4),
        "of the missing nodes: under an Unresolved call site, and elsewhere"
    );
    assert_eq!(total.missing, 40 + 4);

    // The claim "every missing node is under an Unresolved call site" is false, and these three are why. Property accessors: summary mode
    // makes the call when it knows the receiver (`Box.handler` in `swap` is back), but not here: `r` comes from a classmethod that
    // returns an object, which summary mode cannot follow (step 2 of part 04), so the getter has no receiver to run on and there is no
    // edge. `either`: inline lists one Pipeline.run per object, summary joins them (rule 7), so it is not lost information. A fix removes
    // lines from this list; a regression adds one.
    //
    // Weak writes used to be on it (`maybe_opaque`, `two_candidates`): a write that may be on one of several objects, onto an attribute
    // this path holds nothing for, kept only the written handler and dropped the other with no mark. `bind_attr` now joins `Unknown`, so
    // the read is an Unresolved site next to the written handler. That marks the alternative, it does not find it: both programs still
    // miss a node, and constructor effects (part 04, step 3) are what find it.
    //
    // KNOWN LIMIT, unmarked: `Env::join` (`union_into`) treats a name one arm never assigned as nothing, not as `Unknown`. A write on one
    // branch of an `if` onto an object whose attribute the other branch's path holds nothing for is joined without a mark. Marking it
    // would add an Unresolved edge to every read of an attribute a branch happens to write, so it is left until constructor effects make
    // an absent attribute the exception.
    assert_eq!(
        total.not_at_unresolved,
        [
            "03-classes/python/special.py main -> special.Registry.active",
            "03-classes/python/special.py main -> special.Registry.get_h",
            "03-classes/python/state.py either -> state.Pipeline.run",
        ]
    );
    // Edges summary mode has and inline mode does not. All come from ty's static type, the union of every assignment in the class (`Job.h`
    // is A in __init__ and B in configure) or in the function (`outer`'s h is A, then B), used where the receiver was not tracked along
    // the path, and all are ordinary unmarked edges.
    assert_eq!(total.extra, 2);
    let (excluded, errors): (Vec<&String>, Vec<&String>) = total
        .extras
        .iter()
        .partition(|e| e.ends_with("Job.run -> entries.B.go"));
    // Reachable, and excluded only by the entry rule: at a method entry "only __init__ is part of the start", but `configure` may have run
    // before `run`. The rule stays as it is, and this edge is not an error.
    assert_eq!(
        excluded,
        ["03-classes/python/entries.py Job.run -> entries.B.go"]
    );
    // An error: `h` is B whenever `inner` runs, because it can only run after `outer` has finished. A.go is unreachable.
    assert_eq!(
        errors,
        ["03-classes/python/entries.py outer.inner -> entries.A.go"]
    );
}
