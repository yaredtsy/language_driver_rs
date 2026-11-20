//! FAST: outline, markers, header. `sem.rs` is the only file that imports the engine (`bend-parser`, the parser this repo writes for Bend, BD-D1).
//!
//! Text in, data out: nothing here reads or writes a file, and nothing invents an ID.
//!
//! Bend has no docstring, so a marker is a `# <label>: <text>` comment line: a def's in the comment block directly above its declaration, the file's at
//! the top of the file, before the imports ([`MarkerStyle::above`]).
//!
//! ```
//! use bend_syntax::{MarkerStyle, outline};
//!
//! let text = "# Adds.\n# ID: add-1\ndef add(a: Nat, -B: Data) -> Nat: a\n";
//! let defs = outline(text, MarkerStyle::above("ID")).defs;
//! assert_eq!(defs[0].marker.as_deref(), Some("add-1"));
//! assert_eq!(defs[0].qualname, "add");
//! assert_eq!(defs[0].params, ["a", "-B"]);
//! ```

mod header;
mod marker;
mod outline;
mod sem;

pub use analyzer_core::edit::{Edit, MarkerStyle, Placement};
pub use analyzer_core::outline::{Def, DefKind, Outline, SyntaxError};
pub use analyzer_core::text::Span;

/// The defs of one file, in source order, with each def's marker and the file's own. Bend has no nesting, so every `parent` is `None`. Text in, data
/// out: no file is read. Bend builds only [`Placement::Above`]; with another style no marker is read.
pub fn outline(text: &str, style: MarkerStyle<'_>) -> Outline {
    outline::outline(text, style)
}

/// The `occurrence` of each def of `outline`, in the same order: 0-based, the number of defs with the same qualname before it in the file (D18). A
/// qualname is not unique in a file (D15): a law and the def that proves it share one. (qualname, occurrence) is.
///
/// ```
/// use bend_syntax::{MarkerStyle, occurrences, outline};
///
/// let text = "law sym:\n  for x: Nat\n  Eq(x, x)\ndef sym(x):\n  {==}\n";
/// let o = outline(text, MarkerStyle::above("ID"));
/// assert_eq!(occurrences(&o), [0, 1]);
/// ```
pub fn occurrences(outline: &Outline) -> Vec<u32> {
    outline::occurrences(outline)
}

/// The marker written on this def, if any (03 §1.3): the last `# <label>: <text>` line of the comment block directly above its declaration. `outline`
/// already fills `Def::marker` with it. `None` too when `def` is not a def of this text, an outline of another text (the file changed since).
pub fn marker(text: &str, def: &Def, style: MarkerStyle<'_>) -> Option<String> {
    outline(text, style)
        .defs
        .into_iter()
        .find(|d| d.qualname == def.qualname && d.range == def.range)?
        .marker
}

/// The edit that puts `marker_text` on this def, or why it cannot be done. Nothing is written: the edit is for the caller to apply. `def` must come from
/// the outline of this same text. The line goes immediately above the declaration (above `@unsafe` when there is one), below the comment block that is
/// there, with the file's line ending.
pub fn plan_marker(
    text: &str,
    def: &Def,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    marker::plan(text, def, marker_text, style)
}

/// The file's own marker, from a `# <label>: <text>` comment at the top of the file: the first one before the first `import` (or, in a file with none,
/// before the first declaration's doc block). Also `Outline::file_marker`.
pub fn header_marker(text: &str, style: MarkerStyle<'_>) -> Option<String> {
    header::read(text, &sem::parse(text), style)
}

/// The edit that writes the file's marker, or why it cannot be done: a line at offset 0 ([`header_slot`]), with the file's line ending, and an empty line
/// after it when the text it goes above is the first declaration (so that the line is the file's and not that def's).
pub fn plan_header_marker(
    text: &str,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    header::plan(text, marker_text, style)
}

/// Where the file's marker is written: offset 0, or just past a byte-order mark. The same for every file.
pub fn header_slot(text: &str) -> u32 {
    header::slot(text)
}

/// Why a marker was not planned. Match on the variant to act on it; `to_string()` gives the reason as a sentence a person can read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Skip {
    /// The def (or the file) already carries a marker.
    AlreadyMarked { existing: String },
    /// Something other than blanks stands before the declaration on its first line (two defs on one line). Never reformat the user's code to make room.
    NotOnOwnLine,
    /// A parse error overlaps the def, or the file has one and the marker, written, would not be read back from this def (or the file).
    SyntaxError,
    /// The marker text is empty, starts or ends with a blank (it would not read back the same), or has a line break.
    InvalidText,
    /// The text has no def with this qualname and range: the def comes from the outline of another text (an older version of the file). Outline again.
    UnknownDef,
    /// The style asks for a placement Bend does not build: only `Placement::Above`.
    PlacementNotSupported,
}

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self {
            Skip::AlreadyMarked { existing } => {
                return write!(f, "already has a marker: {existing}");
            }
            Skip::NotOnOwnLine => {
                "the definition does not start on a line of its own, and code is never reformatted to make room"
            }
            Skip::SyntaxError => {
                "a syntax error overlaps the definition or is next to it, and a marker written there would not be read back from it"
            }
            Skip::InvalidText => {
                "the marker text is empty, starts or ends with a space, or has a line break"
            }
            Skip::UnknownDef => {
                "the definition was not found in this text: the file changed since it was outlined"
            }
            Skip::PlacementNotSupported => {
                "this marker placement is not supported for Bend (a comment line above the definition is the only one)"
            }
        };
        f.write_str(reason)
    }
}

impl std::error::Error for Skip {}
