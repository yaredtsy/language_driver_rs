//! The table of a body (`Sem::flow`), on `tests/fixtures/flow`: expectations written by hand.

use super::super::view::tests::{candidates, fixture_roots};
use super::*;
use crate::flow::{AnonFlow, HandedFlow, Part};

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
    table_of(sem, file, qualname, occurrence).1
}

/// The def's number and its table.
fn table_of(sem: &Sem, file: &str, qualname: &str, occurrence: u16) -> (DefId, Arc<Flow>) {
    let func = sem
        .entry(file, qualname, occurrence, STYLE)
        .unwrap_or_else(|| panic!("no def {qualname}#{occurrence} in {file}"));
    let id = sem.def_id(func);
    let flow = sem
        .flow(id, STYLE)
        .unwrap_or_else(|| panic!("no table for {qualname}#{occurrence}"));
    (id, flow)
}

/// The name at `at` in `text` and its 1-based line.
fn name_at(text: &str, at: u32) -> (String, usize) {
    let start = at as usize;
    let name: String = text[start..]
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    (name, text[..start].matches('\n').count() + 1)
}

/// An expression as the tests write it: `?`, `data`, `def <qualname>`, `param#<i>`, `anon#<i>`, `var <name>@<line>` (`var <qualname>:<name>@<line>` for a
/// variable of another def, `FILE` for the file's code), `oneof(<a> | <b>)`. `own` is the def of the table the expression is in, `text` the file's.
fn expr(sem: &Sem, own: DefId, text: &str, expr: &Expr) -> String {
    match expr {
        Expr::Unknown => "?".into(),
        Expr::Data => "data".into(),
        Expr::Undefined => "undefined".into(),
        Expr::Def(id) => {
            let func = sem.func(*id).expect("a def of this program");
            format!("def {}", sem.describe(func, STYLE).qualname)
        }
        Expr::Param(index) => format!("param#{index}"),
        Expr::Anon(index) => format!("anon#{index}"),
        Expr::Var(id) => {
            let (name, line) = name_at(text, id.at);
            if id.def == own {
                format!("var {name}@{line}")
            } else {
                let owner = match sem.func(id.def).expect("a def of this program") {
                    func if func.outline_index().is_none() => "FILE".to_owned(),
                    func => sem.describe(func, STYLE).qualname,
                };
                format!("var {owner}:{name}@{line}")
            }
        }
        Expr::OneOf(members) => {
            let members: Vec<String> = members
                .iter()
                .map(|member| self::expr(sem, own, text, member))
                .collect();
            format!("oneof({})", members.join(" | "))
        }
        Expr::Member(object, name) => {
            format!("member({}, {name})", self::expr(sem, own, text, object))
        }
        Expr::Object(parts) => {
            let parts: Vec<String> = parts
                .iter()
                .map(|part| match part {
                    Part::Named(name, value) => {
                        format!("{name}: {}", self::expr(sem, own, text, value))
                    }
                    Part::Spread(value) => format!("...{}", self::expr(sem, own, text, value)),
                    Part::Unnamed => "?".to_owned(),
                })
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        Expr::Omit(object, names) => {
            format!(
                "omit({}; {})",
                self::expr(sem, own, text, object),
                names.join(", ")
            )
        }
        Expr::Default(value, default) => format!(
            "default({}, {})",
            self::expr(sem, own, text, value),
            self::expr(sem, own, text, default)
        ),
    }
}

/// A site as `<kind> <callee> (<args>)`, plus ` in anon#<i>`; `(-)` for arguments that were not read.
fn show(sem: &Sem, own: DefId, text: &str, site: &SiteFlow) -> String {
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
                format!("{spread}{}", expr(sem, own, text, &arg.value))
            })
            .collect::<Vec<_>>()
            .join(", "),
    };
    let anon = site
        .anon
        .map_or(String::new(), |index| format!(" in anon#{index}"));
    format!(
        "{kind} {} ({args}){anon}",
        expr(sem, own, text, &site.callee)
    )
}

/// The names of a set of variables as the table's tests write them: `names: var a@3 var b@3`.
fn show_names(sem: &Sem, own: DefId, text: &str, names: &[VarId]) -> String {
    let all: Vec<String> = names
        .iter()
        .map(|&id| expr(sem, own, text, &Expr::Var(id)))
        .collect();
    format!("names: {}", all.join(" "))
}

/// What a variable's uses say, as ` uses(escapes, passed: f#0, names: var a@3, receiver, called: m n, called any, exported)`: only what is set, in this order; nothing when the uses are
/// default.
fn show_uses(sem: &Sem, own: DefId, text: &str, uses: &Uses) -> String {
    let mut said: Vec<String> = Vec::new();
    if uses.escapes {
        said.push("escapes".into());
    }
    if !uses.passed.is_empty() {
        let all: Vec<String> = uses
            .passed
            .iter()
            .map(|&(def, index)| {
                let func = sem.func(def).expect("a def of this program");
                format!("{}#{index}", sem.describe(func, STYLE).qualname)
            })
            .collect();
        said.push(format!("passed: {}", all.join(" ")));
    }
    if !uses.names.is_empty() {
        said.push(show_names(sem, own, text, &uses.names));
    }
    if uses.receiver {
        said.push("receiver".into());
    }
    if !uses.called.is_empty() {
        said.push(format!("called: {}", uses.called.join(" ")));
    }
    if uses.called_any {
        said.push("called any".into());
    }
    if uses.exported {
        said.push("exported".into());
    }
    if said.is_empty() {
        String::new()
    } else {
        format!(" uses({})", said.join(", "))
    }
}

/// A parameter's row as `#<index>`, then ` rest`, ` pattern` (not named), ` default <expr>`, ` assigned`, ` capable`, each only when it holds.
fn show_param(sem: &Sem, own: DefId, text: &str, index: usize, param: &Param) -> String {
    let mut out = format!("#{index}");
    if param.rest {
        out.push_str(" rest");
    }
    if !param.named {
        out.push_str(" pattern");
    }
    if let Some(default) = &param.default {
        out.push_str(&format!(" default {}", expr(sem, own, text, default)));
    }
    if param.assigned {
        out.push_str(" assigned");
    }
    if param.capable {
        out.push_str(" capable");
    }
    if !param.uses.names.is_empty() {
        out.push_str(&format!(
            " {}",
            show_names(sem, own, text, &param.uses.names)
        ));
    }
    out
}

/// The parameter rows of a def, shown.
fn rows(sem: &Sem, file: &str, qualname: &str, occurrence: u16) -> Vec<String> {
    let (own, flow) = table_of(sem, file, qualname, occurrence);
    let text = sem.text_of(file).unwrap();
    flow.params
        .iter()
        .enumerate()
        .map(|(index, param)| show_param(sem, own, &text, index, param))
        .collect()
}

/// The sites of a def, shown.
fn sites(sem: &Sem, file: &str, qualname: &str, occurrence: u16) -> Vec<String> {
    let (own, flow) = table_of(sem, file, qualname, occurrence);
    let text = sem.text_of(file).unwrap();
    flow.sites
        .iter()
        .map(|site| show(sem, own, &text, site))
        .collect()
}

/// A hand-over as `<text at its place> <expr>`, and ` written` for a def written inside an expression.
fn show_handed(sem: &Sem, own: DefId, text: &str, handed: &HandedFlow) -> String {
    let name = handed
        .at
        .and_then(|at| at.slice(text))
        .unwrap_or("(no place)");
    let written = if handed.written { " written" } else { "" };
    format!("{name} {}{written}", expr(sem, own, text, &handed.value))
}

/// Whether the expression holds a def, directly or inside a `OneOf`.
fn holds_a_def(expr: &Expr) -> bool {
    match expr {
        Expr::Def(_) => true,
        Expr::OneOf(members) => members.iter().any(holds_a_def),
        // a member of a def (`handler.length`) is not the def: a name read that way is not a hand-over
        Expr::Member(..) | Expr::Omit(..) => false,
        Expr::Default(value, default) => holds_a_def(value) || holds_a_def(default),
        Expr::Object(parts) => parts.iter().any(|part| match part {
            Part::Named(_, value) | Part::Spread(value) => holds_a_def(value),
            Part::Unnamed => false,
        }),
        Expr::Unknown
        | Expr::Data
        | Expr::Undefined
        | Expr::Param(_)
        | Expr::Anon(_)
        | Expr::Var(_) => false,
    }
}

#[test]
fn the_table_of_flow_reads_each_callee_and_argument() {
    let sem = open();
    let (own, flow) = table_of(&sem, "src/flow.tsx", "flow", 0);
    let text = sem.text_of("src/flow.tsx").unwrap();
    let shown: Vec<String> = flow
        .sites
        .iter()
        .map(|site| show(&sem, own, &text, site))
        .collect();
    assert_eq!(
        shown,
        [
            "call def target (data, data, ...?)",
            "call ? (param#0, def held, def held, def held)",
            "call ? (anon#0)",
            "call def target (data) in anon#0",
            "call ? (anon#1, def flow.<locals>.named)",
            "call ? (oneof(def held | def target), oneof(param#0 | def held), oneof(param#0 | def held), oneof(param#0 | def held))",
            "call ? (data, data, data, data, data, data, data)",
            "call ? (?, ?, ?, {held: def held}, ?, param#1)",
            "new ? ()",
            "jsx def Card ({title: data})",
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
        .map(|&AnonFlow { span, .. }| span.slice(&text).unwrap())
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
    let (own, flow) = table_of(&sem, "src/flow.tsx", "flow", 0);
    let all: Vec<String> = flow
        .handed
        .iter()
        .map(|entry| show_handed(&sem, own, &text, entry))
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
            "p param#0",
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
            "q param#1",
        ]
    );
    // every other entry is a name nothing is known of
    for (entry, shown) in flow.handed.iter().zip(&all) {
        if entry.value == Expr::Unknown {
            assert!(["Shape ?"].contains(&shown.as_str()), "{shown}");
        }
    }
}

/// A one-of never lists a member twice and is never one member alone: an overloaded function is one def, and `flag ? over : over` is that def.
#[test]
fn a_one_of_never_repeats_a_member() {
    let sem = open();
    let shown = sites(&sem, "src/overloads.ts", "callsOver", 0);
    assert_eq!(shown, ["call def over (data)", "call def over (data)"]);
}

/// One variable's row as `<name>@<line>`, then ` param#<i>`, ` capable`, ` open`, each only when it holds, then ` = <e1>; <e2>; …`.
fn show_var(sem: &Sem, own: DefId, text: &str, var: &crate::flow::Var) -> String {
    let (name, line) = name_at(text, var.at);
    let mut out = format!("{name}@{line}");
    if let Some(index) = var.param {
        out.push_str(&format!(" param#{index}"));
    }
    if var.capable {
        out.push_str(" capable");
    }
    if var.open {
        out.push_str(" open");
    }
    if !var.assigned.is_empty() {
        let all: Vec<String> = var
            .assigned
            .iter()
            .map(|value| expr(sem, own, text, value))
            .collect();
        out.push_str(&format!(" = {}", all.join("; ")));
    }
    out.push_str(&show_uses(sem, own, text, &var.uses));
    out
}

/// A table written out for review: params, vars, sites, anons, handed, `unmodelled` when it is not 0, and `needs` when it is not empty.
fn show_table(sem: &Sem, own: DefId, text: &str, flow: &Flow) -> String {
    let mut out = String::new();
    if flow.params.is_empty() {
        out.push_str("params: none\n");
    } else {
        out.push_str("params:\n");
        for (index, param) in flow.params.iter().enumerate() {
            out.push_str(&format!("  {}\n", show_param(sem, own, text, index, param)));
        }
    }
    if flow.vars.is_empty() {
        out.push_str("vars: none\n");
    } else {
        out.push_str("vars:\n");
        for var in &flow.vars {
            out.push_str(&format!("  {}\n", show_var(sem, own, text, var)));
        }
    }
    if let Some(uses) = &flow.this_uses
        && *uses != Uses::default()
    {
        out.push_str(&format!("this:{}\n", show_uses(sem, own, text, uses)));
    }
    if flow.sites.is_empty() {
        out.push_str("sites: none\n");
    } else {
        out.push_str("sites:\n");
        for site in &flow.sites {
            out.push_str(&format!("  {}\n", show(sem, own, text, site)));
        }
    }
    out.push_str(&format!("anons: {}\n", flow.anons.len()));
    if flow.handed.is_empty() {
        out.push_str("handed: none\n");
    } else {
        out.push_str("handed:\n");
        for held in &flow.handed {
            out.push_str(&format!("  {}\n", show_handed(sem, own, text, held)));
        }
    }
    if flow.unmodelled > 0 {
        out.push_str(&format!("unmodelled: {}\n", flow.unmodelled));
    }
    let needed = crate::needs::needs(&OfSem(sem), own);
    if !needed.is_empty() {
        let all: Vec<String> = needed
            .iter()
            .map(|var| expr(sem, own, text, &Expr::Var(*var)).replace("var ", ""))
            .collect();
        out.push_str(&format!("needs: {}\n", all.join(", ")));
    }
    out
}

/// A `World` over the `Sem`: its tables, and nothing counted.
struct OfSem<'a>(&'a Sem);

impl crate::eval::World for OfSem<'_> {
    fn flow(&self, def: DefId) -> Option<Arc<Flow>> {
        self.0.flow(def, STYLE)
    }

    fn widened(&self) {}

    fn knows(&self, _caps: crate::caps::Caps) -> bool {
        true
    }

    fn note(&self, _key: &'static str) {}

    fn needs(&self, def: DefId) -> Arc<[crate::flow::VarId]> {
        Arc::from(crate::needs::needs(self, def))
    }

    fn needs_known(&self, _def: DefId) -> Option<Arc<[crate::flow::VarId]>> {
        None
    }

    fn relevant(&self, def: DefId) -> Arc<[bool]> {
        Arc::from(crate::relevance::relevant(self, def))
    }

    fn trusted(&self, _var: crate::flow::VarId) -> bool {
        unreachable!("the tables are not evaluated here")
    }

    fn trusted_known(&self, _var: crate::flow::VarId) -> Option<bool> {
        unreachable!("the tables are not evaluated here")
    }

    fn capture(
        &self,
        _list: Vec<(crate::flow::VarId, crate::value::Value)>,
    ) -> (Option<crate::value::Captures>, u32) {
        unreachable!("the tables are not evaluated here")
    }

    fn captured(&self, _captures: crate::value::Captures) -> crate::nested::CaptureList {
        unreachable!("the tables are not evaluated here")
    }

    fn object(
        &self,
        _members: Vec<(Box<str>, crate::value::Value)>,
        _complete: bool,
    ) -> (Option<crate::value::ObjectId>, u32) {
        (None, 0)
    }

    fn object_data(&self, _id: crate::value::ObjectId) -> crate::nested::ObjectData {
        unreachable!("the tables are not evaluated here")
    }

    fn module_value(&self, _var: crate::flow::VarId) -> crate::value::Value {
        unreachable!("the tables are not evaluated here")
    }
}

/// The defs of `file` (whose text is `text`), in outline order, then the file's own code as `FILE`, each with its name.
fn entries_of<'s>(sem: &'s Sem, file: &str, text: &str) -> Vec<(String, Func<'s>)> {
    let dialect = typescript_syntax::Dialect::from_path(file).expect("a source file");
    let outline = typescript_syntax::outline(text, dialect, STYLE);
    let mut entries: Vec<(String, Func<'_>)> = Vec::new();
    let mut seen: HashMap<&str, u16> = HashMap::new();
    for def in &outline.defs {
        let occurrence = seen.entry(def.qualname.as_str()).or_default();
        let func = sem
            .entry(file, &def.qualname, *occurrence, STYLE)
            .unwrap_or_else(|| panic!("no def {}#{occurrence}", def.qualname));
        entries.push((def.qualname.clone(), func));
        *occurrence += 1;
    }
    entries.push((
        "FILE".to_owned(),
        sem.file_entry(file).expect("the file's code"),
    ));
    entries
}

/// The tables of every def of `file` in `root`, in outline order, then the file's own code, as `== <qualname>` and `show_table`.
fn tables_of(root: &Path, file: &str) -> String {
    let sem = Sem::open(root, Path::new(file)).unwrap();
    let text = std::fs::read_to_string(root.join(file)).unwrap();
    let entries = entries_of(&sem, file, &text);
    let mut out = String::new();
    for (name, func) in &entries {
        let id = sem.def_id(*func);
        let flow = sem
            .flow(id, STYLE)
            .unwrap_or_else(|| panic!("no table for {name}"));
        out.push_str(&format!("== {name}\n"));
        out.push_str(&show_table(&sem, id, &text, &flow));
    }
    out
}

/// The tables of the tour (`tests/fixtures/context/tour`), written out for review: what every later phase of the layer starts from.
#[test]
fn the_tables_of_the_tour_are_reviewed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/context/tour");
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/context/tour.flow.txt"),
        &tables_of(&root, "src/tour.tsx"),
    );
}

/// The variables of a body (TC3 step 1): one row for each variable a def declares, what is assigned to it, and how a name of one reads.
#[test]
fn the_variables_of_a_body_are_reviewed() {
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/flow/vars.flow.txt"),
        &tables_of(&root(), "src/vars.ts"),
    );
}

/// Object literals, member accesses, object patterns and JSX attributes (TC4 step 1): the table reads them, nothing evaluates them yet.
#[test]
fn the_objects_of_a_body_are_reviewed() {
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/flow/objects.flow.txt"),
        &tables_of(&root(), "src/objects.tsx"),
    );
}

/// What the body does with each variable's value (TC4 step 3a): `escapes`, `passed`, `names`, `receiver`, `called`, `called_any`, `exported`. Nothing reads them yet.
#[test]
fn the_uses_of_a_variable_are_reviewed() {
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/flow/trust.flow.txt"),
        &tables_of(&root(), "src/trust.tsx"),
    );
}

/// Whether the object each capable variable of every table of `file` holds can be trusted, as `World::trusted` of the `TypeScript` world answers: for each def in
/// `entries_of`'s order, its name and its capable rows in row order, each `<name>@<line>` (as `show_var` begins) and its answer. The variables are asked in table
/// order, or, with `reversed`, from the last to the first; the answers are put back in table order.
fn trust_of(root: &Path, file: &str, reversed: bool) -> Vec<(String, Vec<(String, bool)>)> {
    use crate::eval::World;
    let sem = Sem::open(root, Path::new(file)).unwrap();
    let text = std::fs::read_to_string(root.join(file)).unwrap();
    let lang =
        crate::calls::TypeScript::new(&sem, STYLE).with_layer(Some(crate::caps::Caps::BUILT));
    // (the def's place in the list, its variable, how it is written)
    let mut asked: Vec<(usize, crate::flow::VarId, String)> = Vec::new();
    let entries = entries_of(&sem, file, &text);
    for (place, (_, func)) in entries.iter().enumerate() {
        let id = sem.def_id(*func);
        let flow = sem.flow(id, STYLE).expect("a table");
        for var in flow.vars.iter().filter(|var| var.capable) {
            let (name, line) = name_at(&text, var.at);
            asked.push((
                place,
                crate::flow::VarId {
                    def: id,
                    at: var.at,
                },
                format!("{name}@{line}"),
            ));
        }
    }
    let mut answers: Vec<Option<bool>> = vec![None; asked.len()];
    let order: Vec<usize> = if reversed {
        (0..asked.len()).rev().collect()
    } else {
        (0..asked.len()).collect()
    };
    for at in order {
        answers[at] = Some(lang.trusted(asked[at].1));
    }
    let mut found: Vec<(String, Vec<(String, bool)>)> = entries
        .iter()
        .map(|(name, _)| (name.clone(), Vec::new()))
        .collect();
    for ((place, _, label), answer) in asked.into_iter().zip(answers) {
        found[place]
            .1
            .push((label, answer.expect("every variable was asked")));
    }
    found
}

/// Which variables' objects can be trusted (TC4 step 3b): one line for each capable variable of each table of the trust fixture.
#[test]
fn the_trust_of_each_variable_is_reviewed() {
    let mut out = String::new();
    for (name, rows) in trust_of(&root(), "src/trust.tsx", false) {
        out.push_str(&format!("== {name}\n"));
        if rows.is_empty() {
            out.push_str("  none\n");
        }
        for (label, answer) in rows {
            let said = if answer { "trusted" } else { "untrusted" };
            out.push_str(&format!("  {label} {said}\n"));
        }
    }
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/flow/trust.trust.txt"),
        &out,
    );
}

/// The answer is the least one, whatever variable is asked first (`08-tc4` §10, row 12): two fresh programs, the variables asked in opposite orders.
#[test]
fn the_trust_of_a_variable_does_not_depend_on_which_is_asked_first() {
    let forward = trust_of(&root(), "src/trust.tsx", false);
    let backward = trust_of(&root(), "src/trust.tsx", true);
    assert_eq!(forward, backward);
    assert!(forward.iter().map(|(_, rows)| rows.len()).sum::<usize>() > 30);
}

/// What each kind of expression is read as (the review before TC3, §1.1): sixteen kinds are data, `void 0` and the global `undefined` are nothing.
#[test]
fn the_kinds_of_data_and_nothing_are_reviewed() {
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/flow/data.flow.txt"),
        &tables_of(&root(), "src/data.tsx"),
    );
}

/// A script's variable at the top of the file is open (any other script may write it); one inside a function written in the file is not (TC3 step 4's nit).
#[test]
fn a_script_local_is_not_a_global() {
    analyzer_testkit::expect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/flow/script.flow.txt"),
        &tables_of(&root(), "src/script.ts"),
    );
}

/// What each parameter's row says: rest, pattern, default, assigned, capable (`06-tc2` §2).
#[test]
fn the_parameter_rows_say_what_each_parameter_is() {
    let sem = open();
    let p = "src/params.ts";
    let row = |qualname: &str, occurrence: u16| rows(&sem, p, qualname, occurrence);
    // s n b g y u z lit e su arr ro: data; tup: not looked into; rec: the depth stops; v tl: data; brand: an intersection; f o a k mix fns opt
    assert_eq!(
        row("kinds", 0),
        [
            "#0",
            "#1",
            "#2",
            "#3",
            "#4",
            "#5",
            "#6",
            "#7",
            "#8",
            "#9",
            "#10",
            "#11",
            "#12 capable",
            "#13 capable",
            "#14",
            "#15",
            "#16 capable",
            "#17 capable",
            "#18 capable",
            "#19 capable",
            "#20 capable",
            "#21 capable",
            "#22 capable",
            "#23 capable",
        ]
    );
    assert_eq!(row("generic", 0), ["#0 capable", "#1 capable"]);
    assert_eq!(
        row("uses", 0),
        [
            "#0 capable",
            "#1 pattern capable names: var x@16",
            "#2 rest capable"
        ]
    );
    assert_eq!(
        row("reassigned", 0),
        ["#0 assigned capable", "#1 assigned capable", "#2"]
    );
    assert_eq!(row("quoted", 0), ["#0 capable"]);
    assert_eq!(row("shadowed", 0), ["#0 capable"]);
    assert_eq!(
        row("defaults", 0),
        [
            "#0 default def noop capable",
            "#1 default param#0 capable",
            "#2 default data",
            "#3 capable"
        ]
    );
    assert_eq!(row("overParams", 0), ["#0 capable", "#1"]);
    assert_eq!(row("Holder.constructor", 0), ["#0 capable"]);
    assert_eq!(row("Holder.handler", 0), ["#0 capable"]);
    assert_eq!(row("Holder.run", 0), Vec::<String>::new());
    assert_eq!(
        rows(&sem, "src/flow.tsx", "flow", 0),
        ["#0 assigned capable", "#1"]
    );
    assert_eq!(rows(&sem, "src/flow.tsx", "target", 0), ["#0", "#1 rest"]);
}

/// A name that is a parameter of the def's own declaration is that place; any other name is not.
#[test]
fn a_name_of_a_parameter_is_its_place() {
    let sem = open();
    let p = "src/params.ts";
    let site = |qualname: &str, occurrence: u16| sites(&sem, p, qualname, occurrence);
    assert_eq!(
        site("uses", 0),
        [
            "call param#0 ()",
            "call var x@16 ()",
            "call ? ()",
            "call ? (anon#0)",
            "call var cb@21 () in anon#0",
            "call ? (anon#1)",
            "call param#0 () in anon#1",
            "call ? (param#0)",
        ]
    );
    // a parameter of the enclosing def: a variable of that def (TC3 step 1)
    assert_eq!(site("uses.<locals>.inner", 0), ["call var uses:cb@16 ()"]);
    assert_eq!(
        site("reassigned", 0),
        ["call param#0 ()", "call param#1 ()"]
    );
    assert_eq!(site("quoted", 0), ["call ? (data)", "call param#0 ()"]);
    assert_eq!(site("shadowed", 0), ["call ? (anon#0)", "call param#0 ()"]);
    assert_eq!(
        site("defaults", 0),
        ["call param#0 ()", "call param#1 ()", "call param#3 ()"]
    );
    assert_eq!(site("overParams", 0), ["call param#0 ()"]);
    assert_eq!(site("Holder.constructor", 0), ["call param#0 ()"]);
    assert_eq!(site("Holder.handler", 0), ["call param#0 ()"]);
    assert_eq!(site("Holder.run", 0), ["call ? ()"]);
}

/// An untyped parameter is `any`, which can hold a function.
#[test]
fn an_untyped_parameter_is_capable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flow-js");
    let sem = Sem::open(&root, Path::new("src/params.js")).unwrap();
    let p = "src/params.js";
    assert_eq!(rows(&sem, p, "each", 0), ["#0 capable"]);
    assert_eq!(sites(&sem, p, "each", 0), ["call param#0 ()"]);
    assert_eq!(
        rows(&sem, p, "withDefault", 0),
        ["#0 default anon#0 capable"]
    );
    assert_eq!(sites(&sem, p, "withDefault", 0), ["call param#0 ()"]);
}

/// Every `Expr::Var` in `expr`, `OneOf` members included.
fn vars_in(expr: &Expr, out: &mut Vec<crate::flow::VarId>) {
    match expr {
        Expr::Var(id) => out.push(*id),
        Expr::OneOf(members) => members.iter().for_each(|member| vars_in(member, out)),
        Expr::Member(object, _) | Expr::Omit(object, _) => vars_in(object, out),
        Expr::Default(value, default) => {
            vars_in(value, out);
            vars_in(default, out);
        }
        Expr::Object(parts) => {
            for part in parts.iter() {
                if let Part::Named(_, value) | Part::Spread(value) = part {
                    vars_in(value, out);
                }
            }
        }
        Expr::Unknown
        | Expr::Data
        | Expr::Undefined
        | Expr::Def(_)
        | Expr::Param(_)
        | Expr::Anon(_) => {}
    }
}

/// The tables of every def of every fixture project, and of each file's own code, once each: `(project, def's name, def, table)`.
fn every_table(mut each: impl FnMut(&Sem, &str, DefId, &Flow)) {
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
        let mut seen = HashSet::new();
        for func in sem.project_defs(STYLE) {
            for func in [func, Func::module(func.file)] {
                let id = sem.def_id(func);
                if !seen.insert(id) {
                    continue;
                }
                let name = format!(
                    "{} in {}",
                    sem.describe(func, STYLE).qualname,
                    root.display()
                );
                if let Some(flow) = sem.flow(id, STYLE) {
                    each(&sem, &name, id, &flow);
                }
            }
        }
    }
}

/// A name a table reads as a variable is a row of the def that declares it.
#[test]
fn every_variable_a_table_reads_has_its_row() {
    let (mut tables, mut reads) = (0, 0);
    every_table(|sem, name, _, flow| {
        let mut found = Vec::new();
        for site in &flow.sites {
            vars_in(&site.callee, &mut found);
            for arg in site.args.iter().flat_map(|args| args.iter()) {
                vars_in(&arg.value, &mut found);
            }
        }
        for held in &flow.handed {
            vars_in(&held.value, &mut found);
        }
        for param in &flow.params {
            if let Some(default) = &param.default {
                vars_in(default, &mut found);
            }
        }
        for var in &flow.vars {
            for value in &var.assigned {
                vars_in(value, &mut found);
            }
        }
        for id in found {
            let table = sem
                .flow(id.def, STYLE)
                .unwrap_or_else(|| panic!("no table for the def of {id:?}: {name}"));
            assert!(
                table.vars.iter().any(|var| var.at == id.at),
                "no row for {id:?}: {name}"
            );
            reads += 1;
        }
        tables += 1;
    });
    eprintln!("checked {reads} variable reads in {tables} tables");
    assert!(tables > 100 && reads > 10);
}

/// A parameter's `assigned` is its row's writes, and a named parameter has exactly one row.
#[test]
fn a_parameters_assigned_is_its_rows_writes() {
    let (mut named, mut assigned) = (0, 0);
    every_table(|_, name, _, flow| {
        for (index, param) in flow.params.iter().enumerate() {
            if !param.named {
                continue;
            }
            let rows: Vec<_> = flow
                .vars
                .iter()
                .filter(|var| var.param == Some(index as u32))
                .collect();
            assert_eq!(rows.len(), 1, "parameter {index} of {name}");
            let row = rows[0];
            assert_eq!(
                param.assigned,
                row.capable && (row.open || !row.assigned.is_empty()),
                "parameter {index} of {name}"
            );
            named += 1;
            assigned += usize::from(param.assigned);
        }
    });
    eprintln!("checked {named} named parameters, {assigned} assigned");
    assert!(named > 50 && assigned > 0);
}

/// The table of the method calls of step 4d (`08-tc4` §10): `called` is the last access of each chain that is a callee, `called_any` is a call by a name the
/// table cannot read, and both come with `receiver`.
#[test]
fn the_names_called_on_a_variable_are_the_last_access_of_each_chain() {
    let sem = Sem::open(&root(), Path::new("src/called.ts")).unwrap();
    let text = sem.text_of("src/called.ts").unwrap();
    let (id, flow) = table_of(&sem, "src/called.ts", "f", 0);
    let x = &flow.vars[flow.param_var(0).expect("a row for x")].uses;
    assert!(x.receiver && !x.escapes);
    assert_eq!(&*x.called, ["m".into(), "n".into()]);
    assert!(x.called_any);
    assert_eq!(
        show_uses(&sem, id, &text, x),
        " uses(receiver, called: m n, called any)"
    );
}

/// A fixture of `tests/fixtures/context`, opened as the tour test opens its folder.
fn context_fixture(name: &str, file: &str) -> Sem {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/context")
        .join(name);
    Sem::open(&root, Path::new(file)).unwrap()
}

#[test]
fn a_method_that_calls_this_records_the_name_in_its_this_uses() {
    let sem = context_fixture("method-this-calls-this", "src/main.ts");
    let (_, run) = table_of(&sem, "src/main.ts", "run", 0);
    let o = &run.vars[run.param_var(0).expect("a row for o")].uses;
    assert_eq!(&*o.called, ["f".into(), "reset".into()]);
    assert!(!o.called_any);
    let (_, reset) = table_of(&sem, "src/main.ts", "main.<locals>.reset", 0);
    let uses = reset.this_uses.as_ref().expect("a method has a `this`");
    assert_eq!(&*uses.called, ["clear".into()]);
    assert!(!uses.escapes);
    let (_, clear) = table_of(&sem, "src/main.ts", "main.<locals>.clear", 0);
    assert!(
        clear
            .this_uses
            .as_ref()
            .expect("a method has a `this`")
            .escapes
    );
}

/// The prompt of step 4d-a expected `reset: () => {}` of `method-arrow-member` to be a def; it is not (an arrow held by a property is a def only in a `const`
/// object literal at the top of a file, D21), so it is the anonymous arrow `anon#0` of `main`. `Flow::arrow` is then tested on a def that is an arrow.
#[test]
fn a_def_knows_whether_its_own_node_is_an_arrow() {
    let sem = open();
    assert!(flow_of(&sem, "src/flow.tsx", "held", 0).arrow);
    assert!(!flow_of(&sem, "src/flow.tsx", "flow", 0).arrow);
    let sem = context_fixture("method-arrow-member", "src/main.ts");
    assert!(!flow_of(&sem, "src/main.ts", "run", 0).arrow);
    // `reset: () => {}` is no def: it is the one anonymous arrow of `main`
    let main = flow_of(&sem, "src/main.ts", "main", 0);
    assert_eq!(
        main.anons.iter().map(|anon| anon.arrow).collect::<Vec<_>>(),
        [true]
    );
    assert!(
        sem.entry("src/main.ts", "main.<locals>.reset", 0, STYLE)
            .is_none()
    );
}

#[test]
fn an_anonymous_function_knows_whether_it_is_an_arrow() {
    let sem = context_fixture("method-anon-member", "src/main.tsx");
    let flow = flow_of(&sem, "src/main.tsx", "App", 0);
    let arrows: Vec<bool> = flow.anons.iter().map(|anon| anon.arrow).collect();
    assert_eq!(arrows, [true, false]);
}
