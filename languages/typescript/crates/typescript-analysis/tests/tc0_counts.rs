//! The TC0 counts N2, N3 and N8 (`docs/03-typescript/12-context-layer/04-tc0-counts-and-spikes.md` §3.3): `function_typed_value`, split further, N1 (§3.4): what
//! callers pass, N4 and N7 (§3.5): how often a local is written and where `this.x` gets its value, and N5 (§3.6): what is done with an object, in keys `tc0_…` of `Stats.language`.
//! They exist only behind the feature `__tc0_counts`, so this file is empty without it.
//!
//! **Written before it was run:** the counts of each file of `fixtures/tc0-counts`, by hand, from the table of the design.
#![cfg(feature = "__tc0_counts")]

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::{STYLE, assert_no_outline_gap, tests_dir};
use typescript_analysis::{Budget, Cancel, Entry, Options, Session};

/// The `tc0_` keys (and `function_typed_value_from_this`) summed over `entries` of `file`, each asked at depth 1, and the sum of `function_typed_value`.
fn counts(file: &str, entries: &[&str]) -> (BTreeMap<String, u64>, u64) {
    let root = tests_dir().join("fixtures").join("tc0-counts");
    let session = Session::open(&root, Path::new(file), Options::new(STYLE)).unwrap();
    let budget = Budget::new(Some(1), None);
    let mut sums: BTreeMap<String, u64> = BTreeMap::new();
    let mut total = 0;
    for qualname in entries {
        let entry = Entry::Def {
            qualname,
            occurrence: 0,
        };
        let graph = session
            .graph(file, &entry, &budget, &Cancel::new())
            .unwrap_or_else(|| panic!("no def `{qualname}` in {file}"));
        assert_no_outline_gap(&graph);
        for (key, count) in &graph.stats.language {
            if key.starts_with("tc0_") || key == "function_typed_value_from_this" {
                *sums.entry(key.clone()).or_default() += count;
            }
        }
        total += graph
            .stats
            .language
            .get("function_typed_value")
            .copied()
            .unwrap_or(0);
    }
    (sums, total)
}

/// The keys of `found` that start with one of `prefixes`: each test compares the keys of its own counts, since a count of another kind adds keys to every def.
fn only(found: BTreeMap<String, u64>, prefixes: &[&str]) -> BTreeMap<String, u64> {
    found
        .into_iter()
        .filter(|(key, _)| prefixes.iter().any(|prefix| key.starts_with(prefix)))
        .collect()
}

fn expected(rows: &[(&str, u64)]) -> BTreeMap<String, u64> {
    rows.iter()
        .map(|(key, n)| ((*key).to_owned(), *n))
        .collect()
}

#[test]
fn a_parameter_is_split_by_whose_it_is_how_deep_and_called_or_called_on() {
    let (found, total) = counts(
        "src/params.ts",
        &[
            "own",
            "viaMember",
            "viaPattern",
            "deep",
            "deeper",
            "patternAndMember",
            "method",
            "outer.<locals>.inner",
            "anon",
        ],
    );
    assert_eq!(total, 9);
    assert_eq!(
        only(found, &["tc0_n2_", "tc0_n3_"]),
        expected(&[
            ("tc0_n2_own", 7),
            ("tc0_n2_enclosing", 1),
            ("tc0_n2_anonymous", 1),
            ("tc0_n2_depth_0", 3),
            ("tc0_n2_depth_1", 3),
            ("tc0_n2_depth_2", 2),
            ("tc0_n2_depth_3_or_more", 1),
            ("tc0_n3_value", 8),
            ("tc0_n3_method", 1),
        ])
    );
}

#[test]
fn a_call_result_is_split_by_the_call_that_made_it() {
    let (found, total) = counts("src/results.ts", &["results"]);
    assert_eq!(total, 3);
    assert_eq!(
        only(found, &["tc0_n8_"]),
        expected(&[
            ("tc0_n8_package:fakelib:useState", 1),
            ("tc0_n8_package:fakelib:make", 1),
            ("tc0_n8_package_bound_direct", 1),
            ("tc0_n8_project", 1),
        ])
    );
}

#[test]
fn what_a_caller_passes_is_counted_by_class() {
    let (found, _) = counts("src/args.tsx", &["callers", "Box.go"]);
    assert_eq!(
        only(found, &["tc0_n1:"]),
        expected(&[
            ("tc0_n1:arg:def", 1),
            ("tc0_n1:arg:anonymous", 1),
            ("tc0_n1:arg:object_with_function", 1),
            ("tc0_n1:arg:object_other", 1),
            ("tc0_n1:arg:parameter", 1),
            ("tc0_n1:arg:parameter_member", 1),
            ("tc0_n1:arg:local", 2),
            ("tc0_n1:arg:module_variable", 1),
            ("tc0_n1:arg:new", 1),
            ("tc0_n1:arg:call_result_project", 1),
            ("tc0_n1:arg:call_result_external", 1),
            ("tc0_n1:arg:other:NoSubstitutionTemplateLiteral", 1),
            ("tc0_n1:arg:this_member", 1),
            ("tc0_n1:arg:data_slot", 1),
            ("tc0_n1:arg:spread", 1),
            ("tc0_n1:jsx:def", 1),
            ("tc0_n1:jsx:spread", 1),
            ("tc0_n1:static_class", 2),
        ])
    );
}

#[test]
fn how_often_a_local_is_written_is_counted_per_def() {
    let (found, _) = counts("src/vars.ts", &["locals"]);
    assert_eq!(
        only(found, &["tc0_n4_"]),
        expected(&[
            ("tc0_n4_const", 2),
            ("tc0_n4_once", 2),
            ("tc0_n4_more", 4),
            ("tc0_n4_unmodelled_write", 2),
            ("tc0_n4_data", 1),
            ("tc0_n4_param", 3),
            ("tc0_n4_param_assigned", 1),
        ])
    );
}

#[test]
fn where_this_x_gets_its_value_is_counted_by_the_declaration_and_the_class() {
    let (found, total) = counts("src/fields.ts", &["Svc.run", "Base.go", "Lonely.go"]);
    assert_eq!(total, 7);
    assert_eq!(found["function_typed_value_from_this"], 7);
    assert_eq!(
        only(found, &["tc0_n7_"]),
        expected(&[
            ("tc0_n7_parameter_property", 2),
            ("tc0_n7_constructor_only", 2),
            ("tc0_n7_assigned_elsewhere", 1),
            ("tc0_n7_method", 1),
            ("tc0_n7_never_written", 1),
            ("tc0_n7_class:src/fields.ts#Svc", 5),
            ("tc0_n7_class:src/fields.ts#Base", 1),
            ("tc0_n7_class:src/fields.ts#Lonely", 1),
        ])
    );
}

#[test]
fn what_is_done_with_an_object_is_counted_per_parameter_local_and_module_const() {
    // the def at depth 1, and the file's own code
    let (def, _) = counts("src/objects.tsx", &["objects"]);
    let root = tests_dir().join("fixtures").join("tc0-counts");
    let file = "src/objects.tsx";
    let session = Session::open(&root, Path::new(file), Options::new(STYLE)).unwrap();
    let graph = session
        .graph(
            file,
            &Entry::File,
            &Budget::new(Some(1), None),
            &Cancel::new(),
        )
        .unwrap();
    let mut found = only(def, &["tc0_n5_"]);
    for (key, count) in &graph.stats.language {
        if key.starts_with("tc0_n5_") {
            *found.entry(key.clone()).or_default() += count;
        }
    }
    assert_eq!(
        found,
        expected(&[
            ("tc0_n5_local_objects", 8),
            ("tc0_n5_local_untouched", 1),
            ("tc0_n5_local_written_through", 1),
            ("tc0_n5_local_handed_project", 1),
            ("tc0_n5_local_handed_external", 1),
            ("tc0_n5_local_in_array", 1),
            ("tc0_n5_local_second_name", 1),
            ("tc0_n5_local_returned", 1),
            ("tc0_n5_local_handed_jsx", 1),
            ("tc0_n5_param_objects", 2),
            ("tc0_n5_param_written_through", 1),
            ("tc0_n5_param_handed_project", 1),
            ("tc0_n5_module_objects", 2),
            ("tc0_n5_module_exported", 1),
            ("tc0_n5_module_untouched", 1),
            ("tc0_n5_module_written_through", 1),
        ])
    );
}
