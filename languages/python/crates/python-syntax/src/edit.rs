//! Writing a marker: where `ID: …` goes in a docstring, and the one insertion that puts it there.
//!
//! Ported from pylspt's `edit.rs` (`670ae79a15`). The placement rules are unchanged: extend an existing docstring, or add a
//! `"""ID: …"""` statement. What changed is the inputs: the label comes from `MarkerStyle`, the marker text from the caller, and the
//! docstring's position from `sem`, so nothing here touches the parser.

use std::fmt;

use analyzer_core::edit::{Edit, MarkerStyle, Placement, comment_marker};
use analyzer_core::outline::Def;

use crate::ids;
use crate::sem::{self, RawDef, Slot, SlotKind};

/// Why no edit was made. Never a panic, never a half edit: the caller is told and the file is left alone.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Skip {
    /// A parse error touches the def, so its lines are not known to mean what they look like.
    SyntaxError,
    /// The def already has a marker. `existing` is its text.
    AlreadyMarked { existing: String },
    /// The body has no statement to put the marker before, or its docstring never closes. Nothing is stacked on top of it.
    UnsafeDocComment,
    /// The style asks for a placement that is not built.
    PlacementNotSupported,
    /// The marker text could not be read back: it is empty, or has whitespace, a colon, a quote, a backslash or a control
    /// character. Writing it could also end the docstring early.
    BadMarkerText,
    /// No def in this text has that range: the text is not the one the def came from.
    DefNotFound,
}

impl fmt::Display for Skip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Skip::SyntaxError => f.write_str("syntax error inside this def"),
            Skip::AlreadyMarked { existing } => write!(f, "already has a marker ({existing})"),
            Skip::UnsafeDocComment => f.write_str("no safe place for a marker in this body"),
            Skip::PlacementNotSupported => f.write_str("this marker placement is not supported"),
            Skip::BadMarkerText => {
                f.write_str("the marker text cannot be written into a docstring")
            }
            Skip::DefNotFound => f.write_str("the text has no def at this range"),
        }
    }
}

/// The whitespace between `at`'s line start and `at`: the indentation to reuse byte for byte, tabs and all. `None` when `at` is not
/// the first thing on its line (a one-liner body, or `at` in the middle of a docstring line).
fn line_prefix(text: &str, at: u32) -> Option<&str> {
    let at = at as usize;
    let start = text[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1);
    let prefix = &text[start..at];
    prefix
        .bytes()
        .all(|b| b == b' ' || b == b'\t')
        .then_some(prefix)
}

/// A one-liner has no `body[0]` indentation to copy, so make one: the def's own indentation, one unit deeper.
fn one_liner_indent(text: &str, def_start: u32) -> String {
    format!("{}    ", line_prefix(text, def_start).unwrap_or(""))
}

fn readable(marker_text: &str) -> bool {
    !marker_text.is_empty()
        && !marker_text
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, ':' | '"' | '\'' | '\\'))
}

/// The insertion for `slot`, placed by the rules of the old `id_edit`.
fn insertion(text: &str, def: &RawDef, slot: Slot, line: &str) -> Edit {
    let nl = comment_marker::line_ending(text);
    let indent = line_prefix(text, slot.stmt_start);
    let (offset, new) = match (slot.kind, indent) {
        // a docstring exists: extend it in place
        (
            SlotKind::InLiteral {
                triple_quoted: true,
            },
            Some(ind),
        ) => match line_prefix(text, slot.at) {
            // the closer starts its own line: insert BEFORE its indent, or that indent is left stranded on a line of its own
            Some(before) => (
                slot.at - before.len() as u32,
                format!("{nl}{ind}{line}{nl}"),
            ),
            // the closer follows text, as in `"""Summary."""`
            None => (slot.at, format!("{nl}{nl}{ind}{line}{nl}{ind}")),
        },
        // a newline would not parse
        (
            SlotKind::InLiteral {
                triple_quoted: false,
            },
            _,
        ) => (slot.at, format!(" {line}")),
        // `def f(): """doc"""`
        (
            SlotKind::InLiteral {
                triple_quoted: true,
            },
            None,
        ) => (slot.at, format!(" {line}")),

        // none exists: a whole new statement before body[0]
        (SlotKind::NewStatement, Some(ind)) => (slot.at, format!("\"\"\"{line}\"\"\"{nl}{ind}")),
        (SlotKind::NewStatement, None) => {
            let ind = one_liner_indent(text, def.range.start);
            (slot.at, format!("{nl}{ind}\"\"\"{line}\"\"\"{nl}{ind}"))
        }
    };
    Edit::insert(offset, new)
}

/// The marker on the def at `def.range` in `text`, read from the text now and not from `def`.
pub(crate) fn marker(text: &str, def: &Def, style: MarkerStyle<'_>) -> Option<String> {
    let module = sem::parse(text);
    let raw = module.defs.iter().find(|d| d.range == def.range)?;
    ids::read(raw.docstring.as_deref()?, style.label)
}

/// The edit that puts `marker_text` on `def`, or why it cannot be done. The edit is planned on `text` and not applied.
pub(crate) fn plan_marker(
    text: &str,
    def: &Def,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    match style.placement {
        Placement::Docstring => {}
        // `Placement` is non-exhaustive: a placement core adds later is refused until it is built here
        _ => return Err(Skip::PlacementNotSupported),
    }
    if !readable(marker_text) {
        return Err(Skip::BadMarkerText);
    }
    let module = sem::parse(text);
    let raw = module
        .defs
        .iter()
        .find(|d| d.range == def.range)
        .ok_or(Skip::DefNotFound)?;
    if raw.syntax_error {
        return Err(Skip::SyntaxError);
    }
    if let Some(existing) = raw
        .docstring
        .as_deref()
        .and_then(|doc| ids::read(doc, style.label))
    {
        return Err(Skip::AlreadyMarked { existing });
    }
    let slot = raw.slot.ok_or(Skip::UnsafeDocComment)?;
    let line = format!("{}: {marker_text}", style.label);
    Ok(insertion(text, raw, slot, &line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::outline;
    use analyzer_core::edit::apply;

    const ID: MarkerStyle<'static> = MarkerStyle {
        label: "ID",
        placement: Placement::Docstring,
    };

    /// Mark every unmarked def with a counter, all planned on the original text, apply, and return the result: reproducible, unlike a
    /// real generator. These are pylspt's nine `edit.rs` tests, with the same sources and the same expected texts.
    fn inject(source: &str) -> String {
        let defs = outline(source, ID).defs;
        let mut n = 0;
        let mut edits = Vec::new();
        for def in &defs {
            if def.marker.is_none() {
                n += 1;
                if let Ok(edit) = plan_marker(source, def, &format!("id-{n}"), ID) {
                    edits.push(edit);
                }
            }
        }
        apply(source, &edits).expect("the edits do not overlap")
    }

    #[test]
    fn new_statement_before_a_one_liner_body() {
        let out = inject("def stub():\n    ...\n");
        assert_eq!(out, "def stub():\n    \"\"\"ID: id-1\"\"\"\n    ...\n");
    }

    #[test]
    fn new_statement_before_a_normal_body() {
        let out = inject("def f():\n    return 1\n");
        assert_eq!(out, "def f():\n    \"\"\"ID: id-1\"\"\"\n    return 1\n");
    }

    #[test]
    fn extends_a_single_line_triple_quoted_docstring() {
        let out = inject("def f():\n    \"\"\"Summary.\"\"\"\n    return 1\n");
        assert_eq!(
            out,
            "def f():\n    \"\"\"Summary.\n\n    ID: id-1\n    \"\"\"\n    return 1\n"
        );
    }

    #[test]
    fn extends_a_multi_line_docstring_whose_closer_owns_its_line() {
        let out =
            inject("def f():\n    \"\"\"Summary.\n\n    Details.\n    \"\"\"\n    return 1\n");
        assert_eq!(
            out,
            "def f():\n    \"\"\"Summary.\n\n    Details.\n\n    ID: id-1\n    \"\"\"\n    return 1\n"
        );
    }

    #[test]
    fn extends_a_single_quoted_docstring_on_one_line() {
        let out = inject("def f():\n    'Summary.'\n    return 1\n");
        assert_eq!(out, "def f():\n    'Summary. ID: id-1'\n    return 1\n");
    }

    #[test]
    fn already_marked_def_gets_no_edit() {
        let out = inject("def f():\n    \"\"\"ID: existing\"\"\"\n    return 1\n");
        assert_eq!(
            out,
            "def f():\n    \"\"\"ID: existing\"\"\"\n    return 1\n"
        );
    }

    #[test]
    fn prose_mentioning_the_label_is_treated_as_already_marked() {
        // the reader finds `ID` -> `nothing-real,` here, so the outline already has a marker and none is added
        let src = "def f():\n    \"\"\"The text says ID: nothing-real, and nothing real below.\"\"\"\n    return 1\n";
        assert_eq!(inject(src), src);
    }

    #[test]
    fn nested_defs_get_marked_with_no_collision() {
        let out = inject("class Outer:\n    def inner():\n        return 1\n");
        assert_eq!(
            out,
            "class Outer:\n    \"\"\"ID: id-1\"\"\"\n    def inner():\n        \"\"\"ID: id-2\"\"\"\n        return 1\n"
        );
    }

    #[test]
    fn a_second_run_makes_no_edit() {
        let once = inject("def f():\n    return 1\n");
        let twice = inject(&once);
        assert_eq!(once, twice);
    }

    fn plan(source: &str, text: &str) -> Result<Edit, Skip> {
        let def = outline(source, ID).defs.remove(0);
        plan_marker(source, &def, text, ID)
    }

    #[test]
    fn crlf_files_get_crlf_lines() {
        let out = inject("def f():\r\n    return 1\r\n");
        assert_eq!(
            out,
            "def f():\r\n    \"\"\"ID: id-1\"\"\"\r\n    return 1\r\n"
        );
    }

    #[test]
    fn tab_indentation_is_copied_byte_for_byte() {
        let out = inject("def f():\n\treturn 1\n");
        assert_eq!(out, "def f():\n\t\"\"\"ID: id-1\"\"\"\n\treturn 1\n");
    }

    #[test]
    fn a_label_other_than_id_is_written_as_given() {
        let style = MarkerStyle {
            label: "Key",
            placement: Placement::Docstring,
        };
        let source = "def f():\n    return 1\n";
        let def = outline(source, style).defs.remove(0);
        let edit = plan_marker(source, &def, "k-1", style).unwrap();
        let new = apply(source, &[edit]).unwrap();
        assert_eq!(new, "def f():\n    \"\"\"Key: k-1\"\"\"\n    return 1\n");
        assert_eq!(outline(&new, style).defs[0].marker.as_deref(), Some("k-1"));
    }

    #[test]
    fn a_def_with_a_syntax_error_is_skipped() {
        assert_eq!(
            plan("def broken(:\n    pass\n", "x"),
            Err(Skip::SyntaxError)
        );
    }

    #[test]
    fn an_empty_body_is_not_written_to() {
        // the parser leaves no statement: there is nothing to put the marker before
        let source = "def f():\n";
        assert!(matches!(
            plan(source, "x"),
            Err(Skip::SyntaxError | Skip::UnsafeDocComment)
        ));
    }

    #[test]
    fn a_marked_def_says_what_it_has() {
        let source = "def f():\n    \"\"\"ID: abc\"\"\"\n";
        assert_eq!(
            plan(source, "new"),
            Err(Skip::AlreadyMarked {
                existing: "abc".into()
            })
        );
    }

    #[test]
    fn marker_text_that_cannot_be_read_back_is_refused() {
        let source = "def f():\n    return 1\n";
        for bad in [
            "",
            "two words",
            "a:b",
            "has\"quote",
            "it's",
            "back\\slash",
            "new\nline",
            "\"\"\"",
        ] {
            assert_eq!(plan(source, bad), Err(Skip::BadMarkerText), "{bad:?}");
        }
    }

    #[test]
    fn a_def_from_another_text_is_not_found() {
        let def = outline("def f():\n    return 1\n", ID).defs.remove(0);
        let other = "x = 1\n";
        assert_eq!(plan_marker(other, &def, "x", ID), Err(Skip::DefNotFound));
    }

    #[test]
    fn marker_is_read_from_the_text_at_the_def_range() {
        let source = "def f():\n    \"\"\"ID: abc\"\"\"\n";
        let def = outline(source, ID).defs.remove(0);
        assert_eq!(marker(source, &def, ID).as_deref(), Some("abc"));
        // a text with no def at that range has no marker for it
        assert_eq!(marker("def f():\n    pass\n", &def, ID), None);
    }
}
