//! The outline of one file, and the qualname rule. The rule is frozen at the first tag: see `docs/qualnames.md`.

use analyzer_core::edit::MarkerStyle;
use analyzer_core::outline::{Def, DefKind, Outline, SyntaxError};

use crate::{ids, sem};

/// Python's `__qualname__` without the module: a def inside a **function** gets `<locals>` after that function
/// (`top.<locals>.inner`); inside a class or at module level it is joined with a dot. A block (`if`, `try`, `for`,
/// `while`, `with`, `match`) adds nothing.
fn qualname(defs: &[sem::RawDef], names: &[String], raw: &sem::RawDef) -> String {
    match raw.parent {
        None => raw.name.clone(),
        Some(p) if defs[p].is_class => format!("{}.{}", names[p], raw.name),
        Some(p) => format!("{}.<locals>.{}", names[p], raw.name),
    }
}

pub(crate) fn outline(text: &str, style: MarkerStyle<'_>) -> Outline {
    let module = sem::parse(text);
    let mut names: Vec<String> = Vec::with_capacity(module.defs.len());
    let mut out = Outline::default();
    for raw in &module.defs {
        let qualname = qualname(&module.defs, &names, raw);
        // a def directly in a class body is a method, even when an `if` or `try` sits between
        let kind = if raw.is_class {
            DefKind::Class
        } else if raw.parent.is_some_and(|p| module.defs[p].is_class) {
            DefKind::Method
        } else {
            DefKind::Function
        };
        let mut def = Def::new(qualname.clone(), &raw.name, kind, raw.range, raw.name_range);
        def.parent = raw.parent;
        def.params = raw.params.clone();
        def.marker = raw
            .docstring
            .as_deref()
            .and_then(|doc| ids::read(doc, style.label));
        def.syntax_error = raw.syntax_error;
        names.push(qualname);
        out.defs.push(def);
    }
    out.syntax_errors = module
        .errors
        .into_iter()
        .map(|(range, message)| SyntaxError::new(range, message))
        .collect();
    out
}
