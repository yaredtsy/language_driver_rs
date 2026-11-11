//! The shared checks on every fixture: the outline equals its reviewed snapshot, a marker can be planned on every def and read back with nothing
//! else damaged, and nothing panics on a cut file. One folder per dialect, because the shared runner passes only the text.

use analyzer_testkit::syntax::{
    Subject, cut_every_50_bytes, marker_round_trip, snapshot_every_fixture,
};
use typescript_syntax::{
    Def, Dialect, Edit, MarkerStyle, Outline, Placement, outline, plan_marker,
};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

fn outline_ts(text: &str) -> Outline {
    outline(text, Dialect::Ts, STYLE)
}
fn outline_tsx(text: &str) -> Outline {
    outline(text, Dialect::Tsx, STYLE)
}
fn outline_js(text: &str) -> Outline {
    outline(text, Dialect::Js, STYLE)
}
fn outline_jsx(text: &str) -> Outline {
    outline(text, Dialect::Jsx, STYLE)
}

fn plan_ts(text: &str, def: &Def, marker: &str) -> Option<Edit> {
    plan_marker(text, Dialect::Ts, def, marker, STYLE).ok()
}
fn plan_tsx(text: &str, def: &Def, marker: &str) -> Option<Edit> {
    plan_marker(text, Dialect::Tsx, def, marker, STYLE).ok()
}
fn plan_js(text: &str, def: &Def, marker: &str) -> Option<Edit> {
    plan_marker(text, Dialect::Js, def, marker, STYLE).ok()
}
fn plan_jsx(text: &str, def: &Def, marker: &str) -> Option<Edit> {
    plan_marker(text, Dialect::Jsx, def, marker, STYLE).ok()
}

const TS: Subject = Subject {
    outline: outline_ts,
    plan_marker: plan_ts,
    fixtures: "tests/fixtures/ts",
    snapshots: "tests/snapshots/ts",
};
const TSX: Subject = Subject {
    outline: outline_tsx,
    plan_marker: plan_tsx,
    fixtures: "tests/fixtures/tsx",
    snapshots: "tests/snapshots/tsx",
};
const JS: Subject = Subject {
    outline: outline_js,
    plan_marker: plan_js,
    fixtures: "tests/fixtures/js",
    snapshots: "tests/snapshots/js",
};
const JSX: Subject = Subject {
    outline: outline_jsx,
    plan_marker: plan_jsx,
    fixtures: "tests/fixtures/jsx",
    snapshots: "tests/snapshots/jsx",
};

#[test]
fn snapshots_ts() {
    snapshot_every_fixture(&TS);
}
#[test]
fn snapshots_tsx() {
    snapshot_every_fixture(&TSX);
}
#[test]
fn snapshots_js() {
    snapshot_every_fixture(&JS);
}
#[test]
fn snapshots_jsx() {
    snapshot_every_fixture(&JSX);
}

#[test]
fn marker_round_trip_ts() {
    marker_round_trip(&TS);
}
#[test]
fn marker_round_trip_tsx() {
    marker_round_trip(&TSX);
}
#[test]
fn marker_round_trip_js() {
    marker_round_trip(&JS);
}
#[test]
fn marker_round_trip_jsx() {
    marker_round_trip(&JSX);
}

#[test]
fn nothing_panics_on_a_cut_fixture() {
    for subject in [&TS, &TSX, &JS, &JSX] {
        cut_every_50_bytes(subject);
    }
}
