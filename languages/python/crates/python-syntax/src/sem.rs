//! The only file of this crate that names the parser. It parses one text and returns owned data: no `ruff_*` type leaves
//! it, so everything else is plain text work on that data.

use analyzer_core::text::Span;
use ruff_python_ast::statement_visitor::{StatementVisitor, walk_body, walk_stmt};
use ruff_python_ast::{ModModule, PythonVersion, Stmt, StringFlags};
use ruff_python_parser::{Mode, ParseOptions, Parsed, parse_unchecked};
use ruff_text_size::{Ranged, TextRange};

/// Where a docstring goes, or where a new one does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Slot {
    /// The offset the text goes at: just before the closing quotes of a docstring, or `stmt_start` when there is none.
    pub(crate) at: u32,
    /// The start of the first statement of the body: the indentation anchor, and the tell of a one-liner body.
    pub(crate) stmt_start: u32,
    pub(crate) kind: SlotKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotKind {
    /// A docstring exists: `at` is immediately before its closing quotes.
    InLiteral { triple_quoted: bool },
    /// None exists: a whole new docstring statement goes in at `at`.
    NewStatement,
}

/// One `def`, `async def` or `class` statement, in the order the walk meets them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawDef {
    pub(crate) name: String,
    pub(crate) is_class: bool,
    /// The whole statement, decorators included.
    pub(crate) range: Span,
    pub(crate) name_range: Span,
    /// Positional, positional-only and keyword-only names in order, then `*args`, then `**kwargs`.
    pub(crate) params: Vec<String>,
    /// Index of the enclosing def or class in the same list.
    pub(crate) parent: Option<usize>,
    /// The value of the string literal that is the first statement of the body, as the parser decoded it.
    pub(crate) docstring: Option<String>,
    /// `None` on a body a write must not touch: empty (a syntax error left no statement) or an unclosed docstring.
    pub(crate) slot: Option<Slot>,
    /// A parse error touches the range. It errs toward flagging a neighbour, never toward missing the broken one.
    pub(crate) syntax_error: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Module {
    pub(crate) defs: Vec<RawDef>,
    pub(crate) errors: Vec<(Span, String)>,
}

/// Parses a module. Always gives a result: a syntax error is in `errors`, not a failure.
pub(crate) fn parse(text: &str) -> Module {
    let options = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY313);
    let parsed: Parsed<ModModule> = parse_unchecked(text, options)
        .try_into_module()
        .expect("Mode::Module always produces a module");
    let errors: Vec<TextRange> = parsed.errors().iter().map(|e| e.location).collect();
    let mut walk = Walk {
        errors: &errors,
        current: None,
        out: Vec::new(),
    };
    walk_body(&mut walk, &parsed.syntax().body);
    Module {
        defs: walk.out,
        errors: parsed
            .errors()
            .iter()
            .map(|e| (span(e.location), e.error.to_string()))
            .collect(),
    }
}

fn span(range: TextRange) -> Span {
    Span::new(u32::from(range.start()), u32::from(range.end()))
}

struct Walk<'e> {
    errors: &'e [TextRange],
    current: Option<usize>,
    out: Vec<RawDef>,
}

impl<'a> StatementVisitor<'a> for Walk<'_> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        let (name, is_class, params, body) = match stmt {
            Stmt::FunctionDef(f) => {
                let p = &f.parameters;
                let mut params: Vec<String> = p
                    .iter_non_variadic_params()
                    .map(|a| a.parameter.name.to_string())
                    .collect();
                if let Some(v) = &p.vararg {
                    params.push(format!("*{}", v.name));
                }
                if let Some(k) = &p.kwarg {
                    params.push(format!("**{}", k.name));
                }
                (&f.name, false, params, &f.body)
            }
            Stmt::ClassDef(c) => (&c.name, true, Vec::new(), &c.body),
            // if / try / with / for / while / match: a def can sit in any of them and is named as if they were not there
            _ => {
                walk_stmt(self, stmt);
                return;
            }
        };
        let range = stmt.range();
        let index = self.out.len();
        self.out.push(RawDef {
            name: name.as_str().to_owned(),
            is_class,
            range: span(range),
            name_range: span(name.range()),
            params,
            parent: self.current,
            docstring: docstring(body),
            slot: slot(body),
            syntax_error: self.errors.iter().any(|e| range.intersect(*e).is_some()),
        });
        let outer = self.current.replace(index);
        walk_body(self, body);
        self.current = outer;
    }
}

/// The string literal that is the first statement of `body`, if it is a standalone expression.
fn docstring_literal(body: &[Stmt]) -> Option<&ruff_python_ast::ExprStringLiteral> {
    body.first()?.as_expr_stmt()?.value.as_string_literal_expr()
}

fn docstring(body: &[Stmt]) -> Option<String> {
    Some(docstring_literal(body)?.value.to_str().to_owned())
}

fn slot(body: &[Stmt]) -> Option<Slot> {
    let stmt_start = u32::from(body.first()?.start());
    match docstring_literal(body) {
        Some(doc) => {
            // The LAST part holds the closing quotes: `"a" "b"` is one literal.
            let last = doc.value.as_slice().last()?;
            let flags = last.flags;
            if flags.is_unclosed() {
                return None;
            }
            Some(Slot {
                at: u32::from(last.range().end() - flags.closer_len()),
                stmt_start,
                kind: SlotKind::InLiteral {
                    triple_quoted: flags.is_triple_quoted(),
                },
            })
        }
        None => Some(Slot {
            at: stmt_start,
            stmt_start,
            kind: SlotKind::NewStatement,
        }),
    }
}
