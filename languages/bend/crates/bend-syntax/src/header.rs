//! The file's own marker: a plain `# <label>: <text>` comment at the top of the file, before the imports, in every language a plain comment (a file has
//! no docstring to put it in). Bend has one comment form, so it looks like a def's marker: what tells them apart is where the line is. The rule is 03 §1.3.
//!
//! Where it is read: among the comments before the first `import` line, or, in a file with none, before the first declaration's range (its doc block
//! included): that block is the declaration's. `book_load` skips blank and `#` lines before the first `import`, so a header there is legal Bend.
//! Where it is written: at offset 0, past a byte-order mark.

use analyzer_core::edit::{Edit, MarkerStyle, Placement, apply, comment_marker};

use crate::Skip;
use crate::sem::{self, Parsed};

/// The offset where a header marker is written: the start of the file, past a byte-order mark. 03 §1.3 left a shebang-like first comment as an option;
/// the corpus has none (every file starts with a comment, an import or a declaration), so the slot is the same for every file.
pub(crate) fn slot(text: &str) -> u32 {
    if text.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8() as u32
    } else {
        0
    }
}

/// Where the header region ends: the first import, else the first declaration's range, else the end of the text.
fn region_end(text: &str, parsed: &Parsed) -> u32 {
    let first_decl = parsed.decls.first().map(|d| {
        d.doc
            .map_or(d.span.start, |doc| doc.start.min(d.span.start))
    });
    match (parsed.first_import, first_decl) {
        (Some(i), Some(d)) => i.min(d),
        (Some(at), None) | (None, Some(at)) => at,
        (None, None) => text.len() as u32,
    }
}

/// The marker written in the header: the first `# <label>: <text>` line among the comments before the first import (or the first declaration's range).
pub(crate) fn read(text: &str, parsed: &Parsed, style: MarkerStyle<'_>) -> Option<String> {
    if !matches!(style.placement, Placement::Above) {
        return None;
    }
    let end = region_end(text, parsed);
    parsed
        .comments
        .iter()
        .take_while(|c| c.start < end)
        .find_map(|c| comment_marker::read(text, c.start, "#", style).map(|found| found.text))
}

/// The edit that writes the header marker, or why not: a line at offset 0, with the file's line ending.
///
/// When the text at offset 0 starts the first declaration (its doc block or its first token: 127 corpus files start with code), a line put directly
/// above it would be that def's marker and not the file's. So an empty line follows the marker, and the two stay apart.
pub(crate) fn plan(text: &str, marker_text: &str, style: MarkerStyle<'_>) -> Result<Edit, Skip> {
    if !matches!(style.placement, Placement::Above) {
        return Err(Skip::PlacementNotSupported);
    }
    if !comment_marker::fits_a_line(marker_text) {
        return Err(Skip::InvalidText);
    }
    let parsed = sem::parse(text);
    if let Some(existing) = read(text, &parsed, style) {
        return Err(Skip::AlreadyMarked { existing });
    }
    let at = slot(text);
    let planned =
        comment_marker::plan(text, at, "#", style, marker_text).ok_or(Skip::InvalidText)?;
    let first_decl_line = parsed.decls.first().and_then(|d| {
        comment_marker::own_line(
            text,
            d.doc
                .map_or(d.span.start, |doc| doc.start.min(d.span.start)),
        )
    });
    let mut line = planned.text;
    if first_decl_line == Some(at) {
        line.push_str(comment_marker::line_ending(text));
    }
    let edit = Edit::insert(at, line);
    // an open multi-line string cannot start above offset 0, so only a text the parser could not read at all is tried: the marker must read back
    if !parsed.errors.is_empty() {
        let tried = apply(text, std::slice::from_ref(&edit)).ok_or(Skip::SyntaxError)?;
        if read(&tried, &sem::parse(&tried), style).as_deref() != Some(marker_text) {
            return Err(Skip::SyntaxError);
        }
    }
    Ok(edit)
}
