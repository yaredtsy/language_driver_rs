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
    /// `struct` and `union`
    Struct,
    Enum,
    Trait,
    /// an inline `mod m { }`
    Module,
    /// `type A = …`
    Type,
    Const,
    Static,
    /// `macro_rules! m`
    Macro,
}

/// One item of the file, as plain data.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Item {
    pub(crate) kind: ItemKind,
    /// Index of the enclosing item in the same list. A parent always comes before its children.
    pub(crate) parent: Option<usize>,
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

/// Every item of the file that is a def, in source order, parents before children.
pub(crate) fn items(text: &str, edition: Edition) -> Vec<Item> {
    // `parse` never fails: a file that does not parse still gives a tree (with error nodes), and `Parse::errors()` lists the problems.
    let file = SourceFile::parse(text, engine_edition(edition)).tree();
    let mut out = Vec::new();
    collect(file.items(), None, &mut out);
    out
}

fn collect(items: impl Iterator<Item = ast::Item>, parent: Option<usize>, out: &mut Vec<Item>) {
    for item in items {
        match item {
            ast::Item::Fn(f) => out.extend(function(&f, parent)),
            ast::Item::Struct(s) => out.extend(named(&s, ItemKind::Struct, parent)),
            ast::Item::Union(u) => out.extend(named(&u, ItemKind::Struct, parent)),
            ast::Item::Enum(e) => out.extend(named(&e, ItemKind::Enum, parent)),
            ast::Item::Trait(t) => out.extend(named(&t, ItemKind::Trait, parent)),
            ast::Item::TypeAlias(t) => out.extend(named(&t, ItemKind::Type, parent)),
            // `const _: () = …` has no name (its `_` is a separate token), so it is no def (RS1 §2)
            ast::Item::Const(c) => out.extend(named(&c, ItemKind::Const, parent)),
            ast::Item::Static(s) => out.extend(named(&s, ItemKind::Static, parent)),
            ast::Item::MacroRules(m) => out.extend(named(&m, ItemKind::Macro, parent)),
            // `mod m;` has no item list: it names a file, and the file is the def's home (RS1 §2)
            ast::Item::Module(m) => {
                if let (Some(list), Some(module)) =
                    (m.item_list(), named(&m, ItemKind::Module, parent))
                {
                    out.push(module);
                    let this = Some(out.len() - 1);
                    collect(list.items(), this, out);
                }
            }
            // an `extern` block is not a def and adds nothing to a name: its functions belong to the enclosing module
            ast::Item::ExternBlock(block) => {
                for inner in block
                    .extern_item_list()
                    .into_iter()
                    .flat_map(|l| l.extern_items())
                {
                    if let ast::ExternItem::Fn(f) = inner {
                        out.extend(function(&f, parent));
                    }
                }
            }
            _ => {}
        }
    }
}

/// An item that has a name and nothing else the outline reads. `None` when the parser could not find the name.
fn named<N: AstNode + HasName>(node: &N, kind: ItemKind, parent: Option<usize>) -> Option<Item> {
    let name = node.name()?;
    Some(Item {
        kind,
        parent,
        name: plain_name(name.text()),
        range: span(item_range(node.syntax())),
        name_range: span(name.syntax().text_range()),
        params: Vec::new(),
    })
}

fn function(f: &ast::Fn, parent: Option<usize>) -> Option<Item> {
    let mut item = named(f, ItemKind::Function, parent)?;
    if let Some(list) = f.param_list() {
        if let Some(receiver) = list.self_param() {
            item.params.push(receiver_text(&receiver));
        }
        item.params.extend(
            list.params()
                .filter_map(|p| p.pat())
                .map(|pat| pat.syntax().text().to_string()),
        );
    }
    Some(item)
}

/// The receiver as the four forms of RS1 §5: `self`, `mut self`, `&self`, `&mut self`. A typed receiver (`self: Box<Self>`,
/// `mut self: Rc<Self>`) prints `self`, because its type is not a pattern of the outline; a lifetime (`&'a self`) is dropped, as in
/// every other name (§3.1).
fn receiver_text(receiver: &ast::SelfParam) -> String {
    let mutable = receiver.mut_token().is_some();
    if receiver.ty().is_some() {
        "self".to_owned()
    } else if receiver.amp_token().is_some() {
        if mutable { "&mut self" } else { "&self" }.to_owned()
    } else if mutable {
        "mut self".to_owned()
    } else {
        "self".to_owned()
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
