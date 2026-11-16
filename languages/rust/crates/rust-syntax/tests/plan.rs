//! `plan_marker`, row by row (RS2 §4, §5): the text after the edit is applied, and the reason when there is no edit.

use analyzer_core::edit::apply;
use rust_syntax::{Edition, MarkerStyle, Skip, outline, plan_marker};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: rust_syntax::Placement::Docstring,
};

/// The text after a marker `m1` is planned on the def named `qualname` (its first occurrence) and applied.
fn marked(text: &str, qualname: &str) -> String {
    let edit = planned(text, qualname).unwrap_or_else(|skip| panic!("{qualname}: {skip}"));
    apply(text, &[edit]).unwrap()
}

fn planned(text: &str, qualname: &str) -> Result<rust_syntax::Edit, Skip> {
    let defs = outline(text, Edition::E2024, STYLE).defs;
    let def = defs
        .iter()
        .find(|d| d.qualname == qualname)
        .unwrap_or_else(|| panic!("no def {qualname} in {text:?}"));
    plan_marker(text, Edition::E2024, def, "m1", STYLE)
}

fn skip(text: &str, qualname: &str) -> Skip {
    planned(text, qualname).unwrap_err()
}

#[test]
fn row_1_after_a_last_doc_line_that_is_empty() {
    assert_eq!(
        marked("/// Parses.\n///\nfn f() {}\n", "f"),
        "/// Parses.\n///\n/// ID: m1\nfn f() {}\n"
    );
    // blanks after the `///` make it empty too
    assert_eq!(
        marked("/// Parses.\n///   \nfn f() {}\n", "f"),
        "/// Parses.\n///   \n/// ID: m1\nfn f() {}\n"
    );
}

#[test]
fn row_2_an_empty_line_first_when_the_last_doc_line_is_text() {
    assert_eq!(
        marked("/// Parses.\nfn f() {}\n", "f"),
        "/// Parses.\n///\n/// ID: m1\nfn f() {}\n"
    );
}

#[test]
fn row_4_after_the_last_doc_comment_wherever_it_stands() {
    assert_eq!(
        marked("/// a\n#[inline]\n/// b\n#[must_use]\nfn f() {}\n", "f"),
        "/// a\n#[inline]\n/// b\n///\n/// ID: m1\n#[must_use]\nfn f() {}\n"
    );
    // a plain comment after the docs stays where it was, below the marker
    assert_eq!(
        marked("/// a\n// plain\nfn f() {}\n", "f"),
        "/// a\n///\n/// ID: m1\n// plain\nfn f() {}\n"
    );
    // a blank line between two doc lines changes nothing: the marker goes after the last one
    assert_eq!(
        marked("/// a\n\n/// b\nfn f() {}\n", "f"),
        "/// a\n\n/// b\n///\n/// ID: m1\nfn f() {}\n"
    );
}

#[test]
fn row_5_only_doc_attributes_get_a_new_line_above_the_first_attribute() {
    assert_eq!(
        marked("#[doc = \"d\"]\n#[inline]\nfn f() {}\n", "f"),
        "/// ID: m1\n#[doc = \"d\"]\n#[inline]\nfn f() {}\n"
    );
}

#[test]
fn row_6_no_docs_one_new_line_above_the_first_attribute_or_keyword() {
    assert_eq!(marked("fn f() {}\n", "f"), "/// ID: m1\nfn f() {}\n");
    assert_eq!(
        marked("#[inline]\npub fn f() {}\n", "f"),
        "/// ID: m1\n#[inline]\npub fn f() {}\n"
    );
    // below a plain comment, not above it
    assert_eq!(
        marked("// plain\nfn f() {}\n", "f"),
        "// plain\n/// ID: m1\nfn f() {}\n"
    );
}

#[test]
fn the_line_has_the_indentation_of_the_items_first_line() {
    let text =
        "impl S {\n    /// a\n    fn m() {}\n    fn n() {}\n    #[inline]\n    fn o() {}\n}\n";
    assert_eq!(
        marked(text, "S::m"),
        "impl S {\n    /// a\n    ///\n    /// ID: m1\n    fn m() {}\n    fn n() {}\n    #[inline]\n    fn o() {}\n}\n"
    );
    assert_eq!(
        marked(text, "S::n"),
        "impl S {\n    /// a\n    fn m() {}\n    /// ID: m1\n    fn n() {}\n    #[inline]\n    fn o() {}\n}\n"
    );
    assert_eq!(
        marked(text, "S::o"),
        "impl S {\n    /// a\n    fn m() {}\n    fn n() {}\n    /// ID: m1\n    #[inline]\n    fn o() {}\n}\n"
    );
    // tabs are copied as they are
    assert_eq!(
        marked("mod m {\n\tfn f() {}\n}\n", "m::f"),
        "mod m {\n\t/// ID: m1\n\tfn f() {}\n}\n"
    );
}

#[test]
fn line_endings_are_the_files() {
    assert_eq!(
        marked("/// a\r\nfn f() {}\r\n", "f"),
        "/// a\r\n///\r\n/// ID: m1\r\nfn f() {}\r\n"
    );
    assert_eq!(marked("fn f() {}\r\n", "f"), "/// ID: m1\r\nfn f() {}\r\n");
}

#[test]
fn a_def_that_starts_the_file_after_a_byte_order_mark_gets_the_line_after_it() {
    assert_eq!(
        marked("\u{feff}fn f() {}\n", "f"),
        "\u{feff}/// ID: m1\nfn f() {}\n"
    );
}

#[test]
fn already_marked_answers_with_the_marker_found() {
    assert_eq!(
        skip("/// ID: old\nfn f() {}\n", "f"),
        Skip::AlreadyMarked {
            existing: "old".to_owned()
        }
    );
}

#[test]
fn not_on_its_own_line() {
    assert_eq!(skip("fn a() {} fn b() {}\n", "b"), Skip::NotOnOwnLine);
    assert_eq!(skip("mod m { fn f() {} }\n", "m::f"), Skip::NotOnOwnLine);
    // the first of them owns its line
    assert!(planned("fn a() {} fn b() {}\n", "a").is_ok());
}

#[test]
fn a_syntax_error_in_the_def_or_its_docs() {
    assert_eq!(skip("fn f( {\n", "f"), Skip::SyntaxError);
    assert_eq!(skip("/// a\nfn f() { let = ; }\n", "f"), Skip::SyntaxError);
    // a def next to a broken one is fine
    assert!(planned("fn a() {}\nfn b( {\n", "a").is_ok());
}

#[test]
fn text_that_would_not_read_back_the_same() {
    let text = "fn f() {}\n";
    let defs = outline(text, Edition::E2024, STYLE).defs;
    for bad in ["", " a", "a ", "a\nb", "a\rb", "a */ b", "\t"] {
        assert_eq!(
            plan_marker(text, Edition::E2024, &defs[0], bad, STYLE),
            Err(Skip::InvalidText),
            "{bad:?}"
        );
    }
    assert!(plan_marker(text, Edition::E2024, &defs[0], "a b-c_d.9", STYLE).is_ok());
}

#[test]
fn a_def_of_another_text_is_unknown() {
    let before = "fn f() {}\n";
    let after = "// new line\nfn f() {}\n";
    let stale = outline(before, Edition::E2024, STYLE).defs.remove(0);
    assert_eq!(
        plan_marker(after, Edition::E2024, &stale, "m1", STYLE),
        Err(Skip::UnknownDef)
    );
}

/// Until step 3 moves the block helper to core, a block as the last doc comment is refused, not rewritten and not stacked under.
#[test]
fn a_block_as_the_last_doc_comment_waits_for_step_3() {
    assert_eq!(
        skip("/** a\n * b\n */\nfn f() {}\n", "f"),
        Skip::UnsafeDocComment
    );
    // a block earlier in the docs is no obstacle: the last doc comment is a line
    assert_eq!(
        marked("/** a */\n/// b\nfn f() {}\n", "f"),
        "/** a */\n/// b\n///\n/// ID: m1\nfn f() {}\n"
    );
}

#[test]
fn every_reason_reads_as_a_sentence() {
    for skip in [
        Skip::AlreadyMarked {
            existing: "x".to_owned(),
        },
        Skip::NotOnOwnLine,
        Skip::SyntaxError,
        Skip::UnsafeDocComment,
        Skip::InvalidText,
        Skip::UnknownDef,
        Skip::PlacementNotSupported,
    ] {
        let reason = skip.to_string();
        assert!(reason.len() > 20 && !reason.ends_with('.'), "{reason}");
    }
}

/// An unfinished item above the def takes a new doc line, and the def would not carry the marker: the def itself has no error, so only the
/// trial of the edit catches it.
#[test]
fn a_def_below_an_unfinished_item_is_refused_when_the_marker_would_not_be_its() {
    let text = "struct\nenum Complete {}\n";
    let defs = outline(text, Edition::E2024, STYLE).defs;
    assert!(!defs[0].syntax_error, "the def has no error of its own");
    assert_eq!(skip(text, "Complete"), Skip::SyntaxError);
    // the same shape one item further down is fine
    assert!(planned("fn a() {}\nstruct\nfn b() {}\n", "a").is_ok());
}
