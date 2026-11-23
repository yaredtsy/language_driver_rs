//! Reading a marker, and planning where to put one: the `# <label>: <text>` line in the comment block directly above a declaration (03 §1.3, Q-BD1).
//! Bend has one comment form and no docstring, so the block above the declaration is the def's doc and its marker lives there. Text in, text out:
//! which comments are the def's is the parser's (`Decl::doc`, read in `sem`), and nothing here names the parser.

use analyzer_core::edit::{Edit, MarkerStyle, Placement, apply, comment_marker};
use analyzer_core::outline::Def;
use analyzer_core::text::Span;

use crate::{Skip, outline};

/// The marker in a def's doc block: the last `# <label>: <text>` line of it. `doc` is the block (first `#` to the end of its last line), `comments`
/// every comment of the file in source order. A line is read after the `#` and any blanks: the label, optional blanks, `:`, then the text, trimmed;
/// an empty text is no marker. A `#|` line (a test's expected output) is a comment like any other and is never a marker: after the `#` comes `|`,
/// not the label.
///
/// Several marker lines in one block are possible (a person wrote one, a tool another): the last wins. `plan` never writes a second, so this
/// only matters for text written by hand; the last is the one directly above the declaration, which is the one a tool wrote most recently.
pub(crate) fn read(
    text: &str,
    doc: Option<Span>,
    comments: &[Span],
    style: MarkerStyle<'_>,
) -> Option<String> {
    if !matches!(style.placement, Placement::Above) {
        // a placement this analyzer does not build reads nothing
        return None;
    }
    let doc = doc?;
    let first = comments.partition_point(|c| c.start < doc.start);
    comments[first..]
        .iter()
        .take_while(|c| c.end <= doc.end)
        .filter_map(|c| comment_marker::read(text, c.start, "#", style))
        .last()
        .map(|found| found.text)
}

/// The edit that puts `marker_text` on `def`, or why not. The def must be one of this text's outline: its offsets are used.
///
/// `comment_marker::plan` aimed at the declaration's first token (`@unsafe` when there is one, not the doc above it) puts the line immediately
/// above the declaration, below the block that is there, with the declaration's indentation (always none in Bend) and the file's line ending.
/// Nothing is written between the block and the marker: a `#` line has no Markdown to keep apart.
pub(crate) fn plan(
    text: &str,
    def: &Def,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    if !matches!(style.placement, Placement::Above) {
        return Err(Skip::PlacementNotSupported);
    }
    let (found, parsed) = outline::outline_with_parsed(text, style);
    // the def as this text has it: a def of an older text points somewhere else now
    let at = found
        .defs
        .iter()
        .position(|d| d.qualname == def.qualname && d.range == def.range)
        .ok_or(Skip::UnknownDef)?;
    let (def, decl) = (&found.defs[at], &parsed.decls[at]);

    // the order of the checks is Python's, TypeScript's and Rust's: what the caller can act on first, the shape of the text last
    if !comment_marker::fits_a_line(marker_text) {
        return Err(Skip::InvalidText);
    }
    if def.syntax_error {
        return Err(Skip::SyntaxError);
    }
    // a marked def answers with the marker even when its line is shared: the caller uses the fact
    if let Some(existing) = def.marker.clone() {
        return Err(Skip::AlreadyMarked { existing });
    }
    comment_marker::own_line(text, decl.span.start).ok_or(Skip::NotOnOwnLine)?;
    let edit = comment_marker::plan(text, decl.span.start, "#", style, marker_text)
        .ok_or(Skip::InvalidText)?;
    // In a file with a parse error the new line may not become the def's (a multi-line string left open by a broken def above would hold it as text).
    // So the edit is tried first, and refused unless the same defs come back and this one carries the marker.
    if !found.syntax_errors.is_empty() {
        let tried = apply(text, std::slice::from_ref(&edit)).ok_or(Skip::SyntaxError)?;
        let after = outline::outline(&tried, style);
        let same = after.defs.len() == found.defs.len()
            && after
                .defs
                .iter()
                .zip(&found.defs)
                .enumerate()
                .all(|(i, (a, b))| {
                    a.qualname == b.qualname
                        && if i == at {
                            a.marker.as_deref() == Some(marker_text)
                        } else {
                            a.marker == b.marker
                        }
                });
        if !same {
            return Err(Skip::SyntaxError);
        }
    }
    Ok(edit)
}
