//! The outline: which items are defs, and each one's qualname. The rules are `docs/04-rust/04-default-resolver/02-rs1-outline.md` §2–§3 and,
//! once Rust is tagged, `languages/rust/docs/qualnames.md`. **Qualnames are stored in users' files: never change how an existing kind of
//! def is named.**

use std::collections::HashMap;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::outline::{Def, DefKind, Outline, SyntaxError};
use analyzer_core::text::Span;

use crate::Edition;
use crate::sem::{self, ImplHead, Item, ItemKind};

/// `_style` is read from RS2 on (the marker of each def).
pub(crate) fn outline(text: &str, edition: Edition, _style: MarkerStyle<'_>) -> Outline {
    let mut outline = Outline::default();
    let parsed = sem::parse(text, edition);
    for item in parsed.items {
        // `sem` lists a parent before its children, so the parent's def is already there
        let parent = item.parent.map(|p| &outline.defs[p]);
        let name = defined_name(&item);
        // what stands in a function's body is named after the function, then `{fn}` (B1); `{fn}` cannot be an identifier
        let in_body = parent.is_some_and(|p| matches!(p.kind, DefKind::Function | DefKind::Method));
        let qualname = [
            parent.map(|p| p.qualname.clone()),
            in_body.then(|| "{fn}".to_owned()),
            item.impl_head.as_ref().map(impl_prefix),
            Some(name.clone()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("::");
        let mut def = Def::new(qualname, name, kind(&item), item.range, item.name_range);
        def.parent = item.parent;
        def.params = item.params;
        def.syntax_error = parsed
            .errors
            .iter()
            .any(|(error, _)| overlaps(*error, def.range));
        outline.defs.push(def);
    }
    outline.syntax_errors = parsed
        .errors
        .into_iter()
        .map(|(range, message)| SyntaxError::new(range, message))
        .collect();
    outline
}

/// For each def of `outline`, how many defs before it have the same qualname: 0 for the first, 1 for the next (D18). A qualname is not
/// unique in a file (D15: two `impl S` blocks, `#[cfg]` twins, a const and a same-spelled module), and (qualname, occurrence) is.
///
/// `outline.defs[i]` is the `occurrences(&outline)[i]`th def of its name, in source order. The count is `u16` as in the graph's
/// `DefId`; it stops at `u16::MAX`, a file with 65536 defs of one name being no file anyone wrote.
pub(crate) fn occurrences(outline: &Outline) -> Vec<u16> {
    // only ever looked up, never iterated (rule 9)
    let mut seen: HashMap<&str, u16> = HashMap::new();
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

/// Does a parse error touch a def (RS1 §5)? "When unsure, flag more." An error with a range overlaps the def as ranges do. An **empty**
/// error is a missing token, and the engine puts it right after the last token read, so a def that is never closed (`mod m {`,
/// `fn f() {`) has the missing `}` at its own last byte: an empty error counts from the def's first byte to its last, both included.
/// The cost, accepted: a complete def that ends exactly there (the last item of an unclosed module) is flagged with it.
fn overlaps(error: Span, def: Span) -> bool {
    if error.start == error.end {
        def.start <= error.start && error.start <= def.end
    } else {
        error.start < def.end && error.end > def.start
    }
}

/// The naming context an impl gives its items (`docs/qualnames.md`, I1 to I9): `S`, `<S as T>`, or for a type that is not a path `<[T]>`.
fn impl_prefix(head: &ImplHead) -> String {
    match &head.trait_ {
        Some(t) => format!("<{} as {}>", head.self_ty.text, t.text),
        None if head.self_ty.is_path => head.self_ty.text.clone(),
        None => format!("<{}>", head.self_ty.text),
    }
}

/// The name as it is called: a macro ends in `!`, and `m!` is its whole name (RS1 §3).
fn defined_name(item: &Item) -> String {
    match item.kind {
        ItemKind::Macro => format!("{}!", item.name),
        _ => item.name.clone(),
    }
}

fn kind(item: &Item) -> DefKind {
    match item.kind {
        ItemKind::Function => DefKind::Function,
        ItemKind::Method => DefKind::Method,
        ItemKind::Struct => DefKind::Struct,
        ItemKind::Enum => DefKind::Enum,
        ItemKind::Trait => DefKind::Trait,
        ItemKind::Module => DefKind::Module,
        ItemKind::Type => DefKind::Type,
        ItemKind::Const => DefKind::Other("const".to_owned()),
        ItemKind::Static => DefKind::Other("static".to_owned()),
        ItemKind::Macro => DefKind::Other("macro".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flagged(text: &str) -> Vec<(String, bool)> {
        outline(text, Edition::E2024, MarkerStyle::docstring("ID"))
            .defs
            .into_iter()
            .map(|d| (d.qualname, d.syntax_error))
            .collect()
    }

    #[test]
    fn occurrence_counts_the_defs_of_a_name_before_it() {
        let text = "fn a() {}\nfn b() {}\nfn a() {}\nmod m { fn a() {} }\nfn a() {}";
        let o = outline(text, Edition::E2024, MarkerStyle::docstring("ID"));
        let names: Vec<_> = o
            .defs
            .iter()
            .map(|d| d.qualname.as_str())
            .zip(occurrences(&o))
            .collect();
        assert_eq!(
            names,
            [
                ("a", 0),
                ("b", 0),
                ("a", 1),
                ("m", 0),
                ("m::a", 0),
                ("a", 2)
            ]
        );
    }

    #[test]
    fn an_unclosed_def_is_flagged_and_a_complete_one_before_it_is_not() {
        assert_eq!(
            flagged("fn a() {}\nfn b() {\n"),
            [("a".to_owned(), false), ("b".to_owned(), true)]
        );
        assert_eq!(flagged("fn b() {"), [("b".to_owned(), true)]);
        // the unclosed module is flagged; so is the last def in it, which ends where the `}` was expected (accepted)
        assert_eq!(
            flagged("mod m {\n    fn inner() {}\n"),
            [("m".to_owned(), true), ("m::inner".to_owned(), true)]
        );
    }

    #[test]
    fn an_error_after_a_def_is_the_next_items() {
        // `struct` has no name: the errors are at byte 16, long after `a` (0..9)
        assert_eq!(flagged("fn a() {}\nstruct\n"), [("a".to_owned(), false)]);
    }

    #[test]
    fn what_counts_as_touching() {
        let def = Span::new(5, 9);
        // empty: first byte and last byte are in, one before and one after are out
        assert!(overlaps(Span::new(5, 5), def));
        assert!(overlaps(Span::new(9, 9), def));
        assert!(!overlaps(Span::new(4, 4), def));
        assert!(!overlaps(Span::new(10, 10), def));
        // with a range: half open, so one that starts where the def ends is the next item's, one that ends where it starts is the previous
        assert!(overlaps(Span::new(8, 12), def));
        assert!(!overlaps(Span::new(9, 12), def));
        assert!(!overlaps(Span::new(2, 5), def));
    }
}
