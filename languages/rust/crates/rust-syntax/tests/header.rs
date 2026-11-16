//! The file header (RS2 §6): what `header_marker` reads and where `plan_header_marker` writes it, one fixture per row. The planned text of
//! every `header_*.rs` fixture is a reviewed file under `tests/header_planned/` (`ANALYZER_BLESS=1` writes it; read it before committing).

use std::fs;
use std::path::Path;

use analyzer_core::edit::apply;
use analyzer_testkit::expect_file;
use rust_syntax::{
    Edition, MarkerStyle, Placement, Skip, header_marker, outline, plan_header_marker,
};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new("tests/fixtures").join(name)).unwrap()
}

/// What `header_marker` finds in each header fixture. Every `header_*.rs` file must be here.
const READ: &[(&str, Option<&str>)] = &[
    ("header_none.rs", None),
    ("header_licence_lines.rs", None),
    ("header_licence_block.rs", None),
    ("header_shebang.rs", None),
    ("header_shebang_licence_inner_doc.rs", None),
    ("header_inner_attribute.rs", None),
    ("header_licence_above_inner_attribute.rs", None),
    ("header_inner_doc.rs", None),
    ("header_inner_block_doc.rs", None),
    ("header_licence_then_inner_doc.rs", None),
    ("header_licence_above_inner_doc.rs", None),
    // the header directly above the first item is inside the item's node: found all the same
    ("header_above_item.rs", Some("above_item")),
    ("header_comment_above_item.rs", None),
    ("header_outer_doc_first.rs", None),
    ("header_attribute_first.rs", None),
    ("header_run_then_blank_then_comment.rs", None),
    // in a CRLF file a blank line does not end the node, so the header is a child of the item's node: found
    ("header_crlf_blank_then_item.rs", Some("crlf_blank")),
    ("header_crlf_blank_then_doc.rs", Some("crlf_doc")),
    ("header_crlf_unmarked.rs", None),
    // after the first token, or not a `// ID:` line: not a header
    ("header_after_the_first_token.rs", None),
    ("header_other_label.rs", None),
    ("header_empty_text.rs", None),
    ("header_doc_lookalike.rs", None),
    ("header_block_lookalike.rs", None),
    ("header_two.rs", Some("first")),
    ("header_code_after_block.rs", None),
    ("header_unterminated_block.rs", None),
    ("header_comments_only.rs", None),
    ("header_no_final_newline.rs", None),
    ("header_bom.rs", None),
    ("header_bom_none.rs", None),
    ("header_empty.rs", None),
];

#[test]
fn every_header_fixture_is_listed_and_read_as_the_table_says() {
    let mut on_disk: Vec<String> = fs::read_dir("tests/fixtures")
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.starts_with("header_"))
        .collect();
    on_disk.sort();
    let mut listed: Vec<&str> = READ.iter().map(|(n, _)| *n).collect();
    listed.sort();
    assert_eq!(on_disk, listed, "a header fixture is not in the table");
    for (name, want) in READ {
        assert_eq!(
            header_marker(&fixture(name), STYLE).as_deref(),
            *want,
            "{name}"
        );
    }
}

/// The text after `h1` is planned and applied, or the reason there is no edit.
fn planned(text: &str) -> String {
    match plan_header_marker(text, "h1", STYLE) {
        Ok(edit) => apply(text, &[edit]).unwrap(),
        Err(skip) => format!("SKIP: {skip:?}\n"),
    }
}

#[test]
fn the_planned_text_of_every_header_fixture_is_the_reviewed_one() {
    for (name, _) in READ {
        let text = fixture(name);
        expect_file(
            &Path::new("tests/header_planned").join(name),
            &planned(&text),
        );
    }
}

/// Plan, apply: the marker reads back, a second plan answers `AlreadyMarked` with it, and the defs are the same ones, each with its own
/// marker and none with a new parse error. Every fixture of the crate, not only the header ones.
#[test]
fn a_header_planned_on_every_fixture_round_trips() {
    let mut planned_files = 0;
    for entry in fs::read_dir("tests/fixtures").unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        let Ok(edit) = plan_header_marker(&text, "h1", STYLE) else {
            continue;
        };
        planned_files += 1;
        let new = apply(&text, std::slice::from_ref(&edit)).unwrap();
        let file = path.display();
        assert_eq!(header_marker(&new, STYLE).as_deref(), Some("h1"), "{file}");
        assert_eq!(
            plan_header_marker(&new, "h2", STYLE),
            Err(Skip::AlreadyMarked {
                existing: "h1".to_owned()
            }),
            "{file}: planned twice"
        );
        let (before, after) = (
            outline(&text, Edition::E2024, STYLE),
            outline(&new, Edition::E2024, STYLE),
        );
        assert_eq!(
            after.syntax_errors.len(),
            before.syntax_errors.len(),
            "{file}"
        );
        let named = |o: &rust_syntax::Outline| -> Vec<_> {
            o.defs
                .iter()
                .map(|d| (d.qualname.clone(), d.marker.clone(), d.syntax_error))
                .collect()
        };
        assert_eq!(
            named(&after),
            named(&before),
            "{file}: the header changed a def"
        );
        // only the one comment was added
        let (a, b) = (
            edit.at.start as usize,
            edit.at.start as usize + edit.text.len(),
        );
        assert_eq!(format!("{}{}", &new[..a], &new[b..]), text, "{file}");
    }
    assert!(planned_files >= 40, "{planned_files} files took a header");
}

#[test]
fn the_reasons_there_is_no_edit() {
    let text = "fn f() {}\n";
    for bad in ["", " a", "a ", "a\nb", "a*/b"] {
        assert_eq!(
            plan_header_marker(text, bad, STYLE),
            Err(Skip::InvalidText),
            "{bad:?}"
        );
    }
    assert_eq!(
        plan_header_marker("// ID: old\nfn f() {}\n", "h1", STYLE),
        Err(Skip::AlreadyMarked {
            existing: "old".to_owned()
        })
    );
    // a header put after an unterminated `/*` would be inside it
    assert_eq!(
        plan_header_marker("/* never closed\nfn f() {}\n", "h1", STYLE),
        Err(Skip::SyntaxError)
    );
}

#[test]
fn the_label_is_the_callers() {
    let note = MarkerStyle {
        label: "NOTE",
        placement: Placement::Docstring,
    };
    let text = "// NOTE: n1\n// ID: i1\nfn f() {}\n";
    assert_eq!(header_marker(text, note).as_deref(), Some("n1"));
    assert_eq!(header_marker(text, STYLE).as_deref(), Some("i1"));
    let edit = plan_header_marker("fn f() {}\n", "k", note).unwrap();
    assert_eq!(
        apply("fn f() {}\n", &[edit]).unwrap(),
        "// NOTE: k\nfn f() {}\n"
    );
}

/// §6, "before the first item": the header written directly above the first item is a child of the item's node in any file, so it is found
/// whatever the item is; and the first plan on the unmarked twin lands directly above it too.
#[test]
fn a_header_directly_above_the_first_item_is_found_and_planned_again_is_marked() {
    let marked = fixture("header_above_item.rs");
    assert_eq!(header_marker(&marked, STYLE).as_deref(), Some("above_item"));
    assert_eq!(
        plan_header_marker(&marked, "h2", STYLE),
        Err(Skip::AlreadyMarked {
            existing: "above_item".to_owned()
        })
    );
    // unmarked, with a comment directly above the item: the header goes below it, still directly above the item
    let text = fixture("header_comment_above_item.rs");
    let new = planned(&text);
    assert_eq!(new, "// About f.\n// ID: h1\nfn f() {}\n");
    assert_eq!(header_marker(&new, STYLE).as_deref(), Some("h1"));
    assert!(matches!(
        plan_header_marker(&new, "h2", STYLE),
        Err(Skip::AlreadyMarked { .. })
    ));
}

/// §6, the `\r\n` case (S-RS4): a header, a blank line and the item in a CRLF file. The engine's node holds the header across the blank
/// line, and it is found; a plan puts one in the file's own line ending, and a second plan answers `AlreadyMarked`.
#[test]
fn a_header_and_a_blank_line_in_a_crlf_file_is_found_and_never_planned_twice() {
    for name in [
        "header_crlf_blank_then_item.rs",
        "header_crlf_blank_then_doc.rs",
    ] {
        let text = fixture(name);
        assert!(
            text.contains("\r\n\r\n"),
            "{name} is a CRLF file with a blank line"
        );
        let found = header_marker(&text, STYLE).unwrap_or_else(|| panic!("{name}: not found"));
        assert_eq!(
            plan_header_marker(&text, "h2", STYLE),
            Err(Skip::AlreadyMarked { existing: found }),
            "{name}"
        );
    }
    // unmarked: the line ending is the file's, and what was written is found
    let text = fixture("header_crlf_unmarked.rs");
    let new = planned(&text);
    assert_eq!(
        new,
        "// Copyright (c) Someone.\r\n// ID: h1\r\n\r\nfn f() {}\r\n"
    );
    assert!(
        !new.replace("\r\n", "").contains(['\r', '\n']),
        "no bare line break was added"
    );
    assert_eq!(header_marker(&new, STYLE).as_deref(), Some("h1"));
    assert!(matches!(
        plan_header_marker(&new, "h2", STYLE),
        Err(Skip::AlreadyMarked { .. })
    ));
}
