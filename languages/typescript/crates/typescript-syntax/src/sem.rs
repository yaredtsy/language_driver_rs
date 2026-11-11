//! The only file that imports the engine (`tsrs_*`). Everything else asks `Parsed` and gets plain data or a `RawNode`.
//!
//! tsrs keeps its tree in arenas and hands out `'static` pointers (`P<Node>`). The compiler cannot see when the arena is freed,
//! so a pointer must never outlive `with_parsed`. Two things keep that true: `RawNode`'s field is private to this file, and
//! `Parsed` is neither `Send` nor `Sync` and only lends itself to the closure.

use std::marker::PhantomData;

use analyzer_core::text::Span;
use tsrs_ast::{Kind, ModifierFlags, Node, NodeFlags, SourceFile, SourceFileParseOptions};
use tsrs_core::{P, ScriptKind, tspath};

use crate::Dialect;

/// First chunk of the region a parse lives in. Measured in S-TS2 (`ENGINE.md`): 64 KB held a 2,000-line file at 0.4 ms.
const CHUNK: usize = 64 * 1024;

/// A syntax node, valid only inside the `with_parsed` call that made it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawNode(P<Node>);

/// What the outline asks about a node. Everything else is `Other`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NodeKind {
    SourceFile,
    FunctionDeclaration,
    FunctionExpression,
    ArrowFunction,
    ClassDeclaration,
    ClassExpression,
    MethodDeclaration,
    Constructor,
    GetAccessor,
    SetAccessor,
    PropertyDeclaration,
    ClassStaticBlock,
    VariableStatement,
    VariableDeclarationList,
    VariableDeclaration,
    ObjectLiteral,
    PropertyAssignment,
    InterfaceDeclaration,
    MethodSignature,
    EnumDeclaration,
    TypeAliasDeclaration,
    ModuleDeclaration,
    ModuleBlock,
    ExportAssignment,
    Identifier,
    PrivateIdentifier,
    StringLiteral,
    NumericLiteral,
    ComputedPropertyName,
    Other,
}

/// The modifiers the outline reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Modifiers {
    pub(crate) export: bool,
    pub(crate) default: bool,
    pub(crate) declare: bool,
}

/// A file parsed in a region. Lives only inside `with_parsed`.
pub(crate) struct Parsed<'a> {
    file: P<SourceFile>,
    text: &'a str,
    /// `*const ()` is neither `Send` nor `Sync`: a parse belongs to the thread (and region) that made it.
    _not_send: PhantomData<*const ()>,
}

fn script_kind(dialect: Dialect) -> ScriptKind {
    match dialect {
        Dialect::Ts => ScriptKind::TS,
        Dialect::Tsx => ScriptKind::TSX,
        Dialect::Js => ScriptKind::JS,
        Dialect::Jsx => ScriptKind::JSX,
    }
}

fn file_name(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::Ts => "/file.ts",
        Dialect::Tsx => "/file.tsx",
        Dialect::Js => "/file.js",
        Dialect::Jsx => "/file.jsx",
    }
}

/// Parse `text` in a region of its own, lend the tree to `f`, and free everything when `f` returns.
///
/// `R` should be owned data (`String`, `Vec`, `Outline`). A tsrs pointer inside `R` would dangle: the types cannot forbid it,
/// so only `RawNode` (whose field is private here) and the memory-loop test in `tests/` stand in the way.
pub(crate) fn with_parsed<R>(text: &str, dialect: Dialect, f: impl FnOnce(&Parsed<'_>) -> R) -> R {
    let region = tsrs_core::arena::Region::new_scratch(CHUNK);
    let _scope = region.enter();
    let name = file_name(dialect);
    let opts = SourceFileParseOptions {
        file_name: name.to_string(),
        path: tspath::to_path(name, "/", true),
        external_module_indicator_options: Default::default(),
    };
    // `_owned`, not `parse_source_file`: inside a region it clears the engine's global text-table slot when the region is freed
    // (ENGINE.md, S-TS2).
    let file = tsrs_parser::parse_source_file_owned(opts, text.to_string(), script_kind(dialect));
    f(&Parsed {
        file,
        text,
        _not_send: PhantomData,
    })
}

fn map_kind(kind: Kind) -> NodeKind {
    match kind {
        Kind::SourceFile => NodeKind::SourceFile,
        Kind::FunctionDeclaration => NodeKind::FunctionDeclaration,
        Kind::FunctionExpression => NodeKind::FunctionExpression,
        Kind::ArrowFunction => NodeKind::ArrowFunction,
        Kind::ClassDeclaration => NodeKind::ClassDeclaration,
        Kind::ClassExpression => NodeKind::ClassExpression,
        Kind::MethodDeclaration => NodeKind::MethodDeclaration,
        Kind::Constructor => NodeKind::Constructor,
        Kind::GetAccessor => NodeKind::GetAccessor,
        Kind::SetAccessor => NodeKind::SetAccessor,
        Kind::PropertyDeclaration => NodeKind::PropertyDeclaration,
        Kind::ClassStaticBlockDeclaration => NodeKind::ClassStaticBlock,
        Kind::VariableStatement => NodeKind::VariableStatement,
        Kind::VariableDeclarationList => NodeKind::VariableDeclarationList,
        Kind::VariableDeclaration => NodeKind::VariableDeclaration,
        Kind::ObjectLiteralExpression => NodeKind::ObjectLiteral,
        Kind::PropertyAssignment => NodeKind::PropertyAssignment,
        Kind::InterfaceDeclaration => NodeKind::InterfaceDeclaration,
        Kind::MethodSignature => NodeKind::MethodSignature,
        Kind::EnumDeclaration => NodeKind::EnumDeclaration,
        Kind::TypeAliasDeclaration => NodeKind::TypeAliasDeclaration,
        Kind::ModuleDeclaration => NodeKind::ModuleDeclaration,
        Kind::ModuleBlock => NodeKind::ModuleBlock,
        Kind::ExportAssignment => NodeKind::ExportAssignment,
        Kind::Identifier => NodeKind::Identifier,
        Kind::PrivateIdentifier => NodeKind::PrivateIdentifier,
        Kind::StringLiteral => NodeKind::StringLiteral,
        Kind::NumericLiteral => NodeKind::NumericLiteral,
        Kind::ComputedPropertyName => NodeKind::ComputedPropertyName,
        _ => NodeKind::Other,
    }
}

impl Parsed<'_> {
    pub(crate) fn root(&self) -> RawNode {
        RawNode(self.file.as_node())
    }

    pub(crate) fn kind(&self, n: RawNode) -> NodeKind {
        map_kind(n.0.kind())
    }

    /// The children of `n` in source order.
    pub(crate) fn children(&self, n: RawNode) -> Vec<RawNode> {
        let mut out = Vec::new();
        n.0.for_each_child(&mut |c| {
            out.push(RawNode(c));
            false
        });
        out
    }

    /// The name node of a declaration, when it has one.
    pub(crate) fn name(&self, n: RawNode) -> Option<RawNode> {
        n.0.name().map(RawNode)
    }

    pub(crate) fn initializer(&self, n: RawNode) -> Option<RawNode> {
        n.0.initializer().map(RawNode)
    }

    pub(crate) fn body(&self, n: RawNode) -> Option<RawNode> {
        n.0.body().map(RawNode)
    }

    /// The members of a class, interface, enum or object literal, in source order.
    pub(crate) fn members(&self, n: RawNode) -> Vec<RawNode> {
        match self.kind(n) {
            NodeKind::ObjectLiteral => n.0.properties().iter().copied().map(RawNode).collect(),
            _ => n.0.members().iter().copied().map(RawNode).collect(),
        }
    }

    /// The statements of a source file, block or module block. Other kinds have none.
    pub(crate) fn statements(&self, n: RawNode) -> Vec<RawNode> {
        if n.0.can_have_statements() {
            n.0.statements().iter().copied().map(RawNode).collect()
        } else {
            Vec::new()
        }
    }

    /// The declarations of a `VariableStatement`, in source order.
    pub(crate) fn declarations(&self, n: RawNode) -> Vec<RawNode> {
        let list = match self.kind(n) {
            NodeKind::VariableStatement => n.0.as_variable_statement().declaration_list(),
            NodeKind::VariableDeclarationList => n.0,
            _ => return Vec::new(),
        };
        list.as_variable_declaration_list()
            .declarations()
            .nodes()
            .iter()
            .copied()
            .map(RawNode)
            .collect()
    }

    /// `const` on a `VariableDeclaration` (the engine's rule accepts only `const`: a `let` can be reassigned).
    pub(crate) fn is_const(&self, declaration: RawNode) -> bool {
        tsrs_ast::get_combined_node_flags(declaration.0).intersects(NodeFlags::Const)
    }

    pub(crate) fn modifiers(&self, n: RawNode) -> Modifiers {
        let flags = n.0.modifier_flags();
        Modifiers {
            export: flags.intersects(ModifierFlags::Export),
            default: flags.intersects(ModifierFlags::Default),
            declare: flags.intersects(ModifierFlags::Ambient),
        }
    }

    /// The `default` keyword of `export default …`, for a def that has no name token to point at.
    pub(crate) fn default_keyword(&self, n: RawNode) -> Option<Span> {
        n.0.modifier_nodes()
            .iter()
            .find(|m| m.kind() == Kind::DefaultKeyword)
            .map(|&m| self.span(RawNode(m)))
    }

    /// The parameter names of a function-like node, as written: `a`, `{ x, y }`, `[h, ...t]`. A rest parameter's `...` is not part of it.
    pub(crate) fn params(&self, n: RawNode) -> Vec<String> {
        n.0.parameters()
            .iter()
            .filter_map(|p| p.name())
            .filter_map(|name| self.slice(self.span(RawNode(name))))
            .map(str::to_owned)
            .collect()
    }

    /// The node's own bytes: from its first token (a decorator, a modifier, a keyword) to its end. Leading comments are not in it:
    /// tsrs's `pos` includes all leading trivia, so the start is moved past it.
    pub(crate) fn span(&self, n: RawNode) -> Span {
        let end = n.0.end().source_offset().unwrap_or(0);
        let pos = n.0.pos().source_offset().unwrap_or(0);
        let start = tsrs_scanner::skip_trivia(self.text, pos).min(end);
        Span::new(start, end)
    }

    /// The text a span covers; `None` if it is out of bounds or cuts a character.
    pub(crate) fn slice(&self, span: Span) -> Option<&str> {
        span.slice(self.text)
    }

    /// The parse errors, in the order the engine reports them: a range and an English message. (`message_text()` is empty for
    /// ordinary diagnostics; `localize()` builds the text from the message and its arguments.)
    pub(crate) fn syntax_errors(&self) -> Vec<(Span, String)> {
        let len = u32::try_from(self.text.len()).unwrap_or(u32::MAX);
        self.file
            .diagnostics()
            .iter()
            .map(|d| {
                let start = d.pos().unwrap_or(0).min(len);
                let end = start.saturating_add(d.len().unwrap_or(0)).min(len);
                (Span::new(start, end), d.localize())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str, dialect: Dialect) -> Vec<NodeKind> {
        with_parsed(text, dialect, |p| {
            p.statements(p.root())
                .into_iter()
                .map(|n| p.kind(n))
                .collect()
        })
    }

    #[test]
    fn every_dialect_parses() {
        let ts = "function f(a: number): number { return a; }\nclass C { m() {} }\n";
        let js = "function f(a) { return a; }\nclass C { m() {} }\n";
        for (text, dialect) in [
            (ts, Dialect::Ts),
            (ts, Dialect::Tsx),
            (js, Dialect::Js),
            (js, Dialect::Jsx),
        ] {
            assert_eq!(
                kinds(text, dialect),
                [NodeKind::FunctionDeclaration, NodeKind::ClassDeclaration],
                "{dialect:?}"
            );
            with_parsed(text, dialect, |p| {
                assert!(p.syntax_errors().is_empty(), "{dialect:?}")
            });
        }
    }

    #[test]
    fn the_dialect_changes_how_angle_brackets_parse() {
        let text = "const x = <T>y;\n";
        let errors = |dialect| with_parsed(text, dialect, |p| p.syntax_errors().len());
        assert_eq!(errors(Dialect::Ts), 0, "a cast in .ts");
        assert!(errors(Dialect::Tsx) > 0, "the start of an element in .tsx");
    }

    #[test]
    fn jsx_parses_in_jsx_and_tsx_only() {
        let text = "const e = <div>hi</div>;\n";
        let errors = |dialect| with_parsed(text, dialect, |p| p.syntax_errors().len());
        assert_eq!(errors(Dialect::Tsx), 0);
        assert_eq!(errors(Dialect::Jsx), 0);
        assert!(errors(Dialect::Ts) > 0);
    }

    #[test]
    fn a_node_starts_at_its_first_token_not_at_the_comment_above() {
        let text = "// licence\n/** doc */\n@dec\nexport class C {}\n";
        with_parsed(text, Dialect::Ts, |p| {
            let class = p.statements(p.root())[0];
            let span = p.span(class);
            assert_eq!(p.slice(span), Some("@dec\nexport class C {}"));
        });
    }

    #[test]
    fn names_params_modifiers_and_body() {
        let text = "export default async function go(a, { b, c }, ...rest) {}\n";
        with_parsed(text, Dialect::Js, |p| {
            let f = p.statements(p.root())[0];
            let name = p.name(f).and_then(|n| p.slice(p.span(n)));
            assert_eq!(name, Some("go"));
            assert_eq!(p.params(f), ["a", "{ b, c }", "rest"]);
            let m = p.modifiers(f);
            assert!(m.export && m.default && !m.declare);
            assert!(p.body(f).is_some());
        });
    }

    #[test]
    fn const_and_let_are_told_apart() {
        with_parsed("const a = 1;\nlet b = 2;\nvar c = 3;\n", Dialect::Js, |p| {
            let flags: Vec<bool> = p
                .statements(p.root())
                .into_iter()
                .flat_map(|s| p.declarations(s))
                .map(|d| p.is_const(d))
                .collect();
            assert_eq!(flags, [true, false, false]);
        });
    }

    #[test]
    fn a_syntax_error_is_reported_with_a_range_inside_the_text() {
        let text = "function f( {\n";
        with_parsed(text, Dialect::Ts, |p| {
            let errors = p.syntax_errors();
            assert!(!errors.is_empty());
            assert!(
                errors
                    .iter()
                    .all(|(s, m)| s.end as usize <= text.len() && !m.is_empty())
            );
        });
    }

    #[test]
    fn many_parses_in_a_row_work() {
        for i in 0..200 {
            let text = format!("function f{i}() {{}}\n");
            assert_eq!(kinds(&text, Dialect::Ts), [NodeKind::FunctionDeclaration]);
        }
    }
}
