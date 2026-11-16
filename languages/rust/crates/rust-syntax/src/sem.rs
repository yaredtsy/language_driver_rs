//! The only file that imports the engine (`ra_ap_syntax`). Everything else asks this module and gets owned, engine-free data.
//!
//! rust-analyzer's trees are ordinary reference-counted values, so nothing here is about memory safety; it is rule 4, and it keeps an
//! engine bump inside one file. The facts about the tree that this file relies on were read from the published source and run in RS0
//! (`languages/rust/ENGINE.md`, S-RS4).

use analyzer_core::text::Span;
use ra_ap_syntax::{
    AstNode, NodeOrToken, SourceFile, SyntaxKind, SyntaxNode, SyntaxToken, TextRange, WalkEvent,
    ast::{self, AttrKind, HasAttrs, HasModuleItem, HasName},
};

use crate::Edition;

/// What kind of item a def is. Grows with RS1's steps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ItemKind {
    Function,
    /// a function of a trait or of an impl, with or without a receiver
    Method,
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
    /// For an item of an `impl`: the header that names it. An `impl` is not a def, so it is not in the list; its items are, and this
    /// is what their qualname is built from (`docs/qualnames.md`, I1 to I12).
    pub(crate) impl_head: Option<ImplHead>,
    /// The item's own outer doc comments, in source order. An inner one (`//!`) belongs to the item around it, and `#[doc = "…"]` is an
    /// attribute: neither is here (RS2 §3).
    pub(crate) docs: Vec<DocComment>,
    /// The ranges of its outer `#[doc = …]` attributes, in source order. They are doc fragments for **placing** a marker (rustdoc joins them
    /// with the `///` lines) and are never read (RS2 §3, §4). `#[doc(hidden)]` and the other `doc(…)` lists are not here, nor is a
    /// `#[cfg_attr(…, doc = …)]`.
    pub(crate) doc_attrs: Vec<Span>,
}

/// One outer doc comment of an item, as written.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct DocComment {
    /// `/// a` or `/** b */`. A `\r` before a line break is still in the text.
    pub(crate) text: String,
    pub(crate) range: Span,
}

/// An impl header, read from its tokens only. `where` clauses, the impl's own generics and its keywords are not part of it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct ImplHead {
    pub(crate) self_ty: HeadType,
    pub(crate) trait_: Option<HeadType>,
}

/// A type of an impl header, printed as type text (`docs/qualnames.md`, P1 to P8).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct HeadType {
    /// A path is shortened to its last segment, arguments whole; any other type is printed whole.
    pub(crate) text: String,
    /// The written type is a path without a `<A as B>::` qualifier (I1, I8).
    pub(crate) is_path: bool,
}

fn engine_edition(edition: Edition) -> ra_ap_syntax::Edition {
    match edition {
        Edition::E2015 => ra_ap_syntax::Edition::Edition2015,
        Edition::E2018 => ra_ap_syntax::Edition::Edition2018,
        Edition::E2021 => ra_ap_syntax::Edition::Edition2021,
        Edition::E2024 => ra_ap_syntax::Edition::Edition2024,
    }
}

/// What one file holds: its items, and the parse errors the engine found in it.
pub(crate) struct Parsed {
    /// Every item that is a def, in source order, parents before children.
    pub(crate) items: Vec<Item>,
    /// A range inside the text and the engine's English message, in the order the engine reports them.
    pub(crate) errors: Vec<(Span, String)>,
}

pub(crate) fn parse(text: &str, edition: Edition) -> Parsed {
    // `parse` never fails: a file that does not parse still gives a tree (with error nodes), and `Parse::errors()` lists the problems.
    let parse = SourceFile::parse(text, engine_edition(edition));
    let ranges: Vec<TextRange> = parse.errors().iter().map(|e| e.range()).collect();
    let mut items = Vec::new();
    collect(parse.tree().items(), None, &ranges, &mut items);
    // an error at the end of the text can reach one byte past it; the outline promises ranges inside the text
    let len = u32::try_from(text.len()).unwrap_or(u32::MAX);
    let errors = parse
        .errors()
        .iter()
        .map(|e| {
            let range = span(e.range());
            (
                Span::new(range.start.min(len), range.end.min(len)),
                e.to_string(),
            )
        })
        .collect();
    Parsed { items, errors }
}

fn collect(
    items: impl Iterator<Item = ast::Item>,
    parent: Option<usize>,
    errors: &[TextRange],
    out: &mut Vec<Item>,
) {
    for item in items {
        match item {
            ast::Item::Fn(f) => push_function(&f, ItemKind::Function, parent, None, errors, out),
            ast::Item::Struct(s) => out.extend(named(&s, ItemKind::Struct, parent)),
            ast::Item::Union(u) => out.extend(named(&u, ItemKind::Struct, parent)),
            ast::Item::Enum(e) => out.extend(named(&e, ItemKind::Enum, parent)),
            ast::Item::Trait(t) => {
                if let Some(def) = named(&t, ItemKind::Trait, parent) {
                    out.push(def);
                    let this = Some(out.len() - 1);
                    collect_assoc(t.assoc_item_list(), this, None, errors, out);
                }
            }
            // an impl is no def: it names its items, and an impl whose header cannot be read has nothing to name them from (I10)
            ast::Item::Impl(i) => {
                if let Some(head) = impl_head(&i, errors) {
                    collect_assoc(i.assoc_item_list(), parent, Some(head), errors, out);
                }
            }
            ast::Item::TypeAlias(t) => out.extend(named(&t, ItemKind::Type, parent)),
            ast::Item::Const(c) => push_const(&c, parent, None, errors, out),
            ast::Item::Static(s) => {
                if let Some(def) = named(&s, ItemKind::Static, parent) {
                    out.push(def);
                    let this = Some(out.len() - 1);
                    collect_body(s.body().map(|e| e.syntax().clone()), this, errors, out);
                }
            }
            ast::Item::MacroRules(m) => out.extend(named(&m, ItemKind::Macro, parent)),
            // `mod m;` has no item list: it names a file, and the file is the def's home (RS1 §2)
            ast::Item::Module(m) => {
                if let (Some(list), Some(module)) =
                    (m.item_list(), named(&m, ItemKind::Module, parent))
                {
                    out.push(module);
                    let this = Some(out.len() - 1);
                    collect(list.items(), this, errors, out);
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
                        push_function(&f, ItemKind::Function, parent, None, errors, out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The items of a trait or of an impl. Only functions and consts are defs; an associated `type` and a macro call are not (D14, D15).
fn collect_assoc(
    list: Option<ast::AssocItemList>,
    parent: Option<usize>,
    head: Option<ImplHead>,
    errors: &[TextRange],
    out: &mut Vec<Item>,
) {
    for item in list.into_iter().flat_map(|l| l.assoc_items()) {
        match item {
            ast::AssocItem::Fn(f) => {
                push_function(&f, ItemKind::Method, parent, head.as_ref(), errors, out);
            }
            ast::AssocItem::Const(c) => push_const(&c, parent, head.as_ref(), errors, out),
            _ => {}
        }
    }
}

/// A function is a def, and the items of its body are named under it (B1, B4). A function the parser gave no name has nothing to name
/// them under (B10).
fn push_function(
    f: &ast::Fn,
    kind: ItemKind,
    parent: Option<usize>,
    head: Option<&ImplHead>,
    errors: &[TextRange],
    out: &mut Vec<Item>,
) {
    let Some(mut item) = function(f, kind, parent) else {
        return;
    };
    item.impl_head = head.cloned();
    out.push(item);
    let this = Some(out.len() - 1);
    collect_body(f.body().map(|b| b.syntax().clone()), this, errors, out);
}

/// A named const is a def and the nearest one to what its initializer holds (B6). `const _` is no def and adds nothing to the names
/// inside it: its items belong to the enclosing def (B7). A const with no name and no `_` is a parse error, and its body is not read.
fn push_const(
    c: &ast::Const,
    parent: Option<usize>,
    head: Option<&ImplHead>,
    errors: &[TextRange],
    out: &mut Vec<Item>,
) {
    let owner = match named(c, ItemKind::Const, parent) {
        Some(mut item) => {
            item.impl_head = head.cloned();
            out.push(item);
            Some(out.len() - 1)
        }
        None if c.underscore_token().is_some() => parent,
        None => return,
    };
    collect_body(c.body().map(|e| e.syntax().clone()), owner, errors, out);
}

/// The items written inside a body (a function's block, a const's initializer), through any closure, block or other expression: none
/// of those is a naming frame (B5). An item is handed to `collect`, which reads its own body; the walk does not go into it. Iterative, so
/// a deeply nested expression cannot overflow the stack.
fn collect_body(
    body: Option<SyntaxNode>,
    parent: Option<usize>,
    errors: &[TextRange],
    out: &mut Vec<Item>,
) {
    let Some(body) = body else { return };
    let mut walk = body.preorder();
    while let Some(event) = walk.next() {
        let WalkEvent::Enter(node) = event else {
            continue;
        };
        if let Some(item) = ast::Item::cast(node) {
            collect(std::iter::once(item), parent, errors, out);
            walk.skip_subtree();
        }
    }
}

/// `None` when the header cannot be read (I10): a parse error lies in it, or it has no self type, or a `for` with no type on one side.
/// The engine recovers from `impl for S` by reading `for S` as a higher-ranked type with no `<`, so a header with an error is not read
/// at all: a name made from a guess would be stored in a file.
fn impl_head(i: &ast::Impl, errors: &[TextRange]) -> Option<ImplHead> {
    let start = i.syntax().text_range().start();
    let end = i.assoc_item_list().map_or_else(
        || i.syntax().text_range().end(),
        |l| l.syntax().text_range().start(),
    );
    if errors
        .iter()
        .any(|e| start <= e.start() && e.start() <= end)
    {
        return None;
    }
    let self_ty = head_type(&i.self_ty()?);
    let trait_ = match (i.for_token(), i.trait_()) {
        (None, _) => None,
        (Some(_), Some(t)) => Some(head_type(&t)),
        (Some(_), None) => return None,
    };
    Some(ImplHead { self_ty, trait_ })
}

/// I1, I4, I6, I8: a path is its last segment, anything else (and a path with a `<A as B>::` qualifier) is the whole type.
fn head_type(ty: &ast::Type) -> HeadType {
    if let ast::Type::PathType(p) = ty
        && let Some(path) = p.path()
        && path
            .first_segment()
            .is_some_and(|s| s.type_anchor().is_none())
        && let Some(segment) = path.segment()
    {
        // a leading `::` belongs to the first segment: it is written, but the head is only the last segment's name and arguments
        let mut tokens = Vec::new();
        for child in segment.syntax().children_with_tokens() {
            match child {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::COLON2 => {}
                other => print(other, &mut tokens),
            }
        }
        return HeadType {
            text: join(&tokens),
            is_path: true,
        };
    }
    HeadType {
        text: type_text(ty.syntax()),
        is_path: false,
    }
}

/// An item that has a name and nothing else the outline reads. `None` when the parser could not find the name.
fn named<N: AstNode + HasName + HasAttrs>(
    node: &N,
    kind: ItemKind,
    parent: Option<usize>,
) -> Option<Item> {
    let name = node.name()?;
    Some(Item {
        kind,
        parent,
        name: plain_name(name.text()),
        range: span(item_range(node.syntax())),
        name_range: span(name.syntax().text_range()),
        params: Vec::new(),
        impl_head: None,
        doc_attrs: node
            .attrs()
            .filter(|attr| {
                attr.kind() == AttrKind::Outer
                    && matches!(attr.meta(), Some(ast::Meta::KeyValueMeta(_)))
                    && attr.simple_name().is_some_and(|name| name == "doc")
            })
            .map(|attr| span(attr.syntax().text_range()))
            .collect(),
        docs: node
            .doc_comments()
            .filter(|doc| doc.kind() == AttrKind::Outer)
            .map(|doc| DocComment {
                text: doc.text_with_markers().to_owned(),
                range: span(doc.syntax().text_range()),
            })
            .collect(),
    })
}

fn function(f: &ast::Fn, kind: ItemKind, parent: Option<usize>) -> Option<Item> {
    let mut item = named(f, kind, parent)?;
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

/// A token the printer kept, with what the spacing rules ask about it.
struct Tok {
    text: String,
    kind: SyntaxKind,
    /// the `"C"` of `extern "C"` (P7: a space follows it)
    abi: bool,
}

/// `docs/qualnames.md`, P1 to P8: the tokens of a type, with lifetimes and optional punctuation gone and one space where the rule puts one.
/// It reads the tokens and nothing else, so spacing, line breaks and comments never change the text.
fn type_text(node: &SyntaxNode) -> String {
    let mut tokens = Vec::new();
    print(NodeOrToken::Node(node.clone()), &mut tokens);
    join(&tokens)
}

fn print(element: NodeOrToken<SyntaxNode, SyntaxToken>, out: &mut Vec<Tok>) {
    match element {
        NodeOrToken::Token(token) => print_token(&token, out),
        NodeOrToken::Node(node) => match node.kind() {
            // P2: a lifetime, a lifetime argument and a `for<'a>` binder print nothing
            SyntaxKind::LIFETIME | SyntaxKind::LIFETIME_ARG | SyntaxKind::FOR_BINDER => {}
            SyntaxKind::GENERIC_ARG_LIST
            | SyntaxKind::PARENTHESIZED_ARG_LIST
            | SyntaxKind::TUPLE_TYPE
            | SyntaxKind::PARAM_LIST
            | SyntaxKind::TYPE_BOUND_LIST => print_list(&node, out),
            // everything else, a macro's token tree and a braced const argument included, is printed token by token (P8)
            _ => {
                for child in node.children_with_tokens() {
                    print(child, out);
                }
            }
        },
    }
}

fn print_token(token: &SyntaxToken, out: &mut Vec<Tok>) {
    let kind = token.kind();
    // P1
    if matches!(
        kind,
        SyntaxKind::WHITESPACE
            | SyntaxKind::COMMENT
            | SyntaxKind::OUTER_DOC_COMMENT
            | SyntaxKind::INNER_DOC_COMMENT
    ) {
        return;
    }
    out.push(Tok {
        // P6
        text: plain_name(token.text()),
        kind,
        abi: kind == SyntaxKind::STRING
            && token.parent().is_some_and(|p| p.kind() == SyntaxKind::ABI),
    });
}

/// A list: its items (the child nodes) are printed one after the other, and the `,` or `+` between them only where two items are left
/// (P2, P5). Empty angle brackets go (P3); parentheses never do.
fn print_list(list: &SyntaxNode, out: &mut Vec<Tok>) {
    let mut body = Vec::new();
    let mut items = 0;
    let mut separator = None;
    let mut comma_seen = false;
    for child in list.children_with_tokens() {
        match child {
            NodeOrToken::Node(node) => {
                let mut item = Vec::new();
                print(NodeOrToken::Node(node), &mut item);
                if item.is_empty() {
                    continue;
                }
                if items > 0 {
                    body.extend(separator.take());
                }
                separator = None;
                items += 1;
                body.append(&mut item);
            }
            NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::COMMA | SyntaxKind::PLUS => {
                    comma_seen |= token.kind() == SyntaxKind::COMMA;
                    let mut sep = Vec::new();
                    print_token(&token, &mut sep);
                    separator = sep.pop();
                }
                // P4: the `::` before `<` is optional
                SyntaxKind::COLON2 if list.kind() == SyntaxKind::GENERIC_ARG_LIST => {}
                kind => {
                    if matches!(
                        kind,
                        SyntaxKind::R_PAREN | SyntaxKind::R_BRACK | SyntaxKind::R_ANGLE
                    ) {
                        // P5: a trailing separator goes, but the comma of `(A,)` is what makes it a tuple
                        if list.kind() == SyntaxKind::TUPLE_TYPE && items == 1 && comma_seen {
                            body.extend(separator.take());
                        }
                        separator = None;
                    }
                    print_token(&token, &mut body);
                }
            },
        }
    }
    if list.kind() == SyntaxKind::GENERIC_ARG_LIST && items == 0 {
        return;
    }
    out.append(&mut body);
}

/// P7: one space after `,` `;` `:` and the keywords of the list, on both sides of `+` `=` `->` `as`, after an ABI string, and between two
/// word tokens that would otherwise touch. Never before a closing bracket, and nowhere else.
fn join(tokens: &[Tok]) -> String {
    let mut text = String::new();
    for (i, token) in tokens.iter().enumerate() {
        if i > 0 && space_between(&tokens[i - 1], token) {
            text.push(' ');
        }
        text.push_str(&token.text);
    }
    text
}

fn space_between(prev: &Tok, next: &Tok) -> bool {
    use SyntaxKind::*;
    let closing = matches!(next.kind, R_PAREN | R_BRACK | R_ANGLE | R_CURLY);
    let both_sides = |k| matches!(k, PLUS | EQ | THIN_ARROW | AS_KW);
    let after = matches!(
        prev.kind,
        COMMA | SEMICOLON | COLON | DYN_KW | IMPL_KW | MUT_KW | CONST_KW | UNSAFE_KW | EXTERN_KW
    ) || prev.abi;
    !closing
        && (after
            || both_sides(prev.kind)
            || both_sides(next.kind)
            || (is_word(prev) && is_word(next)))
}

/// An identifier, a keyword or a literal: what runs into the next word if no space is put between them.
fn is_word(token: &Tok) -> bool {
    token
        .text
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '"' | '\''))
}

fn span(range: TextRange) -> Span {
    Span::new(range.start().into(), range.end().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The type text of one type, written as the right side of a `type` alias.
    fn text_of(written: &str) -> String {
        let file = SourceFile::parse(
            &format!("type X = {written};"),
            ra_ap_syntax::Edition::Edition2024,
        )
        .tree();
        let alias = file
            .items()
            .find_map(|i| match i {
                ast::Item::TypeAlias(t) => Some(t),
                _ => None,
            })
            .expect("a type alias");
        type_text(alias.ty().expect("a type").syntax())
    }

    /// Every row of `docs/qualnames.md`'s printing rule (P1 to P8).
    #[test]
    fn the_printing_rule() {
        let rows = [
            ("S< u8 >", "S<u8>"),
            ("S</* why */ u8>", "S<u8>"),
            ("Cow<'a, str>", "Cow<str>"),
            ("S<T, 'a>", "S<T>"),
            ("S<'a>", "S"),
            ("S<'_>", "S"),
            ("&'a mut T", "&mut T"),
            ("dyn Tr + 'static", "dyn Tr"),
            ("dyn 'a + Tr + Send", "dyn Tr + Send"),
            ("for<'a> fn(&'a str) -> &'a str", "fn(&str) -> &str"),
            ("S<>", "S"),
            ("()", "()"),
            ("fn()", "fn()"),
            ("Box<dyn Fn()>", "Box<dyn Fn()>"),
            ("Vec::<u8>", "Vec<u8>"),
            ("HashMap<K,V,>", "HashMap<K, V>"),
            ("(A, B,)", "(A, B)"),
            ("(A,)", "(A,)"),
            ("fn(A,)", "fn(A)"),
            ("r#type", "type"),
            ("&'a mut [u8 ;4]", "&mut [u8; 4]"),
            (
                "Box<dyn Fn(u8)->u8+Send+'static>",
                "Box<dyn Fn(u8) -> u8 + Send>",
            ),
            ("dyn Iterator<Item=u8>", "dyn Iterator<Item = u8>"),
            ("<T as Tr>::Out", "<T as Tr>::Out"),
            ("fn(a:u8)", "fn(a: u8)"),
            ("a :: B", "a::B"),
            ("*const T", "*const T"),
            ("unsafe extern \"C\" fn(i32)", "unsafe extern \"C\" fn(i32)"),
            ("S<{ N+1 }>", "S<{N + 1}>"),
            ("S<{N + 1}>", "S<{N + 1}>"),
            ("m!( a  b )", "m!(a b)"),
        ];
        for (written, printed) in rows {
            assert_eq!(text_of(written), printed, "written: {written}");
        }
    }

    /// The walk of a body is iterative: a body nested far deeper than any real one is read, and the item at the bottom is found.
    #[test]
    fn a_deeply_nested_body_is_read() {
        let depth = 400;
        let text = format!(
            "fn outer() {{ {}fn bottom() {{}}{} }}",
            "{ ".repeat(depth),
            " }".repeat(depth)
        );
        let names: Vec<String> = parse(&text, Edition::E2024)
            .items
            .into_iter()
            .map(|i| i.name)
            .collect();
        assert_eq!(names, ["outer", "bottom"]);
    }

    fn heads(text: &str) -> Vec<ImplHead> {
        parse(text, Edition::E2024)
            .items
            .into_iter()
            .filter_map(|i| i.impl_head)
            .collect()
    }

    /// The name of an impl's items depends on the header's tokens only: spacing, line breaks, comments, lifetimes, the impl's own
    /// generics' bounds and the `where` clause change nothing, and the same holds inside a braced const argument and a macro (P8).
    #[test]
    fn a_header_is_read_from_its_tokens_only() {
        let clean = [
            "impl<'a, T: Clone> From<&'a [T]> for Wrapper<'a, T> where T: Copy { fn m() {} }",
            "impl<T> Tr<{ N + 1 }, m!(a b)> for S<T> { fn m() {} }",
            "impl Tr<Box<dyn Fn(u8) -> u8 + Send>> for a::S<dyn Iterator<Item = u8>> { fn m() {} }",
        ];
        let noisy = [
            "impl <'x,T:Clone+Copy>   From  <  & 'x  /* why */ [T] , >   for  Wrapper\n  < 'x , T >\n where T : Send\n{ fn m() {} }",
            "impl<T>\nTr<{N+1},\n // c\n m!(  a\n b )>\n for\nS < T , >\n{ fn m() {} }",
            "impl Tr< Box<dyn 'static + Fn(u8)->u8 + Send + 'static> > for a :: S < dyn Iterator < Item=u8 , > > { fn m() {} }",
        ];
        for (clean, noisy) in clean.into_iter().zip(noisy) {
            assert_eq!(heads(clean), heads(noisy), "clean: {clean}");
            assert_eq!(heads(clean).len(), 1);
        }
    }
}
