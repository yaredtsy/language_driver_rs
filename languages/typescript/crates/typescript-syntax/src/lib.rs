//! FAST: outline, markers, header. `sem.rs` is the only file that imports the engine.
//!
//! Text in, data out: nothing here reads or writes a file, and nothing invents an ID. The caller owns the text, the label and the marker
//! text, and applies the edits.
//!
//! ```
//! use analyzer_core::edit::apply;
//! use typescript_syntax::{Dialect, MarkerStyle, outline, plan_marker};
//!
//! let text = "/**\n * Parses an order.\n */\nexport function parseOrder(raw: string) {}\n";
//! let style = MarkerStyle::docstring("ID"); // the label is the caller's
//!
//! let defs = outline(text, Dialect::Ts, style).defs;
//! assert_eq!(defs[0].qualname, "parseOrder");
//! assert_eq!(defs[0].marker, None);
//!
//! // plan a marker: an edit, or the reason it cannot be done
//! let edit = plan_marker(text, Dialect::Ts, &defs[0], "3RGJ8M2K", style).unwrap();
//! let marked = apply(text, &[edit]).unwrap();
//! assert_eq!(
//!     marked,
//!     "/**\n * Parses an order.\n * ID: 3RGJ8M2K\n */\nexport function parseOrder(raw: string) {}\n"
//! );
//!
//! // the marker is read back, and survives the def being moved
//! let defs = outline(&marked, Dialect::Ts, style).defs;
//! assert_eq!(defs[0].marker.as_deref(), Some("3RGJ8M2K"));
//! ```

mod header;
mod marker;
mod outline;
mod sem;

pub use analyzer_core::edit::{Edit, MarkerStyle, Placement};
pub use analyzer_core::outline::{Def, DefKind, Outline, SyntaxError};
pub use analyzer_core::text::Span;

/// Which grammar to parse with. The same text parses differently as `.ts` and `.tsx` (`<T>x` is a cast in one and the start of
/// an element in the other).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Dialect {
    Ts,
    Tsx,
    Js,
    Jsx,
}

/// The defs of one file, in source order, parents before children. Text in, data out: no file is read. `style` says which label to read a
/// marker by (`Def::marker`).
pub fn outline(text: &str, dialect: Dialect, style: MarkerStyle<'_>) -> Outline {
    outline::outline(text, dialect, style)
}

/// Why a marker was not planned. The driver turns each variant into a message a person can read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Skip {
    /// Something other than blanks stands before the def on its first line (`const a = () => 1, b = () => 2;`, a minified file). Never reformat the
    /// user's code to make room.
    NotOnOwnLine,
    /// A parse error overlaps the def or the JSDoc block above it.
    SyntaxError,
    /// The def already carries a marker.
    AlreadyMarked { existing: String },
    /// The JSDoc block above is not one we can edit safely (no closing line of its own, a line without its `*`, a tag on the opening line).
    /// A new block is not stacked on top of it: two blocks would hide the user's own.
    UnsafeDocComment,
    /// The comment directly above the def is a directive that applies to the next line (`// @ts-ignore`, `// eslint-disable-next-line`, …):
    /// a block put between them would move what it points at.
    DirectiveAbove,
    /// The marker text is empty, has a line break, or contains `*/`.
    InvalidText,
    /// The style asks for a placement this analyzer does not build.
    PlacementNotSupported,
}

/// The marker written on this def, if any. `outline` already fills `Def::marker` with it.
pub fn marker(text: &str, dialect: Dialect, def: &Def, style: MarkerStyle<'_>) -> Option<String> {
    match style.placement {
        Placement::Docstring => sem::with_parsed(text, dialect, |p| {
            let start = def.range.start;
            marker::marker_of(text, start, &p.comments_before(start), style.label)
        }),
        _ => None,
    }
}

/// The edit that puts `marker_text` on this def, or why it cannot be done. Nothing is written: the edit is for the caller to apply.
pub fn plan_marker(
    text: &str,
    dialect: Dialect,
    def: &Def,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    if !matches!(style.placement, Placement::Docstring) {
        return Err(Skip::PlacementNotSupported);
    }
    sem::with_parsed(text, dialect, |p| {
        let start = def.range.start;
        let errors: Vec<Span> = p
            .syntax_errors()
            .into_iter()
            .map(|(range, _)| range)
            .collect();
        marker::plan(
            text,
            start,
            &p.comments_before(start),
            def.syntax_error,
            &errors,
            style.label,
            marker_text,
        )
    })
}

/// The file's own marker, from a `// <label>: <text>` comment at the top of the file (after a shebang, before the first token).
pub fn header_marker(text: &str, style: MarkerStyle<'_>) -> Option<String> {
    match style.placement {
        // a comment is a comment in every dialect: any one reads the top of the file
        Placement::Docstring => {
            sem::with_parsed(text, Dialect::Ts, |p| header::read(p, text, style))
        }
        _ => None,
    }
}

/// The edit that writes the file's marker, or why it cannot be done. It goes after a `#!` line and after a leading licence block, and before
/// `/// <reference>` lines, `// @ts-check`, a `"use strict"` directive and code.
pub fn plan_header_marker(
    text: &str,
    marker_text: &str,
    style: MarkerStyle<'_>,
) -> Result<Edit, Skip> {
    if !matches!(style.placement, Placement::Docstring) {
        return Err(Skip::PlacementNotSupported);
    }
    sem::with_parsed(text, Dialect::Ts, |p| {
        header::plan(p, text, marker_text, style)
    })
}

impl Dialect {
    /// From a file name or path: `.ts` `.mts` `.cts` → `Ts`, `.tsx` → `Tsx`, `.js` `.mjs` `.cjs` → `Js`, `.jsx` → `Jsx`.
    /// `None` for any other extension. The check is on the extension only, case-insensitive.
    pub fn from_path(path: &str) -> Option<Dialect> {
        let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
        match ext.as_str() {
            "ts" | "mts" | "cts" => Some(Dialect::Ts),
            "tsx" => Some(Dialect::Tsx),
            "js" | "mjs" | "cjs" => Some(Dialect::Js),
            "jsx" => Some(Dialect::Jsx),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialect_from_the_extension() {
        let cases = [
            ("a.ts", Some(Dialect::Ts)),
            ("src/a.mts", Some(Dialect::Ts)),
            ("a.cts", Some(Dialect::Ts)),
            ("a.d.ts", Some(Dialect::Ts)),
            ("View.TSX", Some(Dialect::Tsx)),
            ("a.js", Some(Dialect::Js)),
            ("a.mjs", Some(Dialect::Js)),
            ("a.cjs", Some(Dialect::Js)),
            ("a.jsx", Some(Dialect::Jsx)),
            ("a.json", None),
            ("Makefile", None),
            ("", None),
        ];
        for (path, want) in cases {
            assert_eq!(Dialect::from_path(path), want, "{path}");
        }
    }
}
