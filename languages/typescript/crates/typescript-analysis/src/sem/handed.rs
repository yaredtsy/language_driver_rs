use analyzer_core::outline::DefKind;
use tsrs_ast::{Symbol, SymbolFlags};
use tsrs_checker::{Checker, Type};
use tsrs_compiler::Context;

use super::collect::unwrap_expression;
use super::hierarchy::{callable_declarations, is_variable_like};
use super::*;

/// A def's node that is a **named function expression handed over where it is written**: `xs.map(function step() {})`, `register(function step() {})`.
/// Not `const f = function step() {}` or `{ cb: function step() {} }` (a binding or a property: the name or the literal that holds it hands it over).
pub(super) fn is_handed_function_expression(node: P<Node>) -> bool {
    node.kind() == Kind::FunctionExpression && node.name().is_some() && handed_position(node, true)
}

/// Whether the value `name` stands for is **handed over** where it is written, and not called, read or tested: an argument of a call or of `new`, the
/// value of a property (`key: f`, and `{ f }`), an element of an array, an initializer or the right side of `=`, the value a function returns (`return
/// f`, an arrow's concise body), a JSX attribute's value or a JSX child. Through the wrappers that keep the value (`(f)`, `f as T`, `f satisfies T`,
/// `<T>f`, `f!`), the branches of a conditional, and the right side of `&&`, `||` and `??`.
///
/// Not a hand-over, though it names the value: a callee (`f()`, `(a ? f : g)()`), a tagged template's tag, a decorator, a JSX tag, a condition (`if (f)`,
/// `f ? a : b`), `typeof f`, a comparison, `!f`, the object of an access (`f.length`, `f[0]`), a template's substitution. Those are the language's other
/// sites or reads of the def, and drawing them as "passed" would say the def is given to something.
pub(super) fn is_handed_over(name: P<Node>) -> bool {
    handed_position(name, false)
}

/// Whether an **object literal** is handed over where it is written: the same positions as a name (`is_handed_over`), except that a literal that is only
/// bound (the initializer of a variable, a property value, a default) is made, not handed over: it is handed over by the name that holds it.
pub(super) fn literal_is_handed(literal: P<Node>) -> bool {
    handed_position(literal, true)
}

/// `is_handed_over` and `literal_is_handed`: where `value` is handed over. `creating`: the value is a literal or a function expression, which a binding
/// (an initializer, a property value, a default) only makes.
fn handed_position(value: P<Node>, creating: bool) -> bool {
    let mut at = value;
    loop {
        let Some(parent) = at.parent() else {
            return false;
        };
        match parent.kind() {
            Kind::ParenthesizedExpression
            | Kind::AsExpression
            | Kind::SatisfiesExpression
            | Kind::TypeAssertionExpression
            | Kind::NonNullExpression => at = parent,
            Kind::ConditionalExpression => {
                if parent.as_conditional_expression().condition == at {
                    return false;
                }
                at = parent;
            }
            Kind::BinaryExpression => {
                let binary = parent.as_binary_expression();
                if binary.right() != at {
                    return false;
                }
                match binary.operator_token.kind() {
                    Kind::EqualsToken => {
                        return !matches!(
                            binary.left().kind(),
                            Kind::ObjectLiteralExpression | Kind::ArrayLiteralExpression
                        );
                    }
                    Kind::AmpersandAmpersandToken
                    | Kind::BarBarToken
                    | Kind::QuestionQuestionToken => at = parent,
                    _ => return false,
                }
            }
            Kind::CallExpression | Kind::NewExpression => {
                return parent.expression() != Some(at);
            }
            // `const { load } = api` and `({ load } = api)` read the members they name: the names are followed (`bound_member`), the initializer hands nothing over
            Kind::VariableDeclaration | Kind::Parameter
                if parent.initializer() == Some(at)
                    && parent.name().is_some_and(ast::is_binding_pattern) =>
            {
                return false;
            }
            Kind::PropertyAssignment
            | Kind::VariableDeclaration
            | Kind::Parameter
            | Kind::BindingElement
            | Kind::PropertyDeclaration => return !creating && parent.initializer() == Some(at),
            Kind::ArrowFunction => return parent.body() == Some(at),
            Kind::ShorthandPropertyAssignment
            | Kind::ArrayLiteralExpression
            | Kind::ReturnStatement
            | Kind::JsxExpression => return true,
            _ => return false,
        }
    }
}

/// A def a walked body hands over: made where an expression is, or named where a value is handed over (`is_handed_over`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Handed<'p> {
    pub(crate) func: Func<'p>,
    /// Written inside an expression (a method or arrow of a literal, a named function expression) and not named: it has no place in the body where
    /// its name is. Otherwise it was named (`onClick={handleClose}`, `xs.map(parse)`, `{ f }`) and `at` says where.
    pub(crate) written: bool,
    /// The span of the name, in the file (absolute); `None` for a def that is written inside an expression. One `Handed` for each place a def is
    /// named, so a def named twice is two, each at its own site (D22).
    pub(crate) at: Option<Span>,
}

/// What the object patterns on the left of `=` in a body take (`Sem::assigned_defs`): the variables that get a def of the value, and the variables that get a
/// rest object holding one.
#[derive(Default)]
struct Assigned {
    defs: Vec<P<Symbol>>,
    rests: Vec<P<Symbol>>,
}

/// What reading one body found about hand-overs: the defs it hands over (each an edge), and what it hands over or calls that no edge covers.
pub(crate) struct HandOvers<'p> {
    pub(crate) handed: Vec<Handed<'p>>,
    pub(crate) uncovered: Vec<Uncovered>,
}

impl Sem {
    /// The functions `func`'s body hands over instead of calling: the defs it writes inside an expression (no place for a name: `at` is `None`), and the
    /// project functions it names where a value is handed over (`is_handed_over`: an argument, a property value, an element, an initializer, a return, a
    /// JSX value, a shorthand property), each with the span of the name. Only functions with a body. Whether anything calls them is the caller's
    /// question (the graph's edges). Not looked at: an element access (`api["load"]`), `f.bind(…)` and `f.call(…)`, and a class named as a value.
    pub(crate) fn handed_over<'p>(
        &'p self,
        func: Func<'p>,
        style: MarkerStyle<'_>,
    ) -> HandOvers<'p> {
        let mut uncovered: Vec<Uncovered> = Vec::new();
        let Some(body) = self.body(func, style) else {
            return HandOvers {
                handed: Vec::new(),
                uncovered,
            };
        };
        let mut found: Vec<Handed<'p>> = Vec::new();
        // a named function expression handed over where it is written (`xs.map(function step() {})`): at its name
        if let Some(defs) = self.file_defs(func.file, style) {
            for &(def, handed) in &body.nested {
                let held = Func {
                    file: func.file,
                    def,
                };
                if handed && self.is_function_with_body(held, style) {
                    found.push(Handed {
                        func: held,
                        written: true,
                        at: Some(defs.outline.defs[def].name_range),
                    });
                }
            }
            // an object literal handed over where it is written hands over its function members, each at its own key
            for &literal in &body.lits {
                let mut carried = Vec::new();
                self.functions_of_literal(func.file, &defs, literal, &mut carried);
                for (held, key) in carried {
                    let handed = Handed {
                        func: held,
                        written: true,
                        at: Some(key),
                    };
                    if !found.contains(&handed) {
                        found.push(handed);
                    }
                }
            }
        }
        // a class declared in this body: its decorators run when the body does, and the class's own body is not a part of it
        if let Some(defs) = self.file_defs(func.file, style) {
            for &(def, _) in &body.nested {
                if defs.outline.defs[def].kind == DefKind::Class {
                    let class = Func {
                        file: func.file,
                        def,
                    };
                    uncovered.extend(std::iter::repeat_n(
                        Uncovered::LocalClassDecoratorSite,
                        self.decorator_sites(class, style),
                    ));
                }
            }
        }
        let _scope = self._region.enter();
        let mut checker = self.program.get_type_checker(&Context::default());
        // the variables an object pattern on the left of `=` assigns a def, or a rest object that holds one, to (`({ load, ...others } = api)`)
        let mut assigned = Assigned::default();
        let mut patterns: Vec<Span> = Vec::new();
        for &(left, right) in &body.assigns {
            patterns.push(own_span(func.file.node.text(), left));
            let ty = checker.get_type_at_location(right);
            self.assigned_defs(&mut checker, left, ty, style, &mut assigned);
        }
        // what no edge covers: the function behind a `bind`, a `call` or an `apply`, and behind an element access with a literal key
        for &target in &body.binds {
            if !self
                .functions_named(&mut checker, target, style)
                .0
                .is_empty()
            {
                uncovered.push(Uncovered::Bind);
            }
        }
        for &target in &body.call_apply {
            if !self
                .functions_named(&mut checker, target, style)
                .0
                .is_empty()
            {
                uncovered.push(Uncovered::CallApply);
            }
        }
        for &access in &body.elems {
            let key = access.as_element_access_expression().argument_expression;
            if ast::is_string_literal_like(key)
                && !self.functions_named(&mut checker, key, style).0.is_empty()
            {
                uncovered.push(Uncovered::ElementAccess);
            }
        }
        // a getter read by destructuring runs, and no property access is there to be its site
        for &element in &body.destructured {
            if bound_symbol(&mut checker, element).is_some_and(|symbol| {
                symbol
                    .declarations()
                    .iter()
                    .any(|declaration| declaration.kind() == Kind::GetAccessor)
            }) {
                uncovered.push(Uncovered::DestructuredAccessor);
            }
        }
        // in source order, as the body names them
        for &(span, node) in &body.refs {
            let symbol = if node.kind() == Kind::ShorthandPropertyAssignment {
                checker.get_shorthand_assignment_value_symbol(Some(node))
            } else {
                checker.get_symbol_at_location_exported(node)
            };
            let Some(mut symbol) = symbol else {
                continue;
            };
            // a target of the pattern itself is where the def is taken, not where it is handed over
            let inside_pattern = patterns
                .iter()
                .any(|pattern| pattern.start <= span.start && span.end <= pattern.end);
            if !inside_pattern && assigned.defs.contains(&symbol) {
                uncovered.push(Uncovered::AssignmentDestructuring);
            }
            if assigned.rests.contains(&symbol) {
                uncovered.push(Uncovered::RestElement);
            }
            if symbol.flags().intersects(SymbolFlags::Alias) {
                symbol = checker.get_aliased_symbol(symbol);
            }
            // a name bound by destructuring (`const { load } = api`) stands for the member it names: the declarations of that member
            let mut declarations = Vec::new();
            for declaration in symbol.declarations().to_vec() {
                if declaration.kind() == Kind::BindingElement {
                    // `...others`: the rest object holds the members the pattern did not name; any that is a def goes with it, and no edge says so
                    if declaration
                        .as_binding_element()
                        .dot_dot_dot_token()
                        .is_some()
                    {
                        let ty = checker.get_type_of_symbol(symbol);
                        if self.holds_defs(&mut checker, ty, style) {
                            uncovered.push(Uncovered::RestElement);
                        }
                    }
                    // `for (const { load } of xs)`: no initializer to read, so no member is followed (no edge), and the name is counted
                    if for_of_member(&mut checker, declaration)
                        .is_some_and(|property| self.holds_def(&mut checker, property, style))
                    {
                        uncovered.push(Uncovered::ForOfDestructuring);
                    }
                    declarations.extend(bound_member(&mut checker, declaration));
                } else {
                    declarations.push(declaration);
                }
            }
            for declaration in declarations {
                // a name that holds an object literal (`const local = { traverse: () => {} }`, a nested `local.data`) hands over that literal's
                // function members, at the name
                if (is_variable_like(Some(declaration)) || ast::is_property_assignment(declaration))
                    && let Some(literal) = declaration
                        .initializer()
                        .map(unwrap_expression)
                        .filter(|init| init.kind() == Kind::ObjectLiteralExpression)
                    && let Some(file_node) = ast::get_source_file_of_node(declaration)
                {
                    let file = File {
                        node: file_node,
                        _sem: PhantomData,
                        _one_thread: PhantomData,
                    };
                    if self.is_project_code(file)
                        && let Some(defs) = self.file_defs(file, style)
                    {
                        let mut carried = Vec::new();
                        self.functions_of_literal(file, &defs, literal, &mut carried);
                        for (held, _) in carried {
                            let handed = Handed {
                                func: held,
                                written: true,
                                at: Some(span),
                            };
                            if !found.contains(&handed) {
                                found.push(handed);
                            }
                        }
                    }
                }
                let (functions, classes) =
                    self.functions_of_declaration(&mut checker, declaration, style);
                for held in functions {
                    let handed = Handed {
                        func: held,
                        written: false,
                        at: Some(span),
                    };
                    // one edge for each place and def: the declarations of one symbol can name a def twice
                    if !found.contains(&handed) {
                        found.push(handed);
                    }
                }
                if classes > 0 {
                    uncovered.push(Uncovered::Class);
                }
            }
        }
        HandOvers {
            handed: found,
            uncovered,
        }
    }

    /// Whether a member of `ty` is a project function with a body.
    fn holds_defs(&self, checker: &mut Checker, ty: P<Type>, style: MarkerStyle<'_>) -> bool {
        let properties = checker.get_properties_of_type_exported(ty).to_vec();
        properties
            .into_iter()
            .any(|property| self.holds_def(checker, property, style))
    }

    /// Whether the property `property` is a project function with a body: its declarations, mapped as a call maps them.
    fn holds_def(
        &self,
        checker: &mut Checker,
        property: P<Symbol>,
        style: MarkerStyle<'_>,
    ) -> bool {
        property
            .declarations()
            .to_vec()
            .into_iter()
            .any(|declaration| {
                !self
                    .functions_of_declaration(checker, declaration, style)
                    .0
                    .is_empty()
            })
    }

    /// The variables that the object pattern `left` (the left of `=`) assigns a def of its value to: `load` in `({ load } = api)` and `({ load: go } = api)`,
    /// `run` in `({ inner: { run } } = deep)`, where the member of `ty` that the key names is a project function. A key that is not a name, a member the type
    /// does not have and a target that is not a variable are not followed.
    fn assigned_defs(
        &self,
        checker: &mut Checker,
        left: P<Node>,
        ty: P<Type>,
        style: MarkerStyle<'_>,
        out: &mut Assigned,
    ) {
        for &member in left.properties() {
            // `...others`: the rest object holds the members of the value that the pattern did not name
            if member.kind() == Kind::SpreadAssignment {
                let named: Vec<&str> = left
                    .properties()
                    .iter()
                    .filter(|sibling| sibling.kind() != Kind::SpreadAssignment)
                    .filter_map(|sibling| sibling.name())
                    .map(|name| name.text())
                    .collect();
                let left_over = checker
                    .get_properties_of_type_exported(ty)
                    .to_vec()
                    .into_iter()
                    .filter(|property| !named.contains(&property.name()))
                    .any(|property| self.holds_def(checker, property, style));
                if left_over
                    && let Some(target) = member.expression().map(unwrap_expression)
                    && let Some(variable) = checker.get_symbol_at_location_exported(target)
                {
                    out.rests.push(variable);
                }
                continue;
            }
            let Some(key) = member
                .name()
                .filter(|name| ast::is_identifier(*name) || ast::is_string_literal_like(*name))
            else {
                continue;
            };
            let (target, nested) = match member.kind() {
                Kind::ShorthandPropertyAssignment => (None, None),
                Kind::PropertyAssignment => {
                    let value = member.initializer().map(unwrap_expression);
                    match value {
                        Some(value) if value.kind() == Kind::ObjectLiteralExpression => {
                            (None, Some(value))
                        }
                        Some(value) => (Some(value), None),
                        None => continue,
                    }
                }
                _ => continue,
            };
            let Some(property) = checker.get_property_of_type_exported(ty, key.text()) else {
                continue;
            };
            if let Some(nested) = nested {
                let inner = checker.get_type_of_symbol(property);
                self.assigned_defs(checker, nested, inner, style, out);
                continue;
            }
            if !self.holds_def(checker, property, style) {
                continue;
            }
            let variable = match target {
                None => checker.get_shorthand_assignment_value_symbol(Some(member)),
                Some(target) => checker.get_symbol_at_location_exported(target),
            };
            if let Some(variable) = variable {
                out.defs.push(variable);
            }
        }
    }

    /// The project functions with a body that `node` (a name, an access, a shorthand property) names, and how many project classes: the symbol's
    /// declarations, mapped as a call maps them (`callable_declarations`), kept if they are defs of the project.
    fn functions_named<'p>(
        &'p self,
        checker: &mut Checker,
        node: P<Node>,
        style: MarkerStyle<'_>,
    ) -> (Vec<Func<'p>>, usize) {
        let symbol = if node.kind() == Kind::ShorthandPropertyAssignment {
            checker.get_shorthand_assignment_value_symbol(Some(node))
        } else {
            checker.get_symbol_at_location_exported(node)
        };
        let Some(mut symbol) = symbol else {
            return (Vec::new(), 0);
        };
        if symbol.flags().intersects(SymbolFlags::Alias) {
            symbol = checker.get_aliased_symbol(symbol);
        }
        let mut functions = Vec::new();
        let mut classes = 0;
        for declaration in symbol.declarations().to_vec() {
            let (found, class_count) = self.functions_of_declaration(checker, declaration, style);
            functions.extend(found);
            classes += class_count;
        }
        (functions, classes)
    }

    /// What one declaration is, as something a call would enter: the project functions with a body, and the number of project classes.
    pub(super) fn functions_of_declaration<'p>(
        &'p self,
        checker: &mut Checker,
        declaration: P<Node>,
        style: MarkerStyle<'_>,
    ) -> (Vec<Func<'p>>, usize) {
        let mut functions = Vec::new();
        let mut classes = 0;
        for target in callable_declarations(checker, declaration) {
            let Some(file_node) = ast::get_source_file_of_node(target) else {
                continue;
            };
            let file = File {
                node: file_node,
                _sem: PhantomData,
                _one_thread: PhantomData,
            };
            if !self.is_project_code(file) {
                continue;
            }
            let Some(defs) = self.file_defs(file, style) else {
                continue;
            };
            let Some(&def) = defs.def_of.get(&target) else {
                continue;
            };
            let held = Func { file, def };
            if self.is_function_with_body(held, style) {
                functions.push(held);
            } else if defs.outline.defs[def].kind == DefKind::Class {
                classes += 1;
            }
        }
        (functions, classes)
    }

    /// The function members of an object literal that are defs, through nested literals, each with the span of its key: a method, an arrow or a function
    /// expression held by a property (a def by D21's rule, or not: `def_of` says). A getter, a setter, a shorthand property (named in the literal, so a
    /// hand-over of its own where the literal is written) and a computed key are not looked at.
    fn functions_of_literal<'p>(
        &'p self,
        file: File<'p>,
        defs: &FileDefs,
        literal: P<Node>,
        out: &mut Vec<(Func<'p>, Span)>,
    ) {
        let text = file.node.text();
        for &member in literal.properties() {
            let key = member.name().map(|name| own_span(text, name));
            match member.kind() {
                Kind::MethodDeclaration => {
                    if let (Some(&def), Some(key)) = (defs.def_of.get(&member), key) {
                        out.push((Func { file, def }, key));
                    }
                }
                Kind::PropertyAssignment => {
                    let Some(value) = member.initializer().map(unwrap_expression) else {
                        continue;
                    };
                    if value.kind() == Kind::ObjectLiteralExpression {
                        self.functions_of_literal(file, defs, value, out);
                    } else if let (Some(&def), Some(key)) = (defs.def_of.get(&value), key) {
                        out.push((Func { file, def }, key));
                    }
                }
                _ => {}
            }
        }
    }

    /// A def that is a function written with a body: a function declaration, a function expression, an arrow, a method. Not a class, an accessor,
    /// a constructor, or a signature (an interface's method, an ambient `declare function`: nothing to walk).
    fn is_function_with_body(&self, func: Func<'_>, style: MarkerStyle<'_>) -> bool {
        let Some(defs) = self.file_defs(func.file, style) else {
            return false;
        };
        defs.nodes[func.def].is_some_and(|node| {
            matches!(
                node.kind(),
                Kind::FunctionDeclaration
                    | Kind::FunctionExpression
                    | Kind::ArrowFunction
                    | Kind::MethodDeclaration
            ) && node.body().is_some()
        })
    }
}

/// The member a destructured name stands for: `load` in `const { load } = api` is `api`'s `load`, `inner` in `const { a: { inner } } = api` is the `inner` of
/// `api.a`. The property is looked up on the type of the initializer, so a member the type does not have (an index signature, a rest element, a computed key,
/// an array pattern, a parameter with no initializer) gives nothing: those are not followed.
fn bound_symbol(checker: &mut Checker, element: P<Node>) -> Option<P<Symbol>> {
    let mut keys = Vec::new();
    let mut at = element;
    // from the element up through its patterns to the variable that is initialized
    let initializer = loop {
        let pattern = at.parent()?;
        if pattern.kind() != Kind::ObjectBindingPattern
            || at.as_binding_element().dot_dot_dot_token().is_some()
        {
            return None;
        }
        let key = at.property_name().or(at.name())?;
        if !(ast::is_identifier(key) || ast::is_string_literal_like(key)) {
            return None;
        }
        keys.push(key.text().to_string());
        let holder = pattern.parent()?;
        match holder.kind() {
            Kind::BindingElement => at = holder,
            Kind::VariableDeclaration => break holder.initializer()?,
            _ => return None,
        }
    };
    let mut ty = checker.get_type_at_location(initializer);
    let mut property = None;
    for key in keys.iter().rev() {
        let found = checker.get_property_of_type_exported(ty, key)?;
        ty = checker.get_type_of_symbol(found);
        property = Some(found);
    }
    property
}

/// The declarations of the member a destructured name stands for (`bound_symbol`).
fn bound_member(checker: &mut Checker, element: P<Node>) -> Vec<P<Node>> {
    bound_symbol(checker, element).map_or_else(Vec::new, |symbol| symbol.declarations().to_vec())
}

/// The member a name of a `for (const { load } of xs)` pattern stands for, looked up on the type of the pattern (the element type of `xs`). It exists to be
/// **counted**: following it would be an edge, and a `for … of` is not followed yet (`Uncovered::ForOfDestructuring`). `None` for a rest element, a computed key, a
/// pattern that is not the declaration of a `for … of` and a member the type does not have.
fn for_of_member(checker: &mut Checker, element: P<Node>) -> Option<P<Symbol>> {
    let mut keys = Vec::new();
    let mut at = element;
    let root = loop {
        let pattern = at.parent()?;
        if pattern.kind() != Kind::ObjectBindingPattern
            || at.as_binding_element().dot_dot_dot_token().is_some()
        {
            return None;
        }
        let key = at.property_name().or(at.name())?;
        if !(ast::is_identifier(key) || ast::is_string_literal_like(key)) {
            return None;
        }
        keys.push(key.text().to_string());
        let holder = pattern.parent()?;
        match holder.kind() {
            Kind::BindingElement => at = holder,
            Kind::VariableDeclaration => {
                // the declaration of a `for … of`: it has no initializer, and the other declarations with none (`for … in`, `catch`) give a string or `any`
                let list = holder.parent()?;
                if list.parent()?.kind() != Kind::ForOfStatement {
                    return None;
                }
                break pattern;
            }
            _ => return None,
        }
    };
    let mut ty = checker.get_type_at_location(root);
    let mut property = None;
    for key in keys.iter().rev() {
        let found = checker.get_property_of_type_exported(ty, key)?;
        ty = checker.get_type_of_symbol(found);
        property = Some(found);
    }
    property
}
