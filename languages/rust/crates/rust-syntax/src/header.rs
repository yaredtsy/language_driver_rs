//! The file's own marker: a plain `// <label>: <text>` comment at the top of the file, in every language (a file has no docstring to put it
//! in). The rule is `docs/04-rust/04-default-resolver/03-rs2-markers.md` §6.
//!
//! Where it goes: after a `#!` line, and as the last line of the file's opening comment (a licence, in `//` lines or a `/* */` block), so that
//! the licence keeps its place on the first line, with an empty `//` line between a `//` run and the header; before inner attributes (`#![…]`) and inner doc comments (`//!`), and above the first item.
//! "Before the first item" is measured by tokens from the top of the file, which is the start of the item's range (its first doc comment or
//! attribute, else its first token): never from the start of the engine's node, which can begin with the comments above it.

use analyzer_core::edit::{Edit, MarkerStyle, Placement, apply, comment_marker};
use analyzer_core::text::Span;

use crate::sem::{self, Top};
use crate::{Edition, Skip};

/// The header takes no edition (RS2 §1): what it reads, comments and the first token, does not depend on one.
const EDITION: Edition = Edition::LATEST;

/// The marker written in the header: the first `// <label>: <text>` line among the plain comments before the first item, inner attribute or
/// inner doc comment.
pub(crate) fn read(text: &str, style: MarkerStyle<'_>) -> Option<String> {
    match style.placement {
        Placement::Docstring => read_top(text, &sem::top(text, EDITION), style),
        // a placement this analyzer does not build reads nothing
        _ => None,
    }
}

fn read_top(text: &str, top: &Top, style: MarkerStyle<'_>) -> Option<String> {
    top.comments
        .iter()
        // a `/* */` block is not a header, and a `//` that starts a line is the only kind `comment_marker` reads
        .filter(|c| is_line_comment(text, **c))
        .find_map(|c| comment_marker::read(text, c.start, "//", style).map(|found| found.text))
}

/// The edit that writes the header marker, or why not (RS2 §5, §6).
pub(crate) fn plan(text: &str, marker_text: &str, style: MarkerStyle<'_>) -> Result<Edit, Skip> {
    if !matches!(style.placement, Placement::Docstring) {
        return Err(Skip::PlacementNotSupported);
    }
    if !comment_marker::fits_a_line(marker_text) {
        return Err(Skip::InvalidText);
    }
    let top = sem::top(text, EDITION);
    if let Some(existing) = read_top(text, &top, style) {
        return Err(Skip::AlreadyMarked { existing });
    }
    let mut at = top.start;
    // the comment the header goes under, when there is one
    let mut above = None;
    // the header is the last line of the file's opening comment: the first run of comments, up to a blank line
    for (i, c) in top.comments.iter().enumerate() {
        // a block comment with code after it on its line: the header goes above that line
        let Some(next_line) = line_after(text, *c) else {
            break;
        };
        at = next_line;
        above = Some(*c);
        let below = top.comments.get(i + 1).map_or(top.first, |next| next.start);
        if !adjacent(text, c.end, below) {
            break;
        }
    }
    let eol = comment_marker::line_ending(text);
    let mut line = format!("// {}: {marker_text}{eol}", style.label);
    // Under a `//` line an empty `//` goes first, unless the line is one already: `rustfmt` with `wrap_comments` refills a `//` run as one
    // paragraph, and would join a header that is the run's last line to the text above it (RS2 §6).
    if above.is_some_and(|c| is_line_comment(text, c) && !is_empty_line_comment(text, c)) {
        line.insert_str(0, &format!("//{eol}"));
    }
    // a header directly above `//!` or `/*!` gets an empty line after it, so that nothing reads it as the start of the crate's docs
    let inner_doc_below = top.first_is_inner_doc
        && text
            .get(at as usize..top.first as usize)
            .is_some_and(|between| between.chars().all(|c| c == ' ' || c == '\t'));
    if inner_doc_below {
        line.push_str(eol);
    }
    // the last line has no break: put one first, so that the header is on a line of its own
    let end = text.len() as u32;
    if at == end && end > top.start && !text.ends_with(['\n', '\r']) {
        line.insert_str(0, eol);
    }
    let edit = Edit::insert(at, line);
    // An unterminated `/*` runs to the end of the text, and a header put after it would be inside it. So in a file with a parse error the
    // edit is tried first, and refused unless the marker reads back.
    if top.has_errors {
        let tried = apply(text, std::slice::from_ref(&edit)).ok_or(Skip::SyntaxError)?;
        if read(&tried, style).as_deref() != Some(marker_text) {
            return Err(Skip::SyntaxError);
        }
    }
    Ok(edit)
}

/// Is `comment` a `//` comment (not a `/* */` block)?
fn is_line_comment(text: &str, comment: Span) -> bool {
    text[comment.start as usize..].starts_with("//")
}

/// Is `comment`, a `//` comment, a bare `//`: nothing but blanks after the slashes (a `\r` of a CRLF file included)?
fn is_empty_line_comment(text: &str, comment: Span) -> bool {
    text[comment.start as usize + 2..comment.end as usize]
        .trim()
        .is_empty()
}

/// Only whitespace and at most one line break between `from` and `to`: no blank line separates them.
fn adjacent(text: &str, from: u32, to: u32) -> bool {
    text.get(from as usize..to as usize).is_some_and(|between| {
        between.chars().all(char::is_whitespace) && line_breaks(between) <= 1
    })
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
