//! Reading (and, in `plan`, placing) a marker in the JSDoc block above a def. Plain text work on one comment: no engine type is named here,
//! so that a language with the same `/** … */` shape can share it by the rule of two.
//!
//! The rule is `docs/03-typescript/04-ts2-markers.md` §3 and `languages/typescript/docs/markers.md`.

use analyzer_core::edit::{Edit, comment_marker};
use analyzer_core::text::Span;

use crate::Skip;
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

/// Only whitespace and at most one line break between `from` and `to`: no blank line separates them.
fn adjacent(text: &str, from: u32, to: u32) -> bool {
    text.get(from as usize..to as usize).is_some_and(|between| {
        between.chars().all(char::is_whitespace) && line_breaks(between) <= 1
    })
}

/// The JSDoc block that documents the def starting at `start`: the nearest `/** */` block above it, reached with **no blank line** on the
/// way (a blank line detaches it: the comment then belongs to the file). `//` and `/* */` comments between the block and the def do not
/// detach it: the engine attaches the block through them, and a second block written below them would hide the user's own.
pub(crate) fn doc_above(text: &str, start: u32, comments: &[Comment]) -> Option<Span> {
    let mut below = start;
    for c in comments.iter().rev() {
        if !adjacent(text, c.span.end, below) {
            return None;
        }
        if c.kind == CommentKind::Jsdoc {
            return Some(c.span);
        }
        below = c.span.start;
    }
    None
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

/// Can this text be written as a marker and read back the same? It is on one line, not empty, and has no blanks around it (reading
/// trims them away, so the caller would get back another text than it wrote).
pub(crate) fn fits_a_line(marker_text: &str) -> bool {
    !marker_text.is_empty()
        && marker_text == marker_text.trim()
        && !marker_text.contains(['\n', '\r'])
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

/// Does a block comment's text carry a block tag: a line that, after its leading `*` and spaces, starts with `@`? An inline `{@link …}` is
/// not one.
fn has_block_tag(block: &str) -> bool {
    split_lines(block).iter().enumerate().any(|(i, line)| {
        let s = line.trim();
        let s = if i == 0 {
            s.strip_prefix("/**").unwrap_or(s)
        } else {
            s.strip_prefix('*').unwrap_or(s)
        };
        s.trim_start().starts_with('@')
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

/// The marker on the def starting at `start`, or `None`: the def must own its line, have a JSDoc block above it (`doc_above`), and the
/// block must carry a `<label>:` line.
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

/// Comments that apply to the line below them: a block put between one of these and the def would move what it points at.
const DIRECTIVES: [&str; 10] = [
    "@ts-",
    "eslint",
    "prettier-ignore",
    "istanbul",
    "c8 ",
    "v8 ",
    "biome-ignore",
    "tslint:",
    "jshint",
    "jscs",
];

fn is_directive(comment: &str) -> bool {
    let body = comment
        .trim_start_matches("//")
        .trim_start_matches("/*")
        .trim_end_matches("*/")
        .trim();
    DIRECTIVES.iter().any(|d| body.starts_with(d))
}

/// The edit that puts `marker_text` on the def starting at `start`, or why not. `errors` are the file's parse errors.
pub(crate) fn plan(
    text: &str,
    start: u32,
    comments: &[Comment],
    def_has_error: bool,
    errors: &[Span],
    label: &str,
    marker_text: &str,
) -> Result<Edit, Skip> {
    let own = own_line(text, start).ok_or(Skip::NotOnOwnLine)?;
    // the marker goes inside a `/** */` block: it must also not close the comment
    if !fits_a_line(marker_text) || marker_text.contains("*/") {
        return Err(Skip::InvalidText);
    }
    let block = doc_above(text, start, comments);
    let touches = |b: Span| {
        errors
            .iter()
            .any(|e| e.start < b.end && e.end.max(e.start + 1) > b.start)
    };
    if def_has_error || block.is_some_and(touches) {
        return Err(Skip::SyntaxError);
    }
    if let Some(existing) = block
        .and_then(|b| b.slice(text))
        .and_then(|b| read(b, label))
    {
        return Err(Skip::AlreadyMarked { existing });
    }
    let eol = comment_marker::line_ending(text);
    match block {
        Some(block) => plan_in_block(text, block, label, marker_text),
        None => {
            // a blank line detached the nearest block, but the engine still applies it to the def. Its words may be the file's; its tags
            // (in a `.js` file, the types) are the def's, and a new block below it would switch them off
            let detached = comments.iter().rev().find(|c| c.kind == CommentKind::Jsdoc);
            if detached
                .and_then(|c| c.span.slice(text))
                .is_some_and(has_block_tag)
            {
                return Err(Skip::UnsafeDocComment);
            }
            // a new block goes directly above the def, below any plain comment: unless that comment points at the def
            if let Some(last) = comments.last()
                && adjacent(text, last.span.end, start)
                && last.span.slice(text).is_some_and(is_directive)
            {
                return Err(Skip::DirectiveAbove);
            }
            let indent = &text[own as usize..start as usize];
            Ok(Edit::insert(
                own,
                format!("{indent}/** {label}: {marker_text} */{eol}"),
            ))
        }
    }
}

/// Where a block's lines begin and end (without their breaks), as offsets into `text`.
fn line_spans(text: &str, block: Span) -> Vec<Span> {
    let body = block.slice(text).unwrap_or("");
    let mut spans = Vec::new();
    let mut from = 0usize;
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' | b'\r' => {
                spans.push(Span::new(block.start + from as u32, block.start + i as u32));
                i += if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                from = i;
            }
            _ => i += 1,
        }
    }
    spans.push(Span::new(block.start + from as u32, block.end));
    spans
}

fn leading_blanks(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// Put the marker in an existing JSDoc block. A multi-line block gets a ` * <label>: <text>` line before its first block tag, or before the
/// closing line when it has none. A one-line block is rewritten as several lines.
fn plan_in_block(text: &str, block: Span, label: &str, marker_text: &str) -> Result<Edit, Skip> {
    let body = block.slice(text).ok_or(Skip::UnsafeDocComment)?;
    let lines = line_spans(text, block);
    if lines.len() == 1 {
        return one_line(text, block, body, label, marker_text);
    }
    let eol = comment_marker::line_ending(body);
    let at = |i: usize| lines[i].slice(text).unwrap_or("");
    let last = lines.len() - 1;
    if at(last).trim() != "*/" {
        return Err(Skip::UnsafeDocComment);
    }
    let opening = at(0).trim_start().strip_prefix("/**").unwrap_or("").trim();
    if opening.starts_with('@') {
        return Err(Skip::UnsafeDocComment);
    }
    let mut star_indent: Option<&str> = None;
    let mut tag_line: Option<usize> = None;
    for i in 1..last {
        let line = at(i);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(after_star) = trimmed.strip_prefix('*') else {
            return Err(Skip::UnsafeDocComment);
        };
        star_indent.get_or_insert_with(|| leading_blanks(line));
        if tag_line.is_none() && after_star.trim_start().starts_with('@') {
            tag_line = Some(i);
        }
    }
    let star_indent = star_indent.unwrap_or_else(|| leading_blanks(at(last)));
    let insert_at = lines[tag_line.unwrap_or(last)].start;
    Ok(Edit::insert(
        insert_at,
        format!("{star_indent}* {label}: {marker_text}{eol}"),
    ))
}

fn one_line(
    text: &str,
    block: Span,
    body: &str,
    label: &str,
    marker_text: &str,
) -> Result<Edit, Skip> {
    let line_start = own_line(text, block.start).ok_or(Skip::NotOnOwnLine)?;
    let indent = &text[line_start as usize..block.start as usize];
    let eol = comment_marker::line_ending(text);
    let content = body
        .strip_prefix("/**")
        .and_then(|b| b.strip_suffix("*/"))
        .ok_or(Skip::UnsafeDocComment)?
        .trim();
    let marker_line = format!("{indent} * {label}: {marker_text}");
    let mut lines = vec!["/**".to_owned()];
    if content.is_empty() {
        lines.push(marker_line);
    } else if content.starts_with('@') {
        // the marker ends the description: it goes before a block tag
        lines.push(marker_line);
        lines.push(format!("{indent} * {content}"));
    } else {
        lines.push(format!("{indent} * {content}"));
        lines.push(marker_line);
    }
    lines.push(format!("{indent} */"));
    Ok(Edit::replace(block, lines.join(eol)))
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
    fn another_comment_between_does_not_detach_the_block() {
        let text = "/** doc */\n// note\nfunction f() {}\n";
        let line = Comment {
            span: Span::new(11, 18),
            kind: CommentKind::Line,
        };
        assert_eq!(
            doc_above(text, def_start(text), &[jsdoc(text), line]),
            Some(jsdoc(text).span)
        );
        // a blank line anywhere on the way does
        for text in [
            "/** doc */\n\n// note\nfunction f() {}\n",
            "/** doc */\n// note\n\nfunction f() {}\n",
        ] {
            let at = text.find("//").unwrap() as u32;
            let line = Comment {
                span: Span::new(at, at + 7),
                kind: CommentKind::Line,
            };
            assert_eq!(
                doc_above(text, def_start(text), &[jsdoc(text), line]),
                None,
                "{text:?}"
            );
        }
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
    fn a_block_tag_starts_a_line() {
        assert!(has_block_tag("/** @type {number} */"));
        assert!(has_block_tag("/**\n * Doc.\n * @param x a value\n */"));
        assert!(has_block_tag("/**\n @returns x\n*/"), "no star");
        assert!(!has_block_tag("/** Doc. */"));
        assert!(!has_block_tag(
            "/**\n * See {@link f}.\n * mail me@example.com\n */"
        ));
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
