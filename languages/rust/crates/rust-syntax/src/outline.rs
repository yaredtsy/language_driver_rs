//! The outline: which items are defs, and each one's qualname. The rules are `docs/04-rust/04-default-resolver/02-rs1-outline.md` §2–§3 and,
//! once Rust is tagged, `languages/rust/docs/qualnames.md`. **Qualnames are stored in users' files: never change how an existing kind of
//! def is named.**

use analyzer_core::edit::MarkerStyle;
use analyzer_core::outline::{Def, DefKind, Outline};

use crate::Edition;
use crate::sem::{self, ImplHead, Item, ItemKind};

/// `_style` is read from RS2 on (the marker of each def).
pub(crate) fn outline(text: &str, edition: Edition, _style: MarkerStyle<'_>) -> Outline {
    let mut outline = Outline::default();
    for item in sem::items(text, edition) {
        // `sem` lists a parent before its children, so the parent's def is already there
        let parent = item.parent.map(|p| &outline.defs[p]);
        let name = defined_name(&item);
        let qualname = [
            parent.map(|p| p.qualname.clone()),
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
        outline.defs.push(def);
    }
    outline
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
