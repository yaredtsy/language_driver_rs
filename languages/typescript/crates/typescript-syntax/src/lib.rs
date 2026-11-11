//! FAST: outline, markers, header. `sem.rs` is the only file that imports the engine.

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
