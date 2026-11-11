//! The outline of every fixture equals its reviewed snapshot, never panics, and takes a marker on every def that can.

use analyzer_testkit::syntax::{
    Subject, cut_every_50_bytes, marker_round_trip, snapshot_every_fixture,
};
use python_syntax::{MarkerStyle, outline, plan_marker};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: python_syntax::Placement::Docstring,
};

fn subject() -> Subject {
    Subject {
        outline: |t| outline(t, STYLE),
        plan_marker: |text, def, marker| plan_marker(text, def, marker, STYLE).ok(),
        fixtures: "tests/fixtures",
        snapshots: "tests/snapshots",
    }
}

#[test]
fn every_fixture_matches_its_snapshot() {
    snapshot_every_fixture(&subject());
}

#[test]
fn nothing_panics_on_a_cut_fixture() {
    cut_every_50_bytes(&subject());
}

#[test]
fn a_marker_round_trips_on_every_def() {
    marker_round_trip(&subject());
}
