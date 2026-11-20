//! The shared checks on every fixture: the outline equals its reviewed snapshot and nothing panics on a cut file; and the rows of docs/08-bend/03 §1.1 as
//! plain assertions beside the snapshots. The marker round trip is not here: markers are BD2 step 2 (Q-BD1 is open).

use std::fs;

use analyzer_testkit::syntax::{Subject, cut_every_50_bytes, snapshot_every_fixture};
use bend_syntax::{Def, DefKind, Edit, MarkerStyle, Outline, Placement, occurrences, outline};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

fn outline_of(text: &str) -> Outline {
    outline(text, STYLE)
}

/// Step 2 fills this in. Until then no def accepts a marker, which is why `marker_round_trip` (it fails on a planner that plans nothing) is not run.
fn plan(_text: &str, _def: &Def, _marker: &str) -> Option<Edit> {
    None
}

const BEND: Subject = Subject {
    outline: outline_of,
    plan_marker: plan,
    fixtures: "tests/fixtures",
    snapshots: "tests/snapshots",
};

fn fixture(name: &str) -> String {
    fs::read_to_string(format!("tests/fixtures/{name}.bend"))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn names(o: &Outline) -> Vec<(&str, DefKind)> {
    o.defs
        .iter()
        .map(|d| (d.qualname.as_str(), d.kind.clone()))
        .collect()
}

#[test]
fn every_fixture_equals_its_snapshot() {
    snapshot_every_fixture(&BEND);
}

#[test]
fn a_cut_file_never_panics() {
    cut_every_50_bytes(&BEND);
}

/// Every byte of the short fixtures, too: a cut anywhere gives a well-formed outline whose defs lie inside the text.
#[test]
fn a_file_cut_at_every_byte_is_well_formed() {
    for entry in fs::read_dir("tests/fixtures").expect("fixtures") {
        let text = fs::read_to_string(entry.expect("entry").path()).expect("text");
        if text.len() >= 400 {
            continue;
        }
        for at in (0..=text.len()).filter(|&at| text.is_char_boundary(at)) {
            let o = outline_of(&text[..at]);
            assert!(o.is_well_formed(), "cut at {at} of {text:?}");
            assert!(
                o.defs
                    .iter()
                    .all(|d| d.range.end as usize <= at && d.name_range.end as usize <= at),
                "cut at {at} of {text:?}"
            );
        }
    }
}

/// Bend has no nesting (D8, D10): nothing has a parent, and nothing inside a body is a def.
#[test]
fn nothing_is_nested() {
    for entry in fs::read_dir("tests/fixtures").expect("fixtures") {
        let text = fs::read_to_string(entry.expect("entry").path()).expect("text");
        assert!(outline_of(&text).defs.iter().all(|d| d.parent.is_none()));
    }
    let o = outline_of(&fixture("nested_not_defs"));
    assert_eq!(
        names(&o),
        [("outer", DefKind::Function), ("with_do", DefKind::Function)]
    );
}

/// D1, D2: the qualname is the name as written, dots included; `@unsafe` and `?` are not in it and do not change the kind.
#[test]
fn rows_d1_and_d2_a_def_is_a_function_named_as_written() {
    let o = outline_of(&fixture("def_plain"));
    assert_eq!(
        names(&o),
        [
            ("add", DefKind::Function),
            ("main", DefKind::Function),
            ("Nat.double", DefKind::Function)
        ]
    );
    assert_eq!(o.defs[0].params, ["a", "b"]);
    let o = outline_of(&fixture("def_unsafe"));
    assert_eq!(
        names(&o),
        [
            ("f", DefKind::Function),
            ("g", DefKind::Function),
            ("h", DefKind::Function)
        ]
    );
}

/// D3, D4: a foreign def and a template are defs; the marks are in `params`.
#[test]
fn rows_d3_and_d4_foreign_and_template() {
    let o = outline_of(&fixture("def_foreign"));
    assert_eq!(
        names(&o),
        [("sin", DefKind::Function), ("cos", DefKind::Function)]
    );
    let o = outline_of(&fixture("def_template"));
    assert_eq!(o.defs[0].params, ["~f", "x"]);
    assert_eq!(o.defs[1].params, ["-A", "n", "+x"]);
}

/// D5, D6, BD-D4: a law is `Other("law")`, the def that proves it a `Function`, one qualname, occurrences 0 and 1; the law's clauses give its params.
#[test]
fn rows_d5_and_d6_a_law_and_its_proof_share_a_qualname() {
    let o = outline_of(&fixture("law_and_proof"));
    assert_eq!(
        names(&o),
        [
            ("add_zero", DefKind::Other("law".to_string())),
            ("add_zero", DefKind::Function),
            ("clauses", DefKind::Other("law".to_string()))
        ]
    );
    assert_eq!(occurrences(&o), [0, 1, 0]);
    assert_eq!(o.defs[0].params, ["n"]);
    assert_eq!(o.defs[1].params, ["n"]);
    assert_eq!(o.defs[2].params, ["~k", "-A", "+x", "y"]);
}

/// A def that fills a law of an imported file is named as it is written, through the alias: the alias is part of the qualname (03 §1.1).
#[test]
fn a_def_filling_an_aliased_law_keeps_the_alias() {
    let o = outline_of(&fixture("def_fills_aliased_law"));
    assert_eq!(names(&o), [("M.name", DefKind::Function)]);
}

/// D7, D8: a type is an `Enum`, its constructors are not defs, its parameters are `params`.
#[test]
fn rows_d7_and_d8_a_type_is_an_enum_and_its_constructors_are_not_defs() {
    let o = outline_of(&fixture("type_plain"));
    assert_eq!(
        names(&o),
        [("List", DefKind::Enum), ("Unit", DefKind::Enum)]
    );
    assert_eq!(o.defs[0].params, ["a", "-A"]);
}

/// D9: an import is not a def.
#[test]
fn row_d9_an_import_is_not_a_def() {
    assert_eq!(
        names(&outline_of(&fixture("imports"))),
        [("main", DefKind::Function)]
    );
    assert!(outline_of(&fixture("imports_only")).defs.is_empty());
    assert!(outline_of(&fixture("only_comments")).defs.is_empty());
}

/// 03 §1.2: the range starts at the comment block directly above, else at the first token; a blank line between makes the block not the def's.
#[test]
fn the_range_covers_the_doc_comment_directly_above() {
    let text = fixture("def_plain");
    let o = outline_of(&text);
    let slice = |d: &Def| text[d.range.start as usize..d.range.end as usize].to_string();
    assert!(
        slice(&o.defs[0]).starts_with("# Adds two numbers.\n# Second line of the doc.\ndef add(")
    );
    assert!(
        slice(&o.defs[1]).starts_with("def main"),
        "a blank line separates the comment: {:?}",
        slice(&o.defs[1])
    );
    let text = fixture("def_unsafe");
    let o = outline_of(&text);
    // the comment sits above `@ unsafe`: the range starts at the comment
    assert!(text[o.defs[1].range.start as usize..].starts_with("# the doc of g"));
}

/// 03 §1.2: `name_range` is the name token only.
#[test]
fn the_name_range_is_the_name_only() {
    for entry in fs::read_dir("tests/fixtures").expect("fixtures") {
        let text = fs::read_to_string(entry.expect("entry").path()).expect("text");
        for d in outline_of(&text).defs {
            assert_eq!(
                &text[d.name_range.start as usize..d.name_range.end as usize],
                d.name
            );
        }
    }
}

/// BD-D10: a broken def keeps its name and range and is flagged; the ones around it are intact. A syntax error is one per `ParseError`.
#[test]
fn a_broken_def_is_flagged_and_the_others_are_not() {
    let o = outline_of(&fixture("broken_between"));
    let flags: Vec<(&str, bool)> = o
        .defs
        .iter()
        .map(|d| (d.qualname.as_str(), d.syntax_error))
        .collect();
    assert_eq!(flags, [("first", false), ("broken", true), ("last", false)]);
    assert_eq!(o.syntax_errors.len(), 1);
    assert!(
        o.syntax_errors[0].message.starts_with("expected a name"),
        "{}",
        o.syntax_errors[0].message
    );
    assert_eq!(
        o.defs[1].params,
        ["a"],
        "the parameters read before the error are kept"
    );
    let o = outline_of(&fixture("stray_text"));
    assert_eq!(names(&o).len(), 2);
    assert!(
        o.defs.iter().all(|d| !d.syntax_error),
        "stray text between defs breaks none of them"
    );
    assert_eq!(o.syntax_errors.len(), 1);
}

#[test]
fn the_file_marker_is_not_read_yet() {
    for entry in fs::read_dir("tests/fixtures").expect("fixtures") {
        let text = fs::read_to_string(entry.expect("entry").path()).expect("text");
        let o = outline_of(&text);
        assert!(o.file_marker.is_none());
        assert!(o.defs.iter().all(|d| d.marker.is_none()));
    }
}
