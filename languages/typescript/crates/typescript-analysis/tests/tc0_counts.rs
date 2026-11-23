//! The TC0 counts N2, N3 and N8 (`docs/03-typescript/12-context-layer/04-tc0-counts-and-spikes.md` §3.3): `function_typed_value`, split further, in keys `tc0_…` of
//! `Stats.language`. They exist only behind the feature `__tc0_counts`, so this file is empty without it.
//!
//! **Written before it was run:** the counts of each file of `fixtures/tc0-counts`, by hand, from the table of the design.
#![cfg(feature = "__tc0_counts")]

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::{STYLE, assert_no_outline_gap, tests_dir};
use typescript_analysis::{Budget, Cancel, Entry, Options, Session};

/// The `tc0_` keys summed over `entries` of `file`, each asked at depth 1, and the sum of `function_typed_value`.
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
            if key.starts_with("tc0_") {
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
        found,
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
        found,
        expected(&[
            ("tc0_n8_package:fakelib:useState", 1),
            ("tc0_n8_package:fakelib:make", 1),
            ("tc0_n8_package_bound_direct", 1),
            ("tc0_n8_project", 1),
        ])
    );
}
