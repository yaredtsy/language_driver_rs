//! Reading a marker, and planning where to put one: the `<label>: <text>` line of a def's own doc comments
//! (`docs/04-rust/04-default-resolver/03-rs2-markers.md` §3 and §4). Text in, text out: which comments are the def's is `sem`'s, and nothing
//! here knows the engine.

use analyzer_core::edit::{Edit, MarkerStyle, Placement, apply, comment_marker};
use analyzer_core::outline::Def;
use analyzer_core::text::Span;

use crate::sem::{DocComment, Item};
use crate::{Edition, Skip, outline};

/// The marker in the doc comments of one def, given in source order as written: the first line of them that reads `<label>: <text>`.
///
/// A `///` line is read after the `///` and any blanks, a `/** */` line after its leading `*` (if it has one) and any blanks; the first line of
/// a block after the `/**`. Then the line must be the label, optional blanks, `:`, and a text, which is returned trimmed. An empty text is no
/// marker. A `\r` before a line break is not part of the line. This is TypeScript's and core's `comment_marker` rule (RS2 §3): a marker that
/// goes unread makes `plan_marker` plan a second one.
pub(crate) fn read(docs: &[DocComment], label: &str) -> Option<String> {
    docs.iter().find_map(|doc| {
        if doc.text.starts_with("/**") {
            read_block(&doc.text, label)
        } else {
            read_line(doc.text.strip_prefix("///")?, label)
        }
    })
}

/// A `/** */` block: its lines, each without its indentation and its leading `*`.
fn read_block(block: &str, label: &str) -> Option<String> {
    let lines: Vec<&str> = block.lines().collect();
    let last = lines.len().checked_sub(1)?;
    lines.iter().enumerate().find_map(|(i, line)| {
        let mut line = line.trim_start();
        if i == 0 {
            line = line.strip_prefix("/**")?;
        } else {
            line = line.strip_prefix('*').unwrap_or(line);
        }
        if i == last {
            line = line.trim_end().strip_suffix("*/").unwrap_or(line);
        }
        read_line(line, label)
    })
}

/// What follows the comment's own marker: any blanks, then `<label>`, optional blanks, `:`, the text.
fn read_line(after_marker: &str, label: &str) -> Option<String> {
    let line = after_marker.trim_end_matches('\r');
    let rest = line.trim_start().strip_prefix(label)?;
    let text = rest.trim_start().strip_prefix(':')?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The offset of the first byte of the line that `start` is on, past a byte-order mark. `None` when something other than blanks stands
/// before `start` on that line: the def does not own its line.
fn own_line(text: &str, start: u32) -> Option<u32> {
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

/// The offset of the first byte of the line after the one `from` is on. `None` when the text ends on that line.
fn next_line_start(text: &str, from: u32) -> Option<u32> {
    let rest = text.get(from as usize..)?;
    let at = rest.find(['\n', '\r'])?;
    let eol = if rest[at..].starts_with("\r\n") { 2 } else { 1 };
    Some(from + (at + eol) as u32)
}

/// Can this text be written as a marker and read back the same? It is on one line, not empty, has no blanks around it (reading trims them
/// away, so the caller would get back another text than it wrote), and does not close a block comment.
fn fits_a_line(marker_text: &str) -> bool {
    !marker_text.is_empty()
        && marker_text == marker_text.trim()
        && !marker_text.contains(['\n', '\r'])
        && !marker_text.contains("*/")
}

/// The edit that puts `marker_text` on `def`, or why not (RS2 §4, §5). The def must be one of this text's outline: its offsets are used.
pub(crate) fn plan(
    text: &str,
    edition: Edition,
    def: &Def,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    if !matches!(style.placement, Placement::Docstring) {
        return Err(Skip::PlacementNotSupported);
    }
    let (found, items) = outline::outline_with_items(text, edition, style);
    // the def as this text has it: a def of an older text points somewhere else now, and the checks below read the fresh one
    let at = found
        .defs
        .iter()
        .position(|d| d.qualname == def.qualname && d.range == def.range)
        .ok_or(Skip::UnknownDef)?;
    let (def, item) = (&found.defs[at], &items[at]);

    // the order of the checks is Python's and TypeScript's (RS2 §5): what the caller can act on first, the shape of the text last
    if !fits_a_line(marker_text) {
        return Err(Skip::InvalidText);
    }
    if def.syntax_error {
        return Err(Skip::SyntaxError);
    }
    // a marked def answers with the marker even when its line is shared: the caller uses the fact
    if let Some(existing) = def.marker.clone() {
        return Err(Skip::AlreadyMarked { existing });
    }
    let own = own_line(text, def.range.start).ok_or(Skip::NotOnOwnLine)?;
    let edit = place(text, item, own, style.label, marker_text)?;
    // In a file with a parse error, the new doc line may not become the def's: an unfinished item above it can take it (`struct` with no
    // name, then the def). So the edit is tried first, and refused unless the same defs come back and this one carries the marker. A file
    // without errors needs no trial: the round trip in the tests covers it.
    if !found.syntax_errors.is_empty() {
        let tried = apply(text, std::slice::from_ref(&edit)).ok_or(Skip::SyntaxError)?;
        let after = outline::outline(&tried, edition, style);
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

/// Where the marker line goes, by the item's last doc comment (RS2 §4 rows 1, 2, 4, 5, 6). `own` is the start of the item's first line.
fn place(text: &str, item: &Item, own: u32, label: &str, marker_text: &str) -> Result<Edit, Skip> {
    // the item's indentation, and the file's line ending, are copied
    let indent = &text[own as usize..item.range.start as usize];
    let eol = comment_marker::line_ending(text);
    let line = format!("{indent}/// {label}: {marker_text}{eol}");
    match last_fragment(item) {
        // no fragment at all (row 6): a new line above the first attribute or the keyword
        Fragment::None => Ok(Edit::insert(own, line)),
        Fragment::Comment(comment) => {
            // a block as the last doc comment is step 3's: its lines cannot be edited yet, and a `///` line put below it would be a second
            // doc comment of another shape beside it
            if comment.text.starts_with("/**") {
                return Err(Skip::UnsafeDocComment);
            }
            let empty = comment
                .text
                .strip_prefix("///")
                .is_some_and(|t| t.trim().is_empty());
            after_line_of(text, comment.range.end, indent, eol, !empty, line)
        }
        // an attribute is the last fragment: it can share its line with code, and a line put after that one would be inside the item
        Fragment::Attr(attr) => {
            let rest = text.get(attr.end as usize..).unwrap_or("");
            let rest = rest.split(['\n', '\r']).next().unwrap_or("").trim();
            if !rest.is_empty() && !rest.starts_with("//") {
                return Err(Skip::NotOnOwnLine);
            }
            after_line_of(text, attr.end, indent, eol, true, line)
        }
    }
}

/// The last doc fragment of an item in source order (RS2 §4): the last of its doc comments and of its `#[doc = …]` attributes.
enum Fragment<'a> {
    None,
    Comment(&'a DocComment),
    Attr(Span),
}

fn last_fragment(item: &Item) -> Fragment<'_> {
    match (item.docs.last(), item.doc_attrs.last()) {
        (None, None) => Fragment::None,
        (Some(comment), None) => Fragment::Comment(comment),
        (None, Some(attr)) => Fragment::Attr(*attr),
        (Some(comment), Some(attr)) if attr.end > comment.range.end => Fragment::Attr(*attr),
        (Some(comment), Some(_)) => Fragment::Comment(comment),
    }
}

/// The marker line on the line after the one `end` is on, with an empty `///` line before it when `separate`: after a line of text the
/// marker would join its sentence in Markdown (RS2 §4 rows 1 and 2).
fn after_line_of(
    text: &str,
    end: u32,
    indent: &str,
    eol: &str,
    separate: bool,
    line: String,
) -> Result<Edit, Skip> {
    let after = next_line_start(text, end).ok_or(Skip::SyntaxError)?;
    let separator = if separate {
        format!("{indent}///{eol}")
    } else {
        String::new()
    };
    Ok(Edit::insert(after, format!("{separator}{line}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs(lines: &[&str]) -> Vec<DocComment> {
        lines
            .iter()
            .map(|l| DocComment {
                text: (*l).to_owned(),
                range: analyzer_core::text::Span::new(0, 0),
            })
            .collect()
    }

    #[test]
    fn a_line_is_read_after_any_blanks() {
        for (line, want) in [
            ("/// ID: x", Some("x")),
            ("///ID: x", Some("x")),
            ("/// ID:x", Some("x")),
            ("/// ID : x", Some("x")),
            ("/// ID:   x y  ", Some("x y")),
            ("/// ID: x\r", Some("x")),
            ("///  ID: x", Some("x")),
            ("///     ID: x", Some("x")),
            ("///\tID: x", Some("x")),
            ("/// ID:", None),
            ("/// ID:   ", None),
            ("/// id: x", None),
            ("/// IDX: x", None),
            ("/// see ID: x", None),
            ("//// ID: x", None),
        ] {
            assert_eq!(read(&docs(&[line]), "ID").as_deref(), want, "{line:?}");
        }
    }

    #[test]
    fn a_block_is_read_by_its_lines() {
        for (block, want) in [
            ("/** ID: x */", Some("x")),
            ("/**ID: x*/", Some("x")),
            ("/**\n * a\n * ID: x\n */", Some("x")),
            ("/**\n * a\n * ID: x\n */\r", Some("x")),
            ("/**\r\n * a\r\n * ID: x\r\n */", Some("x")),
            ("/**\n    * ID: x\n    */", Some("x")),
            ("/**\n ID: x\n */", Some("x")),
            ("/** a\n * ID: x */", Some("x")),
            ("/**\n *  ID: x\n */", Some("x")),
            ("/**   ID: x */", Some("x")),
            ("/** ID: */", None),
            ("/** a */", None),
        ] {
            assert_eq!(read(&docs(&[block]), "ID").as_deref(), want, "{block:?}");
        }
    }

    #[test]
    fn the_first_marker_of_the_first_comment_that_has_one_wins() {
        let comments = docs(&["/// a", "/// ID: one", "/** ID: two */", "/// ID: three"]);
        assert_eq!(read(&comments, "ID").as_deref(), Some("one"));
        assert_eq!(read(&[], "ID"), None);
    }

    #[test]
    fn the_label_is_the_callers() {
        let comments = docs(&["/// NOTE: x", "/// ID: y"]);
        assert_eq!(read(&comments, "NOTE").as_deref(), Some("x"));
        assert_eq!(read(&comments, "ID").as_deref(), Some("y"));
    }
}
