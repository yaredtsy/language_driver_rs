//! Typed views of the engine's nodes (H14, `docs/03-typescript/10-review-fixes.md` §6.2): an untyped tree underneath and, on top, one type for each kind of node
//! the analysis reads.
//!
//! The engine's accessors (`expression`, `initializer`, `members`, `text`, …) end in `_ => panic!("Unhandled case …")` for a kind they do not handle, and a
//! guard written by hand beside each call is the rule that failed. A view moves the guard into the type: `cast` checks the kind **once**, and every accessor of a
//! view is total, because the view exists only for kinds the engine's accessor handles. Which kinds those are is read off the accessor's `match` in `tsrs_ast`
//! (`ast.rs`, at the pin of `ENGINE.md`), and `tests.rs` proves it: it tries every cast on every node of the bundled libraries and of the fixtures, calls every
//! accessor of each view that succeeds, and checks that every kind of every view was met. An engine bump that stops handling a kind makes that test panic before
//! a user's file does.
//!
//! The raw accessors are banned outside this module by `clippy.toml`. Two kinds of `None`:
//! - `cast` is a **try** (is this a method?): a failure is normal and silent;
//! - `expect` is for a place that **expected** the kind (a parent the code has just checked by a predicate of its own): a failure is a missing answer, counted
//!   as `engine_kind_unhandled` (`counted`).
#![allow(
    clippy::disallowed_methods,
    reason = "this module is the one place the engine's panicking accessors and downcasts (`as_binary_expression`, …) are called: every call is behind a `cast` of the kinds it handles"
)]

use std::cell::Cell;

use super::*;

thread_local! {
    /// How many times an `expect` failed on this thread.
    static UNHANDLED: Cell<u64> = const { Cell::new(0) };
}

/// The key in `Stats.language` for a node of a kind that an accessor does not handle, met where the code expected one.
pub(crate) const UNHANDLED_KEY: &str = "engine_kind_unhandled";

fn unhandled() {
    UNHANDLED.with(|count| count.set(count.get() + 1));
}

/// Runs `work` and says how many `expect`s failed inside it, **taking them off the thread's count**: the number rides with the result it belongs to. A body is
/// read once and cached, so a count left on the thread would be said by the first request that read it and not by the next (rule 9); `Body` keeps its own, and
/// the walk says it each time the body is walked.
pub(crate) fn counted<T>(work: impl FnOnce() -> T) -> (T, u64) {
    let before = UNHANDLED.with(Cell::get);
    let result = work();
    let made = UNHANDLED.with(Cell::get) - before;
    UNHANDLED.with(|count| count.set(before));
    (result, made)
}

/// A view for the kinds listed: `cast`, `expect`, `node`.
macro_rules! view {
    ($(#[$doc:meta])* $name:ident: $($kind:ident)|+) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub(super) struct $name(P<Node>);

        impl $name {
            /// Every kind this view is made of.
            #[cfg(test)]
            pub(super) const KINDS: &'static [Kind] = &[$(Kind::$kind),+];

            /// A try: `None` when `node` is not one of the kinds, and nothing is said.
            pub(super) fn cast(node: P<Node>) -> Option<Self> {
                matches!(node.kind(), $(Kind::$kind)|+).then_some(Self(node))
            }

            /// `cast` at a place that expected the kind: a failure is counted (`engine_kind_unhandled`).
            #[allow(dead_code, reason = "a view that no place has expected yet")]
            pub(super) fn expect(node: P<Node>) -> Option<Self> {
                let found = Self::cast(node);
                if found.is_none() {
                    unhandled();
                }
                found
            }

        }
    };
}

view! {
    /// A node that has an expression: the engine's `expression` handles each of these kinds, and answers `Some` for all of them.
    HasExpression: PropertyAccessExpression | ElementAccessExpression | ParenthesizedExpression | CallExpression | NewExpression
        | ExpressionWithTypeArguments | ComputedPropertyName | NonNullExpression | TypeAssertionExpression | AsExpression
        | SatisfiesExpression | AwaitExpression | SpreadAssignment | Decorator
}

impl HasExpression {
    pub(super) fn expression(self) -> Option<P<Node>> {
        self.0.expression()
    }
}

view! {
    /// `(x)`, `x as T`, `x satisfies T`, `<T>x`, `x!`: a node that only wraps an expression.
    Wrapper: ParenthesizedExpression | AsExpression | SatisfiesExpression | TypeAssertionExpression | NonNullExpression
}

impl Wrapper {
    pub(super) fn inner(self) -> Option<P<Node>> {
        self.0.expression()
    }
}

view! {
    /// `f(x)` or `new C(x)`: a callee and its arguments.
    CallOrNew: CallExpression | NewExpression
}

impl CallOrNew {
    pub(super) fn callee(self) -> Option<P<Node>> {
        self.0.expression()
    }

    pub(super) fn arguments(self) -> &'static [P<Node>] {
        self.0.arguments()
    }
}

view! {
    /// A node that can hold an initializer (`= value`).
    Initialized: VariableDeclaration | Parameter | BindingElement | PropertyDeclaration | PropertyAssignment | EnumMember
}

impl Initialized {
    pub(super) fn initializer(self) -> Option<P<Node>> {
        self.0.initializer()
    }
}

view! {
    /// A node that has members: a class, an interface, an enum, a type literal, a mapped type.
    Members: ClassDeclaration | ClassExpression | InterfaceDeclaration | EnumDeclaration | TypeLiteral | MappedType
}

impl Members {
    pub(super) fn members(self) -> &'static [P<Node>] {
        self.0.members()
    }
}

view! {
    /// A node that has statements: a file, a block, a module block, a case or default clause.
    Statements: SourceFile | Block | ModuleBlock | CaseClause | DefaultClause
}

impl Statements {
    pub(super) fn statements(self) -> &'static [P<Node>] {
        self.0.statements()
    }
}

view! {
    /// A binding pattern or an array literal: a list of elements.
    Elements: ObjectBindingPattern | ArrayBindingPattern | ArrayLiteralExpression
}

impl Elements {
    pub(super) fn elements(self) -> &'static [P<Node>] {
        self.0.elements()
    }
}

view! {
    /// An object literal: a list of properties.
    Properties: ObjectLiteralExpression
}

impl Properties {
    pub(super) fn properties(self) -> &'static [P<Node>] {
        self.0.properties()
    }
}

view! {
    /// A JSX opening, closing or self-closing element: it has a tag name.
    JsxTag: JsxOpeningElement | JsxClosingElement | JsxSelfClosingElement
}

impl JsxTag {
    pub(super) fn tag_name(self) -> P<Node> {
        self.0.tag_name()
    }
}

view! {
    /// A name that is one token: its value is what the engine's `text` says (an identifier's own name, a string's value, a number's text).
    NameToken: Identifier | PrivateIdentifier | StringLiteral | NumericLiteral | NoSubstitutionTemplateLiteral
}

impl NameToken {
    /// The engine's value of the token: for a string, what it holds without the quotes. The lookup of a property by name asks for this; the **spelling** of a name
    /// in the analysis's output is `spelled`.
    pub(super) fn value(self) -> &'static str {
        self.0.text()
    }
}

view! {
    /// `a + b`, `a = b`, `a && b`: two operands and an operator.
    Binary: BinaryExpression
}

impl Binary {
    pub(super) fn left(self) -> P<Node> {
        self.0.as_binary_expression().left()
    }

    pub(super) fn right(self) -> P<Node> {
        self.0.as_binary_expression().right()
    }

    /// The kind of the operator token (`EqualsToken`, `BarBarToken`, …).
    pub(super) fn operator(self) -> Kind {
        self.0.as_binary_expression().operator_token.kind()
    }
}

view! {
    /// `a[b]`: the expression inside the brackets.
    ElementAccess: ElementAccessExpression
}

impl ElementAccess {
    pub(super) fn argument(self) -> P<Node> {
        self.0.as_element_access_expression().argument_expression
    }
}

view! {
    /// `` tag`text` ``: the tag, which is called.
    TaggedTemplate: TaggedTemplateExpression
}

impl TaggedTemplate {
    pub(super) fn tag(self) -> P<Node> {
        self.0.as_tagged_template_expression().tag
    }
}

view! {
    /// `c ? a : b`: the condition.
    Conditional: ConditionalExpression
}

impl Conditional {
    pub(super) fn condition(self) -> P<Node> {
        self.0.as_conditional_expression().condition
    }
}

view! {
    /// An element of a binding pattern: `a`, `a: b`, `...rest`.
    Binding: BindingElement
}

impl Binding {
    /// `...rest`.
    pub(super) fn is_rest(self) -> bool {
        self.0.as_binding_element().dot_dot_dot_token().is_some()
    }
}

view! {
    /// `static { … }` in a class: its block.
    StaticBlock: ClassStaticBlockDeclaration
}

impl StaticBlock {
    pub(super) fn body(self) -> P<Node> {
        self.0.as_class_static_block_declaration().body
    }
}

/// The spelling of a name node, from its text as written in `file_text` (`typescript_syntax::spell_name`): a computed name is `[Symbol.iterator]` as written. It
/// reads text only, so it cannot panic on any kind of name; `None` when the node's span is not inside the text.
pub(super) fn spelled(file_text: &str, name: P<Node>) -> Option<String> {
    own_span(file_text, name)
        .slice(file_text)
        .map(typescript_syntax::spell_name)
}

// What the code asked of a node it had checked by a predicate of its own (`is_class_like`, `is_variable_like`, a `match` on the parent's kind): the view's `expect`,
// so a node that is not one of the kinds is counted and gives nothing, where the engine's accessor would have panicked.

/// `node.expression()` of a node that has one.
pub(super) fn expression_of(node: P<Node>) -> Option<P<Node>> {
    HasExpression::expect(node)?.expression()
}

/// `node.initializer()` of a node that can have one.
pub(super) fn initializer_of(node: P<Node>) -> Option<P<Node>> {
    Initialized::expect(node)?.initializer()
}

/// `node.members()` of a node that has members; none for any other.
pub(super) fn members_of(node: P<Node>) -> &'static [P<Node>] {
    Members::expect(node).map_or(&[], Members::members)
}

/// `node.statements()` of a node that has statements; none for any other.
pub(super) fn statements_of(node: P<Node>) -> &'static [P<Node>] {
    Statements::expect(node).map_or(&[], Statements::statements)
}

/// `node.elements()` of a pattern or an array literal; none for any other.
pub(super) fn elements_of(node: P<Node>) -> &'static [P<Node>] {
    Elements::expect(node).map_or(&[], Elements::elements)
}

/// `node.properties()` of an object literal; none for any other.
pub(super) fn properties_of(node: P<Node>) -> &'static [P<Node>] {
    Properties::expect(node).map_or(&[], Properties::properties)
}

/// `node.arguments()` of a call or a `new`; none for any other.
pub(super) fn arguments_of(node: P<Node>) -> &'static [P<Node>] {
    CallOrNew::expect(node).map_or(&[], CallOrNew::arguments)
}

/// `node.tag_name()` of a JSX element.
pub(super) fn tag_name_of(node: P<Node>) -> Option<P<Node>> {
    JsxTag::expect(node).map(JsxTag::tag_name)
}

#[cfg(test)]
mod tests;
