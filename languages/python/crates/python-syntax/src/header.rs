//! The file marker: a `# <label>: <text>` comment in the header of a module.
//!
//! Placement: after the leading run of comment lines, which holds a shebang (the OS needs it first), an encoding line (Python honours it
//! on line 1 or 2 only) and a licence block (checkers expect it at the top), and before the first statement, the module docstring
//! included. A comment before the docstring does not change `__doc__`.
//!
//! This works on lines of text and not on the syntax tree: the header is made of comments, which the parser throws away. The line work
//! is core's `comment_marker`, with the prefix `#`.

use analyzer_core::edit::{Edit, MarkerStyle, Placement, comment_marker};

use crate::edit::Skip;

/// `(offset, line)` for the lines before the first statement: only comments and blank lines.
fn header(text: &str) -> impl Iterator<Item = (u32, &str)> {
    text.split_inclusive('\n')
        .scan(0usize, |offset, line| {
            let at = *offset;
            *offset += line.len();
            Some((at as u32, line))
        })
        .take_while(|(_, l)| l.trim().is_empty() || l.trim_start().starts_with('#'))
}

/// The marker text of the first `# <label>: <text>` comment in the header, as written. Reading is tolerant: `#Label:` and extra spaces
/// are fine.
pub(crate) fn header_marker(text: &str, style: MarkerStyle<'_>) -> Option<String> {
    header(text).find_map(|(at, _)| comment_marker::read(text, at, "#", style).map(|f| f.text))
}

/// The offset of the line the marker goes on: the one after the leading run of comment lines. A shebang and an encoding line are
/// comment lines, so they stay above it and the marker never lands between them.
fn insertion_offset(text: &str) -> usize {
    text.split_inclusive('\n')
        .take_while(|l| l.trim_start().starts_with('#'))
        .map(str::len)
        .sum()
}

pub(crate) fn plan_header_marker(
    text: &str,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    match style.placement {
        Placement::Docstring => {}
        // `Placement` is non-exhaustive: a placement core adds later is refused until it is built here
        _ => return Err(Skip::PlacementNotSupported),
    }
    if marker_text.trim().is_empty() || marker_text.contains(['\n', '\r']) {
        return Err(Skip::InvalidText);
    }
    if let Some(existing) = header_marker(text, style) {
        return Err(Skip::AlreadyMarked { existing });
    }
    let offset = insertion_offset(text);
    // The header runs to the end of a text with no final line break: the new line needs a break in front of it and has none behind.
    // Core's `plan` writes above the line it is given, which here would be the last comment.
    if offset == text.len() && !text.is_empty() && !text.ends_with('\n') {
        let eol = comment_marker::line_ending(text);
        let line = format!("{eol}# {}: {marker_text}", style.label);
        return Ok(Edit::insert(offset as u32, line));
    }
    comment_marker::plan(text, offset as u32, "#", style, marker_text).ok_or(Skip::InvalidText)
}

#[cfg(test)]
mod tests {
    use super::*;
    use analyzer_core::edit::apply;

    const STYLE: MarkerStyle<'static> = MarkerStyle {
        label: "Key",
        placement: Placement::Docstring,
    };
    const ID: &str = "0Q8V5W4T2F7KX9M3B6D1RZ4H7K";

    /// The text after the marker is planned and applied; the edit is the only change.
    fn with_marker(text: &str) -> String {
        let edit = plan_header_marker(text, ID, STYLE).unwrap_or_else(|s| panic!("{s}"));
        apply(text, &[edit]).expect("the edit applies")
    }

    #[test]
    fn goes_before_the_docstring_and_after_the_header() {
        let before = "#!/usr/bin/env python3\n# -*- coding: utf-8 -*-\n# Copyright 2026 Acme.\n\"\"\"Orders module.\"\"\"\n\nimport json\n";
        let after = format!(
            "#!/usr/bin/env python3\n# -*- coding: utf-8 -*-\n# Copyright 2026 Acme.\n# Key: {ID}\n\"\"\"Orders module.\"\"\"\n\nimport json\n"
        );
        assert_eq!(with_marker(before), after);
        assert_eq!(header_marker(&after, STYLE).as_deref(), Some(ID));
    }

    #[test]
    fn an_empty_file_gets_it_as_the_first_line() {
        assert_eq!(with_marker(""), format!("# Key: {ID}\n"));
    }

    #[test]
    fn a_header_without_a_final_line_break_still_works() {
        assert_eq!(
            with_marker("# only a comment"),
            format!("# only a comment\n# Key: {ID}")
        );
    }

    #[test]
    fn a_header_that_ends_with_a_line_break_gets_the_line_after_it() {
        assert_eq!(
            with_marker("# only a comment\n"),
            format!("# only a comment\n# Key: {ID}\n")
        );
    }

    #[test]
    fn crlf_files_keep_crlf() {
        assert_eq!(
            with_marker("x = 1\r\n"),
            format!("# Key: {ID}\r\nx = 1\r\n")
        );
        assert_eq!(
            with_marker("# c\r\n# d"),
            format!("# c\r\n# d\r\n# Key: {ID}")
        );
    }

    #[test]
    fn a_blank_line_ends_the_run_of_comments() {
        // the run is the leading comment lines; the marker goes where the run ends, and the blank line stays after it
        assert_eq!(
            with_marker("# a\n\n# b\nx = 1\n"),
            format!("# a\n# Key: {ID}\n\n# b\nx = 1\n")
        );
        // reading looks further: the whole header, blank lines included
        let text = format!("# a\n\n# Key: {ID}\nx = 1\n");
        assert_eq!(header_marker(&text, STYLE).as_deref(), Some(ID));
    }

    #[test]
    fn a_file_that_has_one_is_left_alone() {
        let text = format!("#Key:   {ID}  \nx = 1\n");
        assert_eq!(header_marker(&text, STYLE).as_deref(), Some(ID));
        assert_eq!(
            plan_header_marker(&text, "other", STYLE),
            Err(Skip::AlreadyMarked {
                existing: ID.into()
            })
        );
    }

    #[test]
    fn a_comment_after_the_first_statement_is_not_the_marker() {
        assert_eq!(header_marker(&format!("x = 1\n# Key: {ID}\n"), STYLE), None);
    }

    #[test]
    fn another_label_is_not_this_marker() {
        assert_eq!(header_marker("# Other: x\nx = 1\n", STYLE), None);
    }

    #[test]
    fn text_with_a_line_break_or_nothing_is_refused() {
        for bad in ["", "  ", "a\nb", "a\rb"] {
            assert_eq!(
                plan_header_marker("x = 1\n", bad, STYLE),
                Err(Skip::InvalidText),
                "{bad:?}"
            );
        }
    }
}
