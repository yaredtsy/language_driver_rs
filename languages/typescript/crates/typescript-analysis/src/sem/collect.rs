use super::handed::{is_handed_function_expression, is_handed_over, literal_is_handed};
use super::*;

/// The plain calls in one def's body, as the callee expression's span of each. It follows `tsrs_ls`'s `CallSiteCollector` (`callhierarchy.rs`, `collect`),
/// the same node kinds and the same places where it does not descend, with the plan's differences: it stops at any nested node that is a def of the
/// **outline** (`def_of`), not at what tsrs's call hierarchy calls a declaration, so an anonymous function is part of the def around it and a named one is
/// not; and a class's static block and property initializers are the class's own body. Only calls and constructions are sites so far (`f(x)`, `a.b(x)`,
/// `a?.b(x)`, `super(x)`, `new C(x)`), a tagged template's tag, a decorator, a JSX element that is not intrinsic, and a property access that runs an accessor.
pub(super) struct Collector<'a> {
    pub(super) text: &'a str,
    def_of: &'a HashMap<P<Node>, usize>,
    /// The def being walked: it is in `def_of` too, and must not stop its own walk.
    own: P<Node>,
    pub(super) sites: Vec<CallNode>,
    /// The defs the walk stopped at (they are in `def_of`), with whether each is a named function expression **handed over where it is written**
    /// (`xs.map(function step() {})`): the only nested def that is handed over by its own place. The others (a method or arrow of a literal, a `const` arrow,
    /// a function declaration) are handed over by the literal or the name that holds them.
    pub(super) nested: Vec<(usize, bool)>,
    /// The object literals written in a position where a value is handed over (`literal_is_handed`): what a literal hands over is its function members.
    pub(super) lits: Vec<P<Node>>,
    /// Element accesses (`a["k"]`) in a hand-over position, and the targets of `x.bind(…)` in one and of `x.call(…)` / `x.apply(…)` anywhere (`x`): what no edge
    /// covers, counted by `Sem::handed_over`.
    pub(super) elems: Vec<P<Node>>,
    pub(super) binds: Vec<P<Node>>,
    pub(super) call_apply: Vec<P<Node>>,
    /// The names in a position where a value is **handed over** (`is_handed_over`): an identifier or a property access, or a shorthand property, with the
    /// span of the name. What each names is asked of the checker later (`Sem::handed_over`).
    pub(super) refs: Vec<(Span, P<Node>)>,
    /// The property accesses that are not callees (`note_access`): which are accessors is the checker's to say.
    pub(super) accesses: Vec<AccessCandidate>,
    /// The elements of an object destructuring of a variable (`const { a, b: { c } } = x`): each reads a property without a property access, so an accessor
    /// among them is run by no site (`Uncovered::DestructuredAccessor`, counted by `Sem::handed_over`).
    pub(super) destructured: Vec<P<Node>>,
    /// The assignments whose left side is an object pattern (`({ load } = api)`): the pattern and the value it takes from (`note_assignment`).
    pub(super) assigns: Vec<(P<Node>, P<Node>)>,
}

/// What reading one def's body found. `sites` is the calls; `nested` and `refs` are what the body hands over (`Sem::handed_over`).
pub(super) struct Body {
    pub(super) destructured: Vec<P<Node>>,
    pub(super) assigns: Vec<(P<Node>, P<Node>)>,
    pub(super) sites: Vec<CallNode>,
    pub(super) nested: Vec<(usize, bool)>,
    pub(super) lits: Vec<P<Node>>,
    pub(super) elems: Vec<P<Node>>,
    pub(super) binds: Vec<P<Node>>,
    pub(super) call_apply: Vec<P<Node>>,
    pub(super) refs: Vec<(Span, P<Node>)>,
}

/// A site found in a body: the span of the expression that is resolved (absolute, in the file), that expression, and the node the site is (a call, a `new`, a
/// tagged template, a JSX element, a decorator, a property access). Kept privately by `Sem`, per def, so that `callee` can ask the checker about a site given
/// only its index: the index is plain data, the node never leaves `sem.rs`.
#[derive(Clone, Copy)]
pub(super) struct CallNode {
    pub(super) span: Span,
    /// What is resolved: a call's callee, a `new`'s class, a template's tag, a JSX element's tag name, a decorator's expression, and for an accessor the access
    /// itself (`a.b`), or the key of `a["b"]`.
    pub(super) expr: P<Node>,
    /// The node of the site, which the checker's resolved signature is asked about.
    pub(super) call: P<Node>,
    pub(super) kind: SiteKind,
}

/// What a site is. The first four are calls of a function the source names; an accessor is a call the source does not write: `a.b` runs a getter or a setter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SiteKind {
    /// `f(x)`, `a.b(x)`, `new C(x)`, `super(x)`
    Call,
    /// `<Tag />`: the tag name
    Jsx,
    /// `` tag`text` ``: the tag
    Tagged,
    /// `@decorator` or `@decorator(1)`
    Decorator,
    /// `a.b` (or `a["b"]`) where `b` is an accessor: the getter for a read, the setter for a write, both for `+=` and `++`.
    Accessor(Access),
}

/// How a property access uses the property.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Access {
    Read,
    Write,
    ReadWrite,
}

impl Access {
    pub(super) fn reads(self) -> bool {
        self != Access::Write
    }
    pub(super) fn writes(self) -> bool {
        self != Access::Read
    }
}

/// A property access that may be an accessor: asked of the checker once the body is read (`Sem::body`).
#[derive(Clone, Copy)]
pub(super) struct AccessCandidate {
    pub(super) node: P<Node>,
    /// Where the checker is asked for the property's symbol: the access, or the key of an element access.
    pub(super) location: P<Node>,
    pub(super) access: Access,
}

impl<'a> Collector<'a> {
    pub(super) fn new(text: &'a str, def_of: &'a HashMap<P<Node>, usize>, own: P<Node>) -> Self {
        Collector {
            text,
            def_of,
            own,
            sites: Vec::new(),
            nested: Vec::new(),
            lits: Vec::new(),
            elems: Vec::new(),
            binds: Vec::new(),
            call_apply: Vec::new(),
            refs: Vec::new(),
            accesses: Vec::new(),
            destructured: Vec::new(),
            assigns: Vec::new(),
        }
    }

    pub(super) fn collect(&mut self, node: Option<P<Node>>) {
        let Some(node) = node else {
            return;
        };
        // ambient code is a declaration, not a body
        if node.flags().intersects(NodeFlags::Ambient) {
            return;
        }
        // a nested def's calls are its own. As in tsrs, a nested class still owns the calls in its computed member names.
        if node != self.own
            && let Some(&def) = self.def_of.get(&node)
        {
            self.nested.push((def, is_handed_function_expression(node)));
            if ast::is_class_like(node) {
                for &member in node.members() {
                    if let Some(name) = member.name()
                        && ast::is_computed_property_name(name)
                    {
                        self.collect(name.expression());
                    }
                }
            }
            return;
        }
        match node.kind() {
            // cannot contain a callable node; a name that is not being called may be a function handed over
            Kind::Identifier => {
                // `{ f }` names a value and is a declaration name at once: the shorthand is what the checker is asked about (its symbol for the
                // name is the property; `get_shorthand_assignment_value_symbol` is the variable)
                match node.parent() {
                    Some(parent)
                        if parent.kind() == Kind::ShorthandPropertyAssignment
                            && parent.name() == Some(node) =>
                    {
                        self.reference(node, parent)
                    }
                    _ => self.reference(node, node),
                }
                return;
            }
            Kind::ImportEqualsDeclaration
            | Kind::ImportDeclaration
            | Kind::ExportDeclaration
            | Kind::InterfaceDeclaration
            | Kind::TypeAliasDeclaration => return,
            // not the type side of an assertion
            Kind::TypeAssertionExpression | Kind::AsExpression | Kind::SatisfiesExpression => {
                self.collect(node.expression());
                return;
            }
            // not the type of a variable or parameter
            Kind::VariableDeclaration | Kind::Parameter => {
                if node.kind() == Kind::VariableDeclaration
                    && node.initializer().is_some()
                    && let Some(pattern) = node
                        .name()
                        .filter(|name| name.kind() == Kind::ObjectBindingPattern)
                {
                    self.note_destructured(pattern);
                }
                self.collect(node.name());
                self.collect(node.initializer());
                return;
            }
            Kind::CallExpression => {
                if is_plain_call(node)
                    && let Some(callee) = node.expression()
                {
                    self.sites.push(CallNode {
                        span: own_span(self.text, callee),
                        expr: callee,
                        call: node,
                        kind: SiteKind::Call,
                    });
                }
                self.note_bound_or_applied(node);
                // not the type arguments
                self.collect(node.expression());
                for &argument in node.arguments() {
                    self.collect(Some(argument));
                }
                return;
            }
            Kind::NewExpression => {
                // a construction is a site on the class expression, as a call is one on the callee
                if let Some(class) = node.expression() {
                    self.sites.push(CallNode {
                        span: own_span(self.text, class),
                        expr: class,
                        call: node,
                        kind: SiteKind::Call,
                    });
                }
                self.collect(node.expression());
                for &argument in node.arguments() {
                    self.collect(Some(argument));
                }
                return;
            }
            // `` tag`text` ``: the tag is called
            Kind::TaggedTemplateExpression => {
                let tag = node.as_tagged_template_expression().tag;
                self.sites.push(CallNode {
                    span: own_span(self.text, tag),
                    expr: tag,
                    call: node,
                    kind: SiteKind::Tagged,
                });
            }
            // `<Tag />`: a component is called, an intrinsic element (`<div>`, `<my-el>`, `<svg:rect>`) is not
            Kind::JsxOpeningElement | Kind::JsxSelfClosingElement => {
                let tag = node.tag_name();
                if !is_intrinsic_jsx_tag(tag) {
                    self.sites.push(CallNode {
                        span: own_span(self.text, tag),
                        expr: tag,
                        call: node,
                        kind: SiteKind::Jsx,
                    });
                }
            }
            // `@decorator`, `@decorator(1)`: the expression is called with what it decorates
            Kind::Decorator => {
                if let Some(expression) = node.expression() {
                    self.sites.push(CallNode {
                        span: own_span(self.text, expression),
                        expr: expression,
                        call: node,
                        kind: SiteKind::Decorator,
                    });
                }
            }
            // `({ load } = api)`: the names on the left take members of the value on the right, and no edge follows them
            Kind::BinaryExpression => self.note_assignment(node),
            // `a["k"]` handed over: no edge covers it (a counted remainder)
            Kind::ElementAccessExpression if is_handed_over(node) => {
                self.elems.push(node);
                self.note_access(node);
            }
            Kind::ElementAccessExpression => self.note_access(node),
            // an object literal handed over where it is written hands over its function members
            Kind::ObjectLiteralExpression if literal_is_handed(node) => self.lits.push(node),
            // `a.b`: the access is the name (its own name child is not looked at); the object is walked
            Kind::PropertyAccessExpression if !ast::is_part_of_type_node(node) => {
                self.reference(node, node);
                self.note_access(node);
                self.collect(node.expression());
                return;
            }
            _ => {}
        }
        if ast::is_part_of_type_node(node) {
            return;
        }
        node.for_each_child(&mut |child| {
            self.collect(Some(child));
            false
        });
    }

    /// The elements of an object binding pattern, and of the patterns nested in it. Which of them name one property (not a rest element, not a computed key) is
    /// `bound_symbol`'s to say.
    fn note_destructured(&mut self, pattern: P<Node>) {
        for &element in pattern.elements() {
            if element.kind() != Kind::BindingElement {
                continue;
            }
            self.destructured.push(element);
            if let Some(nested) = element
                .name()
                .filter(|name| name.kind() == Kind::ObjectBindingPattern)
            {
                self.note_destructured(nested);
            }
        }
    }

    /// `({ a, b: c } = value)`: an assignment whose left side is an object pattern. Remembered with its two sides, for `Sem::handed_over`.
    fn note_assignment(&mut self, node: P<Node>) {
        let binary = node.as_binary_expression();
        if binary.operator_token.kind() == Kind::EqualsToken
            && binary.left().kind() == Kind::ObjectLiteralExpression
        {
            self.assigns.push((binary.left(), binary.right()));
        }
    }

    /// `a.b` and `a["b"]` that are not callees and not deleted: the getter or setter runs, if `b` is an accessor. Remembered with how the property is used.
    fn note_access(&mut self, node: P<Node>) {
        if ast::is_part_of_type_node(node) || is_callee_position(node) {
            return;
        }
        let location = if node.kind() == Kind::ElementAccessExpression {
            let key = node.as_element_access_expression().argument_expression;
            if !ast::is_string_literal_like(key) {
                return;
            }
            key
        } else {
            node
        };
        let mut outer = node;
        while let Some(parent) = outer.parent()
            && matches!(
                parent.kind(),
                Kind::NonNullExpression
                    | Kind::AsExpression
                    | Kind::SatisfiesExpression
                    | Kind::TypeAssertionExpression
            )
        {
            outer = parent;
        }
        if outer
            .parent()
            .is_some_and(|parent| parent.kind() == Kind::DeleteExpression)
        {
            return;
        }
        let access = if !ast::is_write_access(outer) {
            Access::Read
        } else if ast::is_write_only_access(outer) {
            Access::Write
        } else {
            Access::ReadWrite
        };
        self.accesses.push(AccessCandidate {
            node,
            location,
            access,
        });
    }

    /// `x.bind(…)` in a hand-over position, and `x.call(…)` / `x.apply(…)` anywhere: `x` is a function that is handed over bound, or called through `call`, and
    /// no edge covers either. Remembered for the count.
    fn note_bound_or_applied(&mut self, call: P<Node>) {
        let Some(callee) = call.expression() else {
            return;
        };
        if callee.kind() != Kind::PropertyAccessExpression {
            return;
        }
        let (Some(target), Some(name)) = (callee.expression(), callee.name()) else {
            return;
        };
        match name.text() {
            "bind" if is_handed_over(call) => self.binds.push(target),
            "call" | "apply" => self.call_apply.push(target),
            _ => {}
        }
    }

    /// A name where a value is handed over: remembered with the span of the name, and the node to ask the checker about.
    fn reference(&mut self, name: P<Node>, asked: P<Node>) {
        if is_handed_over(name) {
            self.refs.push((own_span(self.text, name), asked));
        }
    }

    /// What a def's walk starts from: the parts of the node that are its body (`tsrs_ls`'s `collect_call_sites`, with the plan's class rule).
    pub(super) fn body_of(&mut self, node: P<Node>) {
        match node.kind() {
            Kind::FunctionDeclaration
            | Kind::FunctionExpression
            | Kind::ArrowFunction
            | Kind::MethodDeclaration
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::Constructor => {
                for &parameter in node.parameters() {
                    self.collect(Some(parameter));
                }
                self.collect(node.body());
            }
            Kind::ClassDeclaration | Kind::ClassExpression => {
                if let Some(modifiers) = node.modifiers() {
                    for &modifier in modifiers.nodes() {
                        self.collect(Some(modifier));
                    }
                }
                if let Some(heritage) = ast::get_class_extends_heritage_element(node) {
                    self.collect(heritage.expression());
                }
                for &member in node.members() {
                    if ast::can_have_modifiers(member)
                        && let Some(modifiers) = member.modifiers()
                    {
                        for &modifier in modifiers.nodes() {
                            self.collect(Some(modifier));
                        }
                    }
                    // `constructor(@Inject(T) x)`, `method(@Body() b)`: the decorators of a parameter run when the class is defined, so they belong to the class
                    if matches!(
                        member.kind(),
                        Kind::Constructor
                            | Kind::MethodDeclaration
                            | Kind::GetAccessor
                            | Kind::SetAccessor
                    ) {
                        for &parameter in member.parameters() {
                            if let Some(modifiers) = parameter.modifiers() {
                                for &modifier in modifiers.nodes() {
                                    if modifier.kind() == Kind::Decorator {
                                        self.collect(Some(modifier));
                                    }
                                }
                            }
                        }
                    }
                    match member.kind() {
                        Kind::PropertyDeclaration => self.collect(member.initializer()),
                        Kind::ClassStaticBlockDeclaration => {
                            self.collect(Some(member.as_class_static_block_declaration().body));
                        }
                        _ => {}
                    }
                }
            }
            Kind::ModuleDeclaration => {
                if let Some(body) = node.body()
                    && ast::is_module_block(body)
                {
                    for &statement in body.statements() {
                        self.collect(Some(statement));
                    }
                }
            }
            // an interface, a method signature, an enum, a type alias: nothing to walk
            _ => {}
        }
    }
}

/// Whether `node` is what is called where it stands: the callee of a call or a `new`, a tagged template's tag, a decorator's expression, a JSX tag name (the
/// closing one too), however many parentheses and assertions are between. Such an access is resolved as the callee, and is not an accessor site of its own.
fn is_callee_position(node: P<Node>) -> bool {
    let mut at = node;
    while let Some(parent) = at.parent() {
        match parent.kind() {
            Kind::ParenthesizedExpression
            | Kind::AsExpression
            | Kind::SatisfiesExpression
            | Kind::TypeAssertionExpression
            | Kind::NonNullExpression => at = parent,
            Kind::CallExpression | Kind::NewExpression | Kind::Decorator => {
                return parent.expression() == Some(at);
            }
            Kind::TaggedTemplateExpression => {
                return parent.as_tagged_template_expression().tag == at;
            }
            Kind::JsxOpeningElement | Kind::JsxSelfClosingElement | Kind::JsxClosingElement => {
                return parent.tag_name() == at;
            }
            _ => return false,
        }
    }
    false
}

/// A JSX tag that names no value: `<div>`, `<my-el>` (a lower-case first letter, or a dash), `<svg:rect>` (a namespaced name). TypeScript's rule
/// (`isIntrinsicJsxName`); a tag that is an access (`<props.Icon />`) or starts with a capital is a component.
fn is_intrinsic_jsx_tag(tag: P<Node>) -> bool {
    match tag.kind() {
        Kind::JsxNamespacedName => true,
        Kind::Identifier => {
            let name = tag.text();
            name.starts_with(|c: char| c.is_lowercase()) || name.contains('-')
        }
        _ => false,
    }
}

/// The expression under `(x)`, `x as T`, `x satisfies T`, `<T>x` and `x!`, however many.
pub(super) fn unwrap_expression(mut node: P<Node>) -> P<Node> {
    while matches!(
        node.kind(),
        Kind::ParenthesizedExpression
            | Kind::AsExpression
            | Kind::SatisfiesExpression
            | Kind::TypeAssertionExpression
            | Kind::NonNullExpression
    ) {
        match node.expression() {
            Some(inner) => node = inner,
            None => break,
        }
    }
    node
}

/// `f(x)`, `a.b(x)`, `a?.b(x)`, `super(x)`. Not `import("./m")` and not `require("./m")`: that is loading a module, not calling into a function.
fn is_plain_call(call: P<Node>) -> bool {
    let Some(callee) = call.expression() else {
        return false;
    };
    if callee.kind() == Kind::ImportKeyword {
        return false;
    }
    let is_require = callee.kind() == Kind::Identifier
        && callee.text() == "require"
        && matches!(call.arguments(), [argument] if ast::is_string_literal_like(*argument));
    !is_require
}
