//! Jedi's `get_qualified_names(True)`, joined with dots — the identity every
//! node in the call tree and every base class gets matched by.
//!
//! See `experience/06-scopes-and-definitions/README.md`, step 2.

use ruff_db::parsed::ParsedModuleRef;
use ty_module_resolver::file_to_module;
use ty_python_core::definition::Definition;
use ty_python_core::scope::{NodeWithScopeKind, ScopeKind};
use ty_python_core::semantic_index;
use ty_python_semantic::Db;

/// `module name  +  every enclosing scope's name  +  the definition's own name`.
///
/// `Definition::scope` is the scope the definition is BOUND in (its parent),
/// not its own body scope — so `ancestor_scopes`, which starts with that scope
/// itself, already walks exactly the enclosing chain this needs.
///
/// Two scope kinds are dropped on the way up: `Module` (its contribution comes
/// from `file_to_module` instead, as `"<module>"` is not a real component) and
/// `TypeParams` (`def generic[T]()`'s type-param scope sits between `generic`
/// and its enclosing scope; keeping it doubles the name to `generic.generic`).
///
/// Deliberately reads the scope's name off `Scope::node()` rather than
/// constructing a `ScopeId` — `ScopeId` is a salsa TRACKED STRUCT, and
/// creating one outside a running salsa query panics (`"cannot create a
/// tracked struct disambiguator outside of a tracked function"`) the second
/// time the same `(file, file_scope_id)` pair comes up, which it will for any
/// two sibling definitions sharing an ancestor scope.
pub(crate) fn qualified_name<'db>(
    db: &'db dyn Db,
    definition: Definition<'db>,
    module: &ParsedModuleRef,
) -> Option<String> {
    let name = definition.name(db)?;
    let program_file = definition.program_file(db);
    let module_info = file_to_module(db, program_file.resolver_file(db))?;

    let index = semantic_index(db, program_file);
    let mut components: Vec<&str> = Vec::new();
    for (_file_scope_id, scope) in index.ancestor_scopes(definition.scope(db).file_scope_id(db)) {
        if matches!(scope.kind(), ScopeKind::Module | ScopeKind::TypeParams) {
            continue;
        }
        components.push(scope_name(scope.node(), module));
    }
    components.reverse();

    let mut qname = module_info.name(db).to_string();
    for component in components {
        qname.push('.');
        qname.push_str(component);
    }
    qname.push('.');
    qname.push_str(&name);
    Some(qname)
}

/// Mirrors `ScopeId::name` (`ty_python_core/src/scope.rs`) without the
/// tracked-struct construction that function requires.
fn scope_name<'ast>(node: &NodeWithScopeKind, module: &'ast ParsedModuleRef) -> &'ast str {
    match node {
        NodeWithScopeKind::Module => "<module>",
        NodeWithScopeKind::Class(class) | NodeWithScopeKind::ClassTypeParameters(class) => {
            class.node(module).name.as_str()
        }
        NodeWithScopeKind::Function(function)
        | NodeWithScopeKind::FunctionTypeParameters(function) => {
            function.node(module).name.as_str()
        }
        NodeWithScopeKind::TypeAlias(type_alias)
        | NodeWithScopeKind::TypeAliasTypeParameters(type_alias) => type_alias
            .node(module)
            .name
            .as_name_expr()
            .map(|name| name.id.as_str())
            .unwrap_or("<type alias>"),
        NodeWithScopeKind::Lambda(_) => "<lambda>",
        NodeWithScopeKind::ListComprehension(_) => "<listcomp>",
        NodeWithScopeKind::SetComprehension(_) => "<setcomp>",
        NodeWithScopeKind::DictComprehension(_) => "<dictcomp>",
        NodeWithScopeKind::GeneratorExpression(_) => "<generator>",
    }
}
