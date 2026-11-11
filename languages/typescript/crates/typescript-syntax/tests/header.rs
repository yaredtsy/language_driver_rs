//! The file header marker by example, and as a round trip over every fixture file.

use analyzer_core::edit::apply;
use typescript_syntax::{
    Dialect, MarkerStyle, Placement, Skip, header_marker, outline, plan_header_marker,
};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

fn mark(text: &str) -> Result<String, Skip> {
    let edit = plan_header_marker(text, "h1", STYLE)?;
    let new = apply(text, std::slice::from_ref(&edit)).expect("the edit applies");
    assert_eq!(
        header_marker(&new, STYLE).as_deref(),
        Some("h1"),
        "it reads back:\n{new}"
    );
    Ok(new)
}

#[test]
fn a_file_with_code_first_gets_the_header_on_top() {
    assert_eq!(
        mark("function f() {}\n").unwrap(),
        "// ID: h1\nfunction f() {}\n"
    );
}

#[test]
fn a_shebang_stays_first() {
    assert_eq!(
        mark("#!/usr/bin/env node\nmain();\n").unwrap(),
        "#!/usr/bin/env node\n// ID: h1\nmain();\n"
    );
    assert_eq!(
        mark("#!/usr/bin/env node\r\nmain();\r\n").unwrap(),
        "#!/usr/bin/env node\r\n// ID: h1\r\nmain();\r\n"
    );
}

#[test]
fn a_licence_block_stays_above_the_header() {
    assert_eq!(
        mark("/*\n * Copyright.\n */\n\nfunction f() {}\n").unwrap(),
        "/*\n * Copyright.\n */\n// ID: h1\n\nfunction f() {}\n"
    );
    // a licence that is a JSDoc but is followed by a blank line is not the def's
    assert_eq!(
        mark("/**\n * Licence.\n */\n\nfunction f() {}\n").unwrap(),
        "/**\n * Licence.\n */\n// ID: h1\n\nfunction f() {}\n"
    );
}

#[test]
fn the_first_defs_own_jsdoc_stays_attached_to_it() {
    assert_eq!(
        mark("/**\n * Doc of f.\n */\nfunction f() {}\n").unwrap(),
        "// ID: h1\n/**\n * Doc of f.\n */\nfunction f() {}\n"
    );
    // a licence, then the def's JSDoc: the header goes between them
    assert_eq!(
        mark("/* Licence. */\n/**\n * Doc of f.\n */\nfunction f() {}\n").unwrap(),
        "/* Licence. */\n// ID: h1\n/**\n * Doc of f.\n */\nfunction f() {}\n"
    );
    // a comment between the JSDoc and the def does not detach it (the rule a def's marker is read with)
    assert_eq!(
        mark("/** Doc of f. */\n// eslint-disable-next-line\nfunction f() {}\n").unwrap(),
        "// ID: h1\n/** Doc of f. */\n// eslint-disable-next-line\nfunction f() {}\n"
    );
    assert_eq!(
        mark("/** Doc of f. */\n/* a note */\nfunction f() {}\n").unwrap(),
        "// ID: h1\n/** Doc of f. */\n/* a note */\nfunction f() {}\n"
    );
}

#[test]
fn directives_and_pragmas_come_after_the_header() {
    assert_eq!(
        mark("/// <reference path=\"a.d.ts\" />\n// @ts-check\nimport x from \"x\";\n").unwrap(),
        "// ID: h1\n/// <reference path=\"a.d.ts\" />\n// @ts-check\nimport x from \"x\";\n"
    );
    assert_eq!(
        mark("\"use strict\";\nf();\n").unwrap(),
        "// ID: h1\n\"use strict\";\nf();\n"
    );
    // a licence block, then the references
    assert_eq!(
        mark("/* Licence. */\n/// <reference path=\"a.d.ts\" />\nf();\n").unwrap(),
        "/* Licence. */\n// ID: h1\n/// <reference path=\"a.d.ts\" />\nf();\n"
    );
}

#[test]
fn a_shebang_then_a_licence_then_code() {
    assert_eq!(
        mark("#!/usr/bin/env node\n/* Licence. */\n\"use strict\";\n").unwrap(),
        "#!/usr/bin/env node\n/* Licence. */\n// ID: h1\n\"use strict\";\n"
    );
}

#[test]
fn an_empty_file_and_a_file_of_comments() {
    assert_eq!(mark("").unwrap(), "// ID: h1\n");
    assert_eq!(
        mark("/* only a comment */").unwrap(),
        "/* only a comment */\n// ID: h1\n"
    );
    assert_eq!(
        mark("/* only a comment */\n").unwrap(),
        "/* only a comment */\n// ID: h1\n"
    );
    assert_eq!(mark("// a line\n").unwrap(), "// ID: h1\n// a line\n");
    assert_eq!(
        mark("#!/usr/bin/env node").unwrap(),
        "#!/usr/bin/env node\n// ID: h1\n"
    );
}

#[test]
fn a_block_comment_with_code_on_its_line_is_not_skipped() {
    assert_eq!(mark("/* c */ f();\n").unwrap(), "// ID: h1\n/* c */ f();\n");
}

#[test]
fn the_files_line_ending_and_a_byte_order_mark_are_kept() {
    assert_eq!(
        mark("f();\r\ng();\r\n").unwrap(),
        "// ID: h1\r\nf();\r\ng();\r\n"
    );
    assert_eq!(mark("\u{feff}f();\n").unwrap(), "\u{feff}// ID: h1\nf();\n");
}

#[test]
fn a_marked_header_is_refused_with_what_it_has() {
    assert_eq!(
        mark("// ID: abc\nf();\n"),
        Err(Skip::AlreadyMarked {
            existing: "abc".to_owned()
        })
    );
    // anywhere among the comments before the first token
    assert_eq!(
        mark("/* Licence. */\n// ID: abc\nf();\n"),
        Err(Skip::AlreadyMarked {
            existing: "abc".to_owned()
        })
    );
}

#[test]
fn only_a_line_comment_is_a_header_and_only_before_code() {
    assert_eq!(
        header_marker("/* ID: x */\nf();\n", STYLE),
        None,
        "a block comment"
    );
    assert_eq!(
        header_marker("/** ID: x */\nf();\n", STYLE),
        None,
        "a JSDoc is the def's"
    );
    assert_eq!(header_marker("f();\n// ID: x\n", STYLE), None, "after code");
    assert_eq!(header_marker("// IDENT: x\n", STYLE), None, "another label");
    assert_eq!(
        header_marker("//ID:x\nf();\n", STYLE).as_deref(),
        Some("x"),
        "loose spacing"
    );
}

#[test]
fn the_text_must_fit_on_a_line() {
    for bad in ["", "  ", "two\nlines"] {
        assert_eq!(
            plan_header_marker("f();\n", bad, STYLE),
            Err(Skip::InvalidText),
            "{bad:?}"
        );
    }
}

#[test]
fn the_label_is_the_callers() {
    let style = MarkerStyle::docstring("FileRef");
    let edit = plan_header_marker("f();\n", "k", style).unwrap();
    assert_eq!(apply("f();\n", &[edit]).unwrap(), "// FileRef: k\nf();\n");
}

/// Every fixture file: plan, apply, read back; the defs are unchanged but for their offsets; nothing but the line was added; a second plan is
/// refused.
#[test]
fn every_fixture_takes_a_header_marker() {
    let mut planned = 0;
    for (dir, dialect) in [
        ("tests/fixtures/ts", Dialect::Ts),
        ("tests/fixtures/tsx", Dialect::Tsx),
        ("tests/fixtures/js", Dialect::Js),
        ("tests/fixtures/jsx", Dialect::Jsx),
    ] {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        files.sort();
        for path in files {
            let name = path.display().to_string();
            let text = std::fs::read_to_string(&path).unwrap();
            let before = outline(&text, dialect, STYLE);
            let Ok(edit) = plan_header_marker(&text, "round-trip", STYLE) else {
                assert!(
                    header_marker(&text, STYLE).is_some(),
                    "{name}: refused without having a header"
                );
                continue;
            };
            planned += 1;
            let new = apply(&text, std::slice::from_ref(&edit)).unwrap();
            assert_eq!(
                header_marker(&new, STYLE).as_deref(),
                Some("round-trip"),
                "{name}"
            );
            // nothing but the inserted line was added
            let (a, b) = (
                edit.at.start as usize,
                edit.at.start as usize + edit.text.len(),
            );
            let mut restored = new.clone();
            restored.replace_range(a..b, "");
            assert_eq!(restored, text, "{name}: the edit added more than its line");
            let after = outline(&new, dialect, STYLE);
            let key = |o: &typescript_syntax::Outline| {
                o.defs
                    .iter()
                    .map(|d| {
                        (
                            d.qualname.clone(),
                            d.kind.clone(),
                            d.marker.clone(),
                            d.syntax_error,
                        )
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(key(&after), key(&before), "{name}: the defs changed");
            assert_eq!(
                after.syntax_errors.len(),
                before.syntax_errors.len(),
                "{name}"
            );
            assert!(
                matches!(
                    plan_header_marker(&new, "second", STYLE),
                    Err(Skip::AlreadyMarked { .. })
                ),
                "{name}: marked twice"
            );
        }
    }
    assert!(
        planned > 25,
        "only {planned} files took a header: the round trip proved little"
    );
}
