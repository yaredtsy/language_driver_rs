//! `outline_with_nodes`: one node span per def, in the order of `Outline::defs`, and the node is the one whose body is the def's body, not always the
//! node `Def::range` starts at.

use typescript_syntax::{Dialect, MarkerStyle, outline, outline_with_nodes};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: typescript_syntax::Placement::Docstring,
};

/// (qualname, the text of the node) for every def of `text`.
fn nodes(text: &str) -> Vec<(String, String)> {
    let (outline, spans) = outline_with_nodes(text, Dialect::Ts, STYLE);
    assert_eq!(outline.defs.len(), spans.len());
    outline
        .defs
        .iter()
        .zip(spans)
        .map(|(def, span)| (def.qualname.clone(), span.slice(text).unwrap().to_owned()))
        .collect()
}

#[test]
fn it_reports_the_same_outline_as_outline() {
    let text = "export function f() {}\nclass C { m() {} }\nconst g = () => {};\n";
    assert_eq!(
        outline_with_nodes(text, Dialect::Ts, STYLE).0,
        outline(text, Dialect::Ts, STYLE)
    );
}

#[test]
fn an_overloaded_def_names_its_implementation_not_the_first_signature() {
    let text = "function dup(a: string): string;\nfunction dup(a: any): any { return a; }\n";
    let defs = nodes(text);
    assert_eq!(
        defs,
        [(
            "dup".into(),
            "function dup(a: any): any { return a; }".into()
        )]
    );
    // while the def's own range runs from the first signature
    assert_eq!(outline(text, Dialect::Ts, STYLE).defs[0].range.start, 0);
}

#[test]
fn a_const_arrow_names_the_arrow_not_the_statement() {
    let defs = nodes("export const arrow = (a: number) => a + 1;\n");
    assert_eq!(defs, [("arrow".into(), "(a: number) => a + 1".into())]);
}

#[test]
fn members_constructors_accessors_and_classes_name_their_own_node() {
    let text = "class Box {\n  constructor() {}\n  open() {}\n  get size() { return 1; }\n  handle = () => {};\n}\n";
    let defs = nodes(text);
    let texts: Vec<(&str, &str)> = defs.iter().map(|(q, t)| (q.as_str(), t.as_str())).collect();
    assert_eq!(
        texts,
        [
            ("Box", text.trim_end()),
            ("Box.constructor", "constructor() {}"),
            ("Box.open", "open() {}"),
            ("Box.size", "get size() { return 1; }"),
            ("Box.handle", "() => {}"),
        ]
    );
}

#[test]
fn a_def_with_no_body_names_its_declaration() {
    let text = "interface Writer { write(x: string): void; }\nenum E { A }\ntype T = string;\nnamespace N { export function f() {} }\n";
    let defs = nodes(text);
    let texts: Vec<(&str, &str)> = defs.iter().map(|(q, t)| (q.as_str(), t.as_str())).collect();
    assert_eq!(
        texts,
        [
            ("Writer", "interface Writer { write(x: string): void; }"),
            ("Writer.write", "write(x: string): void;"),
            ("E", "enum E { A }"),
            ("T", "type T = string;"),
            ("N", "namespace N { export function f() {} }"),
            // a modifier is part of the node: `export` is inside the span
            ("N.f", "export function f() {}"),
        ]
    );
}

#[test]
fn a_nested_function_names_its_own_node() {
    let defs = nodes("function outer() { function inner() {} return inner; }\n");
    assert_eq!(
        defs[1],
        ("outer.<locals>.inner".into(), "function inner() {}".into())
    );
}
