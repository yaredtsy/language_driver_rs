//! The file's own marker: a `// <label>: <text>` comment at the top of the file, in every language (a file has no docstring to put it in).
//!
//! Where it goes: after a `#!` line, and as the last line of the file's opening comment (a licence, in `/* */`, `/** */` or `//` lines), so
//! that the licence keeps its place on the first line. It stays above `/// <reference …>` and `// @…` pragma lines, above a directive
//! that points at the next line, above the first def's own JSDoc, and above code.

use analyzer_core::edit::{Edit, MarkerStyle, comment_marker};
use analyzer_core::text::Span;

use crate::Skip;
use crate::marker;
use crate::outline;
use crate::sem::{Above, Comment, CommentKind, Parsed};

/// Where the first line after a `#!` line begins (past a byte-order mark when there is no shebang), and the comments from the top of the file up to
/// the first token.
fn top(p: &Parsed<'_>, text: &str) -> (u32, Vec<Comment>) {
    let shebang = p.shebang();
    let mut at = shebang.len();
    if shebang.is_empty() {
        if text.starts_with('\u{feff}') {
            at = '\u{feff}'.len_utf8();
        }
    } else {
        at += match text.get(at..) {
            Some(rest) if rest.starts_with("\r\n") => 2,
            Some(rest) if rest.starts_with(['\n', '\r']) => 1,
            _ => 0,
        };
    }
    // scan from the top, not from `at`: from a later offset the scanner takes the rest of that line for the previous token's own trailing
    // comments and skips it. From 0 it passes a shebang and a byte-order mark itself.
    (at as u32, p.comments_from(0))
}

/// The marker written in the header: the first `// <label>: <text>` line among the comments before the first token.
pub(crate) fn read(p: &Parsed<'_>, text: &str, style: MarkerStyle<'_>) -> Option<String> {
    let (_, comments) = top(p, text);
    comments
        .iter()
        .filter(|c| c.kind == CommentKind::Line)
        .find_map(|c| comment_marker::read(text, c.span.start, "//", style).map(|f| f.text))
}

/// The edit that writes the header marker, or why not.
pub(crate) fn plan(
    p: &Parsed<'_>,
    text: &str,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    if !marker::fits_a_line(marker_text) {
        return Err(Skip::InvalidText);
    }
    if let Some(existing) = read(p, text, style) {
        return Err(Skip::AlreadyMarked { existing });
    }
    let (mut at, comments) = top(p, text);
    let first_token = comments.last().map(|last| p.token_after(last.span.end));
    // the first def's own JSDoc, by the rule a def's marker is read with. The header goes before it, not between them. A JSDoc above
    // anything else (an `import`, a `"use strict"`) documents nothing: it is the file's
    let own_doc = first_token
        .filter(|&token| outline::starts_def(p, token))
        .and_then(|token| {
            let above = Above {
                start: token,
                comments: comments.clone(),
                opens_file: true,
            };
            marker::doc_above(text, &above)
        });
    for (i, c) in comments.iter().enumerate() {
        let written = c.span.slice(text).unwrap_or("");
        if Some(c.span) == own_doc || is_pragma(c, written) || marker::is_directive(written) {
            break;
        }
        // a block comment with code after it on its line: the header goes above that line
        let Some(next_line) = line_after(text, c.span) else {
            break;
        };
        at = next_line;
        // a blank line ends the file's opening comment: the header is its last line
        let below = comments
            .get(i + 1)
            .map(|next| next.span.start)
            .or(first_token)
            .unwrap_or(c.span.end);
        if !marker::adjacent(text, c.span.end, below) {
            break;
        }
    }
    let eol = comment_marker::line_ending(text);
    let line = format!("// {}: {marker_text}", style.label);
    let end = text.len() as u32;
    // the last line has no break: put one first, so that the header is on a line of its own
    if at == end && end > 0 && !text.ends_with(['\n', '\r']) {
        return Ok(Edit::insert(at, format!("{eol}{line}{eol}")));
    }
    Ok(Edit::insert(at, format!("{line}{eol}")))
}

/// A `//` line the compiler or a tool reads as an instruction for the file: `/// <reference …>`, `// @ts-check`, `// @flow`. The header
/// stays above these, as the plan says: they only need comments before them, and a comment is what we add.
fn is_pragma(comment: &Comment, written: &str) -> bool {
    comment.kind == CommentKind::Line
        && (written.starts_with("///") || written[2..].trim_start().starts_with('@'))
}

/// The offset of the line after the one `comment` ends on, when nothing but blanks follows the comment on its line; the end of the text when
/// there is no next line; `None` when code follows it.
fn line_after(text: &str, comment: Span) -> Option<u32> {
    let rest = text.get(comment.end as usize..)?;
    let line_end = rest.find(['\n', '\r']).unwrap_or(rest.len());
    if !rest[..line_end].chars().all(|c| c == ' ' || c == '\t') {
        return None;
    }
    let mut next = comment.end as usize + line_end;
    if text[next..].starts_with("\r\n") {
        next += 2;
    } else if next < text.len() {
        next += 1;
    }
    Some(next as u32)
}
