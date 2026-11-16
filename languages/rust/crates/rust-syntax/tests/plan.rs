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
fn row_5_a_doc_attribute_as_the_last_fragment_gets_the_marker_after_it() {
    // rustdoc joins `///` lines and `#[doc = …]` in source order: put before the attribute, the marker would glue to its text
    assert_eq!(
        marked("/// a\n#[doc = \"b\"]\nfn f() {}\n", "f"),
        "/// a\n#[doc = \"b\"]\n///\n/// ID: m1\nfn f() {}\n"
    );
    // an `include_str!` page: a marker above it would be the item's rustdoc summary
    assert_eq!(
        marked("#[doc = include_str!(\"x.md\")]\nfn f() {}\n", "f"),
        "#[doc = include_str!(\"x.md\")]\n///\n/// ID: m1\nfn f() {}\n"
    );
    // only attributes: after the last `doc = …`, before the others
    assert_eq!(
        marked("#[doc = \"d\"]\n#[inline]\nfn f() {}\n", "f"),
        "#[doc = \"d\"]\n///\n/// ID: m1\n#[inline]\nfn f() {}\n"
    );
    assert_eq!(
        marked(
            "#[doc = \"a\"]\n#[inline]\n#[doc = \"b\"]\nfn f() {}\n",
            "f"
        ),
        "#[doc = \"a\"]\n#[inline]\n#[doc = \"b\"]\n///\n/// ID: m1\nfn f() {}\n"
    );
    // a `///` line after the attribute is the last fragment, so the rule for lines applies
    assert_eq!(
        marked("#[doc = \"a\"]\n/// b\nfn f() {}\n", "f"),
        "#[doc = \"a\"]\n/// b\n///\n/// ID: m1\nfn f() {}\n"
    );
    // indented, and in a CRLF file
    assert_eq!(
        marked("mod m {\n    #[doc = \"b\"]\n    fn f() {}\n}\n", "m::f"),
        "mod m {\n    #[doc = \"b\"]\n    ///\n    /// ID: m1\n    fn f() {}\n}\n"
    );
    assert_eq!(
        marked("#[doc = \"b\"]\r\nfn f() {}\r\n", "f"),
        "#[doc = \"b\"]\r\n///\r\n/// ID: m1\r\nfn f() {}\r\n"
    );
}

#[test]
fn doc_lists_and_conditional_docs_are_not_fragments() {
    // `doc(hidden)` has no text: the marker goes above the first attribute, as for an item with no docs
    assert_eq!(
        marked("#[doc(hidden)]\nfn f() {}\n", "f"),
        "/// ID: m1\n#[doc(hidden)]\nfn f() {}\n"
    );
    assert_eq!(
        marked("#[doc(alias = \"g\")]\n#[inline]\nfn f() {}\n", "f"),
        "/// ID: m1\n#[doc(alias = \"g\")]\n#[inline]\nfn f() {}\n"
    );
    // `cfg_attr` may or may not give a doc: not a fragment either
    assert_eq!(
        marked("#[cfg_attr(x, doc = \"d\")]\nfn f() {}\n", "f"),
        "/// ID: m1\n#[cfg_attr(x, doc = \"d\")]\nfn f() {}\n"
    );
    // a `doc = …` is never READ as the marker
    let text = "#[doc = \"ID: x\"]\nfn f() {}\n";
    assert_eq!(outline(text, Edition::E2024, STYLE).defs[0].marker, None);
}

#[test]
fn a_doc_attribute_that_shares_its_line_with_code_is_not_on_its_own_line() {
    // a line after this one would be inside the item
    assert_eq!(skip("#[doc = \"b\"] fn f() {}\n", "f"), Skip::NotOnOwnLine);
    assert_eq!(
        skip("#[doc = \"b\"] #[inline]\nfn f() {}\n", "f"),
        Skip::NotOnOwnLine
    );
    // a trailing comment is no code
    assert_eq!(
        marked("#[doc = \"b\"] // why\nfn f() {}\n", "f"),
        "#[doc = \"b\"] // why\n///\n/// ID: m1\nfn f() {}\n"
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

#[test]
fn row_3_a_block_gets_an_empty_line_and_the_marker_before_its_closing_line() {
    // text above: rustdoc would glue the marker to the sentence, so an empty ` *` line goes first
    assert_eq!(
        marked("/**\n * Parses.\n */\nfn f() {}\n", "f"),
        "/**\n * Parses.\n *\n * ID: m1\n */\nfn f() {}\n"
    );
    // the block's own prefix and indentation, whatever they are
    assert_eq!(
        marked(
            "impl S {\n    /**\n     * Parses.\n     */\n    fn m() {}\n}\n",
            "S::m"
        ),
        "impl S {\n    /**\n     * Parses.\n     *\n     * ID: m1\n     */\n    fn m() {}\n}\n"
    );
    assert_eq!(
        marked("/**\r\n * Parses.\r\n */\r\nfn f() {}\r\n", "f"),
        "/**\r\n * Parses.\r\n *\r\n * ID: m1\r\n */\r\nfn f() {}\r\n"
    );
    // rustdoc has no block tags: a line that starts with `@` is text like any other
    assert_eq!(
        marked("/**\n * @a\n * b\n */\nfn f() {}\n", "f"),
        "/**\n * @a\n * b\n *\n * ID: m1\n */\nfn f() {}\n"
    );
    // an attribute after the block does not change where it goes
    assert_eq!(
        marked("/**\n * a\n */\n#[inline]\nfn f() {}\n", "f"),
        "/**\n * a\n *\n * ID: m1\n */\n#[inline]\nfn f() {}\n"
    );
}

#[test]
fn row_3_no_empty_line_when_the_line_above_is_already_empty() {
    // ` *` above: the paragraph is over
    assert_eq!(
        marked("/**\n * Parses.\n *\n */\nfn f() {}\n", "f"),
        "/**\n * Parses.\n *\n * ID: m1\n */\nfn f() {}\n"
    );
    assert_eq!(
        marked("/**\n * Parses.\n *   \n */\nfn f() {}\n", "f"),
        "/**\n * Parses.\n *   \n * ID: m1\n */\nfn f() {}\n"
    );
}

#[test]
fn row_3_a_bare_opening_above_gets_the_marker_as_the_first_line() {
    assert_eq!(
        marked("/**\n */\nfn f() {}\n", "f"),
        "/**\n * ID: m1\n */\nfn f() {}\n"
    );
    assert_eq!(
        marked("    /**\n     */\n    fn f() {}\n", "f"),
        "    /**\n     * ID: m1\n     */\n    fn f() {}\n"
    );
}

#[test]
fn a_block_that_cannot_be_edited_safely_is_left_alone() {
    // closed on its opening line
    assert_eq!(skip("/** a */\nfn f() {}\n", "f"), Skip::UnsafeDocComment);
    // text on the closing line
    assert_eq!(
        skip("/**\n * a\n * b */\nfn f() {}\n", "f"),
        Skip::UnsafeDocComment
    );
    // a line without its star
    assert_eq!(
        skip("/**\n * a\n b\n */\nfn f() {}\n", "f"),
        Skip::UnsafeDocComment
    );
}

#[test]
fn only_the_last_doc_comment_decides() {
    // a block earlier in the docs is no obstacle: the last doc comment is a line
    assert_eq!(
        marked("/** a */\n/// b\nfn f() {}\n", "f"),
        "/** a */\n/// b\n///\n/// ID: m1\n".to_owned() + "fn f() {}\n"
    );
    // a block after a line is the last one, and decides
    assert_eq!(
        marked("/// a\n/**\n * b\n */\nfn f() {}\n", "f"),
        "/// a\n/**\n * b\n *\n * ID: m1\n */\nfn f() {}\n"
    );
    assert_eq!(
        skip("/// a\n/** b */\nfn f() {}\n", "f"),
        Skip::UnsafeDocComment
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

/// RS2 §5, the order of the checks: a marked def answers `AlreadyMarked` with its marker even when its line is shared, because the caller uses
/// the fact; the shape of the text (`NotOnOwnLine`) is asked last.
#[test]
fn the_order_of_the_checks() {
    assert_eq!(
        skip("struct A; /// ID: x\nfn b() {}\n", "b"),
        Skip::AlreadyMarked {
            existing: "x".to_owned()
        }
    );
    // unmarked, the same shape is a shared line
    assert_eq!(
        skip("struct A; /// y\nfn b() {}\n", "b"),
        Skip::NotOnOwnLine
    );
    // the text is asked before the shape, and the syntax error before the marker
    let text = "struct A; fn b() {}\n";
    let defs = outline(text, Edition::E2024, STYLE).defs;
    let b = defs.iter().find(|d| d.qualname == "b").unwrap();
    assert_eq!(
        plan_marker(text, Edition::E2024, b, " bad", STYLE),
        Err(Skip::InvalidText)
    );
    assert_eq!(skip("/// ID: x\nfn f( {\n", "f"), Skip::SyntaxError);
}
