use tsrs_compiler::Context;

use super::*;

/// What a body hands over or calls in a way **no edge covers**, counted per occurrence (D22 step 5): a function handed over by an element access with a
/// literal key, a function handed over bound (`f.bind(…)`, `obj.m.bind(obj)`), a function called through `call` or `apply` (the tree has `Function.call`,
/// not the function), a class named where a value is handed over (a class is built, so a passed edge cannot point at it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Uncovered {
    ElementAccess,
    Bind,
    CallApply,
    Class,
    /// `const { value } = box` where `value` is a getter: the getter runs, and there is no property access to be its site.
    DestructuredAccessor,
    /// `const { a, ...others } = api; use(others)`: the rest object holds defs of `api`, handed over by the name, and a rest element is not followed.
    RestElement,
    /// `({ load } = api); use(load)`: the target takes a def of `api`, handed over by its name, and an assignment target is not followed.
    AssignmentDestructuring,
    /// One site of an instance field initializer of a class that a construction runs: the construction draws the constructor, not the initializers.
    ConstructInitializerSite,
    /// `<Cls />` where `Cls` is a class with a `render` method of the project: the framework calls `render`, and nothing in the project does.
    ClassComponentRender,
    /// `b.fn()` where `fn` is a getter: the one edge is the getter, and the call of what it returns is a second call.
    GetterResultCall,
    /// `for (const { load } of xs) use(load)`: the name stands for a member of the element, handed over by name, and a `for … of` pattern has no initializer to read.
    ForOfDestructuring,
}

impl Uncovered {
    /// The key in `Stats.language`.
    pub(crate) fn counter(self) -> &'static str {
        match self {
            Uncovered::ElementAccess => "uncovered_element_access",
            Uncovered::Bind => "uncovered_bind",
            Uncovered::CallApply => "uncovered_call_apply",
            Uncovered::Class => "uncovered_class",
            Uncovered::DestructuredAccessor => "uncovered_destructured_accessor",
            Uncovered::RestElement => "uncovered_rest_element",
            Uncovered::AssignmentDestructuring => "uncovered_assignment_destructuring",
            Uncovered::ConstructInitializerSite => "uncovered_construct_initializer_sites",
            Uncovered::ClassComponentRender => "uncovered_class_component_render",
            Uncovered::GetterResultCall => "uncovered_getter_result_calls",
            Uncovered::ForOfDestructuring => "uncovered_for_of_destructuring",
        }
    }
}

impl Sem {
    /// The sites of `class`'s own body that are in the initializers of its **instance** fields, as the span of each (`x = make()`): what a construction runs and does not
    /// draw. An arrow or a function expression held by a field is a def, so its calls are not in the class's body and not counted.
    pub(crate) fn initializer_sites(&self, class: Func<'_>, style: MarkerStyle<'_>) -> Vec<Span> {
        let (Some(body), Some(node)) = (self.body(class, style), self.node_of(class, style)) else {
            return Vec::new();
        };
        let initializers = instance_initializers(class.file.node.text(), node);
        body.sites
            .iter()
            .map(|site| site.span)
            .filter(|span| is_within(&initializers, *span))
            .collect()
    }

    /// Whether `class` has a `render` that is a project function with a body, its own or inherited from a class of the project: what a framework calls on
    /// a class component.
    pub(crate) fn has_render(&self, class: Func<'_>, style: MarkerStyle<'_>) -> bool {
        let Some(node) = self.node_of(class, style) else {
            return false;
        };
        let _scope = self._region.enter();
        let mut checker = self.program.get_type_checker(&Context::default());
        let Some(symbol) = checker.get_symbol_of_declaration_exported(node) else {
            return false;
        };
        let ty = checker.get_declared_type_of_symbol_exported(symbol);
        let Some(render) = checker.get_property_of_type_exported(ty, "render") else {
            return false;
        };
        render
            .declarations()
            .to_vec()
            .into_iter()
            .any(|declaration| {
                !self
                    .functions_of_declaration(&mut checker, declaration, style)
                    .0
                    .is_empty()
            })
    }
}

/// The spans of the initializers of `class`'s instance fields (`x = make()`).
fn instance_initializers(text: &str, class: P<Node>) -> Vec<Span> {
    class
        .members()
        .iter()
        .filter(|member| member.kind() == Kind::PropertyDeclaration && !ast::is_static(**member))
        .filter_map(|member| member.initializer())
        .map(|initializer| own_span(text, initializer))
        .collect()
}

/// Whether `inner` lies inside one of `outer`.
fn is_within(outer: &[Span], inner: Span) -> bool {
    outer
        .iter()
        .any(|span| span.start <= inner.start && inner.end <= span.end)
}
