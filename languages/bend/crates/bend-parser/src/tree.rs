//! The tree: what the user wrote, at the surface level (docs/08-bend/02-phase-1-parsing.md §2.2). Sugar is kept, nothing is desugared, every node has a
//! [`Span`] (byte offsets into the text that was parsed), a dotted name is one [`Name`].
//!
//! Step 3 of BD1 fills the outline-shaped half: imports, the three declarations with their names and parameter lists, and the extent of each body. The
//! places where a term will go (a parameter's type, a return type, a body) hold a `Span` for now; step 4 turns each into the term or body it covers.

use analyzer_core::text::Span;

/// The answer of [`crate::parse`]: always a tree, never a failure. What could not be read is in `errors`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Tree {
    /// The `import` lines at the head of the file, in order. Nothing was read from them: the paths come back as written.
    pub imports: Vec<Import>,
    /// The declarations in source order, including broken ones (flagged `syntax_error`, see [`DefDecl::syntax_error`]).
    pub decls: Vec<Decl>,
    /// Every `#` comment, from the `#` to the end of the line (a `\r` before the `\n` is not part of it), in source order. `bend` throws them away;
    /// markers and docs need them.
    pub comments: Vec<Span>,
    /// What could not be read, in source order. Recovery is at declaration boundaries only (BD-D10): see [`ParseError`].
    pub errors: Vec<ParseError>,
}

/// One `import` line: `import Base` or `import <path> as <Alias>` (`bend.ts` `book_load`, a regex on the trimmed line, not `parse_*`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Import {
    /// The path as written: `Base`, `./m.bend`, `../lib/x.bend`. Not checked beyond the line's shape; resolving it is `bend-analysis`'s.
    pub path: String,
    pub path_span: Span,
    pub alias: Option<Name>,
    /// From `import` to the end of the alias (or of the path): a trailing comment is not part of it.
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decl {
    Type(TypeDecl),
    Def(DefDecl),
    Law(LawDecl),
}

impl Decl {
    pub fn name(&self) -> &Name {
        match self {
            Decl::Type(d) => &d.name,
            Decl::Def(d) => &d.name,
            Decl::Law(d) => &d.name,
        }
    }

    /// From the first token of the declaration (`@`, `def`, `type`, `law`) to the last byte of what was read of it. The comment block above is not in it:
    /// it is `doc`.
    pub fn span(&self) -> Span {
        match self {
            Decl::Type(d) => d.span,
            Decl::Def(d) => d.span,
            Decl::Law(d) => d.span,
        }
    }

    pub(crate) fn set_span(&mut self, span: Span) {
        match self {
            Decl::Type(d) => d.span = span,
            Decl::Def(d) => d.span = span,
            Decl::Law(d) => d.span = span,
        }
    }

    pub fn doc(&self) -> Option<Span> {
        match self {
            Decl::Type(d) => d.doc,
            Decl::Def(d) => d.doc,
            Decl::Law(d) => d.doc,
        }
    }

    pub fn syntax_error(&self) -> bool {
        match self {
            Decl::Type(d) => d.syntax_error,
            Decl::Def(d) => d.syntax_error,
            Decl::Law(d) => d.syntax_error,
        }
    }
}

/// `("@unsafe")? "def" Name "?"? "(" [Bind ","?] ")" ("->" Term)? ":" (Body | ["import" STRING]+)`
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DefDecl {
    pub name: Name,
    /// Written `@unsafe` or with the `?` sugar: the flag is about proofs, not about what the def calls.
    pub unsafe_: bool,
    /// Written `def f?(…)`. `unsafe_` is also true then.
    pub question: bool,
    pub params: Vec<Bind>,
    /// The `-> T` of the header. `None` is the form that fills a law (`def f(x):`, its parameters bare names): the parser cannot tell whether such a law
    /// exists, that is a book's question.
    pub ret: Option<Span>,
    pub body: DefBody,
    pub span: Span,
    /// The `#` comment block directly above the declaration (no blank line between), from its first `#` to the end of its last line.
    pub doc: Option<Span>,
    /// The declaration could not be read to its end. The name and the parameters read so far are kept, `span` runs to the last code before the
    /// next declaration, and an entry of [`Tree::errors`] says why.
    pub syntax_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DefBody {
    /// The text of the body, from its first token to its last: lines indented past the declaration's column, the rest of the header's line included.
    /// Step 4 reads it into a body. Empty when the declaration ends after the `:` (a broken or unfinished def).
    Source(Span),
    /// `import "./e.c"` lines: a foreign effect, code we cannot read.
    Foreign(Vec<StrLit>),
}

/// A `"…"` read raw to the next `"` (no escapes): a foreign import's path.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StrLit {
    /// Without the quotes.
    pub value: String,
    /// With the quotes.
    pub span: Span,
}

/// `"type" Name ("<" [Bind ","?] ">")? "is" Term ":" [Ctr]`
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TypeDecl {
    pub name: Name,
    pub params: Vec<Bind>,
    /// The term after `is`: `Data`, `Kind(a)`, …
    pub kind: Option<Span>,
    pub ctrs: Vec<CtrDecl>,
    pub span: Span,
    pub doc: Option<Span>,
    pub syntax_error: bool,
}

/// `Name "{" [Bind ","?] "}"`: a constructor. Not a def (BD-D3).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CtrDecl {
    pub name: Name,
    pub fields: Vec<Bind>,
    /// From the name to the closing `}`.
    pub span: Span,
}

/// `"law" Name ":" [Clause] Body`
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LawDecl {
    pub name: Name,
    pub clauses: Vec<Clause>,
    /// The claim after the clauses (a body): from its first token to its last. Step 4 reads it.
    pub claim: Option<Span>,
    pub span: Span,
    pub doc: Option<Span>,
    pub syntax_error: bool,
}

/// `("for" (Quant | "~") | "exs") Name ":" Term ("where" Term)?`
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Clause {
    /// `exs` (an existential) rather than `for`.
    pub exists: bool,
    pub quant: Quant,
    /// `for ~k: T`.
    pub template: bool,
    pub name: Name,
    pub ty: Span,
    pub where_: Option<Span>,
    /// From `for` / `exs` to the end of the clause.
    pub span: Span,
}

/// A parameter, a type parameter or a constructor field: `+x: A`, `~f: A -> B`, `-A: Kind(a)`, or a bare `a`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Bind {
    /// As written. A template (`~`) has none written: `Plain`.
    pub quant: Quant,
    pub template: bool,
    pub name: Name,
    /// `None` for a bare name. (To Bend a bare name is `-name: Quant`; the surface form keeps it as written.)
    pub ty: Option<Span>,
    /// From the first mark (`~`, `-`, `+`) or the name to the end of the type.
    pub span: Span,
}

/// A binder's quantity mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Quant {
    /// `-x`: erased.
    Erased,
    /// `x`: used once. Also what a name with no mark is.
    Plain,
    /// `+x`: reusable.
    Reusable,
}

/// A name as `bend` reads it: letters, digits, `_` and `.`, ASCII only. The dot means nothing: `Nat.add` is one name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Name {
    pub text: String,
    pub span: Span,
}

/// Something that was not what the grammar expects. `bend` stops at its first error; we go on at the next declaration (BD-D10).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParseError {
    /// From where the reader stopped to the last code before the point where reading resumed: the next line at column 0 that starts with `def`, `type`,
    /// `law`, `@unsafe` or `import`, or the end of the text. Empty when it resumed at once.
    pub span: Span,
    /// What was expected, in `bend`'s words where there are some.
    pub expected: String,
    /// What was there: `'x'`, or `end of input`.
    pub observed: String,
}
