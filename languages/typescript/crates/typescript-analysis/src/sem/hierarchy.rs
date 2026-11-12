use tsrs_ast::{Symbol, SymbolFlags};
use tsrs_checker::Checker;

use super::*;

// ---- the port of `tsrs_ls`'s `callhierarchy.rs`. Names and shapes follow the original so an engine bump can be checked line by line.

/// A function or class expression with an identifier name.
fn is_named_expression(node: P<Node>) -> bool {
    (ast::is_function_expression(node) || ast::is_class_expression(node))
        && node.name().is_some_and(ast::is_identifier)
}

/// An arrow or a function expression.
fn is_function_value(node: P<Node>) -> bool {
    ast::is_arrow_function(node) || ast::is_function_expression(node)
}

/// An arrow or a function expression that is the value of a property of an object literal (`key: () => {}`).
pub(super) fn is_property_function_value(node: P<Node>) -> bool {
    is_function_value(node) && node.parent().is_some_and(ast::is_property_assignment)
}

pub(super) fn is_variable_like(node: Option<P<Node>>) -> bool {
    node.is_some_and(|node| {
        ast::is_property_declaration(node) || ast::is_variable_declaration(node)
    })
}

/// A function, arrow or class expression assigned to a constant variable or class property.
fn is_assigned_expression(node: P<Node>) -> bool {
    if !(ast::is_function_expression(node)
        || ast::is_arrow_function(node)
        || ast::is_class_expression(node))
    {
        return false;
    }
    if node.name().is_some() {
        return false;
    }
    let parent = node.parent();
    if !is_variable_like(parent) {
        return false;
    }
    let parent = parent.expect("is_variable_like says there is one");
    if parent.initializer() != Some(node) {
        return false;
    }
    if !parent.name().is_some_and(ast::is_identifier) {
        return false;
    }
    ast::get_combined_node_flags(parent).intersects(NodeFlags::Const)
        || ast::is_property_declaration(parent)
}

fn is_possible_call_hierarchy_declaration(node: Option<P<Node>>) -> bool {
    node.is_some_and(|node| {
        ast::is_source_file(node)
            || ast::is_module_declaration(node)
            || ast::is_function_declaration(node)
            || ast::is_function_expression(node)
            || ast::is_class_declaration(node)
            || ast::is_class_expression(node)
            || ast::is_class_static_block_declaration(node)
            || ast::is_method_declaration(node)
            || ast::is_method_signature_declaration(node)
            || ast::is_get_accessor_declaration(node)
            || ast::is_set_accessor_declaration(node)
    })
}

fn is_valid_call_hierarchy_declaration(node: Option<P<Node>>) -> bool {
    let Some(node) = node else {
        return false;
    };
    if ast::is_source_file(node) {
        return true;
    }
    if ast::is_module_declaration(node) {
        return node.name().is_some_and(ast::is_identifier);
    }
    ast::is_function_declaration(node)
        || ast::is_class_declaration(node)
        || ast::is_class_static_block_declaration(node)
        || ast::is_method_declaration(node)
        || ast::is_method_signature_declaration(node)
        || ast::is_get_accessor_declaration(node)
        || ast::is_set_accessor_declaration(node)
        || is_named_expression(node)
        || is_assigned_expression(node)
}

/// The node that stands for a declaration when asking the checker about it.
fn reference_node(node: P<Node>) -> Option<P<Node>> {
    if ast::is_source_file(node) {
        return Some(node);
    }
    if let Some(name) = node.name() {
        return Some(name);
    }
    if is_assigned_expression(node) {
        return node.parent().and_then(|parent| parent.name());
    }
    node.modifiers().and_then(|modifiers| {
        modifiers
            .nodes()
            .iter()
            .copied()
            .find(|modifier| modifier.kind() == Kind::DefaultKeyword)
    })
}

pub(super) fn symbol_of_declaration(checker: &mut Checker, node: P<Node>) -> Option<P<Symbol>> {
    if ast::is_class_static_block_declaration(node) {
        return None;
    }
    checker.get_symbol_at_location_exported(reference_node(node)?)
}

/// The implementation of an overloaded function, method or constructor: the declaration with a body.
fn find_implementation(checker: &mut Checker, node: P<Node>) -> Option<P<Node>> {
    if !ast::is_function_like_declaration(Some(node)) || node.body().is_some() {
        return Some(node);
    }
    if ast::is_constructor_declaration(node) {
        return ast::get_first_constructor_with_body(node.parent()?);
    }
    if ast::is_function_declaration(node) || ast::is_method_declaration(node) {
        let value_declaration =
            symbol_of_declaration(checker, node).and_then(|symbol| symbol.value_declaration());
        return value_declaration.filter(|declaration| {
            ast::is_function_like_declaration(Some(*declaration)) && declaration.body().is_some()
        });
    }
    Some(node)
}

/// Every declaration of the symbol that is itself a call hierarchy declaration, in (file, offset) order, with adjacent overload signatures of
/// one parent counted once.
fn find_all_initial_declarations(checker: &mut Checker, node: P<Node>) -> Vec<P<Node>> {
    let Some(symbol) = symbol_of_declaration(checker, node) else {
        return Vec::new();
    };
    let mut declarations: Vec<P<Node>> = symbol.declarations().to_vec();
    declarations.sort_by_key(|declaration| {
        (
            ast::get_source_file_of_node(*declaration).map(|file| file.file_name().to_owned()),
            declaration.pos(),
        )
    });
    let mut found: Vec<P<Node>> = Vec::new();
    let mut last: Option<P<Node>> = None;
    for declaration in declarations {
        if is_valid_call_hierarchy_declaration(Some(declaration)) {
            let separate = last.is_none_or(|last| {
                last.parent() != declaration.parent() || last.end() != declaration.pos()
            });
            if separate {
                found.push(declaration);
            }
            last = Some(declaration);
        }
    }
    found
}

fn find_implementation_or_all_initial_declarations(
    checker: &mut Checker,
    node: P<Node>,
) -> Vec<P<Node>> {
    if ast::is_class_static_block_declaration(node) {
        return vec![node];
    }
    if ast::is_function_like_declaration(Some(node)) {
        if let Some(implementation) = find_implementation(checker, node) {
            return vec![implementation];
        }
        let all = find_all_initial_declarations(checker, node);
        return if all.is_empty() { vec![node] } else { all };
    }
    let all = find_all_initial_declarations(checker, node);
    if all.is_empty() { vec![node] } else { all }
}

/// A call on a union (`w.write()` with `w: JsonWriter | CsvWriter`): the checker gives the property of a union a synthetic symbol that holds the
/// declarations of **every** member (`checker_11.rs`, `create_union_or_intersection_property`) and sets its value declaration only when the members
/// share one. The engine's call hierarchy follows the value declaration alone, so it finds nothing; the declarations are asked here instead. They
/// are the answer for `a.b` and for `a["b"]` with a literal name (the symbol of the literal), and `undefined` in the union adds nothing, since the
/// checker's symbol is the property's, not the type's. Returns the targets in (file, offset) order and whether every declaration was one that
/// can be entered; `None` when the callee has no such symbol (the declarations are not a union's, or there is a value declaration to follow).
pub(super) fn declarations_of_union_symbol(
    checker: &mut Checker,
    callee: P<Node>,
) -> Option<(Vec<P<Node>>, bool)> {
    let location = if ast::is_property_access_expression(callee) {
        callee
    } else if ast::is_element_access_expression(callee) {
        let argument = callee.as_element_access_expression().argument_expression;
        ast::is_string_literal_like(argument).then_some(argument)?
    } else {
        return None;
    };
    let symbol = checker.get_symbol_at_location_exported(location)?;
    if symbol.value_declaration().is_some() || symbol.declarations().is_empty() {
        return None;
    }
    let mut found: Vec<P<Node>> = Vec::new();
    let mut all_resolved = true;
    for &declaration in symbol.declarations() {
        let targets = callable_declarations(checker, declaration);
        all_resolved &= !targets.is_empty();
        for target in targets {
            if !found.contains(&target) {
                found.push(target);
            }
        }
    }
    found.sort_by_key(|declaration| {
        (
            ast::get_source_file_of_node(*declaration).map(|file| file.file_name().to_owned()),
            declaration.pos(),
        )
    });
    Some((found, all_resolved))
}

/// What a declaration of a property or method is, as something to enter: a callable declaration (its implementation, or all its initial
/// declarations), or the arrow or function a property or variable holds (`write = () => {}`), which is what the engine does for a declaration
/// name. Empty when it is neither: a method signature of a function type, a property of type `any`.
pub(super) fn callable_declarations(checker: &mut Checker, declaration: P<Node>) -> Vec<P<Node>> {
    if is_valid_call_hierarchy_declaration(Some(declaration)) {
        return find_implementation_or_all_initial_declarations(checker, declaration);
    }
    if is_variable_like(Some(declaration))
        && let Some(initializer) = declaration
            .initializer()
            .filter(|initializer| is_assigned_expression(*initializer))
    {
        return vec![initializer];
    }
    // the same addition as in `declarations_of` (D21): a member of a union that holds an arrow in an object literal
    if ast::is_property_assignment(declaration)
        && let Some(initializer) = declaration
            .initializer()
            .filter(|initializer| is_function_value(*initializer))
    {
        return vec![initializer];
    }
    Vec::new()
}

/// What the checker's resolved signature says, for a call the hierarchy's rule finds nothing for (`counter["bump"]()`, whose callee is not a name
/// the hierarchy can follow, but whose method the checker knows). Only a declaration that is itself callable counts: a function **type** (a callback
/// parameter) has a signature and no declaration to enter.
pub(super) fn declarations_of_signature(checker: &mut Checker, call: P<Node>) -> Vec<P<Node>> {
    let signature = checker.get_resolved_signature_exported(call);
    match signature.declaration() {
        Some(declaration) if is_valid_call_hierarchy_declaration(Some(declaration)) => {
            find_implementation_or_all_initial_declarations(checker, declaration)
        }
        // `api["load"]()`: the signature of an arrow held by a property of an object literal (D21)
        Some(declaration) if is_property_function_value(declaration) => vec![declaration],
        _ => Vec::new(),
    }
}

/// The declarations the engine's call hierarchy gives for the callee expression `location`; empty when it gives none.
pub(super) fn declarations_of(checker: &mut Checker, location: P<Node>) -> Vec<P<Node>> {
    let mut following_symbol = false;
    let mut location = location;
    loop {
        if is_valid_call_hierarchy_declaration(Some(location)) {
            return find_implementation_or_all_initial_declarations(checker, location);
        }
        if is_possible_call_hierarchy_declaration(Some(location))
            && let Some(ancestor) = ast::find_ancestor(Some(location), |n| {
                is_valid_call_hierarchy_declaration(Some(n))
            })
        {
            return find_implementation_or_all_initial_declarations(checker, ancestor);
        }
        if ast::is_declaration_name(location) {
            let parent = location.parent();
            if is_valid_call_hierarchy_declaration(parent) {
                return find_implementation_or_all_initial_declarations(
                    checker,
                    parent.expect("valid says there is one"),
                );
            }
            if is_possible_call_hierarchy_declaration(parent)
                && let Some(ancestor) =
                    ast::find_ancestor(parent, |n| is_valid_call_hierarchy_declaration(Some(n)))
            {
                return find_implementation_or_all_initial_declarations(checker, ancestor);
            }
            if is_variable_like(parent)
                && let Some(initializer) = parent
                    .and_then(|parent| parent.initializer())
                    .filter(|initializer| is_assigned_expression(*initializer))
            {
                return vec![initializer];
            }
            return Vec::new();
        }
        if ast::is_constructor_declaration(location) {
            return match location.parent() {
                Some(class) if is_valid_call_hierarchy_declaration(Some(class)) => vec![class],
                _ => Vec::new(),
            };
        }
        if location.kind() == Kind::StaticKeyword
            && let Some(block) = location
                .parent()
                .filter(|parent| ast::is_class_static_block_declaration(*parent))
        {
            location = block;
            continue;
        }
        if ast::is_variable_declaration(location)
            && let Some(initializer) = location
                .initializer()
                .filter(|initializer| is_assigned_expression(*initializer))
        {
            return vec![initializer];
        }
        if !following_symbol
            && let Some(mut symbol) = checker.get_symbol_at_location_exported(location)
        {
            if symbol.flags().intersects(SymbolFlags::Alias) {
                symbol = checker.get_aliased_symbol(symbol);
            }
            if let Some(value_declaration) = symbol.value_declaration() {
                following_symbol = true;
                location = value_declaration;
                continue;
            }
        }
        return Vec::new();
    }
}
