//! Reading a marker: the `<label>: <text>` line of a def's own doc comments (`docs/04-rust/04-default-resolver/03-rs2-markers.md` §3).
//! Text in, text out: which comments are the def's is `sem`'s, and nothing here knows the engine.

/// The marker in the doc comments of one def, given in source order as written: the first line of them that reads `<label>: <text>`.
///
/// A `///` line is read after the `///` and any blanks, a `/** */` line after its leading `*` (if it has one) and any blanks; the first line of
/// a block after the `/**`. Then the line must be the label, optional blanks, `:`, and a text, which is returned trimmed. An empty text is no
/// marker. A `\r` before a line break is not part of the line. This is TypeScript's and core's `comment_marker` rule (RS2 §3): a marker that
/// goes unread makes `plan_marker` plan a second one.
pub(crate) fn read(docs: &[String], label: &str) -> Option<String> {
    docs.iter().find_map(|doc| {
        if doc.starts_with("/**") {
            read_block(doc, label)
        } else {
            read_line(doc.strip_prefix("///")?, label)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn docs(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| (*l).to_owned()).collect()
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
