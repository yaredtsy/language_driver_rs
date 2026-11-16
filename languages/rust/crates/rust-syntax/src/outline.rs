//! The outline: which items are defs, and each one's qualname. The rules are `docs/04-rust/04-default-resolver/02-rs1-outline.md` §2–§3 and,
//! once Rust is tagged, `languages/rust/docs/qualnames.md`. **Qualnames are stored in users' files: never change how an existing kind of
//! def is named.**

use analyzer_core::edit::MarkerStyle;
use analyzer_core::outline::{Def, DefKind, Outline};

use crate::Edition;
use crate::sem::{self, Item, ItemKind};

/// `_style` is read from RS2 on (the marker of each def).
pub(crate) fn outline(text: &str, edition: Edition, _style: MarkerStyle<'_>) -> Outline {
    let mut outline = Outline::default();
    outline.defs = sem::top_level_items(text, edition)
        .iter()
        .map(def)
        .collect();
    outline
}

fn def(item: &Item) -> Def {
    let kind = match item.kind {
        ItemKind::Function => DefKind::Function,
    };
    let mut def = Def::new(&item.name, &item.name, kind, item.range, item.name_range);
    def.params = item.params.clone();
    def
}
