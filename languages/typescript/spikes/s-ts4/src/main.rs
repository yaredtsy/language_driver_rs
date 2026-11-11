use tsrs_ast::{Node, SourceFile, SourceFileParseOptions};
use tsrs_core::{P, tspath};

const SRC: &str = r#"/** fn doc */
function plain() {}

/** class doc */
class K {
  /** method doc */
  m() {}

  /** prop arrow doc */
  p = () => {};

  /** getter doc */
  get g() { return 1; }
}

/** decorated doc */
@dec
class D {}

/** const arrow doc */
export const arrow = () => {};

/** exported fn doc */
export function ex() {}

const obj = {
  /** object method doc */
  om() {},
  /** object prop fn doc */
  pf: function () {},
};

/** overload 1 doc */
function ov(a: string): void;
/** overload 2 doc */
function ov(a: number): void;
function ov(a: any) {}

interface I {
  /** interface method doc */
  im(): void;
}

/** first of two */
/** second of two */
function two() {}

/** detached by blank line */

function blank() {}

/** line comment between */
// a line comment
function lc() {}

function none() {}
"#;

fn file_name() -> String {
    format!("/spike/a.{}", std::env::args().nth(1).unwrap_or_else(|| "ts".into()))
}

fn parse(text: &'static str) -> P<SourceFile> {
    let name = &file_name();
    let opts = SourceFileParseOptions {
        file_name: name.to_string(),
        path: tspath::to_path(name, "/spike", true),
        external_module_indicator_options: Default::default(),
    };
    tsrs_parser::parse_source_file(opts, text, tsrs_core::get_script_kind_from_file_name(name))
}

fn label(node: P<Node>) -> String {
    let name = node.name().map(|n| n.text().to_string()).unwrap_or_default();
    format!("{:?} {name}", node.kind())
}

fn report(file: P<SourceFile>, text: &'static str, node: P<Node>) {
    let docs = node.jsdoc(None);
    let pos = node.pos().expect_source("pos");
    let leading: Vec<_> = tsrs_scanner::get_leading_comment_ranges(text, pos)
        .filter(|c| text[c.pos() as usize..].starts_with("/**"))
        .map(|c| (c.pos(), c.end()))
        .collect();
    if !docs.is_empty() || !leading.is_empty() {
        let d: Vec<_> = docs.iter().map(|j| (j.pos().expect_source("p"), j.end().expect_source("e"))).collect();
        println!("{:<34} jsdoc={d:?} leading_/**={leading:?}", label(node));
    } else {
        println!("{:<34} jsdoc=[] leading_/**=[]", label(node));
    }
}

fn walk(file: P<SourceFile>, text: &'static str, node: P<Node>) {
    use tsrs_ast::*;
    if is_function_declaration(node) || is_class_declaration(node) || is_method_declaration(node)
        || is_variable_statement(node) || is_property_declaration(node) || is_get_accessor_declaration(node)
        || is_method_signature_declaration(node) || is_property_assignment(node) || is_variable_declaration(node)
    {
        report(file, text, node);
    }
    node.for_each_child(&mut |c| {
        walk(file, text, c);
        false
    });
}

fn fixture() -> &'static str {
    if file_name().ends_with(".js") {
        let js = SRC
            .replace(": string", "")
            .replace(": number", "")
            .replace(": any", "")
            .replace("): void;\n/**", ");\n/**")
            .replace("interface I {\n  /** interface method doc */\n  im(): void;\n}\n", "")
            .replace("@dec\nclass D", "class D");
        Box::leak(js.into_boxed_str())
    } else {
        SRC
    }
}

fn main() {
    let text: &'static str = fixture();
    let file = parse(text);
    println!("diagnostics: {}", file.diagnostics().len());
    walk(file, text, file.as_node());

    // insert " * ID: abc" into the JSDoc of `plain`, parse again
    let edited: &'static str = Box::leak(
        text.replacen("/** fn doc */", "/**\n * fn doc\n * ID: abc\n */", 1).into_boxed_str(),
    );
    let file2 = parse(edited);
    println!("--- after inserting an ID line into `plain`: diagnostics {}", file2.diagnostics().len());
    for s in file2.statements().nodes().iter().take(1) {
        report(file2, edited, *s);
    }
    overload_insert(text);
}

fn overload_insert(text: &'static str) {
    let needle = "function ov(a: any) {}";
    let needle = if text.contains(needle) { needle } else { "function ov(a) {}" };
    let Some(at) = text.find(needle) else { return };
    let edited: &'static str = Box::leak(
        format!("{}/**\n * ID: xyz\n */\n{}", &text[..at], &text[at..]).into_boxed_str(),
    );
    let file = parse(edited);
    println!("--- after inserting a new block on the overload implementation: diagnostics {}", file.diagnostics().len());
    fn find(file: P<SourceFile>, text: &'static str, node: P<Node>) {
        if tsrs_ast::is_function_declaration(node) && node.name().is_some_and(|n| n.text() == "ov") {
            report(file, text, node);
        }
        node.for_each_child(&mut |c| {
            find(file, text, c);
            false
        });
    }
    find(file, edited, file.as_node());
}

// (the overload-implementation insertion is in `overload_insert` below)
