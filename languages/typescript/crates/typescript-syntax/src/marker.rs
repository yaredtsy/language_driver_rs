//! Reading (and, in `plan`, placing) a marker in the JSDoc block above a def. Plain text work on one comment: no engine type is named here,
//! so that a language with the same `/** … */` shape can share it by the rule of two.
//!
//! The rule is `docs/03-typescript/04-ts2-markers.md` §3 and `languages/typescript/docs/markers.md`.

use analyzer_core::text::Span;

use crate::sem::{Comment, CommentKind};

/// The offset of the first byte of the line that `start` is on, past a byte-order mark. `None` when something other than blanks stands
/// before `start` on that line: the def does not own its line.
pub(crate) fn own_line(text: &str, start: u32) -> Option<u32> {
    let before = text.get(..start as usize)?;
    let line = before.rfind(['\n', '\r']).map_or(0, |i| i + 1);
    let bom = if line == 0 && before.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };
    before[line + bom..]
        .chars()
        .all(|c| c == ' ' || c == '\t')
        .then_some((line + bom) as u32)
}

/// The JSDoc block that documents the def starting at `start`: the last comment before it, if it is a `/** */` block with only whitespace
/// between it and the def and **no blank line** (a blank line detaches it: the comment then belongs to the file). A `//` or `/* */` comment
/// between the block and the def detaches it too: only the nearest block counts, and it must be the nearest comment.
pub(crate) fn doc_above(text: &str, start: u32, comments: &[Comment]) -> Option<Span> {
    let last = comments.last().filter(|c| c.kind == CommentKind::Jsdoc)?;
    let between = text.get(last.span.end as usize..start as usize)?;
    let breaks = line_breaks(between);
    (between.chars().all(char::is_whitespace) && breaks <= 1).then_some(last.span)
}

/// How many line breaks `text` has (`\r\n` is one).
fn line_breaks(text: &str) -> usize {
    let mut count = 0;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => count += 1,
            '\r' => {
                count += 1;
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            _ => {}
        }
    }
    count
}

/// The marker in a block comment's text, `/** … */`: the first line that, after its leading `*` and spaces, reads `<label>:<spaces><text>`.
/// The text is returned as written, trimmed. A one-line block (`/** ID: x */`) is read the same way. A marker under a block tag is read too.
pub(crate) fn read(block: &str, label: &str) -> Option<String> {
    let lines: Vec<&str> = split_lines(block);
    let last = lines.len() - 1;
    lines.iter().enumerate().find_map(|(i, line)| {
        let mut s = line.trim();
        if i == last {
            s = s.strip_suffix("*/").unwrap_or(s).trim_end();
        }
        if i == 0 {
            s = s.strip_prefix("/**").unwrap_or(s);
        } else {
            s = s.strip_prefix('*').unwrap_or(s);
        }
        let rest = s.trim_start().strip_prefix(label)?;
        let text = rest.trim_start().strip_prefix(':')?.trim();
        (!text.is_empty()).then(|| text.to_owned())
    })
}

/// The lines of a text, without their breaks (`\n`, `\r\n` or a lone `\r`). Always at least one line.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut from = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                lines.push(&text[from..i]);
                i += 1;
                from = i;
            }
            b'\r' => {
                lines.push(&text[from..i]);
                i += if bytes.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                from = i;
            }
            _ => i += 1,
        }
    }
    lines.push(&text[from..]);
    lines
}

/// The marker on the def starting at `start`, or `None`: the def must own its line, have a JSDoc block directly above it, and the block
/// must carry a `<label>:` line.
pub(crate) fn marker_of(
    text: &str,
    start: u32,
    comments: &[Comment],
    label: &str,
) -> Option<String> {
    own_line(text, start)?;
    let block = doc_above(text, start, comments)?;
    read(block.slice(text)?, label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jsdoc(text: &str) -> Comment {
        let start = text.find("/**").unwrap() as u32;
        let end = text[start as usize..].find("*/").unwrap() as u32 + start + 2;
        Comment {
            span: Span::new(start, end),
            kind: CommentKind::Jsdoc,
        }
    }

    fn def_start(text: &str) -> u32 {
        text.find("function").unwrap() as u32
    }

    #[test]
    fn a_block_directly_above_is_the_defs() {
        let text = "/** doc */\nfunction f() {}\n";
        assert_eq!(
            doc_above(text, def_start(text), &[jsdoc(text)]),
            Some(jsdoc(text).span)
        );
        let same_line = "/** doc */ function f() {}\n";
        assert!(doc_above(same_line, def_start(same_line), &[jsdoc(same_line)]).is_some());
    }

    #[test]
    fn a_blank_line_detaches_the_block() {
        let text = "/** doc */\n\nfunction f() {}\n";
        assert_eq!(doc_above(text, def_start(text), &[jsdoc(text)]), None);
        let crlf = "/** doc */\r\n\r\nfunction f() {}\r\n";
        assert_eq!(doc_above(crlf, def_start(crlf), &[jsdoc(crlf)]), None);
        let spaces = "/** doc */\n   \nfunction f() {}\n";
        assert_eq!(
            doc_above(spaces, def_start(spaces), &[jsdoc(spaces)]),
            None,
            "a line of blanks is a blank line"
        );
    }

    #[test]
    fn another_comment_between_detaches_the_block() {
        let text = "/** doc */\n// note\nfunction f() {}\n";
        let line = Comment {
            span: Span::new(11, 18),
            kind: CommentKind::Line,
        };
        assert_eq!(doc_above(text, def_start(text), &[jsdoc(text), line]), None);
    }

    #[test]
    fn a_plain_block_is_not_a_jsdoc() {
        let text = "/* ID: x */\nfunction f() {}\n";
        let plain = Comment {
            span: Span::new(0, 11),
            kind: CommentKind::Block,
        };
        assert_eq!(doc_above(text, def_start(text), &[plain]), None);
        assert_eq!(doc_above(text, def_start(text), &[]), None);
    }

    #[test]
    fn read_finds_the_label_line() {
        assert_eq!(read("/** ID: abc */", "ID").as_deref(), Some("abc"));
        assert_eq!(
            read("/**\n * Parses.\n * ID: abc\n */", "ID").as_deref(),
            Some("abc")
        );
        assert_eq!(
            read("/**\n * Parses.\n * @returns x\n * ID: abc\n */", "ID").as_deref(),
            Some("abc"),
            "under a tag it is still read"
        );
        assert_eq!(
            read("/**\n  ID:abc  \n*/", "ID").as_deref(),
            Some("abc"),
            "loose spacing"
        );
        assert_eq!(
            read("/**\r\n * ID: abc\r\n */", "ID").as_deref(),
            Some("abc"),
            "CRLF"
        );
        assert_eq!(
            read("/**\n * ID : abc\n */", "ID").as_deref(),
            Some("abc"),
            "a space before the colon"
        );
        assert_eq!(
            read("/** ID: abc\n * more */", "ID").as_deref(),
            Some("abc"),
            "on the first line"
        );
        assert_eq!(
            read("/**\n * first\n * ID: one\n * ID: two\n */", "ID").as_deref(),
            Some("one"),
            "the first wins"
        );
    }

    #[test]
    fn read_is_not_fooled() {
        assert_eq!(read("/** ID: */", "ID"), None, "empty text");
        assert_eq!(read("/** IDENT: x */", "ID"), None, "longer label");
        assert_eq!(
            read("/** see ID: x */", "ID"),
            None,
            "not at the start of a line"
        );
        assert_eq!(read("/** note: x */", "ID"), None, "other label");
        assert_eq!(read("/**\n * Parses an ID: x\n */", "ID"), None);
        assert_eq!(
            read("/** Ref: k */", "Ref").as_deref(),
            Some("k"),
            "the label is the caller's"
        );
    }

    #[test]
    fn own_line_means_only_blanks_before_the_def() {
        assert_eq!(own_line("  function f() {}", 2), Some(0));
        assert_eq!(own_line("a;\n\tfunction f() {}", 4), Some(3));
        assert_eq!(own_line("a; function f() {}", 3), None);
        assert_eq!(
            own_line("/* c */ function f() {}", 8),
            None,
            "a comment before it on the line"
        );
        assert_eq!(
            own_line("\u{feff}function f() {}", 3),
            Some(3),
            "a byte-order mark is not code and stays first"
        );
    }
}
