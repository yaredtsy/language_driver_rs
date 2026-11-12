use analyzer_core::outline::DefKind;
use tsrs_compiler::Context;

use super::collect::{Collector, SiteKind};
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
    /// One site of a decorator of a class declared inside a walked body: the decorators run when the body does, and the class's own body is not a part of it.
    LocalClassDecoratorSite,
    /// `<Cls />` where `Cls` is a class with a `render` method of the project: the framework calls `render`, and nothing in the project does.
    ClassComponentRender,
    /// `b.fn()` where `fn` is a getter: the one edge is the getter, and the call of what it returns is a second call.
    GetterResultCall,
    /// `for (const { load } of xs) use(load)`: the name stands for a member of the element, handed over by name, and a `for … of` pattern has no initializer to read.
    ForOfDestructuring,
    /// One site in a computed member name (`[key()] = …`) of a class at the top of a file, counted where the class def is walked: the class def's body never reads
    /// the names, and no enclosing def is there to read them.
    ComputedMemberNameSite,
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
            Uncovered::LocalClassDecoratorSite => "uncovered_local_class_decorator_sites",
            Uncovered::ClassComponentRender => "uncovered_class_component_render",
            Uncovered::GetterResultCall => "uncovered_getter_result_calls",
            Uncovered::ForOfDestructuring => "uncovered_for_of_destructuring",
            Uncovered::ComputedMemberNameSite => "uncovered_computed_member_name_sites",
        }
    }
}

impl Sem {
    /// How many sites are in the computed member names of `class` (`[key()] = …`, `[make()]() {}`), when `class` is a class at the top of a file. A class's own
    /// body never reads its member names, and a class nested in a body has them read by the def around it (`Collector::collect`, which draws them there); a
    /// class at the top has no def around it, so the sites are in no tree. Calls, constructions, tags and JSX elements: an accessor in a name is not asked of the
    /// checker here.
    pub(super) fn computed_name_sites(&self, class: Func<'_>, style: MarkerStyle<'_>) -> usize {
        let Some(defs) = self.file_defs(class.file, style) else {
            return 0;
        };
        let def = &defs.outline.defs[class.def];
        let Some(node) = defs.nodes[class.def] else {
            return 0;
        };
        if def.kind != DefKind::Class || def.parent.is_some() {
            return 0;
        }
        let _scope = self._region.enter();
        let mut collector = Collector::new(class.file.node.text(), &defs.def_of, node);
        for &member in node.members() {
            // `expression()` is for a computed name only: asking an identifier for it is a bug in the engine's tree, not an empty answer
            if let Some(name) = member.name()
                && ast::is_computed_property_name(name)
            {
                collector.collect(name.expression());
            }
        }
        collector.sites.len()
    }

    /// How many sites of `class`'s own body are in decorators: the decorators of the class, of its members and of the parameters of its constructor and
    /// methods, and the calls and accessors inside them (`@factory(1)` is two sites: the call of `factory`, and of its result).
    pub(super) fn decorator_sites(&self, class: Func<'_>, style: MarkerStyle<'_>) -> usize {
        let Some(body) = self.body(class, style) else {
            return 0;
        };
        let text = class.file.node.text();
        let decorators: Vec<Span> = body
            .sites
            .iter()
            .filter(|site| site.kind == SiteKind::Decorator)
            .map(|site| own_span(text, site.call))
            .collect();
        body.sites
            .iter()
            .filter(|site| {
                decorators
                    .iter()
                    .any(|d| d.start <= site.span.start && site.span.end <= d.end)
            })
            .count()
    }

    /// How many sites of `class`'s own body are in the initializers of its **instance** fields (`x = make()`): what a construction runs and does not
    /// draw. An arrow or a function expression held by a field is a def, so its calls are not in the class's body and not counted.
    pub(crate) fn initializer_sites(&self, class: Func<'_>, style: MarkerStyle<'_>) -> usize {
        let (Some(body), Some(defs)) = (self.body(class, style), self.file_defs(class.file, style))
        else {
            return 0;
        };
        let Some(node) = defs.nodes[class.def] else {
            return 0;
        };
        let text = class.file.node.text();
        let initializers: Vec<Span> = node
            .members()
            .iter()
            .filter(|member| {
                member.kind() == Kind::PropertyDeclaration && !ast::is_static(**member)
            })
            .filter_map(|member| member.initializer())
            .map(|initializer| own_span(text, initializer))
            .collect();
        body.sites
            .iter()
            .filter(|site| {
                initializers
                    .iter()
                    .any(|i| i.start <= site.span.start && site.span.end <= i.end)
            })
            .count()
    }

    /// Whether `class` has a `render` that is a project function with a body, its own or inherited from a class of the project: what a framework calls on
    /// a class component.
    pub(crate) fn has_render(&self, class: Func<'_>, style: MarkerStyle<'_>) -> bool {
        let Some(node) = self
            .file_defs(class.file, style)
            .and_then(|defs| defs.nodes[class.def])
        else {
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
