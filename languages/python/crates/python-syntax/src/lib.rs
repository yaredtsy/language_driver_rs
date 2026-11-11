//! FAST: outline, markers, header. `sem.rs` is the only file that imports the parser.

mod ids;
mod outline;
mod sem;

pub use analyzer_core::edit::{Edit, MarkerStyle, Placement};
pub use analyzer_core::outline::{Def, DefKind, Outline, SyntaxError};
pub use analyzer_core::text::Span;

/// The defs of one file, in source order, parents before children. Text in, data out: no file is read.
///
/// `style.label` is the word a marker is written under (`ID: …` in a docstring); `Def::marker` holds what was found.
/// A file's header marker is not read here: it has its own label, and `header_marker` reads it.
pub fn outline(text: &str, style: MarkerStyle<'_>) -> Outline {
    outline::outline(text, style)
}
