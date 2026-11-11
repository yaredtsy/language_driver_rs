//! The outline never panics and always returns a well-formed list, whatever the text: every fixture cut at every character boundary, and some
//! hostile inputs.

use typescript_syntax::{Dialect, outline};

fn fixtures(dir: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .map(|p| {
            (
                p.display().to_string(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

fn check(name: &str, text: &str, dialect: Dialect) {
    let o = outline(text, dialect);
    assert!(o.is_well_formed(), "{name}: not a well-formed list");
    let len = text.len() as u32;
    for d in &o.defs {
        assert!(
            d.range.end <= len && d.name_range.end <= len,
            "{name}: a def reaches past the text"
        );
        assert!(
            d.range.slice(text).is_some(),
            "{name}: a range cuts a character: {:?}",
            d.range
        );
        assert!(
            d.name_range.slice(text).is_some(),
            "{name}: a name range cuts a character"
        );
    }
    for e in &o.syntax_errors {
        assert!(e.range.end <= len, "{name}: an error reaches past the text");
    }
}

#[test]
fn every_fixture_cut_at_every_character() {
    for (dir, dialect) in [
        ("tests/fixtures/ts", Dialect::Ts),
        ("tests/fixtures/tsx", Dialect::Tsx),
        ("tests/fixtures/js", Dialect::Js),
        ("tests/fixtures/jsx", Dialect::Jsx),
    ] {
        for (name, text) in fixtures(dir) {
            for (at, _) in text.char_indices().chain([(text.len(), ' ')]) {
                check(&format!("{name} cut at {at}"), &text[..at], dialect);
            }
        }
    }
}

#[test]
fn every_fixture_in_every_dialect() {
    // a `.ts` file read as `.tsx`, a `.js` as `.ts`: wrong dialects give errors, never a panic
    for dir in [
        "tests/fixtures/ts",
        "tests/fixtures/tsx",
        "tests/fixtures/js",
        "tests/fixtures/jsx",
    ] {
        for (name, text) in fixtures(dir) {
            for dialect in [Dialect::Ts, Dialect::Tsx, Dialect::Js, Dialect::Jsx] {
                check(&format!("{name} as {dialect:?}"), &text, dialect);
            }
        }
    }
}

#[test]
fn hostile_inputs() {
    let cases: Vec<(&str, String)> = vec![
        ("empty", String::new()),
        ("only a brace", "{".into()),
        ("unclosed everything", "function f( class C { [ ( {".into()),
        ("nul bytes", "function f() {}\0\0class C {}".into()),
        (
            "control characters",
            "function \u{1} f() {} \u{7f} \u{200b}".into(),
        ),
        (
            "lone surrogate-like text",
            "const s = '\u{fffd}\u{10ffff}';".into(),
        ),
        (
            "a very long line",
            format!("function f() {{ {} }}", "x(); ".repeat(20_000)),
        ),
        ("many defs", "function f() {}\n".repeat(5_000)),
        ("many classes", "class C { m() {} }\n".repeat(2_000)),
        ("only whitespace", " \n\t\r\n ".repeat(1_000)),
        ("unterminated template", "const t = `abc ${".into()),
        (
            "unterminated comment",
            "/* never closed function f() {}".into(),
        ),
        (
            "unterminated string",
            "const s = 'abc\nfunction f() {}".into(),
        ),
        (
            "regex ambiguity",
            "const r = a / b / c; function f() {}".into(),
        ),
    ];
    for (name, text) in cases {
        for dialect in [Dialect::Ts, Dialect::Tsx, Dialect::Js, Dialect::Jsx] {
            check(&format!("{name} as {dialect:?}"), &text, dialect);
        }
    }
}
