//! FAST: outline, markers, header. `sem.rs` is the only file that imports the parser.

mod edit;
mod header;
mod ids;
mod outline;
mod sem;

pub use analyzer_core::edit::{Edit, MarkerStyle, Placement};
pub use analyzer_core::outline::{Def, DefKind, Outline, SyntaxError};
pub use analyzer_core::text::Span;
pub use edit::Skip;

/// The defs of one file, in source order, parents before children. Text in, data out: no file is read.
///
/// `style.label` is the word a marker is written under (`ID: …` in a docstring); `Def::marker` holds what was found.
/// A file's header marker is not read here: it has its own label, and `header_marker` reads it.
pub fn outline(text: &str, style: MarkerStyle<'_>) -> Outline {
    outline::outline(text, style)
}

/// The marker on this def, read from `text` at the def's range. The outline already fills `Def::marker`; this reads it again from the
/// text, so it answers for the text you have and not for the def you were handed. `None` when there is none, or no def at that range.
pub fn marker(text: &str, def: &Def, style: MarkerStyle<'_>) -> Option<String> {
    edit::marker(text, def, style)
}

/// The edit that puts `marker_text` on this def, or why it cannot be done. It is planned on `text`, never applied, and only ever
/// inserts: the old text is a subsequence of the new one.
pub fn plan_marker(
    text: &str,
    def: &Def,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    edit::plan_marker(text, def, marker_text, style)
}

/// The file's own marker: a `# <label>: <text>` comment in the header, before the first statement. Not part of the outline, because the
/// header has its own label.
pub fn header_marker(text: &str, style: MarkerStyle<'_>) -> Option<String> {
    header::header_marker(text, style)
}

/// The edit that adds the header marker after the leading comments (shebang, encoding line, licence) and before the first statement,
/// or why it cannot be done. Planned on `text`, never applied.
pub fn plan_header_marker(
    text: &str,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    header::plan_header_marker(text, marker_text, style)
}
