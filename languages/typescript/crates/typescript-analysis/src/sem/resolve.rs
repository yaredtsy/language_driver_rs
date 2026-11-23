use analyzer_core::graph::{DefInfo, StableDef};
use analyzer_core::outline::DefKind;
#[cfg(feature = "__tc0_counts")]
use tsrs_ast::Symbol;
use tsrs_ast::SymbolFlags;
use tsrs_checker::{Checker, TypeFlags};
#[cfg(feature = "__tc0_counts")]
use tsrs_checker::{ObjectFlags, Type};

use super::collect::{Access, AccessCandidate, CallNode, SiteKind, unwrap_expression};
use super::hierarchy::{
    declarations_of, declarations_of_signature, declarations_of_union_symbol,
    is_property_function_value, symbol_of_declaration,
};
#[cfg(feature = "__tc0_counts")]
use super::view::{CallOrNew, ElementAccess, JsxAttrs, properties_of};
use super::view::{expression_of, initializer_of, spelled};
use super::*;

/// What one declaration the engine found for a site is.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Resolved<'p> {
    /// A def of project code: a function, method, accessor or constructor, or a class. The solver walks it.
    Def(Func<'p>),
    /// Outside project code: the package (`pkg`, `typescript:lib`, or the path from the root of a `.d.ts` of the repo's, or of a file outside the root) and the declaration's own name.
    /// `outside_root` is true for a source file outside the root (D33): a body that was there to walk and is not, which `SiteFacts` counts.
    External {
        package: String,
        name: String,
        outside_root: bool,
    },
    /// The engine gives no declaration to enter.
    Unknown(Why),
}

/// What is known about a site beyond what it resolves to, found while the declarations were at hand.
#[derive(Clone, Default)]
#[cfg_attr(not(feature = "__tc0_counts"), derive(Copy))]
pub(crate) struct SiteFacts {
    /// The site is a JSX element: a class it constructs is a class component.
    pub(crate) jsx: bool,
    /// The callee is a getter (`b.fn()`): the edge is to the getter, and the call of what it returns has no edge.
    pub(crate) getter_result: bool,
    /// How many targets of the site are source files outside the root (`Resolved::External::outside_root`), once per target.
    pub(crate) outside_root: u64,
    /// Where the function of a `function_typed_value` answer comes from, when the site has one (`FunctionOrigin`).
    pub(crate) function_typed: Option<FunctionOrigin>,
    /// The TC0 counts of a `function_typed_value` site (`Tc0Counts`).
    #[cfg(feature = "__tc0_counts")]
    pub(crate) tc0: Option<Tc0Counts>,
}

/// The TC0 counts of one `function_typed_value` site (04-tc0-counts-and-spikes.md §3.3): the keys to note, once per such edge.
#[cfg(feature = "__tc0_counts")]
#[derive(Clone, Debug, Default)]
pub(crate) struct Tc0Counts {
    /// Keys without a run-time part: `tc0_n2_own`, `tc0_n2_depth_1`, `tc0_n3_value`, `tc0_n8_project`, …
    pub(crate) fixed: Vec<&'static str>,
    /// Keys with one: `tc0_n8_package:<package>:<name>`.
    pub(crate) named: Vec<String>,
}

/// Where the function a `function_typed_value` call goes through comes from, told by the **leftmost name** of the callee expression: `onClose` in `onClose()`,
/// `props` in `props.onClose()`, `fs` in `fs.existsSync(p)`. Parentheses, assertions, `!` and `?.` are looked through; a call in the way (`make().run()`) has no
/// leftmost name. It says, per project, how much each step of the context layer would answer (`docs/03-typescript/08-later-context-sensitive.md` §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FunctionOrigin {
    /// A parameter, or a name taken out of one by a pattern (`function Panel({ onClose })`).
    Parameter,
    /// A variable, or a name taken out of one by a pattern, whose initializer is a call (`const [open, setOpen] = useState()`, `const useStore = create(…)`).
    CallResult,
    /// `this`.
    This,
    /// Anything else, a callee with no leftmost name included.
    Other,
}

impl FunctionOrigin {
    /// Every origin, in the order the breakdown says them.
    pub(crate) const ALL: [FunctionOrigin; 4] = [
        FunctionOrigin::Parameter,
        FunctionOrigin::CallResult,
        FunctionOrigin::This,
        FunctionOrigin::Other,
    ];

    /// The key in `Stats.language`, beside `function_typed_value`, whose count is the sum of these four.
    pub(crate) fn counter(self) -> &'static str {
        match self {
            FunctionOrigin::Parameter => "function_typed_value_from_parameter",
            FunctionOrigin::CallResult => "function_typed_value_from_call_result",
            FunctionOrigin::This => "function_typed_value_from_this",
            FunctionOrigin::Other => "function_typed_value_from_other",
        }
    }

    /// What the breakdown calls it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            FunctionOrigin::Parameter => "a parameter",
            FunctionOrigin::CallResult => "a call's result",
            FunctionOrigin::This => "this",
            FunctionOrigin::Other => "something else",
        }
    }
}

/// What runs when a class is constructed: the first constructor up its chain of base classes, since a class with none uses its base's.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Init<'p> {
    /// The constructor to enter: the class's own, or the nearest base class's in project code.
    Enter(Func<'p>),
    /// No class on the chain declares one: there is nothing to enter.
    Nothing,
    /// The chain reaches a base class outside project code (a package's, a library's): its constructor is never entered, and the class is not
    /// the project's to look into.
    External,
    /// The chain reaches a base the checker cannot name as a class (`extends mixin(Base)`, a value of type `any`): we do not know.
    Unknown,
}

/// Why a site has no target. Each is counted apart in `Stats.language` (D16, rule 8), so a missing edge can be explained from a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Why {
    /// A callback parameter, a prop, a store action, a field of type `() => void`: the engine answers with a function type, which has no body (D16).
    FunctionTypedValue,
    /// A value of type `any`, or an untyped JavaScript value.
    AnyOrUntyped,
    /// The receiver is a union (`JsonWriter | CsvWriter`) and a member of it has no declaration to enter (a member of type `any`, a function
    /// type). The members that do are targets; this marks the ones that are not, so the answer is not mistaken for complete.
    UnionReceiver,
    /// `obj[name]()` where the checker cannot narrow `name` to a literal.
    ComputedName,
    /// The engine found a declaration in project code that is not a def of the outline: a bug in one of the two rules (rule 8: counted).
    NotInOutline,
    /// `@decorator(1)`: the function the factory returned is called with what is decorated, and nothing names it (an arrow written in the factory, a
    /// function type): the second call of a decorator that takes arguments.
    DecoratorResult,
    /// `(() => {…})()`, `(function () {…})()`: the call of a function expression written where it is called. Nothing about it is unknown: the function is
    /// right there, part of the body that calls it (its calls are drawn in that body), and no declaration is to be entered (H10).
    AnonymousFunctionCalled,
    /// None of the above: no symbol, or a symbol with nothing to call.
    NoDeclaration,
}

impl Why {
    /// The key in `Stats.language`.
    pub(crate) fn counter(self) -> &'static str {
        match self {
            Why::FunctionTypedValue => "function_typed_value",
            Why::AnyOrUntyped => "any_or_untyped",
            Why::UnionReceiver => "union_receiver",
            Why::ComputedName => "computed_name",
            Why::NotInOutline => "decl_not_in_outline",
            Why::DecoratorResult => "decorator_result",
            Why::AnonymousFunctionCalled => "anonymous_function_called",
            Why::NoDeclaration => "no_declaration",
        }
    }
}

impl Sem {
    /// What the site at `site_index` of `func` calls, as the engine's call hierarchy would say. Never empty: a site with no target is one
    /// `Unknown`. A union receiver, or merged declarations, give one `Resolved` per declaration, in (file, offset) order (rule 9).
    #[cfg(test)]
    pub(crate) fn callee(
        &self,
        func: Func<'_>,
        site_index: usize,
        style: MarkerStyle<'_>,
    ) -> Vec<Resolved<'_>> {
        self.callee_with_facts(func, site_index, style).0
    }

    /// `callee`, and what else is known about the site while the declarations are at hand (`SiteFacts`).
    pub(crate) fn callee_with_facts(
        &self,
        func: Func<'_>,
        site_index: usize,
        style: MarkerStyle<'_>,
    ) -> (Vec<Resolved<'_>>, SiteFacts) {
        let (found, mut facts) = self.callee_found(func, site_index, style);
        facts.outside_root = found
            .iter()
            .filter(|resolved| {
                matches!(
                    resolved,
                    Resolved::External {
                        outside_root: true,
                        ..
                    }
                )
            })
            .count() as u64;
        if found.contains(&Resolved::Unknown(Why::FunctionTypedValue)) {
            facts.function_typed = self.function_origin(func, site_index, style);
            #[cfg(feature = "__tc0_counts")]
            if let Some(origin) = facts.function_typed {
                facts.tc0 = self.tc0_counts(func, site_index, style, origin);
            }
        }
        (found, facts)
    }

    /// Where the function of the site comes from (`FunctionOrigin`), read from the leftmost name of its callee expression. `None` when the def has no body to
    /// read the site from.
    fn function_origin(
        &self,
        func: Func<'_>,
        site_index: usize,
        style: MarkerStyle<'_>,
    ) -> Option<FunctionOrigin> {
        let calls = self.body(func, style)?;
        let callee = calls.sites.get(site_index)?.expr;
        let _scope = self.region.enter();
        let mut checker = self.checker();
        let Some(name) = leftmost_name(callee) else {
            return Some(FunctionOrigin::Other);
        };
        if name.kind() == Kind::ThisKeyword {
            return Some(FunctionOrigin::This);
        }
        let Some(mut symbol) = checker.get_symbol_at_location_exported(name) else {
            return Some(FunctionOrigin::Other);
        };
        // through an import: the declaration it leads to
        if symbol.flags().intersects(SymbolFlags::Alias) {
            symbol = checker.get_aliased_symbol(symbol);
        }
        let origin = symbol
            .declarations()
            .first()
            .map_or(FunctionOrigin::Other, |declaration| {
                origin_of_declaration(*declaration)
            });
        Some(origin)
    }

    /// The TC0 counts of the site (N2 and N3 for a parameter, N8 for a call's result), read from the declaration of the leftmost name of its callee. `None` for the
    /// other origins, and when the tree is not what the origin said it was (the counts then do not add up, and a test says so).
    #[cfg(feature = "__tc0_counts")]
    fn tc0_counts(
        &self,
        func: Func<'_>,
        site_index: usize,
        style: MarkerStyle<'_>,
        origin: FunctionOrigin,
    ) -> Option<Tc0Counts> {
        if !matches!(
            origin,
            FunctionOrigin::Parameter | FunctionOrigin::CallResult
        ) {
            return None;
        }
        let calls = self.body(func, style)?;
        let callee = calls.sites.get(site_index)?.expr;
        let def_node = self.node_of(func, style);
        let _scope = self.region.enter();
        let mut checker = self.checker();
        let name = leftmost_name(callee)?;
        let declaration = first_declaration(&mut checker, name)?;
        let (pattern, owner) = walk_out_of_patterns(declaration)?;
        let members = member_steps(callee);
        let mut counts = Tc0Counts::default();
        if origin == FunctionOrigin::Parameter {
            let function = owner.parent()?;
            counts.fixed.push(match def_node {
                Some(def) if function == def => "tc0_n2_own",
                Some(def) if is_inside(def, function) => "tc0_n2_enclosing",
                Some(def) if is_inside(function, def) => "tc0_n2_anonymous",
                _ => "tc0_n2_other",
            });
            counts.fixed.push(match pattern + members {
                0 => "tc0_n2_depth_0",
                1 => "tc0_n2_depth_1",
                2 => "tc0_n2_depth_2",
                _ => "tc0_n2_depth_3_or_more",
            });
            counts
                .fixed
                .push(match callee_declaration_kind(&mut checker, callee) {
                    Some(
                        Kind::MethodSignature
                        | Kind::MethodDeclaration
                        | Kind::GetAccessor
                        | Kind::SetAccessor,
                    ) => "tc0_n3_method",
                    Some(_) => "tc0_n3_value",
                    None => "tc0_n3_unknown",
                });
        } else {
            let call = initializer_of(owner).and_then(call_of_initializer)?;
            match self.tc0_declared_by(&mut checker, call) {
                CallDeclaredBy::Unknown => counts.fixed.push("tc0_n8_unknown"),
                CallDeclaredBy::Project => counts.fixed.push("tc0_n8_project"),
                CallDeclaredBy::Package { package, name } => {
                    counts
                        .named
                        .push(format!("tc0_n8_package:{package}:{name}"));
                    if members == 0 {
                        counts.fixed.push("tc0_n8_package_bound_direct");
                    }
                }
            }
        }
        Some(counts)
    }

    /// What declares the function a call resolves to, as N8 and N1's `call_result_*` read it: the declaration of the call's resolved signature.
    #[cfg(feature = "__tc0_counts")]
    fn tc0_declared_by(&self, checker: &mut Checker, call: P<Node>) -> CallDeclaredBy {
        let signature = checker.get_resolved_signature_exported(call);
        let Some(declaration) = signature.declaration() else {
            return CallDeclaredBy::Unknown;
        };
        let Some(file) = file_of(declaration) else {
            return CallDeclaredBy::Unknown;
        };
        if self.is_project_code(file) {
            return CallDeclaredBy::Project;
        }
        let package = self.package_of(file).unwrap_or_else(|| self.place_of(file));
        let name = declaration
            .name()
            .and_then(|name| spelled(file.node.text(), name))
            .unwrap_or_else(|| "?".to_owned());
        CallDeclaredBy::Package { package, name }
    }

    /// N1 (04-tc0-counts-and-spikes.md §3.4): what the arguments of a site with a project target are, by class: the keys to note, once per site. `None` for a
    /// site that passes nothing (a tagged template, a decorator, an accessor) and when the tree is not what the site said it was.
    #[cfg(feature = "__tc0_counts")]
    pub(crate) fn tc0_n1(
        &self,
        func: Func<'_>,
        site_index: usize,
        style: MarkerStyle<'_>,
    ) -> Option<Tc0Counts> {
        let calls = self.body(func, style)?;
        let site = *calls.sites.get(site_index)?;
        let _scope = self.region.enter();
        let mut checker = self.checker();
        let mut counts = Tc0Counts::default();
        match site.kind {
            SiteKind::Call => {
                let call = CallOrNew::cast(site.call)?;
                let signature = checker.get_resolved_signature_exported(site.call);
                for (index, argument) in call.arguments().iter().enumerate() {
                    if index >= signature.parameters().len() && !signature.has_rest_parameter() {
                        counts.named.push("tc0_n1:arg:no_slot".to_owned());
                        continue;
                    }
                    let slot = checker.get_type_at_position_exported(signature, index);
                    if !is_capable(slot) {
                        counts.named.push("tc0_n1:arg:data_slot".to_owned());
                        continue;
                    }
                    let class = self.tc0_argument_class(&mut checker, *argument, style);
                    counts.named.push(format!("tc0_n1:arg:{class}"));
                    if is_class_instance(&mut checker, *argument) {
                        counts.named.push("tc0_n1:static_class".to_owned());
                    }
                }
            }
            SiteKind::Jsx => {
                // `site.call` is the opening element, or the self-closing one: the node that holds the attributes
                for attribute in JsxAttrs::cast(site.call)?.attributes() {
                    match attribute.kind() {
                        Kind::JsxSpreadAttribute => {
                            counts.named.push("tc0_n1:jsx:spread".to_owned())
                        }
                        Kind::JsxAttribute => {
                            // `a="x"` and `a` hold no expression: nothing is passed that could be followed
                            let Some(value) = initializer_of(*attribute)
                                .filter(|value| value.kind() == Kind::JsxExpression)
                                .and_then(expression_of)
                            else {
                                continue;
                            };
                            if !is_capable(checker.get_type_at_location(value)) {
                                counts.named.push("tc0_n1:jsx:data_slot".to_owned());
                                continue;
                            }
                            let class = self.tc0_argument_class(&mut checker, value, style);
                            counts.named.push(format!("tc0_n1:jsx:{class}"));
                            if is_class_instance(&mut checker, value) {
                                counts.named.push("tc0_n1:static_class".to_owned());
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => return None,
        }
        Some(counts)
    }

    /// What an argument is (the table of §3.4), the first row that fits, after parentheses, `as`, `satisfies` and `!`.
    #[cfg(feature = "__tc0_counts")]
    fn tc0_argument_class(
        &self,
        checker: &mut Checker,
        node: P<Node>,
        style: MarkerStyle<'_>,
    ) -> String {
        let node = unwrap_expression(node);
        match node.kind() {
            Kind::SpreadElement => return "spread".to_owned(),
            Kind::ArrowFunction => return "anonymous".to_owned(),
            Kind::FunctionExpression => {
                return if node.name().is_none() {
                    "anonymous"
                } else {
                    "def"
                }
                .to_owned();
            }
            Kind::ObjectLiteralExpression => {
                let holds_function =
                    properties_of(node)
                        .iter()
                        .any(|property| match property.kind() {
                            Kind::MethodDeclaration | Kind::GetAccessor | Kind::SetAccessor => true,
                            Kind::PropertyAssignment => {
                                initializer_of(*property).is_some_and(|value| {
                                    matches!(
                                        self.tc0_argument_class(checker, value, style).as_str(),
                                        "anonymous" | "def"
                                    )
                                })
                            }
                            Kind::ShorthandPropertyAssignment => checker
                                .get_shorthand_assignment_value_symbol(Some(*property))
                                .is_some_and(|symbol| self.is_project_def(symbol, style)),
                            _ => false,
                        });
                return if holds_function {
                    "object_with_function"
                } else {
                    "object_other"
                }
                .to_owned();
            }
            Kind::NewExpression => return "new".to_owned(),
            Kind::CallExpression => {
                return match self.tc0_declared_by(checker, node) {
                    CallDeclaredBy::Project => "call_result_project",
                    CallDeclaredBy::Package { .. } => "call_result_external",
                    CallDeclaredBy::Unknown => "call_result_unknown",
                }
                .to_owned();
            }
            _ => {}
        }
        let is_member = matches!(
            node.kind(),
            Kind::PropertyAccessExpression | Kind::ElementAccessExpression
        );
        if (is_member || node.kind() == Kind::Identifier)
            && own_symbol(checker, node).is_some_and(|symbol| self.is_project_def(symbol, style))
        {
            return "def".to_owned();
        }
        if is_member {
            let Some(name) = leftmost_name(node) else {
                return "other_member".to_owned();
            };
            if name.kind() == Kind::ThisKeyword {
                return "this_member".to_owned();
            }
            return match name_origin(checker, name) {
                Some(NameOrigin::Parameter) => "parameter_member",
                Some(NameOrigin::ModuleVariable) => "module_variable_member",
                Some(NameOrigin::Local) => "local_member",
                None => "other_member",
            }
            .to_owned();
        }
        if node.kind() == Kind::Identifier {
            return match name_origin(checker, node) {
                Some(NameOrigin::Parameter) => "parameter".to_owned(),
                Some(NameOrigin::ModuleVariable) => "module_variable".to_owned(),
                Some(NameOrigin::Local) => "local".to_owned(),
                None => "other:Identifier".to_owned(),
            };
        }
        format!("other:{:?}", node.kind())
    }

    /// Whether one of the symbol's declarations is a def of the outline of a file of project code, or holds one: a variable, property declaration or property
    /// assignment whose initializer is.
    #[cfg(feature = "__tc0_counts")]
    fn is_project_def(&self, symbol: P<Symbol>, style: MarkerStyle<'_>) -> bool {
        symbol.declarations().iter().any(|declaration| {
            let Some(file) = file_of(*declaration) else {
                return false;
            };
            if !self.is_project_code(file) {
                return false;
            }
            let Some(defs) = self.file_defs(file, style) else {
                return false;
            };
            defs.def_of.contains_key(declaration)
                || (matches!(
                    declaration.kind(),
                    Kind::VariableDeclaration
                        | Kind::PropertyDeclaration
                        | Kind::PropertyAssignment
                ) && initializer_of(*declaration)
                    .is_some_and(|value| defs.def_of.contains_key(&unwrap_expression(value))))
        })
    }

    fn callee_found(
        &self,
        func: Func<'_>,
        site_index: usize,
        style: MarkerStyle<'_>,
    ) -> (Vec<Resolved<'_>>, SiteFacts) {
        let mut facts = SiteFacts::default();
        let Some(calls) = self.body(func, style) else {
            return (vec![Resolved::Unknown(Why::NoDeclaration)], facts);
        };
        let Some(call) = calls.sites.get(site_index) else {
            debug_assert!(false, "a site index outside the def's sites");
            return (vec![Resolved::Unknown(Why::NoDeclaration)], facts);
        };
        facts.jsx = call.kind == SiteKind::Jsx;
        let _scope = self.region.enter();
        // one checker, so one handle at a time: it is dropped before this returns
        let mut checker = self.checker();
        let callee = call.expr;
        if let SiteKind::Accessor(access) = call.kind {
            return (
                self.accessor_targets(&mut checker, callee, access, style),
                facts,
            );
        }
        if call.kind == SiteKind::Decorator
            && unwrap_expression(callee).kind() == Kind::CallExpression
        {
            return (
                self.decorator_result_targets(&mut checker, call.call, style),
                facts,
            );
        }
        // a named function expression called where it is written is a def of the outline, and the engine has no answer for it: the call is a Call to that def
        // (H11). An anonymous one is no def: its calls are the enclosing body's, and its own call is `anonymous_function_called` (H10, `why_unknown`).
        if let Some(function) = self.named_function_expression_called_in_place(func, callee, style)
        {
            return (vec![self.classify(&mut checker, function, style)], facts);
        }
        // the same for a class: `new (class N {})()` constructs the def `N`, as `new C()` does (H12 a), through `walk::construction`. Only under `new`: calling a
        // class is an error, not a construction. An anonymous class is no def and stays unresolved.
        if call.call.kind() == Kind::NewExpression
            && let Some(class) =
                self.named_class_expression_constructed_in_place(func, callee, style)
        {
            return (vec![self.classify(&mut checker, class, style)], facts);
        }
        // An anonymous function or class written right where it is called or constructed, with no parentheses (`new class {}()`, `!function () {}()`), is the
        // declaration itself: nothing to enter, as with parentheses (H10, H12). The engine's call hierarchy calls it a possible declaration and searches **up**
        // for an enclosing one, and answers the function, the method or the file around it (H15).
        if is_anonymous_function_or_class(unwrap_expression(callee)) {
            return (
                vec![Resolved::Unknown(why_unknown(&mut checker, callee))],
                facts,
            );
        }
        let mut declarations = self.prefer_project_bodies(declarations_of(&mut checker, callee));
        let mut unresolved_members = false;
        if declarations.is_empty()
            && let Some((members, all_resolved)) =
                declarations_of_union_symbol(&mut checker, callee)
        {
            // checked before the signature: for a union the checker's signature names the first member only, which would pass for the whole answer
            declarations = members;
            unresolved_members = !all_resolved;
        }
        if declarations.is_empty()
            && is_named_callee(callee)
            && !is_union_callee(&mut checker, callee)
        {
            // Only for a name, and not for a union of callables: the signature names one declaration, which is the answer when the callee is the
            // named thing and would pass for the whole answer when it is `(flag ? f : g)` (the checker reduces two alike function types to one)
            declarations = declarations_of_signature(&mut checker, call.call);
        }
        if declarations.is_empty() {
            if let Some(external) = self.external_declarations(&mut checker, callee) {
                return (external, facts);
            }
            return (
                vec![Resolved::Unknown(why_unknown(&mut checker, callee))],
                facts,
            );
        }
        // the engine names the getter itself for `b.fn()`: one edge, and what the getter returns is called by a second call that has none
        facts.getter_result = declarations
            .iter()
            .any(|declaration| declaration.kind() == Kind::GetAccessor);
        let mut found = self.classify_all(&mut checker, declarations, style);
        if unresolved_members {
            // a member of the union that has no declaration to enter is not dropped
            found.push(Resolved::Unknown(Why::UnionReceiver));
        }
        (found, facts)
    }

    /// The node of a **named** function expression that `callee` is, written where it is called (H11), when the outline holds it as a def (`def_of`). `None` for
    /// an anonymous one, which is no def, and for anything else. Also what `.call` / `.apply` is asked on (H12 b): a named function expression there is a def that
    /// nothing draws.
    pub(super) fn named_function_expression_called_in_place(
        &self,
        func: Func<'_>,
        callee: P<Node>,
        style: MarkerStyle<'_>,
    ) -> Option<P<Node>> {
        let function =
            function_called_in_place(callee).filter(|f| f.kind() == Kind::FunctionExpression)?;
        self.is_def_of_the_file(func, function, style)
            .then_some(function)
    }

    /// The node of a **named** class expression that `callee` is, written where it is constructed (H12 a), under the parentheses and assertions of the callee, when the
    /// outline holds it as a def.
    fn named_class_expression_constructed_in_place(
        &self,
        func: Func<'_>,
        callee: P<Node>,
        style: MarkerStyle<'_>,
    ) -> Option<P<Node>> {
        let class = unwrap_expression(callee);
        (class.kind() == Kind::ClassExpression && self.is_def_of_the_file(func, class, style))
            .then_some(class)
    }

    fn is_def_of_the_file(&self, func: Func<'_>, node: P<Node>, style: MarkerStyle<'_>) -> bool {
        self.file_defs(func.file, style)
            .is_some_and(|defs| defs.def_of.contains_key(&node))
    }

    /// The property accesses of a body that run an accessor, as sites: for each candidate the checker's symbol of the property is asked, and it is a site when
    /// a declaration of it is an accessor of the half the access needs (a getter for a read, a setter for a write, both for `+=`). One checker for the whole
    /// body, and none when the body has no candidate. A property of a union that is an accessor in some members and a plain property in others is a site for
    /// the accessors only: the plain property runs nothing.
    pub(super) fn accessor_sites(
        &self,
        candidates: &[AccessCandidate],
        text: &str,
    ) -> Vec<CallNode> {
        if candidates.is_empty() {
            return Vec::new();
        }
        let mut checker = self.checker();
        candidates
            .iter()
            .filter(|candidate| {
                accessor_declarations(&mut checker, candidate.location, candidate.access)
                    .is_some_and(|found| !found.is_empty())
            })
            .map(|candidate| CallNode {
                span: own_span(text, candidate.node),
                expr: candidate.location,
                call: candidate.node,
                kind: SiteKind::Accessor(candidate.access),
            })
            .collect()
    }

    /// What an accessor site runs: the getter for a read, the setter for a write, both for `+=` and `++`, as defs of the project or externals, in (file, offset)
    /// order. The site exists only when there is one (`accessor_sites`).
    fn accessor_targets(
        &self,
        checker: &mut Checker,
        location: P<Node>,
        access: Access,
        style: MarkerStyle<'_>,
    ) -> Vec<Resolved<'_>> {
        let mut declarations = accessor_declarations(checker, location, access).unwrap_or_default();
        declarations.sort_by_key(|declaration| {
            (
                ast::get_source_file_of_node(*declaration).map(|file| file.file_name().to_owned()),
                declaration.pos(),
            )
        });
        let mut found = self.classify_all(checker, declarations, style);
        if found.is_empty() {
            found.push(Resolved::Unknown(Why::NoDeclaration));
        }
        found
    }

    /// The second call of `@decorator(1)`: the function that `decorator(1)` returns is called with what is decorated. The first call is a site of its own (a
    /// plain call of `decorator`). The checker's resolved signature of the decorator names the returned function when it is a declaration that can be
    /// entered (`return apply;` with a function `apply`); an arrow written in the factory's `return` is part of the factory's body already (it is no def), and a
    /// function type (`ClassDecorator`) has nothing to enter, so those are `Unknown(DecoratorResult)`: counted, never dropped.
    fn decorator_result_targets(
        &self,
        checker: &mut Checker,
        decorator: P<Node>,
        style: MarkerStyle<'_>,
    ) -> Vec<Resolved<'_>> {
        let declarations = declarations_of_signature(checker, decorator);
        let mut found: Vec<Resolved<'_>> = Vec::new();
        for declaration in declarations {
            let resolved = self.classify(checker, declaration, style);
            if !found.contains(&resolved) {
                found.push(resolved);
            }
        }
        if found.is_empty() {
            found.push(Resolved::Unknown(Why::DecoratorResult));
        }
        found
    }

    /// Our choice among the engine's declarations, for one case (pinned in `ENGINE.md` and the policy table): in a script file a function named like a
    /// library global (`fetch`, `open`, `close`, `name`) is the same symbol as the library's, so the checker lists both and a call to the project's
    /// function would also be an `External` edge to something never called. When a declaration with a **body** is in project code and every other one
    /// is in a declaration file (a bundled library, a package's `.d.ts`), only the project's stay. If no project declaration has a body, or another
    /// declaration is project code, nothing changes: the engine's list is the answer.
    fn prefer_project_bodies(&self, declarations: Vec<P<Node>>) -> Vec<P<Node>> {
        let is_declaration_file = |declaration: &P<Node>| {
            ast::get_source_file_of_node(*declaration)
                .is_some_and(|file| tspath::is_declaration_file_name(file.file_name()))
        };
        let is_project_body = |declaration: &P<Node>| {
            declaration.body().is_some()
                && !is_declaration_file(declaration)
                && ast::get_source_file_of_node(*declaration).is_some_and(|file| {
                    self.is_project_code(File {
                        node: file,
                        _sem: PhantomData,
                        _one_thread: PhantomData,
                    })
                })
        };
        let wins: Vec<P<Node>> = declarations
            .iter()
            .copied()
            .filter(|declaration| is_project_body(declaration))
            .collect();
        let others_are_libraries = declarations
            .iter()
            .filter(|declaration| !wins.contains(declaration))
            .all(is_declaration_file);
        if !wins.is_empty() && others_are_libraries {
            wins
        } else {
            declarations
        }
    }

    /// A callee whose every declaration is **outside** project code, when the engine's hierarchy has no body to enter: a named call into that package
    /// or library, an `External`. It is a holder of a value in a package (`export declare const useThing: () => number`, a property of function type:
    /// not one of D16's function-typed values, which are the ones declared in the project), and it is a library global declared as an interface and a
    /// `var`, which has no body either: `new Map()`, `String(x)`, `Date()`. `None` when any declaration is the project's, so a project's own holder
    /// stays D16's.
    fn external_declarations(
        &self,
        checker: &mut Checker,
        callee: P<Node>,
    ) -> Option<Vec<Resolved<'_>>> {
        // `super(…)` names the base class's constructor: the checker's symbol for the keyword is the constructor's *type* (`ErrorConstructor`), and
        // the same class must not have two names (`new Error()` is `Error`), so the base is read from the `extends` expression, as `init_chain` does
        let callee = if callee.kind() == Kind::SuperKeyword {
            let class = ast::find_ancestor(Some(callee), ast::is_class_like)?;
            expression_of(ast::get_class_extends_heritage_element(class)?)?
        } else {
            callee
        };
        let mut symbol = checker.get_symbol_at_location_exported(callee)?;
        if symbol.flags().intersects(SymbolFlags::Alias) {
            symbol = checker.get_aliased_symbol(symbol);
        }
        let declarations = symbol.declarations().to_vec();
        if declarations.is_empty() {
            return None;
        }
        let mut found: Vec<Resolved<'_>> = Vec::new();
        for declaration in declarations {
            let file = File {
                node: ast::get_source_file_of_node(declaration)?,
                _sem: PhantomData,
                _one_thread: PhantomData,
            };
            if self.is_project_code(file) {
                return None;
            }
            let mut resolved = self.classify(checker, declaration, MarkerStyle::docstring(""));
            // a property of an anonymous type (`declare const toolbox: { run: () => void }`) has no class or interface around it to qualify
            // its name: the receiver does (`toolbox.run`)
            if let Resolved::External { name, .. } = &mut resolved
                && ast::is_property_access_expression(callee)
                && !name.contains('.')
                && let Some(receiver) = expression_of(callee)
                && let Some(receiver_symbol) = checker.get_symbol_at_location_exported(receiver)
            {
                *name = format!(
                    "{}.{name}",
                    checker.symbol_to_string_exported(receiver_symbol)
                );
            }
            if !found.contains(&resolved) {
                found.push(resolved);
            }
        }
        Some(found)
    }

    /// `classify` for each declaration, in order, each answer once. A signature that stands for whatever implements it (`is_signature_without_a_body`) is
    /// `FunctionTypedValue` (D43). When the site has other targets, the receiver is a union and that member is the one with nothing to enter: it is
    /// `UnionReceiver`, as a member of function type is, so the answer is not mistaken for complete.
    fn classify_all(
        &self,
        checker: &mut Checker,
        declarations: Vec<P<Node>>,
        style: MarkerStyle<'_>,
    ) -> Vec<Resolved<'_>> {
        // each answer, and whether it is a signature's
        let mut found: Vec<(Resolved<'_>, bool)> = Vec::new();
        for declaration in declarations {
            let resolved = self.classify(checker, declaration, style);
            if found.iter().all(|(known, _)| *known != resolved) {
                let signature = resolved == Resolved::Unknown(Why::FunctionTypedValue)
                    && is_signature_without_a_body(declaration);
                found.push((resolved, signature));
            }
        }
        let has_other_targets = found.iter().any(|(_, signature)| !signature);
        let mut answers: Vec<Resolved<'_>> = Vec::new();
        for (resolved, signature) in found {
            let resolved = if signature && has_other_targets {
                Resolved::Unknown(Why::UnionReceiver)
            } else {
                resolved
            };
            if !answers.contains(&resolved) {
                answers.push(resolved);
            }
        }
        answers
    }

    /// A declaration the engine found, as a def of project code, an external, or a bug to count.
    fn classify(
        &self,
        checker: &mut Checker,
        declaration: P<Node>,
        style: MarkerStyle<'_>,
    ) -> Resolved<'_> {
        let Some(source_file) = ast::get_source_file_of_node(declaration) else {
            return Resolved::Unknown(Why::NoDeclaration);
        };
        let file = File {
            node: source_file,
            _sem: PhantomData,
            _one_thread: PhantomData,
        };
        if !self.is_project_code(file) {
            // not in `node_modules` and not a bundled library: a `.d.ts` of the repo's, or a file outside the root, named by where it is
            let package = self.package_of(file).unwrap_or_else(|| self.place_of(file));
            // an arrow held by a property of an object literal is named by the property
            let named = if is_property_function_value(declaration) {
                declaration.parent()
            } else {
                Some(declaration)
            };
            // the name as written, spelled by the one rule the outline uses (`typescript_syntax::spell_name`): `[Symbol.iterator]` for a computed name
            let own = named
                .and_then(|node| node.name())
                .and_then(|name| spelled(source_file.text(), name))
                .or_else(|| {
                    symbol_of_declaration(checker, declaration)
                        .map(|symbol| checker.symbol_to_string_exported(symbol))
                })
                .unwrap_or_default();
            let name = qualified_name(declaration, own);
            return Resolved::External {
                package,
                name,
                outside_root: self.is_outside_root_source(file),
            };
        }
        // D43: a signature has no body, and stands where the context layer is meant to put what really runs. It stays a def of the outline (a marker on it keeps
        // its place); only a call through it is a function-typed value
        if is_signature_without_a_body(declaration) {
            return Resolved::Unknown(Why::FunctionTypedValue);
        }
        let Some(defs) = self.file_defs(file, style) else {
            return Resolved::Unknown(Why::NotInOutline);
        };
        // The engine names the first signature of an overloaded function (its call hierarchy finds the implementation only when it is the symbol's
        // value declaration), and the outline's def is the implementation: so a function or method with no body is looked up among the
        // declarations of its own symbol for the one that has.
        let node = if defs.def_of.contains_key(&declaration) || declaration.body().is_some() {
            declaration
        } else {
            implementation_of(checker, declaration).unwrap_or(declaration)
        };
        match defs.def_of.get(&node).copied() {
            Some(def) => Resolved::Def(Func::def(file, def)),
            // A property that holds an arrow and is not a def by D21's rule (a `let`, a default export, an argument's literal): a holder of a
            // function, declared in the project, as a callback property is. The outline says it is not a def on purpose, so this is not the bug below.
            None if is_property_function_value(node) => Resolved::Unknown(Why::FunctionTypedValue),
            // a bug in one of the two rules (what the engine calls callable, what the outline calls a def), or a kind of declaration nobody has looked at yet:
            // counted, never a panic on a user's text. The tests' helper fails on the count, so a test finds it (`tests/common`)
            None => Resolved::Unknown(Why::NotInOutline),
        }
    }

    /// What constructing `class` runs: its own constructor, or the one it inherits. `Nothing` is an answer (no class up the chain declares one);
    /// `External` and `Unknown` are limits, and the caller counts them (rule 8) so that "no constructor to enter" is never read as "no constructor".
    /// With it, the classes the search went through: `class`, then each base class whose constructor it looked for, in order. A construction runs the
    /// instance field initializers of each of them (a class with no constructor of its own runs its base class's, and the base's initializers with it), and
    /// the last one is the class whose constructor is entered, if any.
    pub(crate) fn init_chain<'p>(
        &'p self,
        class: Func<'p>,
        style: MarkerStyle<'_>,
    ) -> (Init<'p>, Vec<Func<'p>>) {
        let mut seen = vec![class];
        let init = self.init_walk(class, style, &mut seen);
        (init, seen)
    }

    fn init_walk<'p>(
        &'p self,
        class: Func<'p>,
        style: MarkerStyle<'_>,
        seen: &mut Vec<Func<'p>>,
    ) -> Init<'p> {
        let mut class = class;
        loop {
            if let Some(constructor) = self.constructor_of(class, style) {
                return Init::Enter(constructor);
            }
            let Some(node) = self.node_of(class, style) else {
                return Init::Unknown;
            };
            let Some(heritage) = ast::get_class_extends_heritage_element(node) else {
                return Init::Nothing;
            };
            let Some(base) = expression_of(heritage) else {
                return Init::Unknown;
            };
            let _scope = self.region.enter();
            let mut checker = self.checker();
            let mut resolved: Vec<Resolved<'_>> = Vec::new();
            let mut declarations = declarations_of(&mut checker, base);
            if declarations.is_empty() {
                // a library's class (`Error`) is an interface and a `var`: the same fallback as a `new` of it
                if let Some(external) = self.external_declarations(&mut checker, base) {
                    resolved = external;
                }
            } else {
                declarations.dedup();
                for declaration in declarations {
                    let found = self.classify(&mut checker, declaration, style);
                    if !resolved.contains(&found) {
                        resolved.push(found);
                    }
                }
            }
            match resolved.as_slice() {
                [Resolved::Def(base_class)]
                    if self.kind_of(*base_class, style) == DefKind::Class =>
                {
                    // a class that extends itself is an error in the program, not a loop here
                    if seen.contains(base_class) {
                        return Init::Unknown;
                    }
                    seen.push(*base_class);
                    class = *base_class;
                }
                [_, ..]
                    if resolved
                        .iter()
                        .all(|r| matches!(r, Resolved::External { .. })) =>
                {
                    return Init::External;
                }
                _ => return Init::Unknown,
            }
        }
    }

    /// Whether the def has a marker that another def of its file owns, so `describe` names it by where it is (D34).
    pub(crate) fn has_a_marker_it_does_not_own(
        &self,
        func: Func<'_>,
        style: MarkerStyle<'_>,
    ) -> bool {
        let Some(index) = func.outline_index() else {
            return false;
        };
        let defs = self
            .file_defs(func.file, style)
            .expect("a Func is made from an outline, so its file has one");
        defs.outline.defs[index].marker.is_some() && !defs.owns_marker[index]
    }

    /// What the def is, in the core's terms: `Marked` by its marker when it has one, else named by where it is (the file relative to the root,
    /// its qualname, and which of the same-named defs of the file it is, in source order).
    pub(crate) fn describe(&self, func: Func<'_>, style: MarkerStyle<'_>) -> DefInfo {
        let file = self.relative_path(func.file).expect(
            "a Func is made from project code, which is under the root (`is_project_code`)",
        );
        // The file's own code has no row in the outline: it is named by its path, with no qualname and no marker (D32). The id is a path id with an empty
        // qname, so no def's marker text can collide with it and the wire's `target_id` is empty.
        let Some(index) = func.outline_index() else {
            return DefInfo::new(
                StableDef::Path {
                    module: file.clone(),
                    qname: String::new(),
                    nth: 0,
                },
                file.clone(),
                String::new(),
                0,
                DefKind::Module,
                file,
                self.range_of(func, style),
            );
        };
        let defs = self
            .file_defs(func.file, style)
            .expect("a Func is made from an outline, so its file has one");
        let def = &defs.outline.defs[index];
        let occurrence = defs.outline.defs[..index]
            .iter()
            .filter(|earlier| earlier.qualname == def.qualname)
            .count() as u16;
        // a marker another def of the file owns is not this def's id (D34): it is named by where it is, as a def with no marker is
        let id = match &def.marker {
            Some(marker) if defs.owns_marker[index] => StableDef::Marked(marker.clone()),
            _ => StableDef::Path {
                module: file.clone(),
                qname: def.qualname.clone(),
                nth: occurrence,
            },
        };
        DefInfo::new(
            id,
            def.qualname.clone(),
            def.qualname.clone(),
            occurrence,
            def.kind.clone(),
            file,
            def.range,
        )
    }
}

/// A function expression or a class expression with no name: it can be neither looked up nor entered by name.
fn is_anonymous_function_or_class(node: P<Node>) -> bool {
    matches!(
        node.kind(),
        Kind::FunctionExpression | Kind::ClassExpression
    ) && node.name().is_none()
}

/// The leftmost name of a callee expression: an identifier or `this`. `a.b`, `a[b]` and `a?.b` go to `a`; parentheses, assertions and `!` are looked through.
/// Anything else (a call, a literal, `super`) has none.
fn leftmost_name(callee: P<Node>) -> Option<P<Node>> {
    let mut node = unwrap_expression(callee);
    loop {
        match node.kind() {
            Kind::Identifier | Kind::ThisKeyword => return Some(node),
            Kind::PropertyAccessExpression | Kind::ElementAccessExpression => {
                node = unwrap_expression(expression_of(node)?);
            }
            _ => return None,
        }
    }
}

/// What a name's declaration says about where its value comes from: a parameter (or a binding taken out of one), a variable with a call as its initializer
/// (or a binding taken out of one), anything else.
fn origin_of_declaration(declaration: P<Node>) -> FunctionOrigin {
    let mut node = declaration;
    // a binding element is inside a pattern, which is inside a binding element, a parameter or a variable
    while node.kind() == Kind::BindingElement {
        let Some(pattern) = node.parent() else {
            return FunctionOrigin::Other;
        };
        let Some(owner) = pattern.parent() else {
            return FunctionOrigin::Other;
        };
        node = owner;
    }
    match node.kind() {
        Kind::Parameter => FunctionOrigin::Parameter,
        Kind::VariableDeclaration if initializer_of(node).is_some_and(is_a_call_result) => {
            FunctionOrigin::CallResult
        }
        _ => FunctionOrigin::Other,
    }
}

/// The symbol at `location`, through an import: the symbol it leads to.
#[cfg(feature = "__tc0_counts")]
fn aliased_symbol_at(checker: &mut Checker, location: P<Node>) -> Option<P<Symbol>> {
    let mut symbol = checker.get_symbol_at_location_exported(location)?;
    if symbol.flags().intersects(SymbolFlags::Alias) {
        symbol = checker.get_aliased_symbol(symbol);
    }
    Some(symbol)
}

/// The first declaration of the symbol of `name`, through an import (as `function_origin` reads it).
#[cfg(feature = "__tc0_counts")]
fn first_declaration(checker: &mut Checker, name: P<Node>) -> Option<P<Node>> {
    aliased_symbol_at(checker, name)?
        .declarations()
        .first()
        .copied()
}

/// The symbol an expression is, by itself: of an identifier, of a property access, of the literal of an element access (none for another argument), through an
/// import.
#[cfg(feature = "__tc0_counts")]
fn own_symbol(checker: &mut Checker, node: P<Node>) -> Option<P<Symbol>> {
    let location = match node.kind() {
        Kind::Identifier | Kind::PropertyAccessExpression => node,
        Kind::ElementAccessExpression => {
            let argument = ElementAccess::cast(node)?.argument();
            ast::is_string_literal_like(argument).then_some(argument)?
        }
        _ => return None,
    };
    aliased_symbol_at(checker, location)
}

/// What declares a name an argument is made from.
#[cfg(feature = "__tc0_counts")]
enum NameOrigin {
    Parameter,
    /// A variable declared at the top of a file or of a namespace.
    ModuleVariable,
    /// A variable declared anywhere else.
    Local,
}

/// Where a name comes from: its declaration, through the binding elements it is taken out of. `None` for a declaration of another kind, and for no symbol.
#[cfg(feature = "__tc0_counts")]
fn name_origin(checker: &mut Checker, name: P<Node>) -> Option<NameOrigin> {
    let (_, owner) = walk_out_of_patterns(first_declaration(checker, name)?)?;
    match owner.kind() {
        Kind::Parameter => Some(NameOrigin::Parameter),
        Kind::VariableDeclaration => Some(if is_module_variable(owner) {
            NameOrigin::ModuleVariable
        } else {
            NameOrigin::Local
        }),
        _ => None,
    }
}

/// A variable declared by a statement of a file or of a module block (`const x = 1;`, not `for (const x of xs)`, not inside a function).
#[cfg(feature = "__tc0_counts")]
fn is_module_variable(declaration: P<Node>) -> bool {
    let statement = declaration
        .parent()
        .filter(|list| list.kind() == Kind::VariableDeclarationList)
        .and_then(|list| list.parent())
        .filter(|statement| statement.kind() == Kind::VariableStatement);
    statement
        .and_then(|statement| statement.parent())
        .is_some_and(|scope| matches!(scope.kind(), Kind::SourceFile | Kind::ModuleBlock))
}

/// The source file a node is in, as a `File`.
#[cfg(feature = "__tc0_counts")]
fn file_of<'p>(node: P<Node>) -> Option<File<'p>> {
    Some(File {
        node: ast::get_source_file_of_node(node)?,
        _sem: PhantomData,
        _one_thread: PhantomData,
    })
}

/// What declares the function a call resolves to.
#[cfg(feature = "__tc0_counts")]
enum CallDeclaredBy {
    /// Code of the project.
    Project,
    /// Anywhere else: the package, and the declaration's own name as written.
    Package { package: String, name: String },
    /// No declaration.
    Unknown,
}

/// Whether a slot of this type can hold a function or an object (TC0's rule; TC2 has its own): a union with a member that can, or a type whose flags meet
/// `Any`, `Unknown`, `Object`, `NonPrimitive`, `TypeParameter`, `Intersection`, `IndexedAccess`, `Conditional` or `Substitution`. An array is an `Object`.
#[cfg(feature = "__tc0_counts")]
fn is_capable(ty: P<Type>) -> bool {
    if ty.flags().intersects(TypeFlags::Union) {
        return ty.types().iter().any(|member| is_capable(*member));
    }
    ty.flags().intersects(
        TypeFlags::Any
            | TypeFlags::Unknown
            | TypeFlags::Object
            | TypeFlags::NonPrimitive
            | TypeFlags::TypeParameter
            | TypeFlags::Intersection
            | TypeFlags::IndexedAccess
            | TypeFlags::Conditional
            | TypeFlags::Substitution,
    )
}

/// Whether the argument's own type is an instance of a class.
#[cfg(feature = "__tc0_counts")]
fn is_class_instance(checker: &mut Checker, argument: P<Node>) -> bool {
    checker
        .get_type_at_location(argument)
        .object_flags()
        .intersects(ObjectFlags::Class)
}

/// From a declaration up through the binding elements it is inside (as `origin_of_declaration` does): how many it passed, and the node reached (the
/// parameter or the variable declaration).
#[cfg(feature = "__tc0_counts")]
fn walk_out_of_patterns(declaration: P<Node>) -> Option<(usize, P<Node>)> {
    let mut node = declaration;
    let mut passed = 0;
    while node.kind() == Kind::BindingElement {
        node = node.parent()?.parent()?;
        passed += 1;
    }
    Some((passed, node))
}

/// The member steps (`.k`, `[k]`) `leftmost_name` goes through from the callee down to the name.
#[cfg(feature = "__tc0_counts")]
fn member_steps(callee: P<Node>) -> usize {
    let mut node = unwrap_expression(callee);
    let mut steps = 0;
    while matches!(
        node.kind(),
        Kind::PropertyAccessExpression | Kind::ElementAccessExpression
    ) {
        let Some(inner) = expression_of(node) else {
            break;
        };
        node = unwrap_expression(inner);
        steps += 1;
    }
    steps
}

/// Whether `ancestor` is above `node` in the tree.
#[cfg(feature = "__tc0_counts")]
fn is_inside(node: P<Node>, ancestor: P<Node>) -> bool {
    let mut up = node.parent();
    while let Some(parent) = up {
        if parent == ancestor {
            return true;
        }
        up = parent.parent();
    }
    false
}

/// The kind of the first declaration of the callee's own symbol: `b` of `a.b`, the literal of `a["b"]`, the name of a plain callee.
#[cfg(feature = "__tc0_counts")]
fn callee_declaration_kind(checker: &mut Checker, callee: P<Node>) -> Option<Kind> {
    let node = unwrap_expression(callee);
    let location = match node.kind() {
        Kind::Identifier | Kind::PropertyAccessExpression => node,
        Kind::ElementAccessExpression => {
            let argument = ElementAccess::cast(node)?.argument();
            ast::is_string_literal_like(argument).then_some(argument)?
        }
        _ => return None,
    };
    Some(first_declaration(checker, location)?.kind())
}

/// The call an initializer is, under parentheses, assertions and `await` (`is_a_call_result`, but the node).
#[cfg(feature = "__tc0_counts")]
fn call_of_initializer(initializer: P<Node>) -> Option<P<Node>> {
    let mut node = unwrap_expression(initializer);
    while node.kind() == Kind::AwaitExpression {
        node = unwrap_expression(expression_of(node)?);
    }
    (node.kind() == Kind::CallExpression).then_some(node)
}

/// Whether `initializer` is a call, under parentheses, assertions and `await`: what a call returns.
fn is_a_call_result(initializer: P<Node>) -> bool {
    let mut node = unwrap_expression(initializer);
    while node.kind() == Kind::AwaitExpression {
        let Some(inner) = expression_of(node) else {
            return false;
        };
        node = unwrap_expression(inner);
    }
    node.kind() == Kind::CallExpression
}

/// A declaration with no body that stands for whatever implements it (D43): a method signature (in a type literal, an interface or `declare global`), a `get` or
/// `set` signature of a type literal or an interface, and an abstract method or accessor. A method of a `declare class` has no body either, and is not one of
/// these: it is the only declaration there is.
pub(super) fn is_signature_without_a_body(declaration: P<Node>) -> bool {
    match declaration.kind() {
        Kind::MethodSignature => true,
        Kind::GetAccessor | Kind::SetAccessor => {
            ast::has_abstract_modifier(declaration)
                || declaration.parent().is_some_and(|parent| {
                    matches!(
                        parent.kind(),
                        Kind::TypeLiteral | Kind::InterfaceDeclaration
                    )
                })
        }
        Kind::MethodDeclaration => ast::has_abstract_modifier(declaration),
        _ => false,
    }
}

/// The name of an external as the outline would qualify it: the names of the classes, interfaces and namespaces around the declaration, then its own
/// (`Math.max`, `Array.map`, `helper`). Two methods of one package must not share a name: the merged wire counts a repeated target as one. A module
/// whose name is a string (`declare module "x"`) adds nothing, since the package says it.
fn qualified_name(declaration: P<Node>, own: String) -> String {
    let mut parts = vec![own];
    let text = ast::get_source_file_of_node(declaration).map_or("", |file| file.text());
    let mut parent = declaration.parent();
    while let Some(node) = parent {
        if matches!(
            node.kind(),
            Kind::ClassDeclaration
                | Kind::ClassExpression
                | Kind::InterfaceDeclaration
                | Kind::ModuleDeclaration
        ) && let Some(name) = node.name().filter(|name| ast::is_identifier(*name))
            && let Some(spelling) = spelled(text, name)
        {
            parts.push(spelling);
        }
        parent = node.parent();
    }
    parts.reverse();
    parts.join(".")
}

/// The declaration with a body among those of the symbol of an overload signature: the implementation, in the same file.
fn implementation_of(checker: &mut Checker, signature: P<Node>) -> Option<P<Node>> {
    if !(ast::is_function_declaration(signature) || ast::is_method_declaration(signature)) {
        return None;
    }
    let file = ast::get_source_file_of_node(signature)?;
    symbol_of_declaration(checker, signature)?
        .declarations()
        .iter()
        .copied()
        .find(|declaration| {
            declaration.body().is_some()
                && ast::get_source_file_of_node(*declaration) == Some(file)
                && (ast::is_function_declaration(*declaration)
                    || ast::is_method_declaration(*declaration))
        })
}

/// The accessors of the property at `location` that an access of kind `access` runs: the getters for a read, the setters for a write, both for a read and a write.
/// From the symbol's own declarations, so a union's property (a synthetic symbol that holds every member's) gives every member's accessor, and a member that is a
/// plain property gives none. `None` when there is no symbol.
fn accessor_declarations(
    checker: &mut Checker,
    location: P<Node>,
    access: Access,
) -> Option<Vec<P<Node>>> {
    let symbol = checker.get_symbol_at_location_exported(location)?;
    if !symbol.flags().intersects(SymbolFlags::Accessor) {
        // a union's synthetic property carries the flags of its members; a plain symbol that is no accessor has nothing to run
        if symbol
            .declarations()
            .iter()
            .all(|declaration| !matches!(declaration.kind(), Kind::GetAccessor | Kind::SetAccessor))
        {
            return Some(Vec::new());
        }
    }
    Some(
        symbol
            .declarations()
            .iter()
            .copied()
            .filter(|declaration| match declaration.kind() {
                Kind::GetAccessor => access.reads(),
                Kind::SetAccessor => access.writes(),
                _ => false,
            })
            .collect(),
    )
}

/// Why the engine has no declaration for this callee: syntax first (a computed name), then the type the checker gives it.
/// The function written where it is called: the arrow or function expression under the parentheses and assertions of the callee (`(() => {})()`,
/// `(function () {})()`, `(<any>function () {})()`, `(function () {} as T)()`, `(function () {})!()`). `!` and `void` before the call are outside the call
/// (`!function () {}()` is `!(…())`), and one inside the parentheses (`(!function () {})()`) makes a boolean, which is not this function. H10 and H11 both start here.
fn function_called_in_place(callee: P<Node>) -> Option<P<Node>> {
    let function = unwrap_expression(callee);
    matches!(
        function.kind(),
        Kind::ArrowFunction | Kind::FunctionExpression
    )
    .then_some(function)
}

fn why_unknown(checker: &mut Checker, callee: P<Node>) -> Why {
    if ast::is_element_access_expression(callee) {
        return Why::ComputedName;
    }
    if function_called_in_place(callee).is_some() {
        return Why::AnonymousFunctionCalled;
    }
    if checker
        .get_type_at_location(callee)
        .flags()
        .intersects(TypeFlags::Any)
    {
        Why::AnyOrUntyped
    } else if is_value_holder(checker, callee) {
        Why::FunctionTypedValue
    } else {
        Why::NoDeclaration
    }
}

/// Whether the callee names something that was **declared as a holder of a value**: a parameter, a property, a variable or a destructured binding
/// (through an import alias too). That is what D16 means by a function-typed value, and it is the evidence behind the count: a callee with no
/// symbol (`makeHandler()()`, `(a ? f : g)()`) or a symbol that is not one of these is `no_declaration`, so a function the resolver failed to
/// enter cannot be counted as a D16 case just because every call that type-checks has a callable callee.
fn is_value_holder(checker: &mut Checker, callee: P<Node>) -> bool {
    let Some(mut symbol) = checker.get_symbol_at_location_exported(callee) else {
        return false;
    };
    if symbol.flags().intersects(SymbolFlags::Alias) {
        symbol = checker.get_aliased_symbol(symbol);
    }
    let declarations = symbol.declarations();
    !declarations.is_empty()
        && declarations
            .iter()
            .all(|declaration| is_value_holder_declaration(*declaration))
}

/// A declaration that holds a value, whatever the value is: a parameter, a property, a variable, a destructured binding.
fn is_value_holder_declaration(declaration: P<Node>) -> bool {
    matches!(
        declaration.kind(),
        Kind::Parameter
            | Kind::PropertySignature
            | Kind::PropertyDeclaration
            | Kind::VariableDeclaration
            | Kind::BindingElement
            | Kind::PropertyAssignment
            | Kind::ShorthandPropertyAssignment
    )
}

/// A callee that is a name: an identifier, a property access or an element access. For these the checker's resolved signature is the signature of
/// the named function or method; for any other expression (a conditional, a `||`, a call's result) it is the signature of whichever type the
/// expression reduced to.
fn is_named_callee(callee: P<Node>) -> bool {
    matches!(
        callee.kind(),
        Kind::Identifier | Kind::PropertyAccessExpression | Kind::ElementAccessExpression
    )
}

/// Whether the callee's type is a union of more than one type once `undefined` and `null` are set aside: `(flag ? f : g)`. The checker's resolved
/// signature names one declaration for such a call, which would pass for the whole answer.
fn is_union_callee(checker: &mut Checker, callee: P<Node>) -> bool {
    let ty = checker.get_type_at_location(callee);
    ty.flags().intersects(TypeFlags::Union)
        && ty
            .types()
            .iter()
            .filter(|member| {
                !member
                    .flags()
                    .intersects(TypeFlags::Undefined | TypeFlags::Null)
            })
            .count()
            > 1
}
