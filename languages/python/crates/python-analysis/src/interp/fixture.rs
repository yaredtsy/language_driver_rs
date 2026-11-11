// src/interp/fixture.rs
use crate::interp::bind::param_names;
use crate::interp::relevant::relevant_params;
use crate::interp::summary::{PlainVal, SVal};
use crate::interp::{
    GraphOut, Limits, Mode, Options, Stats, call_graph, call_tree, solve, to_text,
};
use crate::sem::Sem;
use crate::sem::{ProjectDatabase, open, open_project};
use ruff_db::system::SystemPath;
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
    let sem = Sem::new(&db);
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
pub(super) fn graph_for(fixture: &str, entry: &str) -> GraphOut {
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
        r.require_ids = ids;
    }
    check_with(fixture, &runs);
}

/// One run per `Options`. Every failing block of every run goes in one report.
fn check_with(fixture: &str, runs: &[Options]) {
    let report: String = runs.iter().map(|opts| failures(fixture, opts)).collect();
    assert!(report.is_empty(), "\n{report}");
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
        if got != e.tree {
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
    check("02-function-summaries/python/keys.py");
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
    check("03-classes/python/construct.py"); // inline mode only, until part 04
}

#[test]
fn p03_02_methods() {
    check("03-classes/python/methods.py");
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
fn p05_01_graph() {
    let g = graph_for("02-function-summaries/python/graph.py", "main");
    assert_eq!((g.contexts.len(), g.edges.len()), (8, 9));
}

#[test]
fn p05_01_stable_ids_ignore_discovery_order() {
    use crate::interp::StableContextId;
    use std::collections::BTreeMap;
    // main calls read_a then read_b; main2 calls them in the other order,
    // so their ContextIds differ. Their stable ids must not.
    check_both("05-output/python/stable_ids.py");
    let ids = |entry: &str| -> BTreeMap<String, StableContextId> {
        graph_for("05-output/python/stable_ids.py", entry)
            .contexts
            .into_iter()
            .filter(|c| c.qname != format!("stable_ids.{entry}")) // the entries themselves differ
            .map(|c| (c.qname, c.id))
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
    use crate::interp::{call_wire, merge_for_output};
    // `twice` calls `run` twice from one function: two nodes on the wire, one after the merge
    let path =
        std::fs::canonicalize("tests/fixtures/02-function-summaries/python/graph.py").unwrap();
    let db = open_project(SystemPath::from_std_path(path.parent().unwrap()).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let (wire, _) = call_wire(&db, file, "main", &summary()).unwrap();
    let twice = |t: &crate::interp::FrameJson| {
        t.children
            .iter()
            .find(|c| c.target_qname == "graph.twice")
            .unwrap()
            .clone()
    };
    let before = twice(&wire);
    assert_eq!(before.children.len(), 2);
    assert!(
        before
            .children
            .iter()
            .all(|c| c.call_count == 0 && c.call_site.is_some())
    );
    let after = twice(&merge_for_output(&wire));
    assert_eq!(after.children.len(), 1);
    assert_eq!(after.children[0].call_count, 1);
}

fn wire_of(fixture: &str, opts: &Options) -> crate::interp::FrameJson {
    let path = std::fs::canonicalize(format!("tests/fixtures/{fixture}")).unwrap();
    let db = open_project(SystemPath::from_std_path(path.parent().unwrap()).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    crate::interp::call_wire(&db, file, "main", opts).unwrap().0
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
fn ids(f: &crate::interp::FrameJson, out: &mut Vec<(String, String)>) {
    out.push((f.target_qname.clone(), f.target_id.clone()));
    f.children.iter().for_each(|c| ids(c, out));
}

#[test]
fn wire_ids_use_the_style_by_kind_and_are_empty_without_an_id() {
    let mut got = Vec::new();
    ids(
        &wire_of("06-wire/python/ids_and_classes.py", &styled()),
        &mut got,
    );
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
    let wire = wire_of("06-wire/python/ids_and_classes.py", &summary());
    assert_eq!(wire.target_id, "06000000-0003-0000-0000-000000000000");
}

#[test]
fn merging_joins_siblings_of_one_target_and_is_idempotent() {
    use crate::interp::merge_for_output;
    let wire = wire_of("06-wire/python/ids_and_classes.py", &styled());
    // `main` calls `unmarked` twice itself: two nodes before the merge, one after, counted
    let unmarked = |f: &crate::interp::FrameJson| {
        f.children
            .iter()
            .filter(|c| c.target_qname == "ids_and_classes.unmarked")
            .map(|c| c.call_count)
            .collect::<Vec<_>>()
    };
    assert_eq!(unmarked(&wire), [0, 0]);
    let merged = merge_for_output(&wire);
    assert_eq!(unmarked(&merged), [1]);
    assert_eq!(merge_for_output(&merged), merged);
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
                opts.require_ids = ids;
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
    use crate::interp::call_core_graph;
    let all = every_entry();
    assert!(all.len() > 40, "found only {} entries", all.len());
    for (fixture, entry, opts) in all {
        let (db, file) = open_fixture(&fixture);
        let Some(graph) = call_core_graph(&db, file, &entry, &opts) else {
            continue; // an entry some expectation names on purpose to be missing
        };
        let bad = analyzer_testkit::graph::violations(&graph);
        assert!(bad.is_empty(), "{fixture} {entry}:\n  {}", bad.join("\n  "));
    }
}
