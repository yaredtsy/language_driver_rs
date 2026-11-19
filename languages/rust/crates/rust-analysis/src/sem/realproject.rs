//! RS4 §8, "a real project": this repo's `core/`, every def an entry. No panic, no entry that is `None`, the graph's invariants on every graph, and the
//! numbers of the whole: each site of each def counted **once** (the def's own body, not what it reaches, which would count a callee's sites once per caller).
//! They are the first real `open_*` numbers, and printed so they can be written into `ENGINE.md` with the date.

use std::collections::BTreeMap;
use std::path::Path;

use analyzer_core::outline::DefKind;
use analyzer_testkit::graph::check_invariants;

use super::testing::STYLE;
use super::{Found, Reason, attached};
use crate::env::Env;
use crate::{Cancel, Entry, LoadOptions, MarkerStyle, Options, Project, call_graph};

/// What can start a graph (`Sem::entry`): a function, a method, a struct, an enum, a const, a static.
fn is_entry_kind(kind: &DefKind) -> bool {
    match kind {
        DefKind::Function | DefKind::Method | DefKind::Struct | DefKind::Enum | DefKind::Union => {
            true
        }
        DefKind::Other(what) => what == "const" || what == "static",
        _ => false,
    }
}

#[test]
fn every_def_of_core_is_an_entry_and_the_whole_is_counted() {
    let _load = crate::sem::common::one_load();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    let project = Project::open(
        &repo,
        Path::new("core/analyzer-core/src/lib.rs"),
        &LoadOptions::default(),
    )
    .expect("core/ loads");
    assert_eq!(
        project.warnings(),
        [],
        "a complete load: the numbers below are rung 1's"
    );
    let files: Vec<String> = project
        .files()
        .into_iter()
        .filter(|file| file.starts_with("core/"))
        .collect();
    let options = Options::new(MarkerStyle::docstring("ID"));

    let mut entries = 0u64;
    let mut sum: BTreeMap<String, u64> = BTreeMap::new();
    let mut bump = |key: &str| *sum.entry(key.to_owned()).or_default() += 1;
    attached(project.loaded(), STYLE, |sem| {
        for file in &files {
            let text = std::fs::read_to_string(repo.join(file)).unwrap();
            let outline = rust_syntax::outline(&text, rust_syntax::Edition::E2024, STYLE);
            for (def, occurrence) in outline.defs.iter().zip(rust_syntax::occurrences(&outline)) {
                if !is_entry_kind(&def.kind) {
                    continue;
                }
                let graph = call_graph(
                    &project,
                    file,
                    &Entry::Def {
                        qualname: &def.qualname,
                        occurrence,
                    },
                    &options,
                    &Cancel::new(),
                )
                .unwrap_or_else(|| panic!("{file}:{} is an entry", def.qualname));
                check_invariants(&graph);
                entries += 1;

                // the def's own body: each site once
                let func = sem
                    .entry(file, &def.qualname, occurrence)
                    .expect("the same entry");
                let body = crate::walk::read(sem, func, &Env, &Cancel::new());
                for _ in 0..body.macro_not_expanded {
                    bump("macro_not_expanded");
                }
                for resolved in &body.sites {
                    bump("sites");
                    let passed = resolved.raw.is_passed();
                    for callee in &resolved.callees {
                        if let Some(open) = callee.open {
                            bump(open.counter());
                        }
                        match &callee.found {
                            Found::Def(_) if passed => bump("passed"),
                            Found::Def(_) => bump("calls"),
                            Found::Construct(_) => bump(if passed {
                                "constructor_passed"
                            } else {
                                "constructs"
                            }),
                            Found::External {
                                construct, cause, ..
                            } => {
                                bump(match (passed, construct) {
                                    (true, true) => "constructor_passed",
                                    (true, false) => "external_passed",
                                    (false, _) => "externals",
                                });
                                if !passed && !construct {
                                    bump(&format!("external_cause_{cause:?}"));
                                }
                            }
                            // a trait's required method handed over: no edge, as the walk counts it (D44)
                            Found::Unknown(Reason::FunctionTypedValue) if passed => {
                                bump("signature_passed")
                            }
                            Found::Unknown(reason) => {
                                bump(&format!("unresolved_{}", reason.counter()))
                            }
                        }
                    }
                }
            }
        }
    });
    println!("REAL core/: {} files, {entries} entries", files.len());
    for (key, count) in &sum {
        println!("REAL {key} {count}");
    }

    let count = |key: &str| sum.get(key).copied().unwrap_or(0);
    // the files of both crates of `core/`, and a def for every entry kind: more than a handful, so a load that lost its files fails here
    assert!(
        files.len() >= 13 && entries >= 400,
        "{} files, {entries} entries",
        files.len()
    );
    // every site is accounted for once, as exactly one of these
    let accounted: u64 = [
        "calls",
        "passed",
        "constructs",
        "externals",
        "external_passed",
        "constructor_passed",
        "signature_passed",
    ]
    .iter()
    .map(|key| count(key))
    .sum::<u64>()
        + sum
            .iter()
            .filter(|(key, _)| key.starts_with("unresolved_"))
            .map(|(_, count)| count)
            .sum::<u64>();
    assert_eq!(accounted, count("sites"), "{sum:?}");
    // a project that compiles has nothing the engine could not type, and no def the outline could not name (a bug that must show on the graph): neither
    // an unknown callee or method nor a `def_not_in_outline`. A call through a function-typed value is the one reason there is
    for reason in ["unknown_callee", "unknown_method", "def_not_in_outline"] {
        assert_eq!(count(&format!("unresolved_{reason}")), 0, "{reason}");
    }
    // `open_other` is the number that says the classification of RS4 §4 is missing a case: none on this project. A new case in `core/` that fails this is
    // the classification to extend, or the number to decide to accept
    assert_eq!(count("open_other"), 0);
    // no macro of `core/` is one the engine cannot expand
    assert_eq!(count("macro_not_expanded"), 0);
}
