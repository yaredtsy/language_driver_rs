//! Text edits, and the one-line comment marker every language's file header uses.

use crate::text::Span;

/// One replacement, in the coordinates of the text it was planned on.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Edit {
    pub at: Span,
    pub text: String,
}

impl Edit {
    pub fn replace(at: Span, text: impl Into<String>) -> Self {
        Edit {
            at,
            text: text.into(),
        }
    }

    pub fn insert(offset: u32, text: impl Into<String>) -> Self {
        Edit {
            at: Span::new(offset, offset),
            text: text.into(),
        }
    }
}

/// Apply edits planned on `text`, all at once. The edits may come in any order.
///
/// `None` when an edit is out of bounds, cuts a character in two, or overlaps another. Two insertions at one offset are
/// fine and keep the order they were given in.
pub fn apply(text: &str, edits: &[Edit]) -> Option<String> {
    let mut order: Vec<&Edit> = edits.iter().collect();
    // `sort_by_key` is stable, so equal offsets keep the caller's order.
    order.sort_by_key(|e| (e.at.start, e.at.end));

    let mut out = String::with_capacity(text.len());
    let mut done = 0u32;
    for e in order {
        if e.at.start < done {
            return None;
        }
        out.push_str(text.get(done as usize..e.at.start as usize)?);
        out.push_str(&e.text);
        // an empty edit at `done` must still be checked against the text's bounds
        text.get(e.at.start as usize..e.at.end as usize)?;
        done = e.at.end;
    }
    out.push_str(text.get(done as usize..)?);
    Some(out)
}

/// Where a marker goes. Only `Docstring` is built; a comment-above placement is a later option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Placement {
    Docstring,
}

/// How a marker is written. The label (`"ID"`) belongs to the caller, never to core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerStyle<'a> {
    pub label: &'a str,
    pub placement: Placement,
}

impl<'a> MarkerStyle<'a> {
    pub fn docstring(label: &'a str) -> Self {
        MarkerStyle {
            label,
            placement: Placement::Docstring,
        }
    }
}

/// A `<prefix> <label>: <text>` comment on a line of its own. Knows no language: the caller gives the comment
/// prefix and the offset.
pub mod comment_marker {
    use super::{Edit, MarkerStyle};
    use crate::text::Span;

    /// What [`read`] found.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[non_exhaustive]
    pub struct Found {
        /// The marker text, trimmed.
        pub text: String,
        /// The whole line, without its line break.
        pub line: Span,
    }

    /// The span of the line containing `at`, without its line break. `None` when `at` is out of bounds.
    fn line_at(text: &str, at: u32) -> Option<Span> {
        let at = at as usize;
        if !text.is_char_boundary(at) {
            return None;
        }
        let mut start = text[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1);
        // a byte-order mark is not part of the first line: it is not whitespace to `trim_start`, and a marker must go after it
        if start == 0 && text.starts_with('\u{feff}') && at >= '\u{feff}'.len_utf8() {
            start = '\u{feff}'.len_utf8();
        }
        let end = text[at..].find(['\n', '\r']).map_or(text.len(), |i| at + i);
        Some(Span::new(start as u32, end as u32))
    }

    /// The marker on the line containing `at`, if that line is `<prefix> <label>: <text>` (indentation allowed).
    /// An empty text after the colon is not a marker.
    pub fn read(text: &str, at: u32, prefix: &str, style: MarkerStyle<'_>) -> Option<Found> {
        let line = line_at(text, at)?;
        let rest = line.slice(text)?.trim_start().strip_prefix(prefix)?;
        let rest = rest.trim_start().strip_prefix(style.label)?;
        let marker = rest.trim_start().strip_prefix(':')?.trim();
        (!marker.is_empty()).then(|| Found {
            text: marker.to_owned(),
            line,
        })
    }

    /// The offset of the first byte of the line that `start` is on, past a byte-order mark. `None` when something other than blanks stands
    /// before `start` on that line: whatever starts at `start` does not own its line, and a marker line put above it would not be its own.
    pub fn own_line(text: &str, start: u32) -> Option<u32> {
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

    /// Can this text be written as a marker and read back the same? It is on one line, not empty, has no blanks around it (reading trims
    /// them away, so the caller would get back another text than it wrote), and does not close a block comment (`*/`), which a marker
    /// written into one would.
    pub fn fits_a_line(marker_text: &str) -> bool {
        !marker_text.is_empty()
            && marker_text == marker_text.trim()
            && !marker_text.contains(['\n', '\r'])
            && !marker_text.contains("*/")
    }

    /// Only whitespace and at most one line break between `from` and `to`: no blank line separates them. `false` when the range is not in
    /// the text.
    pub fn adjacent(text: &str, from: u32, to: u32) -> bool {
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

    /// The offset of the line after the one `comment` ends on, when nothing but blanks follows the comment on its line; the end of the text
    /// when there is no next line; `None` when code follows it.
    pub fn line_after(text: &str, comment: Span) -> Option<u32> {
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

    /// The line ending the file already uses: the first one found, `\n` when it has none.
    pub fn line_ending(text: &str) -> &'static str {
        match text.find(['\n', '\r']) {
            Some(i) if text[i..].starts_with("\r\n") => "\r\n",
            Some(i) if text[i..].starts_with('\r') => "\r",
            _ => "\n",
        }
    }

    /// The edit that inserts a marker line above the line containing `at`, with that line's indentation and the file's
    /// line ending. `None` when `at` is out of bounds or the text has a line break in it.
    pub fn plan(
        text: &str,
        at: u32,
        prefix: &str,
        style: MarkerStyle<'_>,
        marker_text: &str,
    ) -> Option<Edit> {
        if marker_text.contains(['\n', '\r']) {
            return None;
        }
        let line = line_at(text, at)?;
        let current = line.slice(text)?;
        let indent = &current[..current.len() - current.trim_start_matches([' ', '\t']).len()];
        let eol = line_ending(text);
        Some(Edit::insert(
            line.start,
            format!("{indent}{prefix} {}: {marker_text}{eol}", style.label),
        ))
    }
}

/// A marker in a `/** … */` block comment: JSDoc in TypeScript, a doc block in Rust. The text work is the same and knows no language: only
/// the block's own lines are read and written. Which blocks are the def's, and what to do with one that cannot be edited, is the caller's.
pub mod block_marker {
    use super::{Edit, comment_marker};
    use crate::text::Span;

    /// The lines of a text, without their breaks (`\n`, `\r\n` or a lone `\r`). Always at least one line.
    pub fn lines(text: &str) -> Vec<&str> {
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

    /// The marker in a block comment's text, `/** … */`: the first line that, after its leading `*` and any blanks, reads
    /// `<label>:<blanks><text>`. The text is returned as written, trimmed. A one-line block (`/** ID: x */`) is read the same way, and so is a
    /// marker under a tag. An empty text is no marker.
    pub fn read(block: &str, label: &str) -> Option<String> {
        let lines = lines(block);
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

    /// The edit that puts ` * <label>: <marker_text>` on a line of its own in a **multi-line** block (`block` is its span in `text`): before
    /// the first block tag when `block_tags` is true (a line starting with `@`, the way JSDoc has them; the marker ends the description),
    /// else before the closing line. The line has the block's own ` * ` prefix and indentation and the block's own line ending.
    ///
    /// With `blank_line` an empty ` *` line goes first, so that a Markdown renderer does not glue the marker to the sentence above it, unless
    /// the line above the marker is already empty (` *`, blanks only) or is a bare `/**`. TypeScript's JSDoc has no such need and passes
    /// `false`.
    ///
    /// `None` when the block cannot be edited safely, and nothing is guessed: the text does not fit a line (`comment_marker::fits_a_line`),
    /// the block is closed on its opening line (the caller may rewrite it, or not), its closing `*/` has other text on its line, a middle
    /// line has no `*`, or (with `block_tags`) a tag stands on the opening line.
    pub fn plan(
        text: &str,
        block: Span,
        label: &str,
        marker_text: &str,
        block_tags: bool,
        blank_line: bool,
    ) -> Option<Edit> {
        if !comment_marker::fits_a_line(marker_text) {
            return None;
        }
        let body = block.slice(text)?;
        let lines = line_spans(text, block);
        let last = lines.len() - 1;
        if last == 0 {
            return None;
        }
        let eol = comment_marker::line_ending(body);
        let at = |i: usize| lines[i].slice(text).unwrap_or("");
        if at(last).trim() != "*/" {
            return None;
        }
        let opening = at(0).trim_start().strip_prefix("/**").unwrap_or("").trim();
        if block_tags && opening.starts_with('@') {
            return None;
        }
        let mut star_indent: Option<&str> = None;
        let mut tag_line: Option<usize> = None;
        for i in 1..last {
            let line = at(i);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let after_star = trimmed.strip_prefix('*')?;
            star_indent.get_or_insert_with(|| leading_blanks(line));
            if block_tags && tag_line.is_none() && after_star.trim_start().starts_with('@') {
                tag_line = Some(i);
            }
        }
        let star_indent = star_indent.unwrap_or_else(|| leading_blanks(at(last)));
        let before = tag_line.unwrap_or(last);
        let insert_at = lines[before].start;
        // the line above the marker: empty after its star, or a bare `/**`
        let above = at(before - 1).trim();
        let paragraph_ends =
            above == "/**" || above.strip_prefix('*').unwrap_or(above).trim().is_empty();
        let separator = if blank_line && !paragraph_ends {
            format!("{star_indent}*{eol}")
        } else {
            String::new()
        };
        Some(Edit::insert(
            insert_at,
            format!("{separator}{star_indent}* {label}: {marker_text}{eol}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::comment_marker::{line_ending, plan, read};
    use super::*;

    const ID: MarkerStyle<'static> = MarkerStyle {
        label: "ID",
        placement: Placement::Docstring,
    };

    #[test]
    fn apply_in_any_order() {
        let edits = [
            Edit::replace(Span::new(6, 11), "there"),
            Edit::replace(Span::new(0, 5), "hi"),
        ];
        assert_eq!(apply("hello world", &edits).as_deref(), Some("hi there"));
    }

    #[test]
    fn apply_keeps_the_order_of_inserts_at_one_offset() {
        let edits = [Edit::insert(1, "x"), Edit::insert(1, "y")];
        assert_eq!(apply("ab", &edits).as_deref(), Some("axyb"));
    }

    #[test]
    fn apply_refuses_overlap_bounds_and_cut_characters() {
        assert_eq!(
            apply(
                "abcdef",
                &[
                    Edit::replace(Span::new(0, 3), ""),
                    Edit::replace(Span::new(2, 4), "")
                ]
            ),
            None
        );
        assert_eq!(apply("abc", &[Edit::insert(9, "x")]), None);
        assert_eq!(apply("abc", &[Edit::replace(Span::new(1, 9), "")]), None);
        assert_eq!(apply("é", &[Edit::insert(1, "x")]), None);
    }

    #[test]
    fn apply_nothing_is_identity() {
        assert_eq!(apply("same", &[]).as_deref(), Some("same"));
    }

    #[test]
    fn read_a_marker_line() {
        let text = "// ID: abc123\nfn f() {}\n";
        let found = read(text, 3, "//", ID).unwrap();
        assert_eq!(found.text, "abc123");
        assert_eq!(found.line, Span::new(0, 13));
    }

    #[test]
    fn read_allows_indentation_and_loose_spacing() {
        assert_eq!(read("  //ID:x \n", 0, "//", ID).unwrap().text, "x");
        assert_eq!(read("\t# ID :  y\n", 0, "#", ID).unwrap().text, "y");
    }

    #[test]
    fn read_is_not_fooled() {
        assert_eq!(read("// ID:\n", 0, "//", ID), None, "empty text");
        assert_eq!(read("// IDENT: x\n", 0, "//", ID), None, "longer label");
        assert_eq!(read("// note: x\n", 0, "//", ID), None, "other label");
        assert_eq!(read("# ID: x\n", 0, "//", ID), None, "other prefix");
        assert_eq!(
            read("x // ID: x\n", 0, "//", ID),
            None,
            "not on its own line"
        );
        assert_eq!(read("abc", 9, "//", ID), None, "out of bounds");
    }

    #[test]
    fn label_is_the_callers() {
        let style = MarkerStyle::docstring("Ref");
        assert_eq!(read("// Ref: k\n", 0, "//", style).unwrap().text, "k");
        assert_eq!(read("// ID: k\n", 0, "//", style), None);
    }

    #[test]
    fn plan_inserts_above_the_line_with_its_indentation() {
        let text = "mod m {\n\tfn f() {}\n}\n";
        let at = text.find("fn f").unwrap() as u32;
        let edit = plan(text, at, "//", ID, "k1").unwrap();
        let out = apply(text, &[edit]).unwrap();
        assert_eq!(out, "mod m {\n\t// ID: k1\n\tfn f() {}\n}\n");
        assert_eq!(read(&out, at, "//", ID).unwrap().text, "k1");
    }

    #[test]
    fn plan_copies_the_files_line_ending() {
        let text = "a\r\nb\r\n";
        let out = apply(text, &[plan(text, 0, "#", ID, "k").unwrap()]).unwrap();
        assert_eq!(out, "# ID: k\r\na\r\nb\r\n");
        assert_eq!(line_ending("a\rb"), "\r");
        assert_eq!(line_ending("no break"), "\n");
    }

    #[test]
    fn plan_on_an_empty_file_and_on_the_last_line() {
        assert_eq!(
            apply("", &[plan("", 0, "//", ID, "k").unwrap()]).as_deref(),
            Some("// ID: k\n")
        );
        let text = "a\nb";
        let out = apply(text, &[plan(text, 3, "//", ID, "k").unwrap()]).unwrap();
        assert_eq!(out, "a\n// ID: k\nb");
    }

    #[test]
    fn a_byte_order_mark_is_not_part_of_the_first_line() {
        let text = "\u{feff}// ID: k\nfn f() {}\n";
        assert_eq!(read(text, 4, "//", ID).unwrap().text, "k");
        assert_eq!(read(text, 4, "//", ID).unwrap().line, Span::new(3, 11));
        let bare = "\u{feff}fn f() {}\n";
        let out = apply(bare, &[plan(bare, 3, "//", ID, "k").unwrap()]).unwrap();
        assert_eq!(out, "\u{feff}// ID: k\nfn f() {}\n", "the mark stays first");
    }

    #[test]
    fn plan_refuses_a_multiline_marker_or_a_bad_offset() {
        assert_eq!(plan("x\n", 0, "//", ID, "a\nb"), None);
        assert_eq!(plan("x\n", 9, "//", ID, "k"), None);
    }

    #[test]
    fn plan_changes_nothing_but_the_inserted_line() {
        let text = "#!/usr/bin/env node\nconsole.log(1)\n";
        let at = text.find("console").unwrap() as u32;
        let edit = plan(text, at, "//", ID, "k").unwrap();
        let out = apply(text, std::slice::from_ref(&edit)).unwrap();
        let (a, b) = (
            edit.at.start as usize,
            edit.at.start as usize + edit.text.len(),
        );
        assert_eq!(format!("{}{}", &out[..a], &out[b..]), text);
    }

    // ---- block_marker and the helpers TypeScript and Rust share ----

    use super::block_marker;

    #[test]
    fn block_read_finds_the_label_line() {
        let read = |block| block_marker::read(block, "ID");
        assert_eq!(read("/** ID: abc */").as_deref(), Some("abc"));
        assert_eq!(
            read("/**\n * Parses.\n * ID: abc\n */").as_deref(),
            Some("abc")
        );
        assert_eq!(
            read("/**\n * Parses.\n * @returns x\n * ID: abc\n */").as_deref(),
            Some("abc"),
            "under a tag it is still read"
        );
        assert_eq!(
            read("/**\n  ID:abc  \n*/").as_deref(),
            Some("abc"),
            "loose spacing"
        );
        assert_eq!(
            read("/**\r\n * ID: abc\r\n */").as_deref(),
            Some("abc"),
            "CRLF"
        );
        assert_eq!(
            read("/**\n * ID : abc\n */").as_deref(),
            Some("abc"),
            "a space before the colon"
        );
        assert_eq!(
            read("/** ID: abc\n * more */").as_deref(),
            Some("abc"),
            "on the first line"
        );
        assert_eq!(
            read("/**\n * first\n * ID: one\n * ID: two\n */").as_deref(),
            Some("one"),
            "the first wins"
        );
        assert_eq!(
            read("/**\n *  ID: x\n */").as_deref(),
            Some("x"),
            "any blanks after the star"
        );
        assert_eq!(
            read("/**\r * ID: x\r */").as_deref(),
            Some("x"),
            "a lone carriage return is a line break"
        );
    }

    #[test]
    fn block_read_is_not_fooled() {
        let read = |block| block_marker::read(block, "ID");
        assert_eq!(read("/** ID: */"), None, "empty text");
        assert_eq!(read("/** IDENT: x */"), None, "longer label");
        assert_eq!(read("/** see ID: x */"), None, "not at the start of a line");
        assert_eq!(read("/** note: x */"), None, "other label");
        assert_eq!(read("/**\n * Parses an ID: x\n */"), None);
        assert_eq!(
            block_marker::read("/** Ref: k */", "Ref").as_deref(),
            Some("k"),
            "the label is the caller's"
        );
    }

    #[test]
    fn adjacent_means_blanks_and_at_most_one_line_break_between() {
        use super::comment_marker::adjacent;
        let text = "// a\n// b\n\n// c\r\n// d\r\n\r\n// e x";
        // end of `// a` to start of `// b`: one `\n`
        assert!(adjacent(text, 4, 5));
        // a blank line
        assert!(!adjacent(text, 9, 11));
        // `\r\n` is one line break, and `\r\n\r\n` are two
        assert!(adjacent(text, 15, 17));
        assert!(!adjacent(text, 21, 25));
        // nothing between them; a character that is not a blank; a range outside the text
        assert!(adjacent("ab", 1, 1));
        assert!(!adjacent("a;b", 1, 3));
        assert!(!adjacent("ab", 1, 9));
    }

    #[test]
    fn line_after_is_the_next_line_when_only_blanks_follow_on_this_one() {
        use super::comment_marker::line_after;
        let span = |start, end| Span::new(start, end);
        assert_eq!(line_after("/* c */\nfn f() {}", span(0, 7)), Some(8));
        assert_eq!(line_after("/* c */  \t\nfn f() {}", span(0, 7)), Some(11));
        assert_eq!(line_after("/* c */\r\nfn f() {}", span(0, 7)), Some(9));
        // no next line: the end of the text, with or without the break
        assert_eq!(line_after("/* c */", span(0, 7)), Some(7));
        assert_eq!(line_after("/* c */  ", span(0, 7)), Some(9));
        assert_eq!(line_after("/* c */\n", span(0, 7)), Some(8));
        // code on the line
        assert_eq!(line_after("/* c */ fn f() {}", span(0, 7)), None);
        assert_eq!(line_after("/* c */", span(0, 99)), None);
    }

    #[test]
    fn own_line_means_only_blanks_before_the_def() {
        use super::comment_marker::own_line;
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
        assert_eq!(own_line("abc", 9), None, "out of bounds");
    }

    #[test]
    fn a_marker_fits_a_line_when_it_reads_back_the_same() {
        use super::comment_marker::fits_a_line;
        for ok in ["k1", "a b", "a-b_c.9", "a/b"] {
            assert!(fits_a_line(ok), "{ok:?}");
        }
        for bad in ["", " a", "a ", "\t", "a\nb", "a\rb", "a*/b", "*/"] {
            assert!(!fits_a_line(bad), "{bad:?}");
        }
    }

    /// The block `text` holds, and the text after a marker `m1` is planned in it and applied.
    fn planned_in(text: &str, tags: bool) -> Option<String> {
        planned(text, tags, false)
    }

    /// As `planned_in`, with the empty line before the marker that Rust asks for.
    fn planned_with_blank_line(text: &str) -> Option<String> {
        planned(text, false, true)
    }

    fn planned(text: &str, tags: bool, blank_line: bool) -> Option<String> {
        let start = text.find("/**").unwrap() as u32;
        let end = text.find("*/").unwrap() as u32 + 2;
        let edit = block_marker::plan(text, Span::new(start, end), "ID", "m1", tags, blank_line)?;
        apply(text, &[edit])
    }

    #[test]
    fn a_marker_goes_before_the_closing_line_with_the_blocks_own_prefix() {
        assert_eq!(
            planned_in("/**\n * Parses.\n */\nfn f() {}\n", false).as_deref(),
            Some("/**\n * Parses.\n * ID: m1\n */\nfn f() {}\n")
        );
        assert_eq!(
            planned_in("    /**\n     * Parses.\n     */\n    fn f() {}\n", false).as_deref(),
            Some("    /**\n     * Parses.\n     * ID: m1\n     */\n    fn f() {}\n")
        );
        // an empty block takes the closing line's indentation
        assert_eq!(
            planned_in("  /**\n   */\n", false).as_deref(),
            Some("  /**\n   * ID: m1\n   */\n")
        );
        // the block's own line ending, not the marker's
        assert_eq!(
            planned_in("/**\r\n * a\r\n */\r\n", false).as_deref(),
            Some("/**\r\n * a\r\n * ID: m1\r\n */\r\n")
        );
        // blank lines inside are kept
        assert_eq!(
            planned_in("/**\n * a\n *\n */\n", false).as_deref(),
            Some("/**\n * a\n *\n * ID: m1\n */\n")
        );
    }

    #[test]
    fn the_empty_line_goes_before_the_marker_when_the_paragraph_is_not_over() {
        // text above: an empty ` *` line first, with the block's own prefix
        assert_eq!(
            planned_with_blank_line("/**\n * Parses.\n */\nfn f() {}\n").as_deref(),
            Some("/**\n * Parses.\n *\n * ID: m1\n */\nfn f() {}\n")
        );
        assert_eq!(
            planned_with_blank_line("    /**\n     * Parses.\n     */\n").as_deref(),
            Some("    /**\n     * Parses.\n     *\n     * ID: m1\n     */\n")
        );
        assert_eq!(
            planned_with_blank_line("/**\r\n * Parses.\r\n */\r\n").as_deref(),
            Some("/**\r\n * Parses.\r\n *\r\n * ID: m1\r\n */\r\n")
        );
        // text on the opening line is text above too
        assert_eq!(
            planned_with_blank_line("/** Parses.\n */\n").as_deref(),
            Some("/** Parses.\n *\n * ID: m1\n */\n")
        );
    }

    #[test]
    fn no_empty_line_when_the_line_above_is_empty_or_a_bare_opening() {
        // already ` *`, with blanks after the star too
        assert_eq!(
            planned_with_blank_line("/**\n * Parses.\n *\n */\n").as_deref(),
            Some("/**\n * Parses.\n *\n * ID: m1\n */\n")
        );
        assert_eq!(
            planned_with_blank_line("/**\n * Parses.\n *   \n */\n").as_deref(),
            Some("/**\n * Parses.\n *   \n * ID: m1\n */\n")
        );
        // a line with no star at all, blank
        assert_eq!(
            planned_with_blank_line("/**\n * Parses.\n\n */\n").as_deref(),
            Some("/**\n * Parses.\n\n * ID: m1\n */\n")
        );
        // a bare `/**` above: the block starts with the marker
        assert_eq!(
            planned_with_blank_line("/**\n */\n").as_deref(),
            Some("/**\n * ID: m1\n */\n")
        );
        assert_eq!(
            planned_with_blank_line("/**  \n */\n").as_deref(),
            Some("/**  \n * ID: m1\n */\n")
        );
    }

    #[test]
    fn the_empty_line_is_asked_for_and_never_the_default() {
        let text = "/**\n * Parses.\n */\n";
        assert_eq!(
            planned_in(text, false).as_deref(),
            Some("/**\n * Parses.\n * ID: m1\n */\n"),
            "TypeScript's shape"
        );
        assert_eq!(
            planned_with_blank_line(text).as_deref(),
            Some("/**\n * Parses.\n *\n * ID: m1\n */\n")
        );
        // with tags, the line above the marker is the one above the first tag
        let tagged = "/**\n * Doc.\n * @param x\n */\n";
        let edit = |blank| {
            let block = Span::new(0, tagged.find("*/").unwrap() as u32 + 2);
            let e = block_marker::plan(tagged, block, "ID", "m1", true, blank).unwrap();
            apply(tagged, &[e]).unwrap()
        };
        assert_eq!(edit(false), "/**\n * Doc.\n * ID: m1\n * @param x\n */\n");
        assert_eq!(
            edit(true),
            "/**\n * Doc.\n *\n * ID: m1\n * @param x\n */\n"
        );
    }

    #[test]
    fn with_block_tags_the_marker_ends_the_description() {
        let text = "/**\n * Doc.\n * @param x a value\n * @returns y\n */\n";
        assert_eq!(
            planned_in(text, true).as_deref(),
            Some("/**\n * Doc.\n * ID: m1\n * @param x a value\n * @returns y\n */\n")
        );
        // without tags (Rust), an `@` line is text like any other
        assert_eq!(
            planned_in(text, false).as_deref(),
            Some("/**\n * Doc.\n * @param x a value\n * @returns y\n * ID: m1\n */\n")
        );
        // a tag on the opening line: with tags the block is not edited
        assert_eq!(planned_in("/** @type {x}\n */\n", true), None);
        assert!(planned_in("/** @type {x}\n */\n", false).is_some());
    }

    #[test]
    fn a_block_that_cannot_be_edited_safely_gives_no_edit() {
        // closed on its opening line: the caller decides what to do with it
        assert_eq!(planned_in("/** a */\n", false), None);
        // text on the closing line
        assert_eq!(planned_in("/**\n * a\n * b */\n", false), None);
        // a middle line with no star
        assert_eq!(planned_in("/**\n * a\n b\n */\n", false), None);
        // a marker that does not fit a line, or would close the block
        let text = "/**\n * a\n */\n";
        let block = Span::new(0, text.find("*/").unwrap() as u32 + 2);
        for bad in ["", " x", "a\nb", "a*/b"] {
            assert_eq!(
                block_marker::plan(text, block, "ID", bad, false, true),
                None,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_planned_block_marker_is_read_back_and_nothing_else_changed() {
        let text = "/**\n * Parses.\n * @returns x\n */\n";
        let block = Span::new(0, text.len() as u32 - 1);
        let edit = block_marker::plan(text, block, "ID", "m1", true, false).unwrap();
        let out = apply(text, std::slice::from_ref(&edit)).unwrap();
        assert_eq!(block_marker::read(&out, "ID").as_deref(), Some("m1"));
        let (a, b) = (
            edit.at.start as usize,
            edit.at.start as usize + edit.text.len(),
        );
        assert_eq!(format!("{}{}", &out[..a], &out[b..]), text);
    }
}
