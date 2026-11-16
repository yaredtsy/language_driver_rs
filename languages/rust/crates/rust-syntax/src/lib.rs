//! FAST: outline, markers, header. `sem.rs` is the only file that imports the engine.
//!
//! Text in, data out: nothing here reads or writes a file, and nothing invents an ID.
//!
//! ```
//! use rust_syntax::{Edition, MarkerStyle, outline};
//!
//! let text = "/// Parses an order.\nfn parse_order(raw: &str) {}\n";
//! let defs = outline(text, Edition::LATEST, MarkerStyle::docstring("ID")).defs;
//! assert_eq!(defs[0].qualname, "parse_order");
//! assert_eq!(defs[0].params, ["raw"]);
//! ```

mod header;
mod marker;
mod outline;
mod sem;

pub use analyzer_core::edit::{Edit, MarkerStyle, Placement};
pub use analyzer_core::outline::{Def, DefKind, Outline, SyntaxError};
pub use analyzer_core::text::Span;

/// The Rust edition a file is written in. It changes what parses (`async`, `dyn` and `gen` are keywords in some editions and not in
/// others), so it is an input: the driver reads it from `Cargo.toml` and falls back to `Edition::LATEST`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Edition {
    E2015,
    E2018,
    E2021,
    E2024,
}

impl Edition {
    pub const LATEST: Edition = Edition::E2024;
}

/// The defs of one file, in source order, parents before children. Text in, data out: no file is read.
pub fn outline(text: &str, edition: Edition, style: MarkerStyle<'_>) -> Outline {
    outline::outline(text, edition, style)
}

/// The `occurrence` of each def of `outline`, in the same order: 0-based, the number of defs with the same qualname before it in the
/// file (D18). A qualname is not unique in a file (D15); (qualname, occurrence) is.
///
/// ```
/// use rust_syntax::{Edition, MarkerStyle, occurrences, outline};
///
/// let text = "impl S { fn m() {} }\nimpl S { fn m() {} }\n";
/// let o = outline(text, Edition::LATEST, MarkerStyle::docstring("ID"));
/// assert_eq!(occurrences(&o), [0, 1]);
/// ```
pub fn occurrences(outline: &Outline) -> Vec<u16> {
    outline::occurrences(outline)
}

/// The marker written on this def, if any (RS2 §3): the first `<label>: <text>` line of its own `///` and `/** */` comments. `outline` already
/// fills `Def::marker` with it. `None` too when `def` is not a def of this text, an outline of another text (the file changed since): its offsets
/// point somewhere else now.
pub fn marker(text: &str, edition: Edition, def: &Def, style: MarkerStyle<'_>) -> Option<String> {
    outline(text, edition, style)
        .defs
        .into_iter()
        .find(|d| d.qualname == def.qualname && d.range == def.range)?
        .marker
}

/// The edit that puts `marker_text` on this def, or why it cannot be done (RS2 §4, §5). Nothing is written: the edit is for the caller to
/// apply. `def` must come from the outline of this same text.
pub fn plan_marker(
    text: &str,
    edition: Edition,
    def: &Def,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    marker::plan(text, edition, def, marker_text, style)
}

/// The file's own marker, from a `// <label>: <text>` comment at the top of the file: the first one before the first item, inner attribute or
/// inner doc comment (RS2 §6). It takes no edition: a file's header does not depend on one.
pub fn header_marker(text: &str, style: MarkerStyle<'_>) -> Option<String> {
    header::read(text, style)
}

/// The edit that writes the file's marker, or why it cannot be done. It goes after a `#!` line and as the last line of a leading licence
/// comment, and before `#![…]`, `//!` and the first item; above a `//!` it is followed by an empty line (RS2 §6).
pub fn plan_header_marker(
    text: &str,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    header::plan(text, marker_text, style)
}

/// Why a marker was not planned. Match on the variant to act on it; `to_string()` gives the reason as a sentence a person can read, for an
/// editor to show next to the def.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Skip {
    /// The def already carries a marker.
    AlreadyMarked { existing: String },
    /// Something other than blanks stands before the def on its first line (`fn a() {} fn b() {}`, generated code). Never reformat the
    /// user's code to make room.
    NotOnOwnLine,
    /// A parse error overlaps the def or its doc comments; or the file has one, and the marker, written, would not be read back from this
    /// def (an unfinished item above it takes the new doc line).
    SyntaxError,
    /// The last doc comment is a `/** */` block that cannot be edited safely.
    UnsafeDocComment,
    /// The marker text is empty, starts or ends with a blank (it would not read back the same), has a line break, or contains `*/`.
    InvalidText,
    /// The text has no def with this qualname and range: the def comes from the outline of another text (an older version of the file).
    /// Its offsets are not trusted: outline the text again.
    UnknownDef,
    /// The style asks for a placement this analyzer does not build.
    PlacementNotSupported,
}

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self {
            Skip::AlreadyMarked { existing } => {
                return write!(f, "the definition already has a marker: {existing}");
            }
            Skip::NotOnOwnLine => {
                "the definition does not start on a line of its own, and code is never reformatted to make room"
            }
            Skip::SyntaxError => {
                "a syntax error overlaps the definition or the doc comments above it, or is next to it, and a marker written there would not \
                 be read back from the definition"
            }
            Skip::UnsafeDocComment => {
                "the last doc comment of the definition is a `/** */` block that cannot be edited safely: it is closed on its opening line, a line has no \
                 leading `*`, or text stands on its closing line"
            }
            Skip::InvalidText => {
                "the marker text is empty, starts or ends with a space, has a line break, or contains `*/`"
            }
            Skip::UnknownDef => {
                "the definition was not found in this text: the file changed since it was outlined"
            }
            Skip::PlacementNotSupported => "this marker placement is not supported for Rust",
        };
        f.write_str(reason)
    }
}

impl std::error::Error for Skip {}
