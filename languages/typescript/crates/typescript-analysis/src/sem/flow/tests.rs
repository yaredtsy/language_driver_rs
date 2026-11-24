//! The table of a body (`Sem::flow`), on `tests/fixtures/flow`: expectations written by hand.

use super::super::view::tests::{candidates, fixture_roots};
use super::*;
use crate::flow::AnonFlow;

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
    assert_eq!(written, ["() => target(2)", "function () {}"]);
    // `held` is a def: its own node is not an anonymous function of its body
    assert!(flow_of(&sem, "src/flow.tsx", "held", 0).anons.is_empty());
}

#[test]
fn the_table_has_every_site_of_every_def_of_every_fixture() {
    let (mut projects, mut defs, mut sites) = (0, 0, 0);
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
    eprintln!("compared {defs} defs and {sites} sites in {projects} fixture projects");
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
