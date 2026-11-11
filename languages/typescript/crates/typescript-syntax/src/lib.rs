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

/// `outline`, and for each def the byte span of the syntax node it names, in the order of `Outline::defs`. That node is the one whose body is the def's
/// body (the function, method, accessor, constructor, arrow or function expression, or the class), or the declaration of a def with no body (an
/// interface and its method signatures, an enum, a type alias, a namespace). It is not always where `Def::range` starts: an overloaded def's range runs
/// from its first signature and the node is its implementation, and `const f = () => {}` has the whole statement as its range and the arrow as its node.
///
/// For a caller that holds the same text as a tree of its own (the program of `typescript-analysis`): the same text parses to the same node spans, so a
/// span finds the node, and the rule for what a def is stays in this crate.
pub fn outline_with_nodes(
    text: &str,
    dialect: Dialect,
    style: MarkerStyle<'_>,
) -> (Outline, Vec<Span>) {
    outline::outline_with_nodes(text, dialect, style)
}

/// Why a marker was not planned. Match on the variant to act on it; `to_string()` gives the reason as a sentence a person can read, for
/// an editor to show next to the def.
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
    /// A new block is not stacked on top of it: the engine uses only the last block above a def, so the user's own would stop applying.
    UnsafeDocComment,
    /// The nearest JSDoc block above the def is the file's opening comment (a blank line ends it before the def) and it carries block
    /// tags (`@param`, `@type`, …). It is the file's, so it is not written into; and a new block below it would switch its tags off for
    /// the def, to which the engine applies them.
    FileCommentAbove,
    /// A comment above the def is a directive that points at the def's line (`// eslint-disable-next-line` directly above it; `// @ts-ignore`
    /// also through blank lines and `//` comments), and the def has no JSDoc block to write in: a new block put between them would move
    /// what the directive points at.
    DirectiveAbove,
    /// The marker text is empty, starts or ends with a blank (it would not read back the same), has a line break, or contains `*/`.
    InvalidText,
    /// The style asks for a placement this analyzer does not build.
    PlacementNotSupported,
    /// The text has no def with this qualname and range: the def comes from the outline of another text (an older version of the file).
    /// Its offsets are not trusted: outline the text again.
    UnknownDef,
}

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self {
            Skip::NotOnOwnLine => {
                "the definition does not start on a line of its own, and code is never reformatted to make room"
            }
            Skip::SyntaxError => {
                "a syntax error overlaps the definition or the doc comment above it"
            }
            Skip::AlreadyMarked { existing } => {
                return write!(f, "the definition already has a marker: {existing}");
            }
            Skip::UnsafeDocComment => {
                "the doc comment above the definition cannot be edited safely: its closing `*/` is not on a line of its own, a line has no \
                 leading `*`, or a tag is on its first line"
            }
            Skip::FileCommentAbove => {
                "the doc comment above the definition is the file's opening comment and carries tags: it is not edited, and a doc comment \
                 put below it would stop its tags from applying to the definition"
            }
            Skip::DirectiveAbove => {
                "a comment above the definition (such as `// @ts-ignore` or `// eslint-disable-next-line`) applies to its first line, and \
                 a doc comment put between them would change what it applies to"
            }
            Skip::InvalidText => {
                "the marker text is empty, starts or ends with a space, has a line break, or contains `*/`"
            }
            Skip::PlacementNotSupported => {
                "this marker placement is not supported for TypeScript and JavaScript"
            }
            Skip::UnknownDef => {
                "the definition was not found in this text: the file changed since it was outlined"
            }
        };
        f.write_str(reason)
    }
}

impl std::error::Error for Skip {}

/// The marker written on this def, if any. `outline` already fills `Def::marker` with it. `None` too when `def` is not a def of this text.
pub fn marker(text: &str, dialect: Dialect, def: &Def, style: MarkerStyle<'_>) -> Option<String> {
    match style.placement {
        Placement::Docstring => sem::with_parsed(text, dialect, |p| {
            if !outline::has_def(p, def) {
                return None;
            }
            marker::marker_of(text, &p.above(def.range.start), style.label)
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
        // the edit is placed by the def's offsets: they must be this text's
        if !outline::has_def(p, def) {
            return Err(Skip::UnknownDef);
        }
        let errors: Vec<Span> = p
            .syntax_errors()
            .into_iter()
            .map(|(range, _)| range)
            .collect();
        marker::plan(
            text,
            &p.above(def.range.start),
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
