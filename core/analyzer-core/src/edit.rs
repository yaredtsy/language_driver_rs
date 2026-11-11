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
}
