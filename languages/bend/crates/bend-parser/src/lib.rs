//! THE ENGINE WE WRITE (BD-D1): `parse(text) -> Tree` for Bend, mirroring `bend2/bend.ts` function for function. The grammar pin is in
//! `languages/bend/ENGINE.md`. Depends on `analyzer-core` for [`analyzer_core::text::Span`] and on nothing else.
//!
//! `parse` never fails and never panics: what it could not read is in [`Tree::errors`], and a declaration it could not finish is kept, flagged. Nothing
//! here reads a file or the network; the `import` lines come back as data.

mod parse;
mod term;
mod tree;

pub use tree::{
    Arg, ArraySize, Bind, Body, Case, Clause, CtrDecl, Decl, DefBody, DefDecl, Do, DoKind, DoStmt,
    Import, LawDecl, Let, Lit, Match, MatchArm, Name, Op, ParseError, Quant, StrLit, Term,
    TermKind, Tree, TypeDecl,
};

/// Reads a Bend source text into its surface tree. Spans are byte offsets into `text`.
pub fn parse(text: &str) -> Tree {
    parse::parse(text)
}
