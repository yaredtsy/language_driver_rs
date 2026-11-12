use analyzer_core::graph::{DefInfo, StableDef};
use analyzer_core::outline::DefKind;
use tsrs_ast::SymbolFlags;
use tsrs_checker::{Checker, TypeFlags};
use tsrs_compiler::Context;

use super::collect::{Access, AccessCandidate, CallNode, SiteKind, unwrap_expression};
use super::hierarchy::{
    declarations_of, declarations_of_signature, declarations_of_union_symbol,
    is_property_function_value, symbol_of_declaration,
};
use super::*;

/// What one declaration the engine found for a site is.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Resolved<'p> {
    /// A def of project code: a function, method, accessor or constructor, or a class. The solver walks it.
    Def(Func<'p>),
    /// Outside project code: the package (`pkg`, `typescript:lib`, or the path of the repo's own `.d.ts`) and the declaration's own name.
    External { package: String, name: String },
    /// The engine gives no declaration to enter.
    Unknown(Why),
}

/// What is known about a site beyond what it resolves to, found while the declarations were at hand.
#[derive(Clone, Copy, Default)]
pub(crate) struct SiteFacts {
    /// The site is a JSX element: a class it constructs is a class component.
    pub(crate) jsx: bool,
    /// The callee is a getter (`b.fn()`): the edge is to the getter, and the call of what it returns has no edge.
    pub(crate) getter_result: bool,
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
        let mut facts = SiteFacts::default();
        let Some(calls) = self.body(func, style) else {
            return (vec![Resolved::Unknown(Why::NoDeclaration)], facts);
        };
        let Some(call) = calls.sites.get(site_index) else {
            debug_assert!(false, "a site index outside the def's sites");
            return (vec![Resolved::Unknown(Why::NoDeclaration)], facts);
        };
        facts.jsx = call.kind == SiteKind::Jsx;
        let _scope = self._region.enter();
        // one checker, so one handle at a time: it is dropped before this returns
        let mut checker = self.program.get_type_checker(&Context::default());
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
        let mut found: Vec<Resolved<'_>> = Vec::new();
        for declaration in declarations {
            let resolved = self.classify(&mut checker, declaration, style);
            if !found.contains(&resolved) {
                found.push(resolved);
            }
        }
        if unresolved_members {
            // a member of the union that has no declaration to enter is not dropped
            found.push(Resolved::Unknown(Why::UnionReceiver));
        }
        (found, facts)
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
        let mut checker = self.program.get_type_checker(&Context::default());
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
        let mut found: Vec<Resolved<'_>> = Vec::new();
        for declaration in declarations {
            let resolved = self.classify(checker, declaration, style);
            if !found.contains(&resolved) {
                found.push(resolved);
            }
        }
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
            ast::get_class_extends_heritage_element(class)?.expression()?
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
                && let Some(receiver) = callee.expression()
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
            let package = self.package_of(file).unwrap_or_else(|| {
                // not in `node_modules` and not a bundled library: the repo's own `.d.ts`, named by where it is
                self.relative_path(file)
                    .unwrap_or_else(|| source_file.file_name().to_owned())
            });
            // an arrow held by a property of an object literal is named by the property
            let named = if is_property_function_value(declaration) {
                declaration.parent()
            } else {
                Some(declaration)
            };
            let own = named
                .and_then(|node| node.name())
                .map(|name| name.text().to_owned())
                .or_else(|| {
                    symbol_of_declaration(checker, declaration)
                        .map(|symbol| checker.symbol_to_string_exported(symbol))
                })
                .unwrap_or_default();
            let name = qualified_name(declaration, own);
            return Resolved::External { package, name };
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
            Some(def) => Resolved::Def(Func { file, def }),
            // A property that holds an arrow and is not a def by D21's rule (a `let`, a default export, an argument's literal): a holder of a
            // function, declared in the project, as a callback property is. The outline says it is not a def on purpose, so this is not the bug below.
            None if is_property_function_value(node) => Resolved::Unknown(Why::FunctionTypedValue),
            None => {
                // a bug in one of the two rules (what the engine calls callable, what the outline calls a def): a test must find it,
                // and a release counts it
                debug_assert!(
                    false,
                    "the engine's declaration in {} is not a def of the outline",
                    source_file.file_name()
                );
                Resolved::Unknown(Why::NotInOutline)
            }
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
            let Some(defs) = self.file_defs(class.file, style) else {
                return Init::Unknown;
            };
            let Some(node) = defs.nodes[class.def] else {
                return Init::Unknown;
            };
            let Some(heritage) = ast::get_class_extends_heritage_element(node) else {
                return Init::Nothing;
            };
            let Some(base) = heritage.expression() else {
                return Init::Unknown;
            };
            let _scope = self._region.enter();
            let mut checker = self.program.get_type_checker(&Context::default());
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
                    if self.def(*base_class, style).kind == DefKind::Class =>
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

    /// What the def is, in the core's terms: `Marked` by its marker when it has one, else named by where it is (the file relative to the root,
    /// its qualname, and which of the same-named defs of the file it is, in source order).
    pub(crate) fn describe(&self, func: Func<'_>, style: MarkerStyle<'_>) -> DefInfo {
        let defs = self
            .file_defs(func.file, style)
            .expect("a Func is made from an outline, so its file has one");
        let def = &defs.outline.defs[func.def];
        let file = self
            .relative_path(func.file)
            .unwrap_or_else(|| func.file.node.file_name().to_owned());
        let occurrence = defs.outline.defs[..func.def]
            .iter()
            .filter(|earlier| earlier.qualname == def.qualname)
            .count() as u16;
        let id = match &def.marker {
            Some(marker) => StableDef::Marked(marker.clone()),
            None => StableDef::Path {
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

/// The name of an external as the outline would qualify it: the names of the classes, interfaces and namespaces around the declaration, then its own
/// (`Math.max`, `Array.map`, `helper`). Two methods of one package must not share a name: the merged wire counts a repeated target as one. A module
/// whose name is a string (`declare module "x"`) adds nothing, since the package says it.
fn qualified_name(declaration: P<Node>, own: String) -> String {
    let mut parts = vec![own];
    let mut parent = declaration.parent();
    while let Some(node) = parent {
        if matches!(
            node.kind(),
            Kind::ClassDeclaration
                | Kind::ClassExpression
                | Kind::InterfaceDeclaration
                | Kind::ModuleDeclaration
        ) && let Some(name) = node.name().filter(|name| ast::is_identifier(*name))
        {
            parts.push(name.text().to_owned());
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
fn why_unknown(checker: &mut Checker, callee: P<Node>) -> Why {
    if ast::is_element_access_expression(callee) {
        return Why::ComputedName;
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
