//! What a call reaches when the engine's declaration is a **signature** (D43, `docs/03-typescript/10-review-fixes.md` §6.1): a method signature in a type
//! literal or an interface, a `get` or `set` signature, an abstract method. All of them are `Unresolved(function_typed_value)`, where they used to be a
//! panic (a type literal's) or a `Call` to a def with no body (an interface's). The outline does not change: the interface and its signatures stay defs.
//!
//! The fixture is `policy/src/signatures.ts`, one entry for each case. **Written before it was run:** each expected list is what the design says; an entry that
//! gave something else was reported and not copied here. `ftv` is one edge with the reason `function_typed_value`.

mod common;

use std::path::Path;

use common::{STYLE, call_graph, edges_as_text};
use typescript_analysis::{Cancel, Entry, Options, Project};

const FTV: &str = "unresolved function_typed_value";

fn edges(entry: &str) -> Vec<String> {
    let root = common::tests_dir().join("policy");
    let file = "src/signatures.ts";
    let project = Project::open(&root, Path::new(file)).unwrap();
    let graph = call_graph(
        &project,
        file,
        &Entry::Def {
            qualname: entry,
            occurrence: 0,
        },
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .unwrap_or_else(|| panic!("no def `{entry}` in {file}"));
    edges_as_text(&graph, &root)
}

/// One entry, and the edges it draws, as `site | what it is`.
fn check(entry: &str, expected: &[(&str, &str)]) {
    let expected: Vec<String> = expected
        .iter()
        .map(|(site, what)| format!("{entry} | {site} | {what}"))
        .collect();
    assert_eq!(edges(entry), expected, "{entry}");
}

#[test]
fn the_owners_two_cases() {
    check("ownerParameter", &[("fs.existsSync", FTV)]);
    check("impl.b", &[("this.a", FTV)]);
}

#[test]
fn a_method_signature_in_a_type_literal_is_a_function_typed_value_wherever_the_literal_is() {
    check("parameterType", &[("o.run", FTV)]);
    check("nestedMember", &[("a.inner.run", FTV)]);
    check("overloadsInLiteral", &[("o.m", FTV)]);
    check("intersection", &[("a.m", FTV), ("a.n", FTV)]);
    check("constraint", &[("t.m", FTV)]);
    check("throughPick", &[("p.m", FTV)]);
    check("optionalMethod", &[("o.m", FTV)]);
    check("literalInInterface", &[("i.inner.run", FTV)]);
    check("declaredConst", &[("toolbox.run", FTV)]);
    check("returnType", &[("make", "call make"), ("make().run", FTV)]);
    check(
        "genericArgument",
        &[
            ("m.get", "external typescript:lib Map.get"),
            ("m.get(\"a\")!.run", FTV),
        ],
    );
    check("inNamespace", &[("t.m", FTV)]);
    check("classProperty", &[("k.ops.run", FTV)]);
    check("taggedTemplate", &[("t.tag", FTV)]);
}

#[test]
fn an_accessor_signature_in_a_type_literal_or_an_interface_is_a_function_typed_value() {
    check("readsTypeGetter", &[("g.v", FTV)]);
    check("writesTypeSetter", &[("g.v", FTV)]);
    check("readsInterfaceGetter", &[("g.v", FTV)]);
}

#[test]
fn an_interface_method_and_an_abstract_method_are_function_typed_values_and_stay_defs() {
    check("viaInterface", &[("w.write", FTV)]);
    check("interfaceOverloads", &[("o.m", FTV)]);
    check("abstractMethod", &[("b.m", FTV)]);
    check("declaredGlobal", &[("window.foo", FTV)]);
    // a class receiver is the class's method; a union with an interface says the interface's member has nothing to enter
    check("classReceiver", &[("w.write", "call JsonWriter.write")]);
    check(
        "unionWithInterface",
        &[
            ("w.write", "call JsonWriter.write"),
            ("w.write", "unresolved union_receiver"),
        ],
    );
}

#[test]
fn the_neighbours_of_a_signature_answer_as_they_did() {
    check("callSignature", &[("f", FTV)]);
    check("interfaceCallSignature", &[("f", FTV)]);
    check("constructSignature", &[("c", FTV)]);
    check("interfaceConstructSignature", &[("c", FTV)]);
    check("functionType", &[("f", FTV)]);
    check("constructorMember", &[("t.Make", FTV)]);
    check("declaredClass", &[("d.m", "call Declared.m")]);
    check("indexSignature", &[("r.x", "unresolved no_declaration")]);
    check("record", &[("r.x", "unresolved no_declaration")]);
}

/// The interface, its signatures and the abstract method are still defs of the outline: a marker on one keeps its place (rule 10).
#[test]
fn the_outline_still_lists_the_interface_its_signatures_and_the_abstract_method() {
    let root = common::tests_dir().join("policy");
    let text = std::fs::read_to_string(root.join("src/signatures.ts")).unwrap();
    let outline = typescript_syntax::outline(&text, typescript_syntax::Dialect::Ts, STYLE);
    let names: Vec<&str> = outline
        .defs
        .iter()
        .map(|def| def.qualname.as_str())
        .collect();
    for expected in ["Writer", "Writer.write", "Base", "Base.m"] {
        assert!(names.contains(&expected), "{expected} is not in {names:?}");
    }
}
