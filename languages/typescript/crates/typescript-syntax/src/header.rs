//! The file's own marker: a `// <label>: <text>` comment at the top of the file, in every language (a file has no docstring to put it in).
//!
//! Where it goes, in order: after a `#!` line; after the leading block comments that are not the first def's own JSDoc (a licence); before
//! any `//` comment (`/// <reference …>`, `// @ts-check`), directive or code, because those only need to be preceded by comments, and a comment
//! is what we add.

use analyzer_core::edit::{Edit, MarkerStyle, comment_marker};
use analyzer_core::text::Span;

use crate::Skip;
use crate::marker;
use crate::sem::{Comment, CommentKind, Parsed};

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
    // the first def's own JSDoc, by the rule a def's marker is read with. The header goes before it, not between them
    let own_doc = comments
        .last()
        .map(|last| p.token_after(last.span.end))
        .and_then(|token| marker::doc_above(text, token, &comments));
    for c in &comments {
        if c.kind == CommentKind::Line || Some(c.span) == own_doc {
            break;
        }
        // a block comment with code after it on its line: the header goes above that line
        let Some(next_line) = line_after(text, c.span) else {
            break;
        };
        at = next_line;
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
