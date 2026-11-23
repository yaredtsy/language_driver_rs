//! The one module of this crate that names a `bend_parser` type, and no `bend_parser` type leaves it (rule 4). `parse` reads the text and hands back owned
//! data: what the outline needs of each declaration, as plain strings and spans. Here that costs nothing (the tree is ours already); the boundary is the
//! point, so that a different parser would replace this file and nothing else.

use analyzer_core::text::Span;

/// What a declaration is, as the parser says it. How that becomes a `DefKind` is decided in one place, `outline::def_kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeclKind {
    Type,
    Def,
    Law,
}

/// One declaration, read.
#[derive(Clone, Debug)]
pub(crate) struct Decl {
    pub(crate) kind: DeclKind,
    /// The name as written, dots included.
    pub(crate) name: String,
    pub(crate) name_span: Span,
    /// The parameters (a def's, a type's, a law's clauses') as written with their marks and without their types: `-A`, `+x`, `~f`, `n`.
    pub(crate) params: Vec<String>,
    /// From the first token (`@`, `def`, `type`, `law`) to the last byte read.
    pub(crate) span: Span,
    /// The `#` comment block directly above.
    pub(crate) doc: Option<Span>,
    pub(crate) syntax_error: bool,
}

/// What the parser could not read: its span, and what it said about it.
#[derive(Clone, Debug)]
pub(crate) struct Error {
    pub(crate) span: Span,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Parsed {
    pub(crate) decls: Vec<Decl>,
    pub(crate) errors: Vec<Error>,
    /// Every `#` comment, each from its `#` to the end of its line, in source order.
    pub(crate) comments: Vec<Span>,
    /// Where the first `import` line starts, if the file has one the parser could read.
    pub(crate) first_import: Option<u32>,
}

/// A binder or a clause name as written: `~f`, `-A`, `+x`, `n`. An `exs` clause has no quantity mark, and the parser says `Plain` for it.
fn mark(template: bool, quant: bend_parser::Quant, name: &str) -> String {
    let template = if template { "~" } else { "" };
    let quant = match quant {
        bend_parser::Quant::Erased => "-",
        bend_parser::Quant::Reusable => "+",
        _ => "",
    };
    format!("{template}{quant}{name}")
}

fn marks(binds: &[bend_parser::Bind]) -> Vec<String> {
    binds
        .iter()
        .map(|b| mark(b.template, b.quant, &b.name.text))
        .collect()
}

fn decl(d: &bend_parser::Decl) -> Option<Decl> {
    let (kind, params) = match d {
        bend_parser::Decl::Def(f) => (DeclKind::Def, marks(&f.params)),
        bend_parser::Decl::Type(t) => (DeclKind::Type, marks(&t.params)),
        bend_parser::Decl::Law(l) => (
            DeclKind::Law,
            l.clauses
                .iter()
                .map(|c| mark(c.template, c.quant, &c.name.text))
                .collect(),
        ),
        _ => return None,
    };
    Some(Decl {
        kind,
        name: d.name().text.clone(),
        name_span: d.name().span,
        params,
        span: d.span(),
        doc: d.doc(),
        syntax_error: d.syntax_error(),
    })
}

pub(crate) fn parse(text: &str) -> Parsed {
    let tree = bend_parser::parse(text);
    Parsed {
        comments: tree.comments.clone(),
        first_import: tree.imports.first().map(|i| i.span.start),
        decls: tree.decls.iter().filter_map(decl).collect(),
        errors: tree
            .errors
            .iter()
            .map(|e| Error {
                span: e.span,
                message: format!("expected {}, found {}", e.expected, e.observed),
            })
            .collect(),
    }
}
