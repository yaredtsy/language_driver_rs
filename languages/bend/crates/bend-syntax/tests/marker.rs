//! A def's marker (03 §1.3, Q-BD1): the last `# <label>: <text>` line of the comment block directly above its declaration, and where `plan_marker`
//! writes one. The testkit's round trip over every fixture is in `outline.rs`; this file is the rules, one case each.

use std::fs;

use analyzer_core::edit::apply;
use bend_syntax::{Def, MarkerStyle, Outline, Skip, marker, outline, plan_marker};

const ID: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: bend_syntax::Placement::Above,
};

fn fixture(name: &str) -> String {
    fs::read_to_string(format!("tests/fixtures/{name}.bend"))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn markers(text: &str) -> Vec<(String, Option<String>)> {
    outline(text, ID)
        .defs
        .into_iter()
        .map(|d| (d.qualname, d.marker))
        .collect()
}

fn first_marker(text: &str) -> Option<String> {
    outline(text, ID)
        .defs
        .first()
        .and_then(|d| d.marker.clone())
}

/// The text after planning `marker` on def `i` and applying the edit.
fn planned(text: &str, i: usize, marker_text: &str) -> String {
    let o = outline(text, ID);
    let edit =
        plan_marker(text, &o.defs[i], marker_text, ID).unwrap_or_else(|e| panic!("{text:?}: {e}"));
    apply(text, &[edit]).expect("the edit applies")
}

fn skip(text: &str, i: usize, marker_text: &str) -> Skip {
    let o = outline(text, ID);
    plan_marker(text, &o.defs[i], marker_text, ID).expect_err("refused")
}

const DEF: &str = "def a() -> Nat: 0\n";

#[test]
fn a_marker_line_is_read_after_any_blanks() {
    for (line, want) in [
        ("# ID: x", Some("x")),
        ("#ID: x", Some("x")),
        ("#ID:x", Some("x")),
        ("# ID : x", Some("x")),
        ("# ID:   x y  ", Some("x y")),
        ("#   ID: x", Some("x")),
        ("  # ID: x", Some("x")),
        ("\t# ID: x", Some("x")),
        ("# ID: x # y", Some("x # y")),
        ("# ID:", None),
        ("# ID:   ", None),
        ("# IDENT: x", None),
        ("# note: x", None),
        ("# Ref: x", None),
        ("# ID x", None),
        ("#| ID: x", None),
        ("#|ID: x", None),
    ] {
        let text = format!("{line}\n{DEF}");
        assert_eq!(first_marker(&text).as_deref(), want, "{line:?}");
    }
}

#[test]
fn a_crlf_line_is_read_without_its_carriage_return() {
    assert_eq!(
        first_marker("# ID: x\r\ndef a() -> Nat: 0\r\n").as_deref(),
        Some("x")
    );
}

/// The label is the caller's.
#[test]
fn another_label_is_another_marker() {
    let text = format!("# Ref: k\n{DEF}");
    assert_eq!(first_marker(&text), None);
    let o = outline(&text, MarkerStyle::above("Ref"));
    assert_eq!(o.defs[0].marker.as_deref(), Some("k"));
}

/// Only the block directly above is the def's (the range rule): a blank line between makes it someone else's.
#[test]
fn a_marker_a_blank_line_away_is_not_the_defs() {
    let o = outline(&fixture("marker_blank_line_away"), ID);
    let got: Vec<_> = o
        .defs
        .iter()
        .map(|d| (d.qualname.as_str(), d.marker.as_deref()))
        .collect();
    assert_eq!(got, [("a", None), ("b", None), ("c", Some("here"))]);
    assert_eq!(
        o.file_marker, None,
        "after the first import, so not the file's either"
    );
}

/// Several marker lines in one block: the last wins (the one nearest the declaration).
#[test]
fn the_last_marker_line_of_the_block_wins() {
    assert_eq!(
        markers(&fixture("marker_two_lines")),
        [("a".to_string(), Some("new".to_string()))]
    );
}

/// A marker in a body, or on the line of code, is a comment and nothing more.
#[test]
fn a_marker_inside_a_body_is_not_a_defs_marker() {
    assert_eq!(
        markers(&fixture("marker_in_body")),
        [("a".to_string(), None), ("b".to_string(), None)]
    );
}

/// The `#|` lines of a Bend test are the expected output: comments that end the file and are above nothing. Never a marker, never a doc.
#[test]
fn a_trailing_expected_output_block_is_not_read_as_a_marker() {
    let text = fixture("trailer_expected_output");
    let o = outline(&text, ID);
    assert_eq!(o.defs.len(), 1);
    assert_eq!(
        o.defs[0].marker.as_deref(),
        Some("a-1"),
        "its own marker, not the `#| ID:` line below it"
    );
    // the range of the def does not reach the trailer
    assert!(!text[o.defs[0].range.start as usize..o.defs[0].range.end as usize].contains("#|"));
    // without a marker of its own the def stays unmarked, though a `#| ID: …` line follows it directly
    let bare = "def a() -> Nat: 0\n#| ID: not-a-marker\n";
    assert_eq!(first_marker(bare), None);
    // and planning writes above the def, not into the trailer
    assert_eq!(
        planned(bare, 0, "m"),
        "# ID: m\ndef a() -> Nat: 0\n#| ID: not-a-marker\n"
    );
    // a `#|` block above a declaration is no marker either: the label does not follow the `#`
    assert_eq!(first_marker("#| ID: x\ndef a() -> Nat: 0\n"), None);
}

#[test]
fn a_law_and_its_proof_are_marked_apart() {
    let o = outline(&fixture("marked_law_unmarked_proof"), ID);
    let got: Vec<_> = o
        .defs
        .iter()
        .map(|d| (d.qualname.as_str(), d.marker.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            ("f", Some("the-law")),
            ("f", None),
            ("g", None),
            ("g", Some("the-proof-of-g"))
        ]
    );
    // the two `f` share a qualname, the marker is what tells them apart: planning on the proof marks the proof only
    let text = fixture("marked_law_unmarked_proof");
    let out = planned(&text, 1, "the-proof-of-f");
    let got: Vec<_> = outline(&out, ID)
        .defs
        .iter()
        .map(|d| d.marker.clone())
        .collect();
    assert_eq!(
        got,
        [
            Some("the-law".into()),
            Some("the-proof-of-f".into()),
            None,
            Some("the-proof-of-g".into())
        ]
    );
}

#[test]
fn a_marker_above_unsafe_is_read_and_planned_above_it() {
    let o = outline(&fixture("marked_def_unsafe"), ID);
    let got: Vec<_> = o
        .defs
        .iter()
        .map(|d| (d.qualname.as_str(), d.marker.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            ("f", Some("unsafe-1")),
            ("g", Some("unsafe-2")),
            ("h", None)
        ]
    );
    // `@unsafe` is the declaration's first token: the line goes above it, not between it and `def`
    let text = fixture("marked_def_unsafe");
    let out = planned(&text, 2, "unsafe-3");
    assert!(
        out.ends_with("# ID: unsafe-3\n@unsafe\ndef h?(x: U32) -> U32: x\n"),
        "{out}"
    );
}

#[test]
fn planning_puts_the_line_below_an_existing_block_and_above_the_declaration() {
    assert_eq!(
        planned("# Adds.\n# More.\ndef a() -> Nat: 0\n", 0, "k"),
        "# Adds.\n# More.\n# ID: k\ndef a() -> Nat: 0\n"
    );
    assert_eq!(planned(DEF, 0, "k"), "# ID: k\ndef a() -> Nat: 0\n");
    // a blank line keeps a banner apart: the marker is directly above the def and the banner stays where it was
    assert_eq!(
        planned("# Array\n# -----\n\ndef a() -> Nat: 0\n", 0, "k"),
        "# Array\n# -----\n\n# ID: k\ndef a() -> Nat: 0\n"
    );
}

#[test]
fn planning_copies_the_line_ending_and_leaves_the_end_alone() {
    let text = fixture("marked_crlf");
    assert_eq!(
        planned(&text, 1, "b-1"),
        "# ID: crlf-1\r\ndef a() -> Nat: 0\r\n\r\n# ID: b-1\r\ndef b() -> Nat:\r\n  0\r\n"
    );
    let text = fixture("no_trailing_newline");
    assert_eq!(
        planned(&text, 1, "k"),
        "def a() -> Nat: 0\n\n# ID: k\ndef b() -> Nat: 1"
    );
    let text = fixture("bom");
    assert_eq!(
        planned(&text, 0, "k"),
        "\u{feff}# ID: k\ndef a() -> Nat: 0\n",
        "the byte-order mark stays first"
    );
}

/// What is planned is one line inserted, and nothing else moves.
#[test]
fn the_edit_is_one_inserted_line() {
    for entry in fs::read_dir("tests/fixtures").expect("fixtures") {
        let text = fs::read_to_string(entry.expect("entry").path()).expect("text");
        let o = outline(&text, ID);
        for def in &o.defs {
            let Ok(edit) = plan_marker(&text, def, "m", ID) else {
                continue;
            };
            assert_eq!(edit.at.start, edit.at.end, "an insertion");
            let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
            assert_eq!(edit.text, format!("# ID: m{eol}"));
            let out = apply(&text, std::slice::from_ref(&edit)).expect("applies");
            let (a, b) = (
                edit.at.start as usize,
                edit.at.start as usize + edit.text.len(),
            );
            assert_eq!(format!("{}{}", &out[..a], &out[b..]), text);
        }
    }
}

#[test]
fn a_marked_def_is_refused_with_its_marker() {
    let text = fixture("marked_def");
    let o = outline(&text, ID);
    assert_eq!(
        plan_marker(&text, &o.defs[0], "other", ID),
        Err(Skip::AlreadyMarked {
            existing: "add-1".to_string()
        })
    );
    assert_eq!(marker(&text, &o.defs[1], ID).as_deref(), Some("id-2"));
    assert_eq!(marker(&text, &o.defs[2], ID), None);
}

#[test]
fn a_text_that_does_not_read_back_is_refused() {
    for bad in ["", " x", "x ", "a\nb", "a\r\nb", "\tx"] {
        assert_eq!(skip(DEF, 0, bad), Skip::InvalidText, "{bad:?}");
    }
}

#[test]
fn a_broken_def_is_refused_and_the_rest_are_marked() {
    let text = "def a() -> Nat: 0\ndef broken(a: Nat, , b) -> Nat: 0\ndef c() -> Nat: 1\n";
    assert_eq!(skip(text, 1, "k"), Skip::SyntaxError);
    let out = planned(text, 2, "k");
    assert_eq!(markers(&out)[2].1.as_deref(), Some("k"));
}

/// An old def of a text that has changed is not trusted.
#[test]
fn a_def_of_another_text_is_unknown() {
    let old = outline("def a() -> Nat: 0\n", ID);
    assert_eq!(
        plan_marker(
            "# new line\n# more\ndef a() -> Nat: 0\n",
            &old.defs[0],
            "k",
            ID
        ),
        Err(Skip::UnknownDef)
    );
}

#[test]
fn a_def_that_shares_its_line_is_not_marked() {
    let text = "def a() -> Nat: 0 def b() -> Nat: 1\n";
    assert_eq!(outline(text, ID).defs.len(), 2);
    assert_eq!(skip(text, 1, "k"), Skip::NotOnOwnLine);
    assert_eq!(
        planned(text, 0, "k"),
        "# ID: k\ndef a() -> Nat: 0 def b() -> Nat: 1\n"
    );
}

/// Bend builds one placement.
#[test]
fn a_docstring_style_reads_and_writes_nothing() {
    let text = "# ID: x\ndef a() -> Nat: 0\n";
    let style = MarkerStyle::docstring("ID");
    let o = outline(text, style);
    assert_eq!(o.defs[0].marker, None);
    assert_eq!(o.file_marker, None);
    assert_eq!(
        plan_marker(text, &o.defs[0], "k", style),
        Err(Skip::PlacementNotSupported)
    );
}

/// A marker line is a comment: the declarations, their names, kinds, params and flags are the same before and after, in every fixture.
#[test]
fn a_marker_never_changes_what_the_outline_sees() {
    for entry in fs::read_dir("tests/fixtures").expect("fixtures") {
        let text = fs::read_to_string(entry.expect("entry").path()).expect("text");
        let before = outline(&text, ID);
        let mut edits = Vec::new();
        for (i, def) in before.defs.iter().enumerate() {
            if let Ok(e) = plan_marker(&text, def, &format!("m{i}"), ID) {
                edits.push((i, e));
            }
        }
        let all: Vec<_> = edits.iter().map(|(_, e)| e.clone()).collect();
        let out = apply(&text, &all).expect("all the edits apply together");
        let after = outline(&out, ID);
        let shape = |o: &Outline| -> Vec<(String, String, Vec<String>, bool)> {
            o.defs
                .iter()
                .map(|d: &Def| {
                    (
                        d.qualname.clone(),
                        format!("{:?}", d.kind),
                        d.params.clone(),
                        d.syntax_error,
                    )
                })
                .collect()
        };
        assert_eq!(shape(&before), shape(&after));
        assert_eq!(before.syntax_errors.len(), after.syntax_errors.len());
        for (i, _) in edits {
            assert_eq!(after.defs[i].marker, Some(format!("m{i}")));
        }
    }
}
