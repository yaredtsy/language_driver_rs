//! A def's byte spans become the right line and column through core's `LineIndex`, on text where bytes, characters and UTF-16 units all
//! differ. This is what the driver does on every outline, so it is tested here, with the outline that feeds it. (pylspt's
//! `check_positions.rs` was a hand-run check of the same thing on ruff's own line index.)

use analyzer_core::text::{Encoding, LineIndex, Position};
use python_syntax::{MarkerStyle, outline};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: python_syntax::Placement::Docstring,
};

fn at(text: &str, index: &LineIndex, offset: u32, encoding: Encoding) -> (u32, u32) {
    let p: Position = index
        .position(text, offset, encoding)
        .expect("an offset inside the text");
    (p.line, p.column)
}

#[test]
fn byte_spans_become_lines_and_columns_in_either_encoding() {
    let text = std::fs::read_to_string("tests/fixtures/unicode_offsets.py").unwrap();
    let index = LineIndex::new(&text);
    let defs = outline(&text, STYLE).defs;
    let class = defs.iter().find(|d| d.qualname == "日本語").unwrap();
    let method = defs.iter().find(|d| d.parent.is_some()).unwrap();

    // `class 日本語:` is on the 7th line (0-based 6). The name starts after `class `: column 6 in every encoding, because
    // everything before it is ASCII
    assert_eq!(
        at(&text, &index, class.name_range.start, Encoding::Utf16),
        (6, 6)
    );
    // it ends after three CJK characters: 3 UTF-16 units, 9 bytes
    assert_eq!(
        at(&text, &index, class.name_range.end, Encoding::Utf16),
        (6, 9)
    );
    assert_eq!(
        at(&text, &index, class.name_range.end, Encoding::Utf8),
        (6, 15)
    );
    assert_eq!(class.name_range.len(), 9);

    // the method name is `𝐱`: one character, two UTF-16 units, four bytes
    assert_eq!(
        at(&text, &index, method.name_range.start, Encoding::Utf16),
        (7, 8)
    );
    assert_eq!(
        at(&text, &index, method.name_range.end, Encoding::Utf16),
        (7, 10)
    );
    assert_eq!(method.name_range.len(), 4);
    // and its qualname is the normalized `x`, one byte, so the two lengths must never be assumed equal
    assert_eq!(method.name, "x");
}
