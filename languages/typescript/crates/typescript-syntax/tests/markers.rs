//! The marker rule by example: what `plan_marker` writes for each case of `docs/03-typescript/04-ts2-markers.md` §3, and each reason it refuses.
//! Every expected text here is the rule; read them as the specification.

use analyzer_core::edit::apply;
use typescript_syntax::{Dialect, MarkerStyle, Skip, marker, outline, plan_marker};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: typescript_syntax::Placement::Docstring,
};

/// Plan a marker `m1` on the def named `qualname` (the first one of that name) and apply it.
fn mark(text: &str, qualname: &str) -> Result<String, Skip> {
    mark_as(text, Dialect::Ts, qualname, "m1")
}

fn mark_as(
    text: &str,
    dialect: Dialect,
    qualname: &str,
    marker_text: &str,
) -> Result<String, Skip> {
    let o = outline(text, dialect, STYLE);
    let def = o
        .defs
        .iter()
        .find(|d| d.qualname == qualname)
        .unwrap_or_else(|| panic!("no def {qualname}"));
    let edit = plan_marker(text, dialect, def, marker_text, STYLE)?;
    let new = apply(text, std::slice::from_ref(&edit)).expect("the edit applies");
    // whatever was planned must read back, on the same def
    let after = outline(&new, dialect, STYLE);
    let again = after.defs.iter().find(|d| d.qualname == qualname).unwrap();
    assert_eq!(
        again.marker.as_deref(),
        Some(marker_text),
        "the marker reads back:\n{new}"
    );
    assert_eq!(
        marker(&new, dialect, again, STYLE).as_deref(),
        Some(marker_text)
    );
    Ok(new)
}

#[test]
fn a_multi_line_block_without_tags_gets_the_line_last() {
    let text = "/**\n * Parses an order.\n */\nfunction f() {}\n";
    assert_eq!(
        mark(text, "f").unwrap(),
        "/**\n * Parses an order.\n * ID: m1\n */\nfunction f() {}\n"
    );
}

#[test]
fn a_block_with_tags_gets_the_line_before_the_first_tag() {
    let text = "/**\n * Parses an order.\n * @param raw the wire text\n * @returns the order\n */\nfunction f(raw) {}\n";
    assert_eq!(
        mark(text, "f").unwrap(),
        "/**\n * Parses an order.\n * ID: m1\n * @param raw the wire text\n * @returns the order\n */\nfunction f(raw) {}\n"
    );
}

#[test]
fn a_blank_star_line_before_the_tags_stays_where_it_was() {
    let text = "/**\n * Parses.\n *\n * @param x a value\n */\nfunction f(x) {}\n";
    assert_eq!(
        mark(text, "f").unwrap(),
        "/**\n * Parses.\n *\n * ID: m1\n * @param x a value\n */\nfunction f(x) {}\n"
    );
}

#[test]
fn a_block_that_is_only_a_tag_gets_the_line_first() {
    let text = "/**\n * @deprecated\n */\nfunction f() {}\n";
    assert_eq!(
        mark(text, "f").unwrap(),
        "/**\n * ID: m1\n * @deprecated\n */\nfunction f() {}\n"
    );
}

#[test]
fn a_one_line_block_becomes_several_lines() {
    assert_eq!(
        mark("/** Parses. */\nfunction f() {}\n", "f").unwrap(),
        "/**\n * Parses.\n * ID: m1\n */\nfunction f() {}\n"
    );
    // a tag stays after the marker: the marker ends the description
    assert_eq!(
        mark("/** @type {number} */\nfunction f() {}\n", "f").unwrap(),
        "/**\n * ID: m1\n * @type {number}\n */\nfunction f() {}\n"
    );
    assert_eq!(
        mark("/** */\nfunction f() {}\n", "f").unwrap(),
        "/**\n * ID: m1\n */\nfunction f() {}\n"
    );
    assert_eq!(
        mark("/***/\nfunction f() {}\n", "f").unwrap(),
        "/**\n * ID: m1\n */\nfunction f() {}\n"
    );
}

#[test]
fn a_one_line_block_keeps_the_indentation() {
    let text = "class C {\n    /** Doc. */\n    m() {}\n}\n";
    assert_eq!(
        mark(text, "C.m").unwrap(),
        "class C {\n    /**\n     * Doc.\n     * ID: m1\n     */\n    m() {}\n}\n"
    );
}

#[test]
fn no_block_means_a_new_one_line_block_above_the_def() {
    assert_eq!(
        mark("function f() {}\n", "f").unwrap(),
        "/** ID: m1 */\nfunction f() {}\n"
    );
    assert_eq!(
        mark("class C {\n\tm() {}\n}\n", "C.m").unwrap(),
        "class C {\n\t/** ID: m1 */\n\tm() {}\n}\n",
        "the def's indentation is copied byte for byte"
    );
}

#[test]
fn a_new_block_goes_below_a_plain_comment() {
    assert_eq!(
        mark("// helper\nfunction f() {}\n", "f").unwrap(),
        "// helper\n/** ID: m1 */\nfunction f() {}\n"
    );
    assert_eq!(
        mark("/* a note */\nfunction f() {}\n", "f").unwrap(),
        "/* a note */\n/** ID: m1 */\nfunction f() {}\n"
    );
}

#[test]
fn a_comment_between_the_block_and_the_def_does_not_detach_the_block() {
    // a second block below the note would hide the first: TypeScript uses only the last one
    assert_eq!(
        mark(
            "/**\n * Real docs.\n */\n// TODO: rename\nfunction f() {}\n",
            "f"
        )
        .unwrap(),
        "/**\n * Real docs.\n * ID: m1\n */\n// TODO: rename\nfunction f() {}\n"
    );
    assert_eq!(
        mark("/** Real docs. */\n/* a note */\nfunction f() {}\n", "f").unwrap(),
        "/**\n * Real docs.\n * ID: m1\n */\n/* a note */\nfunction f() {}\n"
    );
    // a directive stays on the line above the def: the line goes into the block, not between them
    assert_eq!(
        mark(
            "/**\n * Docs.\n */\n// eslint-disable-next-line no-unused-vars\nfunction f(x: any) {}\n",
            "f"
        )
        .unwrap(),
        "/**\n * Docs.\n * ID: m1\n */\n// eslint-disable-next-line no-unused-vars\nfunction f(x: any) {}\n"
    );
    // a comment after the block on its line
    assert_eq!(
        mark("/** Docs. */ // why\nfunction f() {}\n", "f").unwrap(),
        "/**\n * Docs.\n * ID: m1\n */ // why\nfunction f() {}\n"
    );
}

#[test]
fn a_marker_is_read_through_a_comment_between() {
    let text = "/**\n * Docs.\n * ID: abc\n */\n// @ts-ignore\nfunction f() {}\n";
    assert_eq!(
        mark(text, "f"),
        Err(Skip::AlreadyMarked {
            existing: "abc".to_owned()
        })
    );
    // a blank line on the way does not detach the block either, on either side of the comment
    for text in [
        "const a = 1;\n\n/** ID: abc */\n\n// note\nfunction f() {}\n",
        "const a = 1;\n\n/** ID: abc */\n// note\n\nfunction f() {}\n",
    ] {
        assert_eq!(
            outline(text, Dialect::Ts, STYLE).defs[0].marker.as_deref(),
            Some("abc"),
            "{text:?}"
        );
    }
}

#[test]
fn a_block_across_a_blank_line_is_the_defs_and_is_written_into() {
    // checked with tsc 5.1.6: TypeScript applies the block to the def across the blank line, and applies only the last block. So the
    // marker goes into it: a new block below would switch `@param {number}` off (error TS7006 on `x` in a checked `.js` file)
    let typed =
        "import \"x\";\n\n/** @param {number} x */\n\nfunction f(x) { return x.toFixed(); }\n";
    assert_eq!(
        mark_as(typed, Dialect::Js, "f", "m1").unwrap(),
        "import \"x\";\n\n/**\n * ID: m1\n * @param {number} x\n */\n\nfunction f(x) { return x.toFixed(); }\n"
    );
    let words = "const a = 1;\n\n/**\n * Doc.\n */\n\n// note\n\nfunction f() {}\n";
    assert_eq!(
        mark(words, "f").unwrap(),
        "const a = 1;\n\n/**\n * Doc.\n * ID: m1\n */\n\n// note\n\nfunction f() {}\n"
    );
    // and a block that cannot be edited is refused there too: nothing is stacked below it
    assert_eq!(
        mark("const a = 1;\n\n/**\n * Doc. */\n\nfunction f() {}\n", "f"),
        Err(Skip::UnsafeDocComment)
    );
}

#[test]
fn the_files_opening_comment_stays_the_files_and_the_def_gets_its_own() {
    // at the top of the file, down to the first blank line: a licence, an overview. It is not written into
    let text = "/**\n * About the file.\n */\n\nfunction f() {}\n";
    assert_eq!(
        mark(text, "f").unwrap(),
        "/**\n * About the file.\n */\n\n/** ID: m1 */\nfunction f() {}\n"
    );
    // and a marker line in it is not the def's
    let marked = "/**\n * About the file.\n * ID: the-files\n */\n\nfunction f() {}\n";
    assert_eq!(outline(marked, Dialect::Ts, STYLE).defs[0].marker, None);
    assert!(mark(marked, "f").is_ok());
    // a shebang and other comments in the opening comment change nothing
    let script = "#!/usr/bin/env node\n// Copyright.\n/** About the file. */\n\nfunction f() {}\n";
    assert_eq!(
        mark(script, "f").unwrap(),
        "#!/usr/bin/env node\n// Copyright.\n/** About the file. */\n\n/** ID: m1 */\nfunction f() {}\n"
    );
    // a block below the opening comment is the def's
    let both = "/* Licence. */\n\n/** Doc. */\n\nfunction f() {}\n";
    assert_eq!(
        mark(both, "f").unwrap(),
        "/* Licence. */\n\n/**\n * Doc.\n * ID: m1\n */\n\nfunction f() {}\n"
    );
    // with no blank line between the top of the file and the def, the block is the def's own
    assert_eq!(
        mark("/** Doc. */\n// note\nfunction f() {}\n", "f").unwrap(),
        "/**\n * Doc.\n * ID: m1\n */\n// note\nfunction f() {}\n"
    );
}

#[test]
fn the_files_opening_comment_with_tags_is_refused() {
    // neither way is safe. The block is the file's, so it is not written into. But TypeScript applies it to the def, and a new block
    // below it would switch `@param {number}` off: in a checked `.js` file that is error TS7006 on `x`
    let typed = "/** @param {number} x */\n\nfunction f(x) { return x.toFixed(); }\n";
    assert_eq!(
        mark_as(typed, Dialect::Js, "f", "m1"),
        Err(Skip::FileCommentAbove)
    );
    let multi = "/**\n * About the file.\n * @packageDocumentation\n */\n\nfunction f() {}\n";
    assert_eq!(mark(multi, "f"), Err(Skip::FileCommentAbove));
    // through a comment too
    assert_eq!(
        mark("/** @license MIT */\n\n// note\nfunction f() {}\n", "f"),
        Err(Skip::FileCommentAbove)
    );
    // an inline tag is not a block tag
    assert_eq!(
        mark("/** See {@link g}. */\n\nfunction f() {}\n", "f").unwrap(),
        "/** See {@link g}. */\n\n/** ID: m1 */\nfunction f() {}\n"
    );
}

#[test]
fn the_block_goes_above_the_decorators() {
    let text = "@sealed\n@tag(\"x\")\nexport class C {}\n";
    assert_eq!(
        mark(text, "C").unwrap(),
        "/** ID: m1 */\n@sealed\n@tag(\"x\")\nexport class C {}\n"
    );
    let documented = "/**\n * Doc.\n */\n@sealed\nclass C {}\n";
    assert_eq!(
        mark(documented, "C").unwrap(),
        "/**\n * Doc.\n * ID: m1\n */\n@sealed\nclass C {}\n"
    );
}

#[test]
fn a_const_arrow_is_marked_above_its_statement() {
    assert_eq!(
        mark("export const f = () => {};\n", "f").unwrap(),
        "/** ID: m1 */\nexport const f = () => {};\n"
    );
}

#[test]
fn overloads_are_marked_above_the_first_signature() {
    let text = "function f(a: string): string;\nfunction f(a: number): number;\nfunction f(a: any): any {\n  return a;\n}\n";
    assert_eq!(
        mark(text, "f").unwrap(),
        "/** ID: m1 */\nfunction f(a: string): string;\nfunction f(a: number): number;\nfunction f(a: any): any {\n  return a;\n}\n"
    );
}

#[test]
fn the_files_line_ending_is_used() {
    assert_eq!(
        mark("function f() {}\r\n", "f").unwrap(),
        "/** ID: m1 */\r\nfunction f() {}\r\n"
    );
    assert_eq!(
        mark("/**\r\n * Doc.\r\n */\r\nfunction f() {}\r\n", "f").unwrap(),
        "/**\r\n * Doc.\r\n * ID: m1\r\n */\r\nfunction f() {}\r\n"
    );
    assert_eq!(
        mark("/** Doc. */\r\nfunction f() {}\r\n", "f").unwrap(),
        "/**\r\n * Doc.\r\n * ID: m1\r\n */\r\nfunction f() {}\r\n"
    );
}

#[test]
fn a_byte_order_mark_stays_first() {
    assert_eq!(
        mark("\u{feff}function f() {}\n", "f").unwrap(),
        "\u{feff}/** ID: m1 */\nfunction f() {}\n"
    );
}

#[test]
fn the_label_is_the_callers() {
    let style = MarkerStyle::docstring("Ref");
    let text = "function f() {}\n";
    let o = outline(text, Dialect::Ts, style);
    let edit = plan_marker(text, Dialect::Ts, &o.defs[0], "k", style).unwrap();
    assert_eq!(
        apply(text, &[edit]).unwrap(),
        "/** Ref: k */\nfunction f() {}\n"
    );
}

#[test]
fn several_defs_on_one_line_are_refused() {
    let text =
        "function a() {} function b() {}\nconst c = () => 1, d = () => 2;\nclass E { m() {} }\n";
    assert_eq!(mark(text, "b"), Err(Skip::NotOnOwnLine));
    assert_eq!(mark(text, "c"), Err(Skip::NotOnOwnLine));
    assert_eq!(mark(text, "d"), Err(Skip::NotOnOwnLine));
    assert_eq!(mark(text, "E.m"), Err(Skip::NotOnOwnLine));
    assert!(mark(text, "a").is_ok(), "the first def of a line owns it");
    assert!(mark(text, "E").is_ok());
}

#[test]
fn a_comment_before_the_def_on_its_line_is_not_room() {
    assert_eq!(
        mark("/* c */ function f() {}\n", "f"),
        Err(Skip::NotOnOwnLine)
    );
}

#[test]
fn a_syntax_error_in_the_def_or_its_block_is_refused() {
    assert_eq!(
        mark("function f( {\n  const x =\n}\n", "f"),
        Err(Skip::SyntaxError)
    );
    // an error inside the block above
    let text = "/**\n * Doc.\n */\nfunction f() {}\n";
    let o = outline(text, Dialect::Ts, STYLE);
    let mut def = o.defs[0].clone();
    assert!(plan_marker(text, Dialect::Ts, &def, "k", STYLE).is_ok());
    def.syntax_error = true;
    assert_eq!(
        plan_marker(text, Dialect::Ts, &def, "k", STYLE),
        Err(Skip::SyntaxError)
    );
}

#[test]
fn a_def_from_another_text_is_refused() {
    let old = "function f() {}\n";
    let new = "const s = `\n  x\n`;\nfunction f() {}\n";
    let stale = outline(old, Dialect::Ts, STYLE).defs[0].clone();
    // the old offsets point into the template literal now: a block planned there would become part of the string
    assert_eq!(
        plan_marker(new, Dialect::Ts, &stale, "k", STYLE),
        Err(Skip::UnknownDef)
    );
    // the same start is not enough: the def's end moved, so the caller's text is not this one
    let longer = "function f() { return 1; }\n";
    assert_eq!(
        plan_marker(longer, Dialect::Ts, &stale, "k", STYLE),
        Err(Skip::UnknownDef)
    );
    // and no marker is read at a stale offset: here the old `f` sits where `g` is now
    let moved = "/** ID: of-g */\nfunction g() {}\nfunction f() {}\n";
    let mut at_g = stale.clone();
    at_g.range = outline(moved, Dialect::Ts, STYLE).defs[0].range;
    assert_eq!(marker(moved, Dialect::Ts, &at_g, STYLE), None);
    assert_eq!(
        plan_marker(moved, Dialect::Ts, &at_g, "k", STYLE),
        Err(Skip::UnknownDef)
    );
}

#[test]
fn a_marked_def_is_refused_with_what_it_has() {
    let text = "/**\n * Doc.\n * ID: abc\n */\nfunction f() {}\n";
    assert_eq!(
        mark(text, "f"),
        Err(Skip::AlreadyMarked {
            existing: "abc".to_owned()
        })
    );
    // a marker under a tag counts too
    let under = "/**\n * @returns x\n * ID: low\n */\nfunction f() {}\n";
    assert_eq!(
        mark(under, "f"),
        Err(Skip::AlreadyMarked {
            existing: "low".to_owned()
        })
    );
}

#[test]
fn a_block_that_cannot_be_edited_safely_is_refused_and_nothing_is_stacked_on_it() {
    // no closing line of its own
    assert_eq!(
        mark("/**\n * Doc. */\nfunction f() {}\n", "f"),
        Err(Skip::UnsafeDocComment)
    );
    // a line without its star
    assert_eq!(
        mark("/**\n * Doc.\n free text\n */\nfunction f() {}\n", "f"),
        Err(Skip::UnsafeDocComment)
    );
    // a tag on the opening line
    assert_eq!(
        mark("/** @param x\n * more\n */\nfunction f(x) {}\n", "f"),
        Err(Skip::UnsafeDocComment)
    );
}

#[test]
fn a_directive_above_the_def_is_not_separated_from_it() {
    for directive in [
        "// @ts-ignore",
        "// @ts-expect-error",
        "// eslint-disable-next-line no-console",
        "/* eslint-disable-next-line */",
        "// prettier-ignore",
        "// istanbul ignore next",
        "// biome-ignore lint: reason",
        "// deno-lint-ignore no-explicit-any",
        "// oxlint-disable-next-line",
        "// noinspection JSUnusedGlobalSymbols",
        "/* c8 ignore next */",
        "// $FlowFixMe[incompatible-type]",
    ] {
        let text = format!("{directive}\nfunction f() {{}}\n");
        assert_eq!(mark(&text, "f"), Err(Skip::DirectiveAbove), "{directive}");
    }
    // a directive further up, with a plain comment in between, does not point at the def: the plain comment does
    assert!(
        mark(
            "// eslint-disable-next-line\n// note\nfunction f() {}\n",
            "f"
        )
        .is_ok()
    );
    // a linter's directive separated by a blank line points at nothing of ours
    assert!(mark("// eslint-disable-next-line\n\nfunction f() {}\n", "f").is_ok());
}

#[test]
fn the_compilers_directive_reaches_the_def_through_blank_lines_and_line_comments() {
    // checked with tsc 5.1.6: the error on the def's line stays suppressed in the first three, and comes back when a block comment
    // stands between
    for reaches in [
        "// @ts-ignore\n\nfunction f() {}\n",
        "// @ts-ignore\n// a note\nfunction f() {}\n",
        "// @ts-expect-error\n\n// a note\n\nfunction f() {}\n",
        "/* @ts-ignore */\n// a note\nfunction f() {}\n",
    ] {
        assert_eq!(mark(reaches, "f"), Err(Skip::DirectiveAbove), "{reaches:?}");
    }
    assert!(mark("// @ts-ignore\n/* a note */\nfunction f() {}\n", "f").is_ok());
    // with a block to write in, nothing moves: the line goes into the block
    assert_eq!(
        mark("/** Docs. */\n// @ts-ignore\nfunction f() {}\n", "f").unwrap(),
        "/**\n * Docs.\n * ID: m1\n */\n// @ts-ignore\nfunction f() {}\n"
    );
    // a one-line block that is the directive would stop being one as several lines
    assert_eq!(
        mark("/** @ts-ignore */\nfunction f() {}\n", "f"),
        Err(Skip::DirectiveAbove)
    );
}

#[test]
fn a_comment_for_the_whole_file_is_a_plain_comment() {
    for (comment, name) in [
        (
            "/* eslint-disable no-console */",
            "a range, not the next line",
        ),
        ("// @ts-nocheck", "a file pragma"),
        ("// @ts-check", "a file pragma"),
        ("// deno-lint-ignore-file", "a file directive"),
        ("// eslinting is done in CI", "a word that starts like one"),
    ] {
        let text = format!("{comment}\nfunction f() {{}}\n");
        assert_eq!(
            mark(&text, "f").unwrap(),
            format!("{comment}\n/** ID: m1 */\nfunction f() {{}}\n"),
            "{name}"
        );
    }
}

#[test]
fn marker_text_that_would_break_the_comment_is_refused() {
    for bad in [
        "",
        "   ",
        "two\nlines",
        "carriage\rreturn",
        "closes */ early",
        // reading trims: these would come back as another text
        " leading",
        "trailing ",
        "\ttab",
    ] {
        assert_eq!(
            mark_as("function f() {}\n", Dialect::Ts, "f", bad),
            Err(Skip::InvalidText),
            "{bad:?}"
        );
    }
}

#[test]
fn javascript_and_jsx_are_marked_the_same_way() {
    for dialect in [Dialect::Js, Dialect::Jsx] {
        assert_eq!(
            mark_as("/** Doc. */\nfunction f() {}\n", dialect, "f", "m1").unwrap(),
            "/**\n * Doc.\n * ID: m1\n */\nfunction f() {}\n"
        );
    }
}

#[test]
fn a_marker_on_a_method_of_an_object_literal_and_a_class_property() {
    let text = "const api = {\n  load() {},\n};\nclass C {\n  handle = () => {};\n}\n";
    assert_eq!(
        mark(text, "api.load").unwrap(),
        "const api = {\n  /** ID: m1 */\n  load() {},\n};\nclass C {\n  handle = () => {};\n}\n"
    );
    assert_eq!(
        mark(text, "C.handle").unwrap(),
        "const api = {\n  load() {},\n};\nclass C {\n  /** ID: m1 */\n  handle = () => {};\n}\n"
    );
}

#[test]
fn every_skip_carries_a_reason_a_person_can_read() {
    let all = [
        Skip::NotOnOwnLine,
        Skip::SyntaxError,
        Skip::AlreadyMarked {
            existing: "abc".to_owned(),
        },
        Skip::UnsafeDocComment,
        Skip::FileCommentAbove,
        Skip::DirectiveAbove,
        Skip::InvalidText,
        Skip::PlacementNotSupported,
        Skip::UnknownDef,
    ];
    let reasons: Vec<String> = all.iter().map(|skip| skip.to_string()).collect();
    for (skip, reason) in all.iter().zip(&reasons) {
        assert!(reason.len() > 20, "{skip:?}: {reason:?}");
        assert!(
            reason.starts_with(|c: char| c.is_lowercase()) && !reason.ends_with('.'),
            "{skip:?}: a clause that fits after \"not marked: \": {reason:?}"
        );
        assert!(!reason.contains('\n'), "{skip:?}: one line");
        assert_eq!(
            reasons.iter().filter(|r| *r == reason).count(),
            1,
            "{skip:?}: its own reason"
        );
    }
    assert_eq!(
        reasons[2], "the definition already has a marker: abc",
        "the existing marker is shown"
    );
    // a reason is what the planner returns, so it can be shown as it comes
    let skip = mark("/* c */ function f() {}\n", "f").unwrap_err();
    assert!(skip.to_string().contains("line of its own"));
}

/// The marker fixtures must actually exercise planning and the refusals, or the round trip proves little.
#[test]
fn the_marker_fixtures_plan_and_refuse() {
    use std::collections::BTreeMap;
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let mut dir: Vec<_> = std::fs::read_dir("tests/fixtures/ts")
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    dir.sort();
    for path in dir {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.starts_with("marker_") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let o = outline(&text, Dialect::Ts, STYLE);
        for def in &o.defs {
            let outcome = match plan_marker(&text, Dialect::Ts, def, "tally", STYLE) {
                Ok(_) => "planned".to_owned(),
                Err(Skip::AlreadyMarked { .. }) => "already marked".to_owned(),
                Err(other) => format!("{other:?}"),
            };
            *tally.entry(outcome).or_default() += 1;
        }
    }
    let count = |k: &str| tally.get(k).copied().unwrap_or(0);
    println!("{tally:?}");
    assert!(count("planned") >= 25, "{tally:?}");
    assert!(count("already marked") >= 8, "{tally:?}");
    assert!(count("NotOnOwnLine") >= 1, "{tally:?}");
    assert_eq!(
        count("DirectiveAbove"),
        6,
        "ignored, linted, formatted, separatedByBlank, denoLinted, reachedThroughANote: {tally:?}"
    );
    assert_eq!(count("UnsafeDocComment"), 3, "{tally:?}");
    assert_eq!(count("FileCommentAbove"), 1, "{tally:?}");
}

// D21: an arrow held by a property of a `const` object literal is a def, and its marker is a JSDoc block above the property.

#[test]
fn a_property_arrow_without_a_block_gets_a_new_block_above_the_property_at_its_indent() {
    let text = "const api = {\n  load: () => {},\n};\n";
    assert_eq!(
        mark(text, "api.load").unwrap(),
        "const api = {\n  /** ID: m1 */\n  load: () => {},\n};\n"
    );
}

#[test]
fn a_property_arrow_with_a_block_gets_the_line_before_the_first_tag() {
    let text = "const api = {\n  /**\n   * Saves.\n   * @param x the thing\n   */\n  save: (x: number) => {},\n};\n";
    assert_eq!(
        mark(text, "api.save").unwrap(),
        "const api = {\n  /**\n   * Saves.\n   * ID: m1\n   * @param x the thing\n   */\n  save: (x: number) => {},\n};\n"
    );
}

#[test]
fn an_async_property_arrow_in_a_nested_wrapped_literal_is_marked_above_its_property() {
    let text = "const api = {\n  tasks: {\n    update: async () => {},\n  },\n} as const;\n";
    assert_eq!(
        mark(text, "api.tasks.update").unwrap(),
        "const api = {\n  tasks: {\n    /** ID: m1 */\n    update: async () => {},\n  },\n} as const;\n"
    );
}

#[test]
fn a_marker_on_a_property_arrow_does_not_touch_its_neighbours_or_the_names_inside_it() {
    let text =
        "const api = {\n  load: () => {\n    function inner() {}\n  },\n  other: () => {},\n};\n";
    let marked = mark(text, "api.load").unwrap();
    let before: Vec<String> = outline(text, Dialect::Ts, STYLE)
        .defs
        .iter()
        .map(|d| d.qualname.clone())
        .collect();
    let after: Vec<String> = outline(&marked, Dialect::Ts, STYLE)
        .defs
        .iter()
        .map(|d| d.qualname.clone())
        .collect();
    assert_eq!(before, after);
    assert_eq!(before, ["api.load", "api.inner", "api.other"]);
    // the neighbour has no marker
    let other = outline(&marked, Dialect::Ts, STYLE)
        .defs
        .into_iter()
        .find(|d| d.qualname == "api.other")
        .unwrap();
    assert_eq!(other.marker, None);
}
