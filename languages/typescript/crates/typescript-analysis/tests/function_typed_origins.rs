//! `function_typed_value`, split by where the function comes from (H13 (e), `docs/03-typescript/10-review-fixes.md` §6.1): four keys of `Stats.language` beside
//! the total, whose sum is the total. The split is by the leftmost name of the callee expression.
//!
//! **Written before it was run:** each entry of `policy/src/origins.ts` has the counts the design says.

mod common;

use std::path::Path;

use common::{STYLE, call_graph, cases, open_project};
use typescript_analysis::{Cancel, Entry, Options, Project};

const KEYS: [&str; 4] = [
    "function_typed_value_from_parameter",
    "function_typed_value_from_call_result",
    "function_typed_value_from_this",
    "function_typed_value_from_other",
];

/// The four counts of an entry of `policy/src/origins.ts`, in the order of `KEYS`, and the total.
fn counts(entry: &str) -> ([u64; 4], u64) {
    let root = common::tests_dir().join("policy");
    let project = Project::open(&root, Path::new("src/origins.ts")).unwrap();
    let graph = call_graph(
        &project,
        "src/origins.ts",
        &Entry::Def {
            qualname: entry,
            occurrence: 0,
        },
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .unwrap_or_else(|| panic!("no def `{entry}`"));
    let language = |key: &str| graph.stats.language.get(key).copied().unwrap_or(0);
    (KEYS.map(language), language("function_typed_value"))
}

#[test]
fn a_parameter_or_a_name_taken_out_of_one_is_from_a_parameter() {
    // `cb()`, `props.go()`, `onClose()` (a pattern in the parameter list), `(maybe!)?.go?.()`
    assert_eq!(counts("fromParameter"), ([4, 0, 0, 0], 4));
}

#[test]
fn a_variable_with_a_call_as_its_initializer_is_from_a_call_result() {
    // `setOpen()` (an array pattern), `makeIt()`
    assert_eq!(counts("fromCallResult"), ([0, 2, 0, 0], 2));
}

#[test]
fn a_name_that_comes_through_an_import_is_the_declaration_the_import_leads_to() {
    assert_eq!(counts("fromImport"), ([0, 1, 0, 0], 1));
}

#[test]
fn this_is_from_this() {
    assert_eq!(counts("Store.run"), ([0, 0, 1, 0], 1));
}

#[test]
fn anything_else_is_from_other() {
    // `later()` (an arrow in a `let`), `declared()` (a `declare const`)
    assert_eq!(counts("fromOther"), ([0, 0, 0, 2], 2));
}

/// For every entry of the policy table, every scenario and the recursion fixture: the four add up to the total, and the total is the number of edges with the
/// reason.
#[test]
fn the_four_add_up_to_the_total_for_every_policy_entry_and_scenario() {
    let mut with_some = 0;
    for case in cases() {
        let project = open_project(&case);
        let graph = call_graph(
            &project,
            &case.file,
            &Entry::Def {
                qualname: &case.entry,
                occurrence: 0,
            },
            &Options::new(STYLE),
            &Cancel::new(),
        )
        .unwrap_or_else(|| panic!("{}: no def", case.name));
        let language = |key: &str| graph.stats.language.get(key).copied().unwrap_or(0);
        let total = language("function_typed_value");
        assert_eq!(
            KEYS.map(language).iter().sum::<u64>(),
            total,
            "{}",
            case.name
        );
        if total > 0 {
            with_some += 1;
        }
    }
    assert!(with_some > 5, "only {with_some} cases have one");
}
