//! FAST: outline, markers, header. `sem.rs` is the only file that imports the engine (`bend-parser`, the parser this repo writes for Bend, BD-D1).
//!
//! Text in, data out: nothing here reads or writes a file, and nothing invents an ID.
//!
//! ```
//! use bend_syntax::{MarkerStyle, outline};
//!
//! let text = "# Adds.\ndef add(a: Nat, -B: Data) -> Nat: a\n";
//! let defs = outline(text, MarkerStyle::docstring("ID")).defs;
//! assert_eq!(defs[0].qualname, "add");
//! assert_eq!(defs[0].params, ["a", "-B"]);
//! ```

mod outline;
mod sem;

pub use analyzer_core::edit::{Edit, MarkerStyle, Placement};
pub use analyzer_core::outline::{Def, DefKind, Outline, SyntaxError};
pub use analyzer_core::text::Span;

/// The defs of one file, in source order. Bend has no nesting, so every `parent` is `None`. Text in, data out: no file is read. `style` is taken now and
/// used by the marker (BD2 step 2), so that the signature does not move at the tag.
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
/// let o = outline(text, MarkerStyle::docstring("ID"));
/// assert_eq!(occurrences(&o), [0, 1]);
/// ```
pub fn occurrences(outline: &Outline) -> Vec<u32> {
    outline::occurrences(outline)
}
