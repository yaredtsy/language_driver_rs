// src/interp/fixture.rs
use crate::interp::bind::param_names;
use crate::interp::relevant::relevant_params;
use crate::interp::summary::{PlainVal, SVal};
use crate::interp::{Cuts, Limits, Mode, Options, Stats, to_text};
use crate::sem::Sem;
use crate::sem::{ProjectDatabase, open, open_project};
use analyzer_core::graph::Entry;
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

/// A `# expect:` name (`outer.inner`, `Host.meth.Local.m`: the dotted way, with no `<locals>`) as the outline names it: the first def of
/// the file whose qualname, with every `.<locals>` removed, is `dotted`, at occurrence 0 (the first def with its qualname). The fixtures and
/// their snapshot headers keep the dotted names; `Sem` takes only the outline's (D20).
fn entry_named(db: &ProjectDatabase, file: ruff_db::files::File, dotted: &str) -> Option<String> {
    let marker = Options::default().marker;
    let text = Sem::new(db, marker).source_text(file);
    let outline = python_syntax::outline(&text, marker);
    outline
        .defs
        .iter()
        .find(|d| d.qualname.replace(".<locals>", "") == dotted)
        .map(|d| d.qualname.clone())
}

// The interp functions take the outline's `Entry`; the harness asks by a dotted name. These shadow them with the same names, so
// every call below reads as before.

pub(super) fn solve<'db>(
    db: &'db ProjectDatabase,
    file: ruff_db::files::File,
    dotted: &str,
    opts: &Options,
) -> Option<(
    crate::interp::solver::Solver<'db>,
    crate::interp::EntryRoot<'db>,
)> {
    let qualname = entry_named(db, file, dotted)?;
    crate::interp::solve(
        db,
        file,
        &Entry {
            qualname: &qualname,
            occurrence: 0,
        },
        opts,
    )
}

pub(super) fn call_tree(
    db: &ProjectDatabase,
    file: ruff_db::files::File,
    dotted: &str,
    opts: &Options,
) -> Option<(crate::interp::Node, Stats)> {
    let qualname = entry_named(db, file, dotted)?;
    crate::interp::call_tree(
        db,
        file,
        &Entry {
            qualname: &qualname,
            occurrence: 0,
        },
        opts,
    )
}

pub(super) fn call_graph(
    db: &ProjectDatabase,
    file: ruff_db::files::File,
    dotted: &str,
    opts: &Options,
) -> Option<analyzer_core::graph::Graph> {
    let qualname = entry_named(db, file, dotted)?;
    crate::interp::call_graph(
        db,
        file,
        &Entry {
            qualname: &qualname,
            occurrence: 0,
        },
        opts,
    )
}

pub(super) fn call_frame(
    db: &ProjectDatabase,
    file: ruff_db::files::File,
    dotted: &str,
    opts: &Options,
    merged: bool,
) -> Option<(crate::wire::Frame, analyzer_core::graph::Stats)> {
    let qualname = entry_named(db, file, dotted)?;
    crate::interp::call_frame(
        db,
        file,
        &Entry {
            qualname: &qualname,
            occurrence: 0,
        },
        opts,
        merged,
    )
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

/// Solve `entry` of `fixture` in summary mode and hand the solver to `body`, which reads its contexts and summaries. The database lives
/// only as long as this call, so a test cannot take a summary out of it.
pub(super) fn with_solved<R>(
    fixture: &str,
    entry: &str,
    body: impl for<'db> FnOnce(&mut crate::interp::solver::Solver<'db>) -> R,
) -> R {
    let (db, file) = open_fixture(fixture);
    let (mut s, _) = solve(&db, file, entry, &summary()).unwrap();
    body(&mut s)
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
    let qualname = entry_named(&db, file, func).unwrap();
    let def = sem
        .entry(
            file,
            &Entry {
                qualname: &qualname,
                occurrence: 0,
            },
        )
        .unwrap();
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
/// Summary mode as the harness runs it.
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
            "max_path" => limits.max_path = n as usize,
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
        ("zero_arg_super", &["self"]),
        ("named_super", &["self"]),
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

/// A cancel that fires after the second walk: `main` and `a` are walked, and nothing is walked past them.
#[test]
fn p09_01_cancel_after_the_second_walk() {
    use analyzer_core::graph::{Cut as CCut, Status};
    let fixture = "02-function-summaries/python/cuts.py";
    let opts = Options {
        cancel_at_walk: Some(2),
        ..summary()
    };
    let path = std::fs::canonicalize(format!("tests/fixtures/{fixture}")).unwrap();
    let db = open_project(SystemPath::from_std_path(path.parent().unwrap()).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let (tree, st) = call_tree(&db, file, "main", &opts).unwrap();
    assert_eq!(st.walks, 2);
    assert!(st.cuts.cancelled >= 1, "{:?}", st.cuts);
    assert_eq!(
        Cuts {
            cancelled: 0,
            ..st.cuts.clone()
        },
        Cuts::default(),
        "no cut but the cancel"
    );
    let graph = call_graph(&db, file, "main", &opts).unwrap();
    let root = graph.entries[0].context().unwrap();
    assert_eq!(
        graph.context(root).unwrap().status,
        Status::Incomplete(CCut::Cancelled)
    );
    // `b` and `top` are listed and not walked; `handler` is not here, because `top`'s return was never read
    assert_eq!(
        to_text(&tree, "cuts"),
        "main [incomplete]\n  a [incomplete]\n    b [incomplete]\n  top [incomplete]\n"
    );
}

/// A context cut before its walk carries its own cut, not the fallback of a context that was never asked for.
#[test]
fn p09_01_a_context_cut_before_its_walk_says_which_limit() {
    let path =
        std::fs::canonicalize("tests/fixtures/02-function-summaries/python/cuts.py").unwrap();
    let db = open_project(SystemPath::from_std_path(path.parent().unwrap()).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let opts = Options {
        limits: Limits {
            max_stack: 2,
            ..Limits::default()
        },
        ..summary()
    };
    let graph = call_graph(&db, file, "main", &opts).unwrap();
    let statuses: Vec<String> = graph
        .contexts
        .iter()
        .map(|c| format!("{} {:?}", graph.def(&c.id.func).unwrap().qname, c.status))
        .collect();
    assert_eq!(
        statuses,
        [
            "cuts.a Incomplete(MaxDepth)",
            "cuts.b Incomplete(MaxDepth)",
            "cuts.main Incomplete(MaxDepth)",
            "cuts.mid Incomplete(MaxDepth)",
            "cuts.top Incomplete(MaxDepth)",
        ]
    );
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
    check_both("03-classes/python/state.py");
}

#[test]
fn p03_04_isolation() {
    check_both("03-classes/python/isolation.py");
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
    check_both("03-classes/python/writes.py");
}
#[test]
fn p03_06_special() {
    check_both("03-classes/python/special.py");
}

#[test]
fn p03_07_fanout_counted() {
    let st = stats_for("03-classes/python/chains.py", "fanout", &inline());
    assert_eq!(st.chain_fanout_hits, 1); // `b.h` is cut once; `.handle` on 8 values is under the cap
}

#[test]
fn p03_07_chains() {
    check_both("03-classes/python/chains.py");
}

/// Remove the `ignore` when step 03.08 is built. Covers methods, classmethods,
/// staticmethods, properties, closures at any depth, classes in functions and
/// methods, and the end-of-run rule for an escaped closure (`fire`).
#[test]
fn p03_08_entries() {
    check_both("03-classes/python/entries.py");
}

/// The key of a closure's own run counts its free values from depth 0, as its parameters; a closure passed as a value keeps them one step
/// below its parameter (effects plan, row 11). `fp_text` writes a fingerprint as `Class{attr: …}`.
#[test]
fn p03_08_a_closures_own_run_keys_its_free_values_from_depth_0() {
    let f = "03-classes/python/closure_keys.py";
    check_both(f);
    // the contexts of `func` in the solver, each as `args [..] free [..]`
    fn keys(s: &crate::interp::solver::Solver<'_>, func: &str) -> Vec<String> {
        let want = format!("closure_keys.{func}");
        s.contexts
            .iter()
            .filter(|(_, c)| s.sem.qname(c.func) == want)
            .map(|(_, c)| {
                let args: Vec<String> = c.args.iter().map(|&a| fp_text(s, a)).collect();
                let free: Vec<String> = c
                    .free
                    .iter()
                    .map(|(n, fp)| format!("{}={}", n.as_str(), fp_text(s, *fp)))
                    .collect();
                format!("args [{}] free [{}]", args.join(", "), free.join(", "))
            })
            .collect()
    }
    // the closure's own run: `svc` is an input, so the handler is in the key
    with_solved(f, "direct", |s| {
        assert_eq!(
            keys(s, "direct.step"),
            ["args [] free [svc=Svc{p: Pipeline{handler: JsonHandler}}]"]
        );
    });
    // the closure as a value: `svc` is one step below `cb`, so `p` is cut at the depth limit (unchanged by row 11), and so is the run of
    // `step` that `run_it` makes
    with_solved(f, "passed", |s| {
        assert_eq!(
            keys(s, "run_it"),
            ["args [Closure(passed.step; svc=Svc{p: Pipeline})] free []"]
        );
        assert_eq!(
            keys(s, "passed.step"),
            ["args [] free [svc=Svc{p: Pipeline{}}]"] // the object holds state, and what it holds is not known
        );
    });
}

/// A fingerprint as `Class{attr: …}` (a test's reading of a key).
fn fp_text(s: &crate::interp::solver::Solver<'_>, id: crate::interp::key::FpId) -> String {
    use crate::interp::key::Fp;
    let attrs = |attrs: &[(crate::sem::Name, crate::interp::key::FpId)]| {
        attrs
            .iter()
            .map(|(n, a)| format!("{}: {}", n.as_str(), fp_text(s, *a)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match &s.fps[id] {
        Fp::Instance { class, attrs: a } => {
            let name = s.sem.qname(*class);
            format!("{}{{{}}}", name.rsplit('.').next().unwrap(), attrs(a))
        }
        Fp::Opaque(class) => s.sem.qname(*class).rsplit('.').next().unwrap().to_owned(),
        Fp::Closure { func, free, .. } => {
            let free: Vec<String> = free
                .iter()
                .map(|(n, a)| format!("{}={}", n.as_str(), fp_text(s, *a)))
                .collect();
            let name = s.sem.qname(*func);
            format!(
                "Closure({}; {})",
                name.trim_start_matches("closure_keys."),
                free.join(", ")
            )
        }
        Fp::Unknown => "Unknown".to_owned(),
        other => format!("{other:?}"),
    }
}

#[test]
fn p03_08_prologue_runs_the_parents() {
    let f = "03-classes/python/entries.py";
    // the same counts in both modes: one prologue (`Walk::prologue`) starts every entry
    for opts in [inline(), summary()] {
        prologue_counts(f, &opts);
    }
}

fn prologue_counts(f: &str, opts: &Options) {
    let walks = |entry| stats_for(f, entry, opts).prologue_walks;
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
fn p04_02_coming_back() {
    check_both("04-class-summaries/python/coming_back.py");
}

/// A site with two callees: each callee's new objects stay apart (the `t` in `OriginKey::New`).
#[test]
fn p04_02_two_callees() {
    check_both("04-class-summaries/python/two_callees.py");
}

/// The tree cannot say it (the renderer joins the siblings of one def at a site into one node, so summary mode draws both handlers under
/// one `Box.run`), the graph can: two `Box.run` contexts, each with its own handler and no other. Built with the callee index left out
/// of `OriginKey::New`, the second callee's `Box` replaces the first's and one context is lost.
#[test]
fn p04_02_two_callees_each_run_sees_only_its_own_handler() {
    let g = graph_for("04-class-summaries/python/two_callees.py", "main");
    let name = |f: &analyzer_core::graph::StableDef| g.def(f).map(|d| d.qname.clone()).unwrap();
    let mut runs: Vec<Vec<String>> = g
        .contexts
        .iter()
        .filter(|c| name(&c.id.func) == "two_callees.Box.run")
        .map(|c| {
            g.edges_from(&c.id)
                .iter()
                .map(|e| match &e.to {
                    analyzer_core::graph::EdgeTo::Context(to, _) => name(&to.func),
                    other => panic!("unexpected edge {other:?}"),
                })
                .collect()
        })
        .collect();
    runs.sort();
    assert_eq!(
        runs,
        [
            ["two_callees.CsvHandler.process"],
            ["two_callees.JsonHandler.process"]
        ]
    );
    assert_eq!(g.stats.language.get("ref_unresolved"), None);
}

/// A callee's writes onto the objects its caller can see reach the caller (CS3b).
#[test]
fn p04_03_effects() {
    check_both("04-class-summaries/python/effects.py");
}

/// The edges out of `entry`'s contexts that go to a `process`, as `graph_text` writes them: the callee's name and its basis.
fn process_edges(fixture: &str, entry: &str) -> Vec<String> {
    let text = graph_text(&graph_for(fixture, entry));
    text.lines()
        .filter_map(|l| l.split(" -> ").nth(1))
        .filter(|to| to.contains(".process#"))
        .map(|to| {
            let (name, rest) = to.split_once('#').unwrap();
            let basis = rest.split_whitespace().nth(1).unwrap_or("");
            format!("{name} {basis}")
        })
        .collect()
}

/// A strong write of a value the walk cannot say removes what the attribute held, so the read asks the engine's declared type: a `Type`
/// edge to `Handler.process`. A caller that kept `Json` would draw `Json.process`, a call Python makes no more, with nothing to mark it.
#[test]
fn p04_03_forget() {
    let f = "04-class-summaries/python/forget.py";
    check_both(f);
    for entry in ["main", "main2"] {
        assert_eq!(
            process_edges(f, entry),
            ["forget.Handler.process Type"],
            "{entry}"
        );
    }
}

/// A site with a project callee and an engine callee keeps what the project callee wrote (CS3b.1): the write is weak there, and `p.run()`
/// may call either handler.
#[test]
fn p04_03_mixed_callee() {
    check_both("04-class-summaries/python/mixed_callee.py");
}

/// A write onto a closure's free object is applied by the run the closure was defined in.
#[test]
fn p04_03_closure_effect() {
    check_both("04-class-summaries/python/closure_effect.py");
}

/// A recursive call's effects are not carried until CS8: counted once for the one site, nothing wrong drawn.
#[test]
fn p04_03_cycle_effect() {
    let f = "04-class-summaries/python/cycle_effect.py";
    check_both(f);
    let st = stats_for(f, "main", &summary());
    assert_eq!(st.effects_in_cycle, 1);
    let graph = graph_for(f, "main");
    assert_eq!(graph.stats.language.get("effects_in_cycle"), Some(&1));
}

/// Inline mode only until CS4 (resolve, then write).
#[test]
fn p04_04_resolve_first() {
    check_both("04-class-summaries/python/resolve_first.py");
}

const DEEP_WRITE_CAPPED: &str = "04-class-summaries/python/deep_write_capped.py";
const TWO_LEVELS: &str = "04-class-summaries/python/deep_write_two_levels.py";
const DEEP_READ: &str = "04-class-summaries/python/deep_read.py";
const DEEP_RECURSION: &str = "04-class-summaries/python/deep_recursion.py";

/// The options a fixture's `# limits:` line asks for, summary mode with effects.
fn fixture_options(fixture: &str) -> Options {
    let source = std::fs::read_to_string(format!("tests/fixtures/{fixture}")).unwrap();
    let (limits, _) = fixture_limits(&source).expect("the fixture has a `# limits:` line");
    Options {
        limits,
        ..summary()
    }
}

/// `entry`'s graph under `opts`: the counters and each context's status.
fn graph_under(fixture: &str, entry: &str, opts: &Options) -> analyzer_core::graph::Graph {
    let (db, file) = open_fixture(fixture);
    call_graph(&db, file, entry, opts).unwrap()
}

fn deep_write_count(g: &analyzer_core::graph::Graph) -> Option<u64> {
    g.stats.language.get("deep_write").copied()
}

fn deep_return_count(g: &analyzer_core::graph::Graph) -> Option<u64> {
    g.stats.language.get("deep_return").copied()
}

/// CS5b: a write onto an object past the key reaches the caller by its path (decisions 1, 2 and §3 of `05-deep-writes.md`); past
/// `max_path` it is the counted fallback.
#[test]
fn p04_05_deep_writes() {
    check_both("04-class-summaries/python/deep_write.py");
    check_both(DEEP_WRITE_CAPPED);
}

/// Option B: past `max_path` the write is counted, and not marked: every context is `Finished`, and the count says one summary fell back.
#[test]
fn p04_05_a_write_past_max_path_is_counted_and_marks_nothing() {
    use analyzer_core::graph::Status;
    let g = graph_under(
        DEEP_WRITE_CAPPED,
        "main",
        &fixture_options(DEEP_WRITE_CAPPED),
    );
    println!("{}", graph_text(&g)); // --show-output
    assert_eq!(deep_write_count(&g), Some(1));
    assert_eq!(deep_return_count(&g), None); // the write is the loss here, no object leaves with its identity gone
    assert!(g.contexts.iter().all(|c| c.status == Status::Finished));
    // a path that fits names the object: no fallback, and no counter at all
    let g = graph_under(DEEP_WRITE_CAPPED, "main", &summary());
    assert_eq!(deep_write_count(&g), None);
}

/// §4 `deep_write_two_levels.py`: the effect reaches `main` through two summaries.
#[test]
fn p04_05_a_deep_write_travels_up_through_two_summaries() {
    check_both(TWO_LEVELS);
}

/// §3 row 4 with the cap: `outer_held` names the written object by a path one step longer than `rewire` does, and with `max_path` 3 that
/// path is past the cap. The effect it carries up is `rewire`'s own fallback: weak `Unknown` at `holder.svc.app.config.sink`.
#[test]
fn p04_05_a_callers_path_past_max_path_falls_back_in_the_caller() {
    use analyzer_core::graph::Status;
    let opts = Options {
        limits: Limits {
            max_path: 3,
            ..Limits::default()
        },
        ..summary()
    };
    let g = graph_under(TWO_LEVELS, "main_held", &opts);
    println!("{}", graph_text(&g)); // --show-output
    assert_eq!(deep_write_count(&g), Some(1)); // `outer_held` only: `rewire`'s own path fits
    assert!(g.contexts.iter().all(|c| c.status == Status::Finished));
    let (db, file) = open_fixture(TWO_LEVELS);
    let (s, _) = solve(&db, file, "main_held", &opts).unwrap();
    let held = s
        .contexts
        .iter()
        .find(|(_, c)| s.sem.qname(c.func) == "deep_write_two_levels.outer_held")
        .map(|(ctx, _)| ctx)
        .unwrap();
    let res = s.slots[held].result.as_ref().unwrap();
    let [e] = res.effects.as_slice() else {
        panic!("expected one effect, got {:?}", res.effects);
    };
    assert_eq!(path_text(&e.target), "arg0.svc.app.config");
    assert_eq!(e.attr.as_str(), "sink");
    assert!(!e.strong);
    assert_eq!(
        e.value.as_slice(),
        &[crate::interp::summary::SVal::Plain(
            crate::interp::summary::PlainVal::Unknown
        )]
    );
}

/// `max_path` is never less than the key's own depth: a smaller value is raised to it, so every object a key names has a path that fits.
#[test]
fn p04_05_max_path_is_at_least_fp_depth() {
    let limits = |max_path, fp_depth| Limits {
        max_path,
        fp_depth,
        ..Limits::default()
    };
    assert_eq!(limits(0, 2).path_cap(), 2);
    assert_eq!(limits(1, 3).path_cap(), 3);
    assert_eq!(limits(2, 2).path_cap(), 2);
    assert_eq!(limits(6, 2).path_cap(), 6);
    assert_eq!(Limits::default().path_cap(), 6);
}

/// Claim 3 (05 §6): an object past the cap holds no state, and a read of it goes to the engine's declared type, as the read of an object
/// that has a name does. Nothing is written in `peek`, so its edges are the same under `max_path` 2 and 6.
#[test]
fn p04_05_a_read_below_the_cap_goes_to_the_engine() {
    check_both(DEEP_READ);
    let capped = graph_under(DEEP_READ, "main", &fixture_options(DEEP_READ));
    let named = graph_under(DEEP_READ, "main", &summary());
    assert_eq!(graph_text(&capped), graph_text(&named));
    assert_eq!(deep_write_count(&capped), None);
}

/// A path as a test reads it: `arg0.next.next`.
fn path_text(p: &crate::interp::value::Path) -> String {
    use crate::interp::value::{Root, Step};
    let mut out = match p.root {
        Root::Param(i) => format!("arg{i}"),
        Root::Free(i) => format!("free{i}"),
        Root::Default(i) => format!("default{i}"),
    };
    for step in &p.steps {
        match step {
            Step::Attr(name) => out.push_str(&format!(".{name}")),
            other => out.push_str(&format!(".{other:?}")),
        }
    }
    out
}

/// What `last`'s returns held after one round of its group's fixpoint: how many paths into the argument (`Ref`s), the number of steps
/// of the longest, and the other values.
struct Round {
    refs: usize,
    longest: usize,
    others: Vec<String>,
}

/// `last`'s returns after each round of its fixpoint, printed one line a round; and the request's counters.
fn rounds_of_last(fixture: &str, opts: &Options) -> (Vec<Round>, Stats) {
    use crate::interp::summary::{PlainVal, SVal};
    let (db, file) = open_fixture(fixture);
    let (s, _) = solve(&db, file, "main", opts).unwrap();
    let mut rounds = Vec::new();
    for (round, ctx, returns) in &s.round_returns {
        if s.sem.qname(s.contexts[*ctx].func) != "deep_recursion.last" {
            continue;
        }
        let mut r = Round {
            refs: 0,
            longest: 0,
            others: Vec::new(),
        };
        for v in returns {
            match v {
                SVal::Ref(p) => {
                    r.refs += 1;
                    r.longest = r.longest.max(p.steps.len());
                }
                SVal::Plain(PlainVal::Opaque(c)) => {
                    r.others.push(format!("opaque {}", s.sem.qname(*c)))
                }
                SVal::Plain(PlainVal::Ty(_)) => r.others.push("ty".to_owned()), // the engine's own type: `None`, an intersection
                other => r.others.push(format!("{other:?}")),
            }
        }
        r.others.sort();
        println!(
            "round {round}: {} paths, the longest arg0{}; {}",
            r.refs,
            ".next".repeat(r.longest),
            r.others.join(", ")
        ); // --show-output
        rounds.push(r);
    }
    (rounds, s.stats.clone())
}

/// Both modes draw the same tree for the list `main` builds and walks.
#[test]
fn p04_05_deep_recursion_tree() {
    check_both(DEEP_RECURSION);
}

/// Claim 4 (05 §6), the default `max_path`: a path that would grow with every round stops at the cap, the summaries become equal, and the
/// group settles.
#[test]
fn p04_05_a_recursion_that_grows_a_path_settles_at_max_path() {
    use analyzer_core::graph::Status;
    let (rounds, stats) = rounds_of_last(DEEP_RECURSION, &summary());
    assert_eq!(stats.rounds_capped, 0);
    assert!(rounds.len() < Limits::default().max_rounds as usize);
    // the paths grow one step a round and stop at the cap: the cap is `Limits::default().max_path`, and the one past it is `Opaque`
    let cap = Limits::default().max_path;
    assert!(rounds.iter().all(|r| r.longest <= cap));
    let last = rounds.last().unwrap();
    assert_eq!(last.longest, cap);
    assert!(
        last.others
            .contains(&"opaque deep_recursion.Node".to_owned())
    );
    let g = graph_under(DEEP_RECURSION, "main", &summary());
    assert!(g.contexts.iter().all(|c| c.status == Status::Finished));
    // the object past the cap leaves five summaries as `Opaque`: counted under its own name, and no write fell back
    assert_eq!(deep_return_count(&g), Some(5));
    assert_eq!(deep_write_count(&g), None);
}

/// Claim 4, scratch run: with `max_path` far above `max_rounds`, each round names one `.next` more, no two rounds are equal, and the
/// fixpoint is cut by `max_rounds`.
#[test]
fn p04_05_the_same_recursion_without_a_cap_is_cut_by_max_rounds() {
    use analyzer_core::graph::{Cut, Status};
    let opts = Options {
        limits: Limits {
            max_path: 1000,
            ..Limits::default()
        },
        ..summary()
    };
    let (rounds, stats) = rounds_of_last(DEEP_RECURSION, &opts);
    assert!(stats.rounds_capped >= 1, "the group settled");
    // one `.next` longer every round, until `max_rounds` cuts it
    for (i, r) in rounds
        .iter()
        .take(opts.limits.max_rounds as usize)
        .enumerate()
    {
        assert_eq!(
            (i + 1, r.refs),
            (r.longest, r.longest + 1),
            "round {}",
            i + 1
        );
    }
    let g = graph_under(DEEP_RECURSION, "main", &opts);
    assert_eq!(deep_return_count(&g), None); // nothing is past a cap of 1000
    assert!(
        g.contexts
            .iter()
            .any(|c| c.status == Status::Incomplete(Cut::MaxRounds))
    );
}

/// Inline mode only until CS6 (split by type).
#[test]
fn p04_06_split() {
    check("04-class-summaries/python/split.py");
}

/// CS7: two parameters holding one object are one object in the callee (07 §2): `same` draws `JsonHandler.process`, `different` does not.
#[test]
fn p04_07_aliases() {
    check_both("04-class-summaries/python/aliases.py");
    check_both("04-class-summaries/python/alias_pair.py");
    check_both("04-class-summaries/python/alias_free.py"); // the known limit of decision 4, in both modes
}

/// 07 §5 claim 1: the receiver is slot 0, so `a.merge(a)` needs no special case.
#[test]
fn p04_07_a_receiver_that_is_also_an_argument_is_one_object() {
    check_both("04-class-summaries/python/self_merge.py");
}

/// The digest of `set_and_read`'s one context under `different` (two `Box`es holding a `CsvHandler`, no aliasing), as a key without an
/// alias vector gave it: pinned on 2026-11-13, before `Context.alias` existed (CS7).
fn digest_of_set_and_read(entry: &str) -> Vec<String> {
    let (db, file) = open_fixture("04-class-summaries/python/aliases.py");
    let (s, _) = solve(&db, file, entry, &summary()).unwrap();
    let mut out: Vec<String> = s
        .contexts
        .iter()
        .filter(|(_, k)| s.sem.qname(k.func) == "aliases.set_and_read")
        .map(|(c, _)| {
            let d = s.stable_id(c).digest;
            d.iter().map(|b| format!("{b:02x}")).collect()
        })
        .collect();
    out.sort();
    out
}

const DIGEST_BEFORE_ALIAS: &str = "580af8cf013e505a6d505eeb31764101";

/// 07 §3: a key whose alias vector is the identity has the digest it had before the field existed, and `different` is such a key.
#[test]
fn p04_07_a_key_with_no_aliasing_has_the_digest_it_had() {
    assert_eq!(digest_of_set_and_read("different"), [DIGEST_BEFORE_ALIAS]);
}

/// `same` is the same key but for the alias vector `[0, 0]`: another digest. The identity written out is the digest of today; `[0, 0]` is not.
#[test]
fn p04_07_an_alias_vector_that_is_not_the_identity_changes_the_digest() {
    let hex = |d: [u8; 16]| -> String { d.iter().map(|b| format!("{b:02x}")).collect() };
    let (db, file) = open_fixture("04-class-summaries/python/aliases.py");
    let (mut s, _) = solve(&db, file, "different", &summary()).unwrap();
    let (_, base) = s
        .contexts
        .iter()
        .find(|(_, k)| s.sem.qname(k.func) == "aliases.set_and_read")
        .map(|(c, k)| (c, k.clone()))
        .unwrap();
    assert_eq!(base.alias.as_slice(), [0, 1]);
    let mut with = |alias: &[u8]| {
        let mut k = base.clone();
        k.alias = alias.iter().copied().collect();
        let id = s.contexts.intern(k);
        hex(s.stable_id(id).digest)
    };
    let (identity, same) = (with(&[0, 1]), with(&[0, 0]));
    assert_eq!(identity, DIGEST_BEFORE_ALIAS);
    assert_ne!(same, DIGEST_BEFORE_ALIAS);
    // and it is what the walk of `same` gives
    assert_eq!(digest_of_set_and_read("same"), [same]);
}

/// `same` and `different` are two contexts of `set_and_read`; two calls of `different` share one.
#[test]
fn p04_07_same_and_different_are_two_contexts_and_two_calls_share_one() {
    let count = |entry: &str| {
        let (db, file) = open_fixture("04-class-summaries/python/aliases.py");
        let (s, _) = solve(&db, file, entry, &summary()).unwrap();
        s.contexts
            .iter()
            .filter(|(_, k)| s.sem.qname(k.func) == "aliases.set_and_read")
            .count()
    };
    assert_eq!((count("same"), count("different")), (1, 1));
    let (db, file) = open_fixture("04-class-summaries/python/alias_pair.py");
    let (s, _) = solve(&db, file, "main", &summary()).unwrap();
    let mut alias: Vec<Vec<u8>> = s
        .contexts
        .iter()
        .filter(|(_, k)| s.sem.qname(k.func) == "alias_pair.set_and_read")
        .map(|(_, k)| k.alias.to_vec())
        .collect();
    alias.sort();
    assert_eq!(alias, [vec![0, 0], vec![0, 1]]); // `same` twice and `different` twice: two contexts
}

/// Inline mode only until CS8 (recursion with objects).
#[test]
fn p04_08_recursion_objects() {
    check("04-class-summaries/python/recursion_objects.py");
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
    call_frame(db, file, "main", opts, merged).unwrap().0
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
            // a program no test names was never run
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
/// an `Unresolved` site next to the written handler. That is without effects; with them (CS3b), the constructors have written
/// `Csv` onto both objects, so the weak write joins onto what is there and the read has no `Unknown` to mark.
#[test]
fn a_weak_write_marks_what_it_may_have_dropped_and_a_strong_one_marks_nothing() {
    let unresolved = |entry: &str, opts: &Options| {
        let (db, file) = open_fixture("03-classes/python/writes.py");
        let g = call_graph(&db, file, entry, opts).unwrap();
        g.edges
            .iter()
            .filter(|e| matches!(e.to, analyzer_core::graph::EdgeTo::Unresolved(_)))
            .count()
    };
    let opts = summary();
    assert_eq!(unresolved("outside", &opts), 0);
    assert_eq!(unresolved("two_candidates", &opts), 0);
}

/// The golden wire of a real project. Not run by default: it needs a project that is not in the repo and the files in `out/golden/`, and the
/// prefixes are the caller's, which no crate may spell. Run it by hand:
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
    use crate::interp::WireStyle;
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
        // pylspt keyed without effects, so its context digests are not this crate's since CS4b: the comparison with it leaves `contexts` out.
        // The tree is what it protects; the digests are pinned by this crate's own golden.
        fn without_contexts(v: &mut Value) {
            if let Some(o) = v.as_object_mut() {
                o.remove("contexts");
            }
            if let Some(kids) = v.get_mut("children").and_then(Value::as_array_mut) {
                kids.iter_mut().for_each(without_contexts);
            }
        }
        let mut stripped = new.clone();
        without_target_names(&mut stripped);
        without_contexts(&mut stripped);
        let mut removed = Vec::new();
        strip(&mut stripped, &accessors, &mut removed);
        let mut reference_tree = reference.clone();
        without_contexts(&mut reference_tree);
        let (mut a, mut b) = (reference_tree, stripped);
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
    // 53 programs are verified in both modes and 2 only inline (`recursion_objects.py`, `split.py`, in `04-class-summaries/`); 55 are run at
    // all, and all have `# expect:` blocks (`deep_write.py` moved to both modes at CS5b, and `deep_write_capped.py`, `deep_write_two_levels.py`,
    // `deep_read.py` and `deep_recursion.py` joined, all verified; `aliases.py` moved at CS7, and `alias_pair.py`, `alias_free.py` and `self_merge.py` joined)
    let verified = by_fixture.iter().filter(|(_, v, _)| *v).count();
    assert_eq!(
        (verified, by_fixture.len() - verified),
        (53, 2),
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

/// A class as the entry and a function that constructs it, side by side: `Child` inherits `__init__` from another file, so the root's subtree
/// is in `inherit_b.py` and its call site says so (`in inherit_b.py`). Under `make`, the node `Child` has the same subtree and a call site in
/// `inherit_a.py`. `tests/snapshots/08-entries/python/inherit_a.wire`; reviewed, not just blessed.
#[test]
fn the_wire_of_a_class_entry_and_of_its_construction_side_by_side() {
    let (db, file) = open_fixture("08-entries/python/inherit_a.py");
    let mut text = String::new();
    for entry in ["Child", "make"] {
        let (frame, _) = call_frame(&db, file, entry, &summary(), false).unwrap();
        text.push_str(&format!("== {entry}\n{}\n", wire_text(&frame)));
    }
    let path = std::path::Path::new("tests/snapshots/08-entries/python/inherit_a.wire");
    analyzer_testkit::expect_file(path, &text);
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
    let opts = summary();
    let started = std::time::Instant::now();
    let graph = call_graph(&db, file, &entry, &opts).unwrap();
    println!("solve_ms {}", started.elapsed().as_millis());
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
        (55, 170),
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

/// The gap test, at its end (CS3e). It pinned, entry by entry, what summary mode missed against inline mode on the six class programs of
/// `03-classes/` that were checked in inline mode only: 48 entries, 21 failing at CS3a (35 nodes missing, 2 extra). Effects (CS3b), the
/// mixed-callee fix, the one prologue (CS3c) and a `super()` that counts as a use of `self` took it to 0, and the six are `check_both`.
/// What is left of the gap is said in its block and nowhere else: `state.py` `either` has an `# expect inline:` block (a
/// `Pipeline.run` for each object) and an `# expect summary:` block (one, with both handlers: rule 7, README §5, no information lost; CS6
/// may split it). (`entries.py` `Svc.build.step` had the same split for the key's depth; row 11 of 03 §7 closed it.) So this test
/// only has to hold that no `03-classes/` program goes back to inline-only: that would be a gap nobody measures.
#[test]
fn what_summary_mode_misses_on_the_class_programs_and_what_the_graph_says_there() {
    let this_file = include_str!("fixture.rs");
    let inline_only: BTreeSet<String> = every_entry()
        .into_iter()
        .map(|(fixture, _, _)| fixture)
        .filter(|f| f.starts_with("03-classes/") && this_file.contains(&format!("check(\"{f}\")")))
        .collect();
    assert!(
        inline_only.is_empty(),
        "checked in inline mode only again: {inline_only:?}"
    );
}

/// The programs of `04-class-summaries/` checked in inline mode only (CS0): what summary mode gives on each block that is its own (`# expect:`
/// and `# expect summary:`), compared with the block as `failures` compares in summary mode. Pinned against the blocks, not against inline
/// mode: `split.py` and the `# expect summary:` entries say on purpose where summary differs from inline. A step that fixes an entry removes
/// it from the list; an entry added is a regression.
#[test]
fn what_summary_mode_gives_on_the_part_04_programs_against_their_blocks() {
    let this_file = include_str!("fixture.rs");
    let mut fixtures: Vec<(String, Options)> = Vec::new();
    for (fixture, _, opts) in every_entry() {
        if fixture.starts_with("04-class-summaries/")
            && this_file.contains(&format!("check(\"{fixture}\")"))
            && fixtures.last().is_none_or(|(f, _)| *f != fixture)
        {
            fixtures.push((fixture, opts));
        }
    }
    let (mut blocks, mut failing) = (0, Vec::new());
    for (fixture, opts) in &fixtures {
        let (db, file) = open_fixture(fixture);
        let source = std::fs::read_to_string(format!("tests/fixtures/{fixture}")).unwrap();
        let module = fixture.rsplit('/').next().unwrap().trim_end_matches(".py");
        for e in expectations(&source)
            .into_iter()
            .filter(|e| e.mode.is_none_or(|m| m == Mode::Summary))
        {
            let got = call_tree(&db, file, &e.entry, opts)
                .map(|(t, _)| to_text(&t, module))
                .unwrap_or_else(|| format!("(no function `{}`)\n", e.entry));
            blocks += 1;
            if sorted_siblings(&got) != sorted_siblings(&e.tree) {
                println!(
                    "--- {fixture}: {}\nexpected:\n{}got:\n{got}",
                    e.entry, e.tree
                ); // --show-output
                failing.push(format!("{fixture} {}", e.entry));
            }
        }
    }
    assert_eq!(
        (fixtures.len(), blocks),
        (2, 5),
        "the part 04 programs checked inline only, and their summary-mode blocks"
    );
    // 5 blocks are summary mode's; these 2 fail. `effects.py` is verified in both modes since CS3b (its five blocks left this list),
    // `resolve_first.py` since CS4, `deep_write.py` since CS5b (the write past the key reaches `main` by its path), and `aliases.py` since
    // CS7 (the key says which slots are one object: `same` draws `JsonHandler.process`; `nested`'s summary block is the known limit, now
    // checked in both modes). The three that pass here: `recursion_objects.py visitor` (whose `Report.emit` comes from ty's type of
    // `v.found`, a `Type` edge, not from the walk), and `identity_limit` and `tree_fill` (below).
    //
    // `recursion_objects.py` `identity_limit` and `tree_fill` joined the list at CS3b and left it at CS3b.2: their blocks pinned a known
    // limit that both modes had ("Python: JsonHandler.process"), and with effects summary mode draws the call (run under Python, both
    // programs call `JsonHandler.process`). Each is now an `# expect inline:` block (the limit, as it was) and an `# expect summary:` block
    // (with the call). Each site that rests on a cycle is marked (`unknown_callee`, `effects_in_cycle`).
    //
    // The wrong edges that stay here, each with its reason (README §7 question 2): `split.py main` pairs each handler with both keys
    // (inline mode draws the same mix: CS6). `use_walk` waits for CS8.
    assert_eq!(
        failing,
        [
            "04-class-summaries/python/recursion_objects.py use_walk",
            "04-class-summaries/python/split.py main",
        ]
    );
}
