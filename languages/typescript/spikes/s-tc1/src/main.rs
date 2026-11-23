//! S-TC1 (`docs/03-typescript/12-context-layer/04-tc0-counts-and-spikes.md` §3.1): does tsrs answer what the plan says?
//!
//! Throwaway. It loads `fixture/` the way `typescript-analysis`'s `Sem::build` loads a config, calls each engine function the plan
//! names once, and prints the answer beside the one the plan expected (one line per answer, `ok` or `NO`).

use std::fmt::Display;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{Arc, Mutex};

use tsrs_ast::{self as ast, Kind, Node, SourceFile};
use tsrs_checker::{Checker, ObjectFlags};
use tsrs_compiler::{Context, ProgramOptions, ProgramRegions};
use tsrs_core::arena::Region;
use tsrs_core::{CompilerOptions, P};
use tsrs_tsoptions::ParseConfigHost;
use tsrs_vfs::{FS, bundled, cachedvfs, osvfs};

const CHUNK: usize = 1 << 20;

/// Where tsrs reads config files from: the real disk, with the bundled `lib.*.d.ts` mounted (as `Sem`'s).
struct ConfigHost {
    fs: Arc<dyn FS>,
    cwd: String,
}

impl ParseConfigHost for ConfigHost {
    fn fs(&self) -> &dyn FS {
        &*self.fs
    }
    fn get_current_directory(&self) -> &str {
        &self.cwd
    }
}

/// One file of the program, with its text.
struct File {
    name: &'static str,
    node: P<SourceFile>,
    text: &'static str,
}

/// The tallies and the printing.
struct Rep {
    ok: usize,
    no: usize,
}

impl Rep {
    fn row(&mut self, id: &str, ask: &str, got: impl Display, expected: &str) {
        let got = got.to_string();
        self.shown(id, ask, &got, &got, expected);
    }

    /// `shown` is printed, `compared` is what is held against `expected` (a declaration prints its line, which is not compared).
    fn shown(&mut self, id: &str, ask: &str, shown: &str, compared: &str, expected: &str) {
        let verdict = if compared == expected {
            self.ok += 1;
            "ok"
        } else {
            self.no += 1;
            "NO"
        };
        println!("{id:<3} {ask}  | got: {shown} | expected: {expected} | {verdict}");
    }
}

static PANIC: Mutex<String> = Mutex::new(String::new());

/// Runs one case; a panic inside the engine (its accessors panic on a kind they do not handle) is a `NO` for the case, not the end of the run.
fn guard(rep: &mut Rep, id: &str, case: impl FnOnce(&mut Rep)) {
    let result = catch_unwind(AssertUnwindSafe(|| case(rep)));
    if result.is_err() {
        let why = PANIC.lock().map(|m| m.clone()).unwrap_or_default();
        rep.row(
            id,
            "(the case)",
            format!("not callable (panic: {why})"),
            "an answer",
        );
    }
}

/// The node's own text: from its first token (past the trivia in front) to its end.
fn own_text(text: &'static str, node: P<Node>) -> &'static str {
    let end = node.end().source_offset().unwrap_or(0);
    let pos = node.pos().source_offset().unwrap_or(0);
    let start = tsrs_scanner::skip_trivia(text, pos).min(end);
    &text[start as usize..end as usize]
}

fn start_offset(text: &str, node: P<Node>) -> usize {
    let end = node.end().source_offset().unwrap_or(0);
    let pos = node.pos().source_offset().unwrap_or(0);
    tsrs_scanner::skip_trivia(text, pos).min(end) as usize
}

fn walk(node: P<Node>, visit: &mut dyn FnMut(P<Node>)) {
    visit(node);
    node.for_each_child(&mut |child| {
        walk(child, visit);
        false
    });
}

fn name_of(node: P<Node>) -> Option<&'static str> {
    node.name()
        .filter(|n| ast::is_identifier(*n))
        .map(|n| n.text())
}

/// Whether `node` is inside the function declaration called `name`.
fn inside_function(node: P<Node>, name: &str) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == Kind::FunctionDeclaration && name_of(parent) == Some(name) {
            return true;
        }
        current = parent.parent();
    }
    false
}

/// The node of `kind` whose own text is exactly `text` (inside the function `within`, when given). Zero or several is an `Err` for the
/// case to print as its answer.
fn find(file: &File, kind: Kind, text: &str, within: Option<&str>) -> Result<P<Node>, String> {
    let mut found = Vec::new();
    walk(file.node.as_node(), &mut |node| {
        if node.kind() == kind
            && own_text(file.text, node) == text
            && within.is_none_or(|name| inside_function(node, name))
        {
            found.push(node);
        }
    });
    match found.as_slice() {
        [one] => Ok(*one),
        other => Err(format!(
            "not callable ({} nodes of kind {kind:?} with text `{text}` in {})",
            other.len(),
            file.name
        )),
    }
}

/// The nearest enclosing function, method or class of `node` (not `node` itself), by name.
fn def_name(node: P<Node>) -> String {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(
            parent.kind(),
            Kind::FunctionDeclaration
                | Kind::MethodDeclaration
                | Kind::ClassDeclaration
                | Kind::Constructor
                | Kind::FunctionExpression
                | Kind::ArrowFunction
                | Kind::ClassExpression
        ) {
            return name_of(parent).unwrap_or("<anonymous>").to_owned();
        }
        current = parent.parent();
    }
    "<top level>".to_owned()
}

/// `Kind in def`, and the same with the place: what is compared, and what is printed.
fn declaration(files: &[File], node: P<Node>) -> (String, String) {
    let compared = format!("{:?} in {}", node.kind(), def_name(node));
    let mut root = node;
    while let Some(parent) = root.parent() {
        root = parent;
    }
    let place = files
        .iter()
        .find(|file| file.node.as_node() == root)
        .map(|file| {
            let line = 1 + file.text[..start_offset(file.text, node)]
                .matches('\n')
                .count();
            format!("{}:{line}", file.name)
        })
        .unwrap_or_else(|| "outside the fixture".to_owned());
    let shown = format!("{compared} ({place})");
    (compared, shown)
}

fn names(symbols: &[P<ast::Symbol>]) -> String {
    let all: Vec<_> = symbols.iter().map(|s| s.name()).collect();
    format!("[{}]", all.join(", "))
}

fn line_of(file: &File, node: P<Node>) -> usize {
    1 + file.text[..start_offset(file.text, node)]
        .matches('\n')
        .count()
}

fn short(text: &str) -> String {
    let one_line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.len() > 32 {
        format!("{}…", &one_line[..32])
    } else {
        one_line
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        if let Ok(mut slot) = PANIC.lock() {
            *slot = info.to_string().replace('\n', " ");
        }
    }));

    // ---- load: the sequence TC1 will repeat inside `sem` ----
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixture")
        .canonicalize()
        .expect("fixture folder");
    let cwd = dir.to_string_lossy().replace('\\', "/");
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(cachedvfs::from(osvfs::fs())));
    let host = ConfigHost {
        fs: Arc::clone(&fs),
        cwd: cwd.clone(),
    };

    let region = Region::new_scratch(CHUNK);
    let loaders = Arc::new(ProgramRegions::default());
    let program = {
        let _scope = region.enter();
        let (config, diagnostics) = tsrs_tsoptions::get_parsed_command_line_of_config_file(
            &format!("{cwd}/tsconfig.json"),
            None,
            None,
            &host,
            None,
        );
        assert!(
            diagnostics.is_empty(),
            "config diagnostics: {}",
            diagnostics[0].localize()
        );
        let mut config = config.expect("config");
        let options = config
            .compiler_options()
            .map(|o| (*o).clone())
            .unwrap_or_default();
        config.parsed_config.compiler_options = Some(P::new(CompilerOptions {
            checkers: Some(1),
            ..options
        }));
        let compiler_host = tsrs_compiler::new_compiler_host(
            &cwd,
            fs,
            &bundled::default_lib_path(),
            None,
            None,
            None,
        );
        let mut options = ProgramOptions::new(P::new(config), compiler_host);
        options.use_source_of_project_reference = true;
        options.checker_regions = true;
        options.regions = Some(Arc::clone(&loaders));
        let program = tsrs_compiler::new_program(options);
        program.bind_source_files();
        program
    };

    let file = |name: &'static str| {
        let node = program
            .get_source_file(&format!("{cwd}/src/{name}"))
            .unwrap_or_else(|| panic!("{name} is not in the program"));
        File {
            name,
            node,
            text: node.text(),
        }
    };
    let files = [file("tour.tsx"), file("extra.ts")];
    let (tour, extra) = (&files[0], &files[1]);

    let mut rep = Rep { ok: 0, no: 0 };
    {
        // the one checker handle of the run
        let mut handle = program.get_type_checker(&Context::default());
        let chk: &mut Checker = &mut handle;
        e1(&mut rep, chk, extra);
        e2(&mut rep, chk, &files, extra);
        e3(&mut rep, chk, extra);
        e4(&mut rep, chk, extra);
        e5(&mut rep, chk, &files);
        e6(&mut rep, chk, &files);
        e7(&mut rep, chk, tour, extra);
        e8(&mut rep, chk, &files, extra);
        e9(&mut rep, chk, extra);
        e10(&mut rep, chk, &files, tour);
        e11(&mut rep, chk, extra);
    }

    // ---- the checker's own diagnostics, after the cases (a second handle while one is held would wait for ever) ----
    for f in &files {
        let diagnostics = program.get_semantic_diagnostics(&Context::default(), Some(f.node));
        println!(
            "diag {}: {} semantic diagnostics",
            f.name,
            diagnostics.len()
        );
        for d in diagnostics {
            let line = d
                .pos()
                .map(|p| 1 + f.text[..p as usize].matches('\n').count());
            println!(
                "diag   TS{} line {}: {}",
                d.code(),
                line.map_or("?".to_owned(), |l| l.to_string()),
                d.localize()
            );
        }
    }
    println!("ok: {}  NO: {}", rep.ok, rep.no);
}

fn e1(rep: &mut Rep, chk: &mut Checker, extra: &File) {
    guard(rep, "E1", |rep| {
        let ask = "get_resolved_signature_exported(withThis(handler))";
        let call = match find(extra, Kind::CallExpression, "withThis(handler)", None) {
            Ok(call) => call,
            Err(why) => return rep.row("E1", ask, why, "[cb]"),
        };
        let sig = chk.get_resolved_signature_exported(call);
        rep.row(
            "E1",
            &format!("{ask}.parameters()"),
            names(sig.parameters()),
            "[cb]",
        );
        let this = sig
            .this_parameter()
            .map_or("None".to_owned(), |s| s.name().to_owned());
        rep.row("E1", &format!("{ask}.this_parameter()"), this, "this");
    });
}

fn e2(rep: &mut Rep, chk: &mut Checker, files: &[File], extra: &File) {
    guard(rep, "E2", |rep| {
        let ask = "get_resolved_signature_exported(run(1, handler))";
        let call = match find(extra, Kind::CallExpression, "run(1, handler)", None) {
            Ok(call) => call,
            Err(why) => return rep.row("E2", ask, why, "Parameter, own: true"),
        };
        let own = [
            find(extra, Kind::Parameter, "x: T", Some("run")),
            find(extra, Kind::Parameter, "cb: (v: T) => void", Some("run")),
        ];
        let sig = chk.get_resolved_signature_exported(call);
        let parameters = sig.parameters();
        if parameters.len() != 2 {
            rep.row(
                "E2",
                &format!("{ask}.parameters().len()"),
                parameters.len(),
                "2",
            );
        }
        for (index, symbol) in parameters.iter().enumerate() {
            let ask = format!(
                "{ask}.parameters()[{index}] ({}).value_declaration()",
                symbol.name()
            );
            let Some(declaration_node) = symbol.value_declaration() else {
                rep.row("E2", &ask, "None", "Parameter, own: true");
                continue;
            };
            let (_, place) = declaration(files, declaration_node);
            let same = matches!(own.get(index), Some(Ok(node)) if *node == declaration_node);
            let compared = format!("{:?}, own: {same}", declaration_node.kind());
            rep.shown(
                "E2",
                &ask,
                &format!("{compared} ({place})"),
                &compared,
                "Parameter, own: true",
            );
        }
    });
}

fn e3(rep: &mut Rep, chk: &mut Checker, extra: &File) {
    guard(rep, "E3", |rep| {
        let ask = "get_type_at_position_exported(sig(useState(false)), 0)";
        let call = match find(extra, Kind::CallExpression, "useState(false)", None) {
            Ok(call) => call,
            Err(why) => return rep.row("E3", ask, why, "boolean"),
        };
        let sig = chk.get_resolved_signature_exported(call);
        let ty = chk.get_type_at_position_exported(sig, 0);
        rep.row("E3", ask, chk.type_to_string_exported(ty), "boolean");
    });
}

fn e4(rep: &mut Rep, chk: &mut Checker, extra: &File) {
    guard(rep, "E4", |rep| {
        let call_text = "many(1, handler, handler)";
        let call = match find(extra, Kind::CallExpression, call_text, None) {
            Ok(call) => call,
            Err(why) => return rep.row("E4", call_text, why, "true"),
        };
        let sig = chk.get_resolved_signature_exported(call);
        rep.row(
            "E4",
            &format!("has_rest_parameter({call_text})"),
            sig.has_rest_parameter(),
            "true",
        );
        rep.row(
            "E4",
            &format!("min_argument_count({call_text})"),
            sig.min_argument_count(),
            "1",
        );
        rep.row(
            "E4",
            &format!("parameters().len({call_text})"),
            sig.parameters().len(),
            "2",
        );
        for position in 1..=3 {
            let ty = chk.get_type_at_position_exported(sig, position);
            rep.row(
                "E4",
                &format!("get_type_at_position_exported({call_text}, {position})"),
                chk.type_to_string_exported(ty),
                "() => void",
            );
        }
    });
}

fn e5(rep: &mut Rep, chk: &mut Checker, files: &[File]) {
    guard(rep, "E5", |rep| {
        let (tour, extra) = (&files[0], &files[1]);
        // `each(encode)`: the argument `encode` to `each`'s slot 0
        let call = match find(tour, Kind::CallExpression, "each(encode)", Some("main")) {
            Ok(call) => call,
            Err(why) => return rep.row("E5", "each(encode)", why, "true"),
        };
        let sig = chk.get_resolved_signature_exported(call);
        let argument = call.arguments()[0];
        let source = chk.get_type_at_location(argument);
        let target = chk.get_type_at_position_exported(sig, 0);
        let (source_text, target_text) = (
            chk.type_to_string_exported(source),
            chk.type_to_string_exported(target),
        );
        let assignable = chk.is_type_assignable_to_exported(source, target);
        rep.row(
            "E5",
            &format!("is_type_assignable_to_exported(each(encode) arg `{source_text}`, slot 0 `{target_text}`)"),
            assignable,
            "true",
        );

        // `1` (the first argument of `many(1, handler, handler)`) held against `many`'s slot 1, a function: not assignable
        let call = match find(
            extra,
            Kind::CallExpression,
            "many(1, handler, handler)",
            None,
        ) {
            Ok(call) => call,
            Err(why) => return rep.row("E5", "many(1, handler, handler)", why, "false"),
        };
        let sig = chk.get_resolved_signature_exported(call);
        let argument = call.arguments()[0];
        let source = chk.get_type_at_location(argument);
        let target = chk.get_type_at_position_exported(sig, 1);
        let (source_text, target_text) = (
            chk.type_to_string_exported(source),
            chk.type_to_string_exported(target),
        );
        let assignable = chk.is_type_assignable_to_exported(source, target);
        rep.row(
            "E5",
            &format!(
                "is_type_assignable_to_exported(many arg 0 `{source_text}`, slot 1 `{target_text}`)"
            ),
            assignable,
            "false",
        );
    });
}

fn e6(rep: &mut Rep, chk: &mut Checker, files: &[File]) {
    let tour = &files[0];
    let cases = [
        (
            "onClose()",
            "Panel",
            "BindingElement in Panel",
            "Parameter in Panel",
        ),
        ("f()", "each", "Parameter in each", "Parameter in each"),
        (
            "h()",
            "main",
            "VariableDeclaration in main",
            "VariableDeclaration in main",
        ),
    ];
    for (call_text, within, expected_declaration, expected_root) in cases {
        guard(rep, "E6", |rep| {
            let ask = format!("callee of `{call_text}` (in {within})");
            let call = match find(tour, Kind::CallExpression, call_text, Some(within)) {
                Ok(call) => call,
                Err(why) => return rep.row("E6", &ask, why, expected_declaration),
            };
            let callee = call.expression().expect("a call has a callee");
            let Some(symbol) = chk.get_symbol_at_location_exported(callee) else {
                return rep.row(
                    "E6",
                    &format!("get_symbol_at_location_exported({ask})"),
                    "None",
                    expected_declaration,
                );
            };
            let Some(value_declaration) = symbol.value_declaration() else {
                return rep.row(
                    "E6",
                    &format!("{ask}: symbol.value_declaration()"),
                    "None",
                    expected_declaration,
                );
            };
            let (compared, shown) = declaration(files, value_declaration);
            rep.shown(
                "E6",
                &format!("{ask}: value_declaration()"),
                &shown,
                &compared,
                expected_declaration,
            );
            let root = ast::get_root_declaration(value_declaration);
            let (compared, shown) = declaration(files, root);
            rep.shown(
                "E6",
                &format!("{ask}: get_root_declaration()"),
                &shown,
                &compared,
                expected_root,
            );
        });
    }
}

fn class_and_symbol(chk: &mut Checker, node: P<Node>) -> (bool, String, String) {
    let ty = chk.get_type_at_location(node);
    let class = ty.object_flags().intersects(ObjectFlags::Class);
    let symbol = ty
        .symbol()
        .map_or("None".to_owned(), |s| s.name().to_owned());
    (class, symbol, chk.type_to_string_exported(ty))
}

fn e7(rep: &mut Rep, chk: &mut Checker, tour: &File, extra: &File) {
    guard(rep, "E7", |rep| {
        let ask = "get_type_at_location(`new Json()` in send(new Json()))";
        match find(tour, Kind::NewExpression, "new Json()", None) {
            Err(why) => rep.row("E7", ask, why, "true"),
            Ok(node) => {
                let (class, symbol, printed) = class_and_symbol(chk, node);
                rep.row(
                    "E7",
                    &format!("{ask}: type `{printed}` object_flags has Class"),
                    class,
                    "true",
                );
                rep.row("E7", &format!("{ask}: type.symbol()"), symbol, "Json");
            }
        }
        let call = match find(extra, Kind::CallExpression, "send(w)", None) {
            Ok(call) => call,
            Err(why) => return rep.row("E7", "send(w)", why, "false"),
        };
        let ask = "get_type_at_location(`w` in send(w), w: Writer = new Json())";
        let (class, symbol, printed) = class_and_symbol(chk, call.arguments()[0]);
        rep.row(
            "E7",
            &format!("{ask}: type `{printed}` object_flags has Class"),
            class,
            "false",
        );
        rep.row("E7", &format!("{ask}: type.symbol()"), symbol, "Writer");
    });
}

fn e8(rep: &mut Rep, chk: &mut Checker, files: &[File], extra: &File) {
    for (text, class) in [("new Json()", "Json"), ("new Sub()", "Sub")] {
        guard(rep, "E8", |rep| {
            let ask = format!(
                "get_property_of_type_exported(type of `{text}`, \"write\").value_declaration()"
            );
            let node = match find(extra, Kind::NewExpression, text, Some("e8")) {
                Ok(node) => node,
                Err(why) => return rep.row("E8", &ask, why, "MethodDeclaration in Json"),
            };
            let ty = chk.get_type_at_location(node);
            let Some(property) = chk.get_property_of_type_exported(ty, "write") else {
                return rep.row(
                    "E8",
                    &ask,
                    format!("None (no `write` on {class})"),
                    "MethodDeclaration in Json",
                );
            };
            let Some(value_declaration) = property.value_declaration() else {
                return rep.row(
                    "E8",
                    &ask,
                    "None (no value_declaration)",
                    "MethodDeclaration in Json",
                );
            };
            let (compared, shown) = declaration(files, value_declaration);
            rep.shown("E8", &ask, &shown, &compared, "MethodDeclaration in Json");
        });
    }
}

/// The references to a symbol and `is_write_access` of each, against the expected list; a missing or extra reference is a row of its own.
fn references_case(
    rep: &mut Rep,
    chk: &mut Checker,
    id: &str,
    file: &File,
    declaration_node: Result<P<Node>, String>,
    what: &str,
    expected: &[bool],
) {
    let name = match declaration_node {
        Ok(node) => node.name().expect("a declaration with a name"),
        Err(why) => return rep.row(id, what, why, &expected.len().to_string()),
    };
    let Some(symbol) = chk.get_symbol_at_location_exported(name) else {
        return rep.row(
            id,
            &format!("get_symbol_at_location_exported({what})"),
            "None",
            "a symbol",
        );
    };
    let references = chk.get_references_to_symbol_in_file(file.node, symbol);
    rep.row(
        id,
        &format!("get_references_to_symbol_in_file({what}).len()"),
        references.len(),
        &expected.len().to_string(),
    );
    for index in 0..references.len().max(expected.len()) {
        let expect = expected
            .get(index)
            .map_or("(no such reference)".to_owned(), |w| w.to_string());
        match references.get(index) {
            Some(&reference) => {
                let context = reference
                    .parent()
                    .map_or(String::new(), |p| short(own_text(file.text, p)));
                rep.row(
                    id,
                    &format!(
                        "is_write_access(ref {index}, line {}, in `{context}`)",
                        line_of(file, reference)
                    ),
                    ast::is_write_access(reference),
                    &expect,
                );
            }
            None => rep.row(
                id,
                &format!("is_write_access(ref {index})"),
                "(no such reference)",
                &expect,
            ),
        }
    }
}

fn e9(rep: &mut Rep, chk: &mut Checker, extra: &File) {
    guard(rep, "E9", |rep| {
        let declaration_node = find(extra, Kind::PropertyDeclaration, "x = 1;", None);
        references_case(
            rep,
            chk,
            "E9",
            extra,
            declaration_node,
            "Box.x",
            &[false, true, false],
        );
    });
}

fn e10(rep: &mut Rep, chk: &mut Checker, files: &[File], tour: &File) {
    guard(rep, "E10", |rep| {
        let ask = "get_resolved_signature_exported(<Panel onClose={join} />)";
        let element = match find(
            tour,
            Kind::JsxSelfClosingElement,
            "<Panel onClose={join} />",
            None,
        ) {
            Ok(element) => element,
            Err(why) => return rep.row("E10", ask, why, "FunctionDeclaration in <top level>"),
        };
        let sig = chk.get_resolved_signature_exported(element);
        match sig.declaration() {
            Some(node) => {
                let (compared, shown) = declaration(files, node);
                let named = name_of(node).unwrap_or("<anonymous>");
                rep.shown(
                    "E10",
                    &format!("{ask}.declaration() named `{named}`"),
                    &shown,
                    &compared,
                    "FunctionDeclaration in <top level>",
                );
                rep.row("E10", &format!("{ask}.declaration() name"), named, "Panel");
            }
            None => rep.row("E10", &format!("{ask}.declaration()"), "None", "Panel"),
        }
        rep.row(
            "E10",
            &format!("{ask}.parameters().len() = {}", names(sig.parameters())),
            sig.parameters().len(),
            "1",
        );
    });
}

fn e11(rep: &mut Rep, chk: &mut Checker, extra: &File) {
    guard(rep, "E11", |rep| {
        let declaration_node = find(extra, Kind::VariableDeclaration, "k = encode", None);
        references_case(
            rep,
            chk,
            "E11",
            extra,
            declaration_node,
            "k of e11",
            &[false, true, true, true, true, true, false],
        );
    });
}
