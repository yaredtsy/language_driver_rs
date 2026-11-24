//! The table of a body (`Sem::flow`), on `tests/fixtures/flow`: expectations written by hand.

use super::super::view::tests::{candidates, fixture_roots};
use super::*;
use crate::flow::{AnonFlow, HandedFlow};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: typescript_syntax::Placement::Docstring,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flow")
}

fn open() -> Sem {
    Sem::open(&root(), Path::new("src/flow.tsx")).unwrap()
}

fn flow_of(sem: &Sem, file: &str, qualname: &str, occurrence: u16) -> Arc<Flow> {
    let func = sem
        .entry(file, qualname, occurrence, STYLE)
        .unwrap_or_else(|| panic!("no def {qualname}#{occurrence} in {file}"));
    sem.flow(sem.def_id(func), STYLE)
        .unwrap_or_else(|| panic!("no table for {qualname}#{occurrence}"))
}

/// An expression as the tests write it: `?`, `data`, `def <qualname>`, `anon#<i>`, `oneof(<a> | <b>)`.
fn expr(sem: &Sem, expr: &Expr) -> String {
    match expr {
        Expr::Unknown => "?".into(),
        Expr::Data => "data".into(),
        Expr::Def(id) => {
            let func = sem.func(*id).expect("a def of this program");
            format!("def {}", sem.describe(func, STYLE).qualname)
        }
        Expr::Anon(index) => format!("anon#{index}"),
        Expr::OneOf(members) => {
            let members: Vec<String> = members
                .iter()
                .map(|member| self::expr(sem, member))
                .collect();
            format!("oneof({})", members.join(" | "))
        }
    }
}

/// A site as `<kind> <callee> (<args>)`, plus ` in anon#<i>`; `(-)` for arguments that were not read.
fn show(sem: &Sem, site: &SiteFlow) -> String {
    let kind = match site.kind {
        FlowKind::Call => "call",
        FlowKind::New => "new",
        FlowKind::Jsx => "jsx",
        FlowKind::Tagged => "tagged",
        FlowKind::Decorator => "decorator",
        FlowKind::Accessor => "accessor",
    };
    let args = match &site.args {
        None => "-".to_owned(),
        Some(args) => args
            .iter()
            .map(|arg| {
                let spread = if arg.spread { "..." } else { "" };
                format!("{spread}{}", expr(sem, &arg.value))
            })
            .collect::<Vec<_>>()
            .join(", "),
    };
    let anon = site
        .anon
        .map_or(String::new(), |index| format!(" in anon#{index}"));
    format!("{kind} {} ({args}){anon}", expr(sem, &site.callee))
}

/// A hand-over as `<text at its place> <expr>`, and ` written` for a def written inside an expression.
fn show_handed(sem: &Sem, text: &str, handed: &HandedFlow) -> String {
    let name = handed
        .at
        .and_then(|at| at.slice(text))
        .unwrap_or("(no place)");
    let written = if handed.written { " written" } else { "" };
    format!("{name} {}{written}", expr(sem, &handed.value))
}

/// Whether the expression holds a def, directly or inside a `OneOf`.
fn holds_a_def(expr: &Expr) -> bool {
    match expr {
        Expr::Def(_) => true,
        Expr::OneOf(members) => members.iter().any(holds_a_def),
        Expr::Unknown | Expr::Data | Expr::Anon(_) => false,
    }
}

#[test]
fn the_table_of_flow_reads_each_callee_and_argument() {
    let sem = open();
    let flow = flow_of(&sem, "src/flow.tsx", "flow", 0);
    let shown: Vec<String> = flow.sites.iter().map(|site| show(&sem, site)).collect();
    assert_eq!(
        shown,
        [
            "call def target (data, data, ...?)",
            "call ? (?, def held, def held, def held)",
            "call ? (anon#0)",
            "call def target (data) in anon#0",
            "call ? (anon#1, def flow.<locals>.named)",
            "call ? (oneof(def held | def target), oneof(? | def held), oneof(? | def held), oneof(? | def held))",
            "call ? (data, data, data, data, data, data, data)",
            "call ? (?, ?, ?, ?, ?, ?)",
            "new ? ()",
            "jsx def Card (?)",
            "call ? (anon#2)",
            "call ? (anon#3) in anon#2",
            "call def target (data) in anon#3",
        ]
    );
    assert_eq!(flow.unhandled, 0);
}

#[test]
fn the_parameters_are_the_declarations_own_without_this() {
    let sem = open();
    let rests = |file: &str, qualname: &str, occurrence: u16| -> Vec<bool> {
        flow_of(&sem, file, qualname, occurrence)
            .params
            .iter()
            .map(|param| param.rest)
            .collect()
    };
    assert_eq!(rests("src/flow.tsx", "flow", 0), [false, false]);
    assert_eq!(rests("src/flow.tsx", "target", 0), [false, true]);
    assert_eq!(rests("src/flow.tsx", "run", 0), [true]);
    assert_eq!(rests("src/flow.tsx", "held", 0), Vec::<bool>::new());
    assert_eq!(rests("src/flow.tsx", "Card", 0), [false]);
    assert_eq!(
        rests("src/flow.tsx", "flow.<locals>.named", 0),
        Vec::<bool>::new()
    );
    assert_eq!(rests("src/flow.tsx", "Shape", 0), Vec::<bool>::new());
    assert_eq!(rests("src/kinds.ts", "Point", 0), Vec::<bool>::new());
    assert_eq!(rests("src/kinds.ts", "Point.constructor", 0), [false, true]);
    assert_eq!(rests("src/kinds.ts", "Point.size", 0), Vec::<bool>::new());
    assert_eq!(rests("src/kinds.ts", "Point.size", 1), [false]);
    assert_eq!(rests("src/kinds.ts", "Point.move", 0), [false]);
}

#[test]
fn the_anonymous_functions_are_listed_in_source_order() {
    let sem = open();
    let text = sem.text_of("src/flow.tsx").unwrap();
    let flow = flow_of(&sem, "src/flow.tsx", "flow", 0);
    let written: Vec<&str> = flow
        .anons
        .iter()
        .map(|&AnonFlow { span }| span.slice(&text).unwrap())
        .collect();
    assert_eq!(
        written,
        [
            "() => target(2)",
            "function () {}",
            "() => run(() => target(3))",
            "() => target(3)"
        ]
    );
    // `held` is a def: its own node is not an anonymous function of its body
    assert!(flow_of(&sem, "src/flow.tsx", "held", 0).anons.is_empty());
}

#[test]
fn the_table_has_every_site_of_every_def_of_every_fixture() {
    let (mut projects, mut defs, mut sites, mut handed) = (0, 0, 0, 0);
    for root in fixture_roots() {
        let mut files = Vec::new();
        candidates(&root, &mut files);
        let Some(sem) = files
            .iter()
            .take(5)
            .find_map(|file| Sem::open(&root, file).ok())
        else {
            continue;
        };
        projects += 1;
        for func in sem.project_defs(STYLE) {
            let wanted = sem.call_sites(func, STYLE);
            let flow = sem.flow(sem.def_id(func), STYLE);
            let found: Option<Vec<Span>> = flow
                .as_ref()
                .map(|flow| flow.sites.iter().map(|site| site.span).collect());
            assert_eq!(
                found,
                wanted,
                "{} in {}",
                sem.describe(func, STYLE).qualname,
                root.display()
            );
            if let Some(flow) = &flow {
                // the table sees what D22 sees: the defs of `handed_over` are the defs of the table, and no other entry holds one
                let key = |at: Option<Span>, id: DefId| (at, id.file(), id.index());
                let mut wanted: Vec<_> = sem
                    .handed_over(func, STYLE)
                    .handed
                    .iter()
                    .map(|found| key(found.at, sem.def_id(found.func)))
                    .collect();
                let mut found: Vec<_> = flow
                    .handed
                    .iter()
                    .filter_map(|entry| match entry.value {
                        Expr::Def(id) => Some(key(entry.at, id)),
                        _ => None,
                    })
                    .collect();
                wanted.sort();
                found.sort();
                let name = sem.describe(func, STYLE).qualname;
                assert_eq!(found, wanted, "{name} in {}", root.display());
                for entry in &flow.handed {
                    let by_handed_over = wanted.iter().any(|&(at, ..)| at == entry.at);
                    assert!(
                        by_handed_over || !holds_a_def(&entry.value),
                        "expr_of found a def at {:?} where handed_over found none: {name} in {}",
                        entry.at,
                        root.display()
                    );
                }
                handed += flow.handed.len();
            }
            if let Some(flow) = flow {
                assert_eq!(
                    flow.unhandled,
                    0,
                    "{} in {}",
                    sem.describe(func, STYLE).qualname,
                    root.display()
                );
                sites += flow.sites.len();
            }
            defs += 1;
        }
    }
    eprintln!(
        "compared {defs} defs, {sites} sites and {handed} hand-overs in {projects} fixture projects"
    );
    assert!(projects > 20 && defs > 100 && sites > 100);
}

#[test]
fn a_number_of_one_program_is_not_read_through_another() {
    let (a, b) = (open(), open());
    let func = a.entry("src/flow.tsx", "flow", 0, STYLE).unwrap();
    let id = a.def_id(func);
    assert!(a.func(id) == Some(func));
    assert!(b.func(id).is_none());
    // and the table is made once
    let first = a.flow(id, STYLE).unwrap();
    let second = a.flow(id, STYLE).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    // the other program has a number of its own for the same def, which reads
    let other = b.entry("src/flow.tsx", "flow", 0, STYLE).unwrap();
    assert!(b.flow(b.def_id(other), STYLE).is_some());
    assert!(b.flow(id, STYLE).is_none());
}

#[test]
fn the_hand_overs_of_flow_are_handed_overs_defs_and_the_other_names() {
    let sem = open();
    let text = sem.text_of("src/flow.tsx").unwrap();
    let flow = flow_of(&sem, "src/flow.tsx", "flow", 0);
    let all: Vec<String> = flow
        .handed
        .iter()
        .map(|entry| show_handed(&sem, &text, entry))
        .collect();
    eprintln!("{}", all.join("\n"));
    let known: Vec<&String> = flow
        .handed
        .iter()
        .zip(&all)
        .filter(|(entry, _)| entry.value != Expr::Unknown)
        .map(|(_, shown)| shown)
        .collect();
    assert_eq!(
        known,
        [
            "held def held",
            "held def held",
            "held def held",
            "named def flow.<locals>.named written",
            "held def held",
            "target def target",
            "held def held",
            "held def held",
            "held def held",
            "held def held",
            "held def held",
            "held def held",
        ]
    );
    // every other entry is a name nothing is known of
    for (entry, shown) in flow.handed.iter().zip(&all) {
        if entry.value == Expr::Unknown {
            assert!(
                ["p ?", "q ?", "Shape ?"].contains(&shown.as_str()),
                "{shown}"
            );
        }
    }
}

/// The tables of the tour (`tests/fixtures/context/tour`), written out for review: what every later phase of the layer starts from.
#[test]
fn the_tables_of_the_tour_are_reviewed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/context/tour");
    let sem = Sem::open(&root, Path::new("src/tour.tsx")).unwrap();
    let text = std::fs::read_to_string(root.join("src/tour.tsx")).unwrap();
    let outline = typescript_syntax::outline(&text, typescript_syntax::Dialect::Tsx, STYLE);
    let mut entries: Vec<(String, Func<'_>)> = Vec::new();
    let mut seen: HashMap<&str, u16> = HashMap::new();
    for def in &outline.defs {
        let occurrence = seen.entry(def.qualname.as_str()).or_default();
        let func = sem
            .entry("src/tour.tsx", &def.qualname, *occurrence, STYLE)
            .unwrap_or_else(|| panic!("no def {}#{occurrence}", def.qualname));
        entries.push((def.qualname.clone(), func));
        *occurrence += 1;
    }
    entries.push((
        "FILE".to_owned(),
        sem.file_entry("src/tour.tsx").expect("the file's code"),
    ));
    let mut out = String::new();
    for (name, func) in &entries {
        let flow = sem
            .flow(sem.def_id(*func), STYLE)
            .unwrap_or_else(|| panic!("no table for {name}"));
        out.push_str(&format!("== {name}\n"));
        match flow.params.iter().position(|param| param.rest) {
            Some(rest) => out.push_str(&format!("params: {}, rest at {rest}\n", flow.params.len())),
            None => out.push_str(&format!("params: {}\n", flow.params.len())),
        }
        if flow.sites.is_empty() {
            out.push_str("sites: none\n");
        } else {
            out.push_str("sites:\n");
            for site in &flow.sites {
                out.push_str(&format!("  {}\n", show(&sem, site)));
            }
        }
        out.push_str(&format!("anons: {}\n", flow.anons.len()));
        if flow.handed.is_empty() {
            out.push_str("handed: none\n");
        } else {
            out.push_str("handed:\n");
            for held in &flow.handed {
                out.push_str(&format!("  {}\n", show_handed(&sem, &text, held)));
            }
        }
    }
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/context/tour.flow.txt"),
        &out,
    );
}
