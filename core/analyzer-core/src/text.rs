//! Byte spans and byte offset ⇄ (line, column). Nothing here knows a language.

use serde::Serialize;

/// A byte range `start..end` in one file's UTF-8 text.
///
/// Every engine has its own range type; each `sem.rs` converts to this one at its edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// `end` below `start` is a caller bug; it is clamped to an empty span at `start` rather than panicking.
    pub fn new(start: u32, end: u32) -> Self {
        Span {
            start,
            end: end.max(start),
        }
    }

    pub fn len(self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn contains(self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }

    /// The text this span covers, or `None` when it is out of bounds or cuts a character in two.
    pub fn slice(self, text: &str) -> Option<&str> {
        text.get(self.start as usize..self.end as usize)
    }
}

/// How a column is counted. The wire's rule is UTF-16 (the driver asks for it); the engines count bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Encoding {
    Utf8,
    Utf16,
}

/// A zero-based line and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

/// The start offset of every line of one text. A line ends at `\n`, `\r\n` or a lone `\r`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    starts: Vec<u32>,
    len: u32,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut starts = vec![0];
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                    i += 2;
                    starts.push(i as u32);
                }
                b'\n' | b'\r' => {
                    i += 1;
                    starts.push(i as u32);
                }
                _ => i += 1,
            }
        }
        LineIndex {
            starts,
            len: bytes.len() as u32,
        }
    }

    /// A text with no characters still has one (empty) line.
    pub fn line_count(&self) -> u32 {
        self.starts.len() as u32
    }

    pub fn line_start(&self, line: u32) -> Option<u32> {
        self.starts.get(line as usize).copied()
    }

    /// The line `offset` is on. An offset past the end belongs to no line.
    pub fn line_of(&self, offset: u32) -> Option<u32> {
        if offset > self.len {
            return None;
        }
        // `partition_point` counts the line starts at or before `offset`; the last of them is our line.
        Some(self.starts.partition_point(|&s| s <= offset) as u32 - 1)
    }

    /// `None` when `offset` is out of bounds or in the middle of a character.
    pub fn position(&self, text: &str, offset: u32, encoding: Encoding) -> Option<Position> {
        if !text.is_char_boundary(offset as usize) {
            return None;
        }
        let line = self.line_of(offset)?;
        let start = self.starts[line as usize];
        let before = &text[start as usize..offset as usize];
        let column = match encoding {
            Encoding::Utf8 => before.len(),
            Encoding::Utf16 => before.encode_utf16().count(),
        };
        Some(Position {
            line,
            column: column as u32,
        })
    }

    /// The inverse of [`position`](Self::position). `None` for a line that does not exist, a column past the end of its
    /// line, or a column that falls inside a character (or inside a UTF-16 surrogate pair).
    pub fn offset(&self, text: &str, pos: Position, encoding: Encoding) -> Option<u32> {
        let start = self.line_start(pos.line)?;
        let end = self
            .starts
            .get(pos.line as usize + 1)
            .copied()
            .unwrap_or(self.len);
        let line = text.get(start as usize..end as usize)?;
        let mut units = 0u32;
        for (i, ch) in line.char_indices() {
            if units == pos.column {
                return Some(start + i as u32);
            }
            if units > pos.column {
                return None;
            }
            units += match encoding {
                Encoding::Utf8 => ch.len_utf8() as u32,
                Encoding::Utf16 => ch.len_utf16() as u32,
            };
        }
        (units == pos.column).then_some(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: u32, column: u32) -> Position {
        Position { line, column }
    }

    #[test]
    fn span_slice_refuses_a_cut_character() {
        assert_eq!(Span::new(0, 3).slice("café"), Some("caf"));
        assert_eq!(Span::new(0, 4).slice("café"), None);
        assert_eq!(Span::new(0, 99).slice("café"), None);
    }

    #[test]
    fn span_new_clamps_a_reversed_range() {
        assert_eq!(Span::new(5, 2), Span { start: 5, end: 5 });
    }

    #[test]
    fn empty_text_has_one_line() {
        let index = LineIndex::new("");
        assert_eq!(index.line_count(), 1);
        assert_eq!(index.position("", 0, Encoding::Utf8), Some(pos(0, 0)));
    }

    #[test]
    fn line_endings() {
        let text = "a\nb\r\nc\rd";
        let index = LineIndex::new(text);
        assert_eq!(index.line_count(), 4);
        assert_eq!(
            (0..4)
                .map(|l| index.line_start(l).unwrap())
                .collect::<Vec<_>>(),
            [0, 2, 5, 7]
        );
        // the `\n` of a `\r\n` is still on the line it ends
        assert_eq!(index.line_of(4), Some(1));
    }

    #[test]
    fn newline_is_the_last_column_of_its_line() {
        let text = "a = 1\nb = 2\n";
        let index = LineIndex::new(text);
        assert_eq!(index.position(text, 5, Encoding::Utf8), Some(pos(0, 5)));
        assert_eq!(index.position(text, 6, Encoding::Utf8), Some(pos(1, 0)));
        assert_eq!(index.position(text, 12, Encoding::Utf8), Some(pos(2, 0)));
        assert_eq!(index.position(text, 13, Encoding::Utf8), None);
    }

    #[test]
    fn columns_by_encoding() {
        // `é` is 2 bytes, 1 UTF-16 unit; `😀` is 4 bytes, 2 UTF-16 units
        let text = "é😀x";
        let index = LineIndex::new(text);
        let x = text.find('x').unwrap() as u32;
        assert_eq!(index.position(text, x, Encoding::Utf8), Some(pos(0, 6)));
        assert_eq!(index.position(text, x, Encoding::Utf16), Some(pos(0, 3)));
    }

    #[test]
    fn offset_inside_a_character_is_none() {
        let text = "é";
        let index = LineIndex::new(text);
        assert_eq!(index.position(text, 1, Encoding::Utf8), None);
    }

    #[test]
    fn offset_is_the_inverse_of_position() {
        let text = "é😀x\nline two\r\nlast";
        let index = LineIndex::new(text);
        for enc in [Encoding::Utf8, Encoding::Utf16] {
            for (offset, _) in text.char_indices().chain([(text.len(), ' ')]) {
                let offset = offset as u32;
                let p = index.position(text, offset, enc).unwrap();
                assert_eq!(index.offset(text, p, enc), Some(offset), "{enc:?} {offset}");
            }
        }
    }

    #[test]
    fn offset_rejects_the_middle_of_a_surrogate_pair() {
        let text = "😀x";
        let index = LineIndex::new(text);
        assert_eq!(index.offset(text, pos(0, 1), Encoding::Utf16), None);
        assert_eq!(index.offset(text, pos(0, 2), Encoding::Utf16), Some(4));
        assert_eq!(index.offset(text, pos(0, 99), Encoding::Utf16), None);
        assert_eq!(index.offset(text, pos(3, 0), Encoding::Utf16), None);
    }
}
