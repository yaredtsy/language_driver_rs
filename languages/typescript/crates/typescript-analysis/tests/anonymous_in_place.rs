//! An anonymous class or function written where it is constructed or called, with no parentheses (H15, `docs/03-typescript/10-review-fixes.md` §6.3). The
//! engine's call hierarchy treats it as a possible declaration and searches **up** for an enclosing one, so the answer was the function, the method or the file
//! around it: a `Call` to the enclosing def (a recursion that is not there), or, at the top of a file, a declaration that is no def (`decl_not_in_outline`).
//! There is nothing to enter: an anonymous class constructed in place is `Unresolved(no_declaration)` (the `anonymous` row, H12), an anonymous function called in
//! place is `Unresolved(anonymous_function_called)` (H10), with or without parentheses.
//!
//! **Written before it was run:** each expected list is what those two rules say.

mod common;

use std::path::Path;

use common::{STYLE, call_graph, edges_as_text};
use typescript_analysis::{Cancel, Entry, Options, Project};

fn edges(entry: &str) -> Vec<String> {
    let root = common::tests_dir().join("fixtures/anonymous-in-place");
    let project = Project::open(&root, Path::new("src/main.ts")).unwrap();
    let graph = call_graph(
        &project,
        "src/main.ts",
        &Entry::Def {
            qualname: entry,
            occurrence: 0,
        },
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .unwrap_or_else(|| panic!("no def `{entry}`"));
    edges_as_text(&graph, &root)
}

fn check(entry: &str, expected: &[(&str, &str)]) {
    let expected: Vec<String> = expected
        .iter()
        .map(|(site, what)| format!("{entry} | {site} | {what}"))
        .collect();
    assert_eq!(edges(entry), expected, "{entry}");
}

const NO_DECLARATION: &str = "unresolved no_declaration";

#[test]
fn an_anonymous_class_constructed_in_place_has_no_declaration_with_or_without_parentheses() {
    check(
        "inFunction",
        &[
            ("(class extends mock<Service>() {})", NO_DECLARATION),
            ("mock", "call mock"),
            ("(class {})", NO_DECLARATION),
            ("class extends mock<Service>() {}", NO_DECLARATION),
            ("mock", "call mock"),
            ("class {}", NO_DECLARATION),
        ],
    );
    check(
        "Outer.method",
        &[
            ("class extends mock<Service>() {}", NO_DECLARATION),
            ("mock", "call mock"),
        ],
    );
    check(
        "inArrow",
        &[
            ("class extends mock<Service>() {}", NO_DECLARATION),
            ("mock", "call mock"),
        ],
    );
}

#[test]
fn in_a_callback_at_the_top_of_a_file_the_enclosing_declaration_is_not_the_file() {
    check(
        "service.openDatabase",
        &[
            ("class extends mock<Service>() {}", NO_DECLARATION),
            ("mock", "call mock"),
        ],
    );
    // wordDistance.test.ts: the constructor of an anonymous class, which passes another one to `super`
    check(
        "constructor",
        &[
            ("super", "construct Base init Base.constructor"),
            ("class extends mock<Service>() {}", NO_DECLARATION),
            ("mock", "call mock"),
        ],
    );
}

#[test]
fn an_anonymous_function_called_or_constructed_in_place_is_counted_as_it_is_with_parentheses() {
    check(
        "functions",
        &[
            ("function () {}", "unresolved anonymous_function_called"),
            ("function () {}", "unresolved anonymous_function_called"),
        ],
    );
}
