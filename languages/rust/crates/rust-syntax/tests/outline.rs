//! The shared checks on every fixture: the outline equals its reviewed snapshot, and nothing panics on a cut file. `plan_marker` is RS2's.

use analyzer_testkit::syntax::{Subject, cut_every_50_bytes, snapshot_every_fixture};
use rust_syntax::{Def, Edition, MarkerStyle, Outline, Placement, outline};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

fn outline_2024(text: &str) -> Outline {
    outline(text, Edition::E2024, STYLE)
}

/// RS2 builds the marker edit; until then no def can be marked.
fn no_marker_yet(_: &str, _: &Def, _: &str) -> Option<analyzer_core::edit::Edit> {
    None
}

const RS: Subject = Subject {
    outline: outline_2024,
    plan_marker: no_marker_yet,
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
