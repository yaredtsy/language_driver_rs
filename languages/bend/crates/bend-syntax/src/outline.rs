//! The outline of a Bend file (docs/08-bend/03 §1): a flat list of defs, in source order, none nested (Bend has no nesting, so `parent` is always `None`).

use std::collections::HashMap;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::outline::{Def, DefKind, Outline, SyntaxError};
use analyzer_core::text::Span;

use crate::sem::{self, DeclKind};
use crate::{header, marker};

/// What a declaration is to us, in one place (BD-D3, BD-D4; Q-BD2 may change the law's before the tag). A `type` is an `Enum`; its constructors are not
/// defs. A `law` is `Other("law")`. A `def` is a `Function`, and so is the def that fills a law: it has the law's name, and the two share a qualname.
fn def_kind(kind: DeclKind) -> DefKind {
    match kind {
        DeclKind::Type => DefKind::Enum,
        DeclKind::Def => DefKind::Function,
        DeclKind::Law => DefKind::Other("law".to_string()),
    }
}

/// The qualname, in one place (BD-D6): the declared name as written, dots included, no path, no joiner. The `?` of `def f?(…)` and the `@unsafe` are not
/// part of it, and an alias is part of it only because the user wrote it (`def M.name(x)` is `M.name`).
fn qualname(decl: &sem::Decl) -> String {
    decl.name.clone()
}

/// The defs of one file, in source order, with their markers and the file's own.
pub(crate) fn outline(text: &str, style: MarkerStyle<'_>) -> Outline {
    outline_with_parsed(text, style).0
}

/// The outline and what it was made from: `parsed.decls[i]` is `outline.defs[i]`, so a planner can reach the declaration's own first token, which the
/// def's range (it starts at the doc block above) does not say.
pub(crate) fn outline_with_parsed(text: &str, style: MarkerStyle<'_>) -> (Outline, sem::Parsed) {
    let parsed = sem::parse(text);
    let mut outline = Outline::default();
    for decl in &parsed.decls {
        // the range starts at the comment block directly above, or at the declaration's first token, whichever comes first
        let start = decl
            .doc
            .map_or(decl.span.start, |doc| doc.start.min(decl.span.start));
        let mut def = Def::new(
            qualname(decl),
            decl.name.clone(),
            def_kind(decl.kind),
            Span::new(start, decl.span.end),
            decl.name_span,
        );
        def.params.clone_from(&decl.params);
        def.syntax_error = decl.syntax_error;
        def.marker = marker::read(text, decl.doc, &parsed.comments, style);
        outline.defs.push(def);
    }
    outline.file_marker = header::read(text, &parsed, style);
    outline.syntax_errors = parsed
        .errors
        .iter()
        .map(|e| SyntaxError::new(e.span, e.message.clone()))
        .collect();
    (outline, parsed)
}

/// For each def of `outline`, how many defs before it have the same qualname: 0 for the first, 1 for the next (D18). A qualname is not unique in a file:
/// a law and the def that proves it share one (BD-D4), and (qualname, occurrence) is.
pub(crate) fn occurrences(outline: &Outline) -> Vec<u32> {
    // only ever looked up, never iterated (rule 9)
    let mut seen: HashMap<&str, u32> = HashMap::new();
    outline
        .defs
        .iter()
        .map(|def| {
            let count = seen.entry(def.qualname.as_str()).or_insert(0);
            let occurrence = *count;
            *count = count.saturating_add(1);
            occurrence
        })
        .collect()
}
