//! The outline of every fixture equals its reviewed snapshot. One folder per dialect, because the shared runner passes only the text.

use analyzer_testkit::syntax::{Subject, snapshot_every_fixture};
use typescript_syntax::{Dialect, outline};

fn subject(outline: fn(&str) -> typescript_syntax::Outline, dir: &'static str) -> Subject {
    Subject {
        outline,
        // markers are TS2: until then nothing is planned
        plan_marker: |_, _, _| None,
        fixtures: dir,
        snapshots: match dir {
            "tests/fixtures/ts" => "tests/snapshots/ts",
            "tests/fixtures/tsx" => "tests/snapshots/tsx",
            "tests/fixtures/js" => "tests/snapshots/js",
            _ => "tests/snapshots/jsx",
        },
    }
}

#[test]
fn ts_fixtures() {
    snapshot_every_fixture(&subject(|t| outline(t, Dialect::Ts), "tests/fixtures/ts"));
}

#[test]
fn tsx_fixtures() {
    snapshot_every_fixture(&subject(|t| outline(t, Dialect::Tsx), "tests/fixtures/tsx"));
}

#[test]
fn js_fixtures() {
    snapshot_every_fixture(&subject(|t| outline(t, Dialect::Js), "tests/fixtures/js"));
}

#[test]
fn jsx_fixtures() {
    snapshot_every_fixture(&subject(|t| outline(t, Dialect::Jsx), "tests/fixtures/jsx"));
}
