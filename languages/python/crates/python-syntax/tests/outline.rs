//! The outline of every fixture equals its reviewed snapshot, and never panics.

use analyzer_testkit::syntax::{Subject, cut_every_50_bytes, snapshot_every_fixture};
use python_syntax::{MarkerStyle, outline};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: python_syntax::Placement::Docstring,
};

fn subject() -> Subject {
    Subject {
        outline: |t| outline(t, STYLE),
        // markers come next: until then nothing is planned
        plan_marker: |_, _, _| None,
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
