use super::{BodyScope, Name, PropertyParts, Scope, Shape};
use ruff_python_ast::StmtFunctionDef;
use ty_python_core::definition::Definition;
use ty_python_core::scope::{NodeWithScopeRef, ScopeKind};
use ty_python_core::semantic_index;
use ty_python_semantic::HasDefinition;
use ty_python_semantic::types::Type;

impl<'db> super::Sem<'db> {
    /// The scope of a function's body. [check] the accessor name.
    pub(crate) fn body_scope(
        &self,
        def: Definition<'db>,
        node: &StmtFunctionDef,
    ) -> BodyScope<'db> {
        let file = def.program_file(self.db);
        let index = semantic_index(self.db, file);
        BodyScope {
            file,
            scope: index.node_scope(NodeWithScopeRef::Function(node)),
        }
    }
    /// `None` when `name` isn't a property on `class`. Asked on the class: on an
    /// instance ty applies the descriptor and the property itself is gone.
    pub(crate) fn property_parts(
        &self,
        class: Definition<'db>,
        name: &str,
    ) -> Option<PropertyParts<'db>> {
        let file = class.program_file(self.db);
        let Type::PropertyInstance(p) = self.member(self.class_type(class), name, file)? else {
            return None;
        };
        // `property(get_h)` holds `get_h` as a `Callable`. Kept local: `shape` must not
        // turn every Callable that has a definition into a Function.
        let project = |t: Option<Type<'db>>| {
            let d = self.function_definition(t?, file)?;
            self.is_project(d).then_some(d)
        };
        Some(PropertyParts {
            getter: project(p.getter(self.db)),
            setter: project(p.setter(self.db)),
        })
    }

    /// The classes and functions around `def`, outermost first, then `def` itself.
    #[allow(dead_code)] // inline mode is the fixture suite's cross-check and is not reachable from the public API
    pub(crate) fn scope_chain(&self, def: Definition<'db>) -> Vec<Scope<'db>> {
        let file = def.program_file(self.db);
        let index = semantic_index(self.db, file);
        let parsed = self.parsed(def);
        let model = self.model(file);
        let mut chain: Vec<Scope<'db>> = index
            .ancestor_scopes(def.scope(self.db).file_scope_id(self.db))
            .filter_map(|(_, scope)| match scope.node() {
                ty_python_core::scope::NodeWithScopeKind::Class(c) => {
                    Some(Scope::Class(c.node(&parsed).definition(&model)))
                }
                ty_python_core::scope::NodeWithScopeKind::Function(f) => {
                    Some(Scope::Function(f.node(&parsed).definition(&model)))
                }
                _ => None, // the module; type-parameter scopes hold nothing
            })
            .collect();
        chain.reverse();
        chain.push(Scope::Function(def));
        chain
    }

    /// `__init__` as found through the MRO, if it is a `def` somewhere.
    pub(crate) fn init_of(&self, class: Definition<'db>) -> Option<Definition<'db>> {
        let file = class.program_file(self.db);
        let t = self.member(self.class_type(class), "__init__", file)?;
        match self.shape(t, file) {
            Shape::Function(d) => Some(d),
            _ => None,
        }
    }
    /// If `name` is a free variable of `scope`, the enclosing FUNCTION scope
    /// that binds it. `None` for a local, a global or a builtin.
    pub(crate) fn free_variable_scope(
        &self,
        scope: BodyScope<'db>,
        name: &str,
    ) -> Option<BodyScope<'db>> {
        let index = semantic_index(self.db, scope.file);
        let mut ancestors = index.ancestor_scopes(scope.scope);
        ancestors.next(); // the scope itself
        if index
            .place_table(scope.scope)
            .symbol_by_name(name)
            .is_some_and(|s| s.is_bound())
        {
            return None; // bound here: a local, even if not assigned yet on this path
        }
        for (id, s) in ancestors {
            match s.kind() {
                ScopeKind::Class | ScopeKind::TypeParams => continue, // skipped by name resolution
                ScopeKind::Module => return None,                     // a global
                _ => {
                    if index
                        .place_table(id)
                        .symbol_by_name(name)
                        .is_some_and(|s| s.is_bound())
                    {
                        return Some(BodyScope {
                            file: scope.file,
                            scope: id,
                        });
                    }
                }
            }
        }
        None
    }
    /// Names `func` (or a function nested in it) reads that are bound in an
    /// enclosing FUNCTION scope outside `func`. Sorted.
    pub(crate) fn free_variables(&self, func: Definition<'db>) -> Vec<Name> {
        let parsed = self.parsed(func);
        let Some(node) = self.function_node(func, &parsed) else {
            return Vec::new();
        };
        let own = self.body_scope(func, node);
        let index = semantic_index(self.db, own.file);
        let mut scopes = vec![own.scope];
        let mut i = 0;
        while i < scopes.len() {
            let kids: Vec<_> = index.child_scopes(scopes[i]).map(|(id, _)| id).collect();
            scopes.extend(kids);
            i += 1;
        }
        let mut out = Vec::new();
        for scope in scopes {
            for symbol in index.place_table(scope).symbols() {
                if !symbol.is_used() {
                    continue;
                }
                let at = BodyScope {
                    file: own.file,
                    scope,
                };
                if let Some(bound) = self.free_variable_scope(at, symbol.name().as_str()) {
                    // free in `scope`, and bound outside `func`
                    if !index
                        .ancestor_scopes(bound.scope)
                        .any(|(id, _)| id == own.scope)
                    {
                        out.push(symbol.name().clone());
                    }
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }
}
