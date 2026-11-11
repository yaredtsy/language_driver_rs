//! The outline of every fixture equals its reviewed snapshot. One folder per dialect, because the shared runner passes only the text.

use analyzer_testkit::syntax::{Subject, cut_every_50_bytes, snapshot_every_fixture};
use typescript_syntax::{Dialect, MarkerStyle, outline};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: typescript_syntax::Placement::Docstring,
};

type OutlineFn = fn(&str) -> typescript_syntax::Outline;

fn subject(outline: OutlineFn, dir: &'static str) -> Subject {
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
    snapshot_every_fixture(&subject(
        |t| outline(t, Dialect::Ts, STYLE),
        "tests/fixtures/ts",
    ));
}

#[test]
fn tsx_fixtures() {
    snapshot_every_fixture(&subject(
        |t| outline(t, Dialect::Tsx, STYLE),
        "tests/fixtures/tsx",
    ));
}

#[test]
fn js_fixtures() {
    snapshot_every_fixture(&subject(
        |t| outline(t, Dialect::Js, STYLE),
        "tests/fixtures/js",
    ));
}

#[test]
fn jsx_fixtures() {
    snapshot_every_fixture(&subject(
        |t| outline(t, Dialect::Jsx, STYLE),
        "tests/fixtures/jsx",
    ));
}

#[test]
fn nothing_panics_on_a_cut_fixture() {
    // Naming the element type makes each non-capturing closure coerce to a `fn` pointer; without it every closure has its own type
    // and the array does not compile.
    let subjects: [(OutlineFn, &'static str); 4] = [
        (|t| outline(t, Dialect::Ts, STYLE), "tests/fixtures/ts"),
        (|t| outline(t, Dialect::Tsx, STYLE), "tests/fixtures/tsx"),
        (|t| outline(t, Dialect::Js, STYLE), "tests/fixtures/js"),
        (|t| outline(t, Dialect::Jsx, STYLE), "tests/fixtures/jsx"),
    ];
    for (outline_fn, dir) in subjects {
        cut_every_50_bytes(&subject(outline_fn, dir));
    }
}
