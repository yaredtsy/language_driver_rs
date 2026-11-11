//! A long-running process must not grow: the engine keeps its tree in arenas and never frees a node on its own, so `with_parsed` parses
//! inside a region. This is the S-TS2 loop (`languages/typescript/ENGINE.md`) as a test: parse a 2,000-line file 10,000 times and compare
//! the process's memory early and late. Without the region the same loop grows by hundreds of MB. (Early is round 100, not later: without
//! a region RSS climbs fast and then swings at a high level, so a late "early" sample would hide the leak.) Checked by hand: with the region
//! entered the peak is about 8 MB; with `region.enter()` removed from `sem.rs` the peak is 200-390 MB and this test fails.

use typescript_syntax::{Dialect, MarkerStyle, outline};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: typescript_syntax::Placement::Docstring,
};

/// About 2,000 lines of functions, classes and arrows.
fn big_text() -> String {
    let mut text = String::new();
    for i in 0..250 {
        text.push_str(&format!(
            "/** doc {i} */\nexport function f{i}(a: number): number {{\n  return g{i}(a) + 1;\n}}\n\
             class C{i} {{\n  m() {{}}\n  h = () => {{}};\n}}\nconst k{i} = () => {{}};\n"
        ));
    }
    text
}

fn rss_mb() -> f64 {
    let usage = memory_stats::memory_stats().expect("this platform reports memory");
    usage.physical_mem as f64 / (1024.0 * 1024.0)
}

#[test]
fn ten_thousand_outlines_do_not_grow_the_process() {
    let text = big_text();
    assert!(text.lines().count() > 1_500);
    let expected = outline(&text, Dialect::Ts, STYLE).defs.len();
    assert!(
        expected > 700,
        "the text should hold many defs, got {expected}"
    );

    let mut early = 0.0;
    let mut peak: f64 = 0.0;
    for round in 1..=10_000 {
        let defs = outline(&text, Dialect::Ts, STYLE).defs.len();
        assert_eq!(defs, expected);
        if round == 100 {
            early = rss_mb();
        } else if round > 100 && round % 250 == 0 {
            // the peak, not the last value: a leak swings with the system's memory pressure (the OS may take freed pages back at any time)
            peak = peak.max(rss_mb());
        }
    }
    assert!(
        peak - early < 30.0,
        "memory grew from {early:.1} MB (round 100) to a peak of {peak:.1} MB (rounds 250 to 10,000)"
    );
}
