//! The only file that imports the engine (`ra_ap_syntax`). Everything else asks this module and gets owned, engine-free data.
//!
//! rust-analyzer's trees are ordinary reference-counted values, so nothing here is about memory safety; it is rule 4, and it keeps an
//! engine bump inside one file. The facts about the tree that this file relies on were read from the published source and run in RS0
//! (`languages/rust/ENGINE.md`, S-RS4).

use analyzer_core::text::Span;
use ra_ap_syntax::{
    AstNode, SourceFile, SyntaxKind, SyntaxNode, TextRange,
    ast::{self, HasModuleItem, HasName},
};

use crate::Edition;

/// What kind of item a def is. Grows with RS1's steps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ItemKind {
    Function,
}

/// One item of the file, as plain data.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Item {
    pub(crate) kind: ItemKind,
    pub(crate) name: String,
    pub(crate) range: Span,
    pub(crate) name_range: Span,
    pub(crate) params: Vec<String>,
}

fn engine_edition(edition: Edition) -> ra_ap_syntax::Edition {
    match edition {
        Edition::E2015 => ra_ap_syntax::Edition::Edition2015,
        Edition::E2018 => ra_ap_syntax::Edition::Edition2018,
        Edition::E2021 => ra_ap_syntax::Edition::Edition2021,
        Edition::E2024 => ra_ap_syntax::Edition::Edition2024,
    }
}

/// The items written at the top level of the file, in source order.
pub(crate) fn top_level_items(text: &str, edition: Edition) -> Vec<Item> {
    // `parse` never fails: a file that does not parse still gives a tree (with error nodes), and `Parse::errors()` lists the problems.
    let file = SourceFile::parse(text, engine_edition(edition)).tree();
    file.items()
        .filter_map(|item| match item {
            ast::Item::Fn(f) => function(&f),
            _ => None,
        })
        .collect()
}

fn function(f: &ast::Fn) -> Option<Item> {
    // a function the parser could not finish has no name: there is nothing to name it by
    let name = f.name()?;
    let mut params = Vec::new();
    if let Some(list) = f.param_list() {
        if let Some(receiver) = list.self_param() {
            params.push(receiver_text(&receiver));
        }
        params.extend(
            list.params()
                .filter_map(|p| p.pat())
                .map(|pat| pat.syntax().text().to_string()),
        );
    }
    Some(Item {
        kind: ItemKind::Function,
        name: plain_name(name.text()),
        range: span(item_range(f.syntax())),
        name_range: span(name.syntax().text_range()),
        params,
    })
}

/// The receiver as the three forms the plan names: `self`, `&self`, `&mut self`. A lifetime, `mut` on a by-value receiver and an
/// explicit type (`self: Box<Self>`) are spelling, not a different receiver for a reader of the outline.
fn receiver_text(receiver: &ast::SelfParam) -> String {
    if receiver.amp_token().is_none() {
        "self".to_owned()
    } else if receiver.mut_token().is_some() {
        "&mut self".to_owned()
    } else {
        "&self".to_owned()
    }
}

/// `r#type` is the name `type`: `r#` is spelling (RS1 §3, last row).
fn plain_name(written: &str) -> String {
    written.strip_prefix("r#").unwrap_or(written).to_owned()
}

/// RS1 §5: from the first doc comment or attribute, else from the first token that is not a comment or whitespace, to the node's end.
///
/// **Never `node.text_range().start()`**: the engine puts the plain comments directly above an item, and a trailing `// c` from the
/// previous item's line, at the start of the item's node (S-RS4). A doc comment is a `DOC_COMMENT` node and an attribute an `ATTR`
/// node, both children of the item, in source order.
fn item_range(node: &SyntaxNode) -> TextRange {
    let start = node
        .children_with_tokens()
        .find(|c| {
            let kind = c.kind();
            matches!(kind, SyntaxKind::DOC_COMMENT | SyntaxKind::ATTR)
                || !matches!(kind, SyntaxKind::COMMENT | SyntaxKind::WHITESPACE)
        })
        .map_or_else(|| node.text_range().start(), |c| c.text_range().start());
    TextRange::new(start, node.text_range().end())
}

fn span(range: TextRange) -> Span {
    Span::new(range.start().into(), range.end().into())
}
