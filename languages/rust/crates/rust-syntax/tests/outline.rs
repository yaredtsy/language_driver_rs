//! The shared checks on every fixture: the outline equals its reviewed snapshot, nothing panics on a cut file, and a marker planned on each def
//! and applied gives the same defs, each with its marker.

use std::collections::BTreeMap;
use std::fs;

use analyzer_testkit::syntax::{
    Subject, cut_every_50_bytes, marker_round_trip, snapshot_every_fixture,
};
use rust_syntax::{
    Def, Edition, MarkerStyle, Outline, Placement, marker, occurrences, outline, plan_marker,
};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

fn outline_2024(text: &str) -> Outline {
    outline(text, Edition::E2024, STYLE)
}

/// The testkit's round trip wants an `Option`: a def that is refused (a `Skip`) is left alone.
fn plan(text: &str, def: &Def, marker_text: &str) -> Option<analyzer_core::edit::Edit> {
    plan_marker(text, Edition::E2024, def, marker_text, STYLE).ok()
}

const RS: Subject = Subject {
    outline: outline_2024,
    plan_marker: plan,
    fixtures: "tests/fixtures",
    snapshots: "tests/snapshots",
};

#[test]
fn every_fixture_equals_its_snapshot() {
    snapshot_every_fixture(&RS);
}

#[test]
fn a_cut_file_never_panics() {
    cut_every_50_bytes(&RS);
}

/// Plan, apply, outline again: the same defs, the def has its marker, no other def changed, and a second plan is refused (RS2 §7).
#[test]
fn a_marker_planned_on_every_def_of_every_fixture_round_trips() {
    marker_round_trip(&RS);
}

/// RS2 §7: the cut test, planning a marker on every def of every cut. Never panics, and an edit that is planned applies.
#[test]
fn planning_on_a_cut_file_never_panics() {
    for entry in fs::read_dir("tests/fixtures").unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        for at in (0..=text.len())
            .step_by(50)
            .filter(|&at| text.is_char_boundary(at))
        {
            let part = &text[..at];
            for def in &outline_2024(part).defs {
                if let Ok(edit) = plan_marker(part, Edition::E2024, def, "m1", STYLE) {
                    assert!(
                        analyzer_core::edit::apply(part, &[edit]).is_some(),
                        "{} cut at {at}: the edit for {} does not apply",
                        path.display(),
                        def.qualname
                    );
                }
            }
        }
    }
}

/// The cut test of the testkit looks at the defs. This one looks at the errors a cut file reports: inside the text, and a def that
/// holds one is flagged. Every fixture, cut at every 7th byte.
#[test]
fn a_cut_file_reports_its_errors_inside_the_text() {
    for entry in fs::read_dir("tests/fixtures").unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        for at in (0..=text.len())
            .step_by(7)
            .filter(|&at| text.is_char_boundary(at))
        {
            let outline = outline_2024(&text[..at]);
            for error in &outline.syntax_errors {
                assert!(
                    error.range.start <= error.range.end && error.range.end as usize <= at,
                    "{} cut at {at}: error {:?} is outside the text",
                    path.display(),
                    error.range
                );
            }
            assert!(
                outline.defs.iter().all(|d| !d.syntax_error) || !outline.syntax_errors.is_empty(),
                "{} cut at {at}: a def is flagged but no error is reported",
                path.display()
            );
        }
    }
}

/// `(qualname, occurrence)` of every def of a file, in source order.
fn names(outline: &Outline) -> Vec<(&str, u16)> {
    outline
        .defs
        .iter()
        .map(|d| d.qualname.as_str())
        .zip(occurrences(outline))
        .collect()
}

/// B11: a snapshot cannot show `occurrence` (the outline's text has none), so the twins of `body_collisions.rs` are listed here.
#[test]
fn the_twins_of_a_collision_are_told_apart_by_occurrence() {
    let text = fs::read_to_string("tests/fixtures/body_collisions.rs").unwrap();
    let outline = outline_2024(&text);
    let repeated: Vec<_> = names(&outline)
        .into_iter()
        .filter(|(qualname, _)| {
            [
                "C::helper",
                "X::helper",
                "S::helper",
                "g",
                "m::f",
                "T::helper",
            ]
            .contains(qualname)
        })
        .collect();
    assert_eq!(
        repeated,
        [
            ("C::helper", 0),
            ("C::helper", 1),
            ("X::helper", 0),
            ("X::helper", 1),
            ("S::helper", 0),
            ("S::helper", 1),
            ("g", 0),
            ("g", 1),
            ("m::f", 0),
            ("m::f", 1),
            // `T::{fn}::helper` is another name, so the method is the only `T::helper`
            ("T::helper", 0),
        ]
    );
}

/// D18 on every fixture: (qualname, occurrence) is unique, and occurrence counts from 0 in source order.
#[test]
fn qualname_and_occurrence_name_one_def_in_every_fixture() {
    for entry in fs::read_dir("tests/fixtures").unwrap() {
        let path = entry.unwrap().path();
        let outline = outline_2024(&fs::read_to_string(&path).unwrap());
        let mut next: BTreeMap<&str, u16> = BTreeMap::new();
        for (qualname, occurrence) in names(&outline) {
            let want = next.entry(qualname).or_insert(0);
            assert_eq!(occurrence, *want, "{}: {qualname}", path.display());
            *want += 1;
        }
    }
}

/// The repo's own source, large, real and always there (RS1 §7): every def has a qualname that ends in its name, the name is written
/// where `name_range` says, nothing is flagged (the files compile), and (qualname, occurrence) is unique per file.
#[test]
fn every_def_of_the_cores_own_source_is_named() {
    let dir = "../../../../core/analyzer-core/src";
    let mut files = 0;
    let mut defs = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let outline = outline_2024(&text);
        let file = path.display();
        assert!(outline.is_well_formed(), "{file}");
        assert!(
            outline.syntax_errors.is_empty(),
            "{file}: {:?}",
            outline.syntax_errors
        );
        let mut seen = BTreeMap::new();
        for (def, occurrence) in outline.defs.iter().zip(occurrences(&outline)) {
            let at = format!("{file}: {} #{occurrence}", def.qualname);
            assert!(!def.name.is_empty(), "{at}: no name");
            assert!(
                def.qualname.ends_with(&def.name),
                "{at}: the name is not the end of the qualname"
            );
            let written = &text[def.name_range.start as usize..def.name_range.end as usize];
            assert_eq!(
                written.trim_start_matches("r#"),
                def.name.trim_end_matches('!'),
                "{at}"
            );
            assert!(
                !def.syntax_error,
                "{at}: flagged, but the file has no error"
            );
            assert!(
                seen.insert((def.qualname.as_str(), occurrence), ())
                    .is_none(),
                "{at}: not unique"
            );
        }
        files += 1;
        defs += outline.defs.len();
    }
    // a guard against a path that stopped pointing at the files: an empty loop proves nothing
    assert!(files >= 5 && defs >= 100, "{files} files, {defs} defs");
}

/// `marker` is what `outline` puts in `Def::marker`, for every def of every fixture; and some fixture defs do carry one, so the equality
/// is not between two `None`s.
#[test]
fn marker_reads_what_the_outline_carries() {
    let mut marked = 0;
    for entry in fs::read_dir("tests/fixtures").unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        for def in &outline_2024(&text).defs {
            let read = marker(&text, Edition::E2024, def, STYLE);
            assert_eq!(read, def.marker, "{}: {}", path.display(), def.qualname);
            marked += usize::from(read.is_some());
        }
    }
    assert!(
        marked >= 40,
        "{marked} marked defs: the fixtures lost their markers"
    );
}

/// A def of another text (an outline made before the file changed) is not a def of this one: its offsets point somewhere else.
#[test]
fn marker_of_a_stale_def_is_none() {
    let before = "/// ID: a1\nfn a() {}\n";
    let after = "// a new first line\n/// ID: a1\nfn a() {}\n";
    let stale = outline_2024(before).defs.remove(0);
    assert_eq!(
        marker(before, Edition::E2024, &stale, STYLE).as_deref(),
        Some("a1")
    );
    assert_eq!(marker(after, Edition::E2024, &stale, STYLE), None);
    let fresh = outline_2024(after).defs.remove(0);
    assert_eq!(
        marker(after, Edition::E2024, &fresh, STYLE).as_deref(),
        Some("a1")
    );
}

/// The label is the caller's: `NOTE` reads `/// NOTE:` lines and not `/// ID:` ones.
#[test]
fn the_label_is_a_parameter() {
    let text = "/// NOTE: n1\n/// ID: i1\nfn a() {}\n";
    let def = outline_2024(text).defs.remove(0);
    let note = MarkerStyle {
        label: "NOTE",
        placement: Placement::Docstring,
    };
    assert_eq!(
        marker(text, Edition::E2024, &def, note).as_deref(),
        Some("n1")
    );
    assert_eq!(
        marker(text, Edition::E2024, &def, STYLE).as_deref(),
        Some("i1")
    );
    assert_eq!(
        outline(text, Edition::E2024, note).defs[0]
            .marker
            .as_deref(),
        Some("n1")
    );
}

/// RS2 §4: every def of `plan_doc_shapes.rs` is unmarked and has a row, so every one is planned: the round trip meets each shape.
#[test]
fn every_row_of_the_plan_fixture_is_planned() {
    let text = fs::read_to_string("tests/fixtures/plan_doc_shapes.rs").unwrap();
    let defs = outline_2024(&text).defs;
    assert!(defs.len() >= 12, "{} defs", defs.len());
    for def in &defs {
        assert!(def.marker.is_none(), "{} is marked already", def.qualname);
        let planned = plan_marker(&text, Edition::E2024, def, "m1", STYLE);
        assert!(planned.is_ok(), "{}: {planned:?}", def.qualname);
    }
}
