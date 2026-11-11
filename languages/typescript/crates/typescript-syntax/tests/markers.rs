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
    // a blank line on the way detaches the block, on either side of the comment
    for text in [
        "/** ID: abc */\n\n// note\nfunction f() {}\n",
        "/** ID: abc */\n// note\n\nfunction f() {}\n",
    ] {
        assert_eq!(outline(text, Dialect::Ts, STYLE).defs[0].marker, None);
    }
}

#[test]
fn a_detached_block_stays_the_files_and_the_def_gets_its_own() {
    let text = "/**\n * About the file.\n */\n\nfunction f() {}\n";
    assert_eq!(
        mark(text, "f").unwrap(),
        "/**\n * About the file.\n */\n\n/** ID: m1 */\nfunction f() {}\n"
    );
}

#[test]
fn a_detached_block_with_tags_is_refused_and_nothing_is_stacked_on_it() {
    // TypeScript still applies the block across the blank line. A new block below it would switch `@param {number}` off: in a checked
    // `.js` file that is error TS7006 on `x`
    let typed = "/** @param {number} x */\n\nfunction f(x) { return x.toFixed(); }\n";
    assert_eq!(
        mark_as(typed, Dialect::Js, "f", "m1"),
        Err(Skip::UnsafeDocComment)
    );
    let multi = "/**\n * Doc.\n * @returns nothing\n */\n\nfunction f() {}\n";
    assert_eq!(mark(multi, "f"), Err(Skip::UnsafeDocComment));
    // through a comment too
    assert_eq!(
        mark("/** @deprecated */\n\n// note\nfunction f() {}\n", "f"),
        Err(Skip::UnsafeDocComment)
    );
    // an inline tag is not a block tag
    assert!(mark("/** See {@link g}. */\n\nfunction f() {}\n", "f").is_ok());
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
    // a directive separated by a blank line points at nothing of ours
    assert!(mark("// @ts-ignore\n\nfunction f() {}\n", "f").is_ok());
}

#[test]
fn marker_text_that_would_break_the_comment_is_refused() {
    for bad in [
        "",
        "   ",
        "two\nlines",
        "carriage\rreturn",
        "closes */ early",
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
        3,
        "ignored, linted, formatted: {tally:?}"
    );
    assert_eq!(
        count("UnsafeDocComment"),
        4,
        "three blocks that cannot be edited, one detached block with tags: {tally:?}"
    );
}
