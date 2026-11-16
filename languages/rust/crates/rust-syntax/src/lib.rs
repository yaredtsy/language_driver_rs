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

mod outline;
mod sem;

pub use analyzer_core::edit::{MarkerStyle, Placement};
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
