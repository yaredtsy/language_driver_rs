use crate::stable::{StableDef, StableType};
use analyzer_core::edit::MarkerStyle;
use analyzer_core::outline::DefKind;
use ruff_db::files::{File, system_path_to_file};
use ruff_db::parsed::{ParsedModuleRef, parsed_module};
use ruff_db::source::source_text;
use ruff_db::system::SystemPath;
use ruff_python_ast::visitor::{Visitor, walk_stmt};
use ruff_python_ast::{Alias, AnyNodeRef, ExprName, ExprRef, Stmt, StmtFunctionDef};
use ruff_text_size::{Ranged, TextRange, TextSize};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use ty_module_resolver::file_to_module;

use ty_project::Db as _;
pub(crate) use ty_project::ProjectDatabase;
use ty_project::glob::IncludeResult;
use ty_python_core::ProgramFile;
use ty_python_core::scope::{FileScopeId, NodeWithScopeRef, ScopeKind};
use ty_python_core::semantic_index;
use ty_python_semantic::types::analyzer_ext;
use ty_python_semantic::{
    Db as _, HasDefinition, HasType, ImportAliasResolution, ResolvedDefinition,
    definitions_for_name,
};

pub(crate) use ruff_python_ast::name::Name;
pub(crate) use ty_python_core::definition::{Definition, DefinitionKind};
pub(crate) use ty_python_semantic::types::{Type, TypeDefinition};
pub(crate) use ty_python_semantic::{ProgramEnvironment, SemanticModel};

/// Decorators that keep the wrapped function's signature.
pub(crate) const SIGNATURE_KEEPING: &[&str] = &[
    "functools.wraps",
    "functools.lru_cache",
    "functools.cache",
    "builtins.classmethod",
    "builtins.staticmethod",
    "builtins.property.setter",
];

pub(crate) struct Sem<'db> {
    pub db: &'db ProjectDatabase,
    /// Per file: a def's start offset to its `nth` among the defs with its qname.
    nth: RefCell<FxHashMap<File, Rc<FxHashMap<TextSize, u16>>>>,
    /// Per file: the outline's names for its defs (D20).
    named: RefCell<FxHashMap<File, Rc<NamedDefs>>>,
    /// The label a marker is read under (`ID: …` in a docstring), the caller's.
    marker: MarkerStyle<'static>,
}

/// What the outline calls a def: the one naming rule a request and an answer share (D20).
#[derive(Clone, Debug)]
pub(crate) struct OutlineName {
    pub qualname: String,
    /// Which of the defs of the file with this qualname it is, 0-based, in source order.
    pub occurrence: u16,
    /// What the outline says it is: `Method` for a method, not `Function`.
    pub kind: DefKind,
}

/// The outline's names for the defs of one file.
#[derive(Default)]
pub(crate) struct NamedDefs {
    /// The start of a def's name to what the outline calls it.
    by_name: FxHashMap<u32, OutlineName>,
}

impl NamedDefs {
    fn new(text: &str, marker: MarkerStyle<'_>) -> Self {
        let mut seen: FxHashMap<&str, u16> = FxHashMap::default();
        let outline = python_syntax::outline(text, marker);
        let mut named = NamedDefs::default();
        for def in &outline.defs {
            let n = seen.entry(def.qualname.as_str()).or_insert(0);
            named.by_name.insert(
                def.name_range.start,
                OutlineName {
                    qualname: def.qualname.clone(),
                    occurrence: *n,
                    kind: def.kind.clone(),
                },
            );
            *n += 1;
        }
        named
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct BodyScope<'db> {
    pub file: ProgramFile<'db>,
    pub scope: FileScopeId,
}
/// A class or a function around an entry.
#[allow(dead_code)] // inline mode is the fixture suite's cross-check and is not reachable from the public API
pub(crate) enum Scope<'db> {
    Class(Definition<'db>),
    Function(Definition<'db>),
}

/// What a function defined in a class is called on.
#[allow(dead_code)] // inline mode is the fixture suite's cross-check and is not reachable from the public API
pub(crate) enum EntryKind {
    Method,
    ClassMethod,
    StaticMethod,
}

/// A property's project accessors. A library accessor is a stub: not walked.
pub(crate) struct PropertyParts<'db> {
    pub getter: Option<Definition<'db>>,
    pub setter: Option<Definition<'db>>,
}

/// What a bound method is bound to.
pub(crate) enum BoundTo {
    Object,
    Class,
    Other,
}
pub(crate) enum Shape<'db> {
    Function(Definition<'db>), // a `def`, as ty sees it
    Class(Definition<'db>),
    Instance(Definition<'db>), // an object of this class, whichever one
    BoundMethod {
        func: Definition<'db>,
        receiver: Option<Type<'db>>, // what ty bound it to
    },
    SubclassOf(Option<Definition<'db>>), // `type[X]`; `None` for `type[Any]`, `type[T]`
    Alias(Type<'db>),                    // a type alias: what it stands for
    Union(Vec<Type<'db>>),               // keep the alternatives
    Module,
    Unknown, // Any, Unknown, Never: nothing to go on
    Other,   // keep as `Ty`
}
impl<'db> Sem<'db> {
    pub(crate) fn definition_of(
        &self,
        model: &SemanticModel<'db>,
        d: &StmtFunctionDef,
    ) -> Definition<'db> {
        d.definition(model)
    }
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
    pub(crate) fn class_type(&self, class: Definition<'db>) -> Type<'db> {
        analyzer_ext::type_of_definition(self.db, class)
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

    /// An object of the class a type expression names (`Handler` → a `Handler`).
    /// Only a class-ish type; a union is the caller's to split (`shape`).
    pub(crate) fn instance_of(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Option<Type<'db>> {
        let env = ProgramEnvironment::from_file(file);
        analyzer_ext::instance_of(self.db, &env, ty)
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

    /// A function in `class`, by what ty makes of it on an instance: the same order as `member`.
    #[allow(dead_code)] // inline mode is the fixture suite's cross-check and is not reachable from the public API
    pub(crate) fn entry_kind(&self, class: Definition<'db>, func: Definition<'db>) -> EntryKind {
        let name = self.function_name(func);
        if self.property_parts(class, &name).is_some() {
            return EntryKind::Method; // a property's getter runs on an object
        }
        let file = class.program_file(self.db);
        match self
            .member(self.instance_type(class), &name, file)
            .map(|t| (t, self.shape(t, file)))
        {
            Some((t, Shape::BoundMethod { .. })) => match self.bound_to(t) {
                BoundTo::Class => EntryKind::ClassMethod,
                _ => EntryKind::Method,
            },
            Some((_, Shape::Function(_))) => EntryKind::StaticMethod, // no receiver on an instance
            _ => EntryKind::Method, // ty can't tell (an unknown decorator): a method
        }
    }

    /// The class `def` is a classmethod of: what its first parameter, `cls`, holds when it is the entry. `None` for a method, a static
    /// method, a function, and a method of a class nested in a function (no way to say which class there).
    pub(crate) fn classmethod_class(&self, def: Definition<'db>) -> Option<Definition<'db>> {
        let chain = self.scope_chain(def);
        match chain.iter().rev().nth(1)? {
            Scope::Class(c) => {
                matches!(self.entry_kind(*c, def), EntryKind::ClassMethod).then_some(*c)
            }
            Scope::Function(_) => None,
        }
    }

    /// The project class `def` is an instance method of (a property's getter too): what an entry on it is called on. `None` for a
    /// classmethod, a static method, a function, a method of a class nested in a function, and a class that is not the project's.
    pub(crate) fn method_class(&self, def: Definition<'db>) -> Option<Definition<'db>> {
        let chain = self.scope_chain(def);
        match chain.iter().rev().nth(1)? {
            Scope::Class(c) if self.is_project(*c) => {
                matches!(self.entry_kind(*c, def), EntryKind::Method).then_some(*c)
            }
            _ => None,
        }
    }

    pub(crate) fn def_range(&self, def: Definition<'db>) -> TextRange {
        def.full_range(self.db, &self.parsed(def)).range()
    }

    #[allow(dead_code)] // inline mode is the fixture suite's cross-check and is not reachable from the public API
    pub(crate) fn function_name(&self, def: Definition<'db>) -> String {
        let parsed = self.parsed(def);
        self.function_node(def, &parsed)
            .map_or_else(String::new, |f| f.name.to_string())
    }

    /// An instance of the class, for looking up attributes on an object.
    pub(crate) fn instance_type(&self, class: Definition<'db>) -> Type<'db> {
        let env = ProgramEnvironment::from_file(class.program_file(self.db));
        analyzer_ext::instance_of(self.db, &env, self.class_type(class)).unwrap_or(Type::unknown())
    }

    /// What ty bound a method to, by kind only. Matches the variant of `__self__`,
    /// never compares it with a type built here: `Box[int]` and `Box` must both count.
    pub(crate) fn bound_to(&self, method: Type<'db>) -> BoundTo {
        match analyzer_ext::bound_method_self(self.db, method) {
            // `super().m` is bound to the synthesized `Self` type variable: an object
            Some(Type::NominalInstance(_) | Type::TypeVar(_)) => BoundTo::Object,
            Some(Type::ClassLiteral(_) | Type::GenericAlias(_) | Type::SubclassOf(_)) => {
                BoundTo::Class
            }
            _ => BoundTo::Other,
        }
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
    pub(crate) fn new(db: &'db ProjectDatabase, marker: MarkerStyle<'static>) -> Self {
        Sem {
            db,
            nth: RefCell::default(),
            named: RefCell::default(),
            marker,
        }
    }
    pub(crate) fn model(&self, file: ProgramFile<'db>) -> SemanticModel<'db> {
        SemanticModel::new(self.db, file)
    }
    /// ty's type for an expression of `model`'s file. Unknown if ty has none.
    pub(crate) fn type_of(&self, model: &SemanticModel<'db>, expr: ExprRef<'_>) -> Type<'db> {
        expr.inferred_type(model).unwrap_or(Type::unknown())
    }
    pub(crate) fn member(
        &self,
        ty: Type<'db>,
        name: &str,
        file: ProgramFile<'db>,
    ) -> Option<Type<'db>> {
        let env = ProgramEnvironment::from_file(file);
        analyzer_ext::member(self.db, &env, ty, name)
    }
    /// A literal's class (`Literal[3]` is `int`); anything else unchanged.
    pub(crate) fn widen(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Type<'db> {
        let env = ProgramEnvironment::from_file(file);
        analyzer_ext::widen(self.db, &env, ty)
    }
    pub(crate) fn type_of_import(&self, model: &SemanticModel<'db>, alias: &Alias) -> Type<'db> {
        alias.inferred_type(model).unwrap_or(Type::unknown())
    }
    pub(crate) fn shape(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Shape<'db> {
        match ty {
            Type::Union(u) => Shape::Union(u.elements(self.db).to_vec()), // [check] accessor
            Type::Dynamic(_) | Type::Never => Shape::Unknown,
            Type::ModuleLiteral(_) => Shape::Module,
            Type::FunctionLiteral(_) => {
                let env = ProgramEnvironment::from_file(file);
                match ty.definition(self.db, &env) {
                    Some(TypeDefinition::Function(d)) => Shape::Function(d),
                    _ => Shape::Other,
                }
            }
            Type::BoundMethod(_) => {
                let env = ProgramEnvironment::from_file(file);
                match ty.definition(self.db, &env) {
                    Some(TypeDefinition::Function(d)) => Shape::BoundMethod {
                        func: d,
                        receiver: analyzer_ext::bound_method_self(self.db, ty),
                    },
                    _ => Shape::Other,
                }
            }
            Type::NominalInstance(_) => {
                let env = ProgramEnvironment::from_file(file);
                match ty.definition(self.db, &env) {
                    Some(TypeDefinition::StaticClass(d)) => Shape::Instance(d),
                    _ => Shape::Other,
                }
            }
            Type::SubclassOf(_) => {
                let env = ProgramEnvironment::from_file(file);
                match ty.definition(self.db, &env) {
                    Some(TypeDefinition::StaticClass(d)) => Shape::SubclassOf(Some(d)),
                    _ => Shape::SubclassOf(None),
                }
            }
            Type::TypeAlias(alias) => Shape::Alias(alias.value_type(self.db)),
            Type::ClassLiteral(_) | Type::GenericAlias(_) => {
                let env = ProgramEnvironment::from_file(file);
                match ty.definition(self.db, &env) {
                    Some(TypeDefinition::StaticClass(d)) => Shape::Class(d),
                    _ => Shape::Other,
                }
            }
            _ => Shape::Other,
        }
    }

    /// The `def` behind a function type: ty's own function, else the one a `Callable` kept.
    /// Kept apart from `callee_definition`: `shape` must not turn every Callable into a Function.
    fn function_definition(
        &self,
        ty: Type<'db>,
        file: ProgramFile<'db>,
    ) -> Option<Definition<'db>> {
        match self.shape(ty, file) {
            Shape::Function(d) => Some(d),
            _ => analyzer_ext::callable_definition(self.db, ty),
        }
    }

    /// The definition behind a callable type: its function, its class, or the
    /// function of its bound method.
    pub(crate) fn callee_definition(
        &self,
        ty: Type<'db>,
        file: ProgramFile<'db>,
    ) -> Option<Definition<'db>> {
        match self.shape(ty, file) {
            Shape::Class(d) | Shape::BoundMethod { func: d, .. } => Some(d),
            _ => self.function_definition(ty, file),
        }
    }

    /// The docstring `ID:` of a project `def` or `class`. The one reader `scan` uses.
    pub(crate) fn docstring_id(&self, def: Definition<'db>) -> Option<String> {
        if !self.is_project(def) {
            return None;
        }
        let parsed = self.parsed(def);
        let body = match def.kind(self.db) {
            DefinitionKind::Function(f) => &f.node(&parsed).body,
            DefinitionKind::Class(c) => &c.node(&parsed).body,
            _ => return None,
        };
        // the docstring is the first statement, when it is a string literal on its own
        let doc = body
            .first()?
            .as_expr_stmt()?
            .value
            .as_string_literal_expr()?
            .value
            .to_str();
        python_syntax::docstring_marker(doc, self.marker)
    }

    pub(crate) fn is_class(&self, def: Definition<'db>) -> bool {
        matches!(def.kind(self.db), DefinitionKind::Class(_))
    }

    /// A definition's identity outside this request: its docstring `ID:` when it
    /// is project code that has one, else where it is (module, qualified name, nth).
    pub(crate) fn stable_def(&self, def: Definition<'db>) -> StableDef {
        let parsed = self.parsed(def);
        let start = match def.kind(self.db) {
            DefinitionKind::Function(f) => f.node(&parsed).range().start(),
            DefinitionKind::Class(c) => c.node(&parsed).range().start(),
            _ => self.def_position(def).1,
        };
        if let Some(id) = self.docstring_id(def) {
            return StableDef::Marked(id);
        }
        let module = file_to_module(self.db, def.program_file(self.db).resolver_file(self.db))
            .map(|m| m.name(self.db).to_string())
            .unwrap_or_default();
        let full = self.qname(def);
        let qname = full
            .strip_prefix(&format!("{module}."))
            .unwrap_or(&full)
            .to_string();
        let nth = self
            .nth_of(def.file(self.db), &parsed)
            .get(&start)
            .copied()
            .unwrap_or(0);
        StableDef::Path { module, qname, nth }
    }

    /// The outline's names for the defs of `file`, read once.
    fn named_defs(&self, file: File) -> Rc<NamedDefs> {
        if let Some(found) = self.named.borrow().get(&file) {
            return found.clone();
        }
        let table = Rc::new(NamedDefs::new(
            source_text(self.db, file).as_str(),
            self.marker,
        ));
        self.named.borrow_mut().insert(file, table.clone());
        table
    }

    /// The outline's name, occurrence and kind for a def or class: `None` for any other kind of definition, or when the outline
    /// has no def at the start of its name.
    pub(crate) fn outline_name(&self, def: Definition<'db>) -> Option<OutlineName> {
        let parsed = self.parsed(def);
        let start = match def.kind(self.db) {
            DefinitionKind::Function(f) => f.node(&parsed).name.range().start(),
            DefinitionKind::Class(c) => c.node(&parsed).name.range().start(),
            _ => return None,
        };
        self.named_defs(def.file(self.db))
            .by_name
            .get(&u32::from(start))
            .cloned()
    }

    /// `nth` of every `def` and `class` in a file, by start offset: its place
    /// among the defs with the same qualified name, in source order.
    fn nth_of(&self, file: File, parsed: &ParsedModuleRef) -> Rc<FxHashMap<TextSize, u16>> {
        if let Some(found) = self.nth.borrow().get(&file) {
            return found.clone();
        }
        struct Walker {
            path: Vec<String>,
            seen: FxHashMap<String, u16>,
            out: FxHashMap<TextSize, u16>,
        }
        impl<'a> Visitor<'a> for Walker {
            fn visit_stmt(&mut self, stmt: &'a Stmt) {
                let name = match stmt {
                    Stmt::FunctionDef(f) => Some(f.name.to_string()),
                    Stmt::ClassDef(c) => Some(c.name.to_string()),
                    _ => None,
                };
                let Some(name) = name else {
                    return walk_stmt(self, stmt);
                };
                self.path.push(name);
                let n = self.seen.entry(self.path.join(".")).or_insert(0);
                self.out.insert(stmt.range().start(), *n);
                *n += 1;
                walk_stmt(self, stmt);
                self.path.pop();
            }
        }
        let mut w = Walker {
            path: Vec::new(),
            seen: FxHashMap::default(),
            out: FxHashMap::default(),
        };
        w.visit_body(&parsed.syntax().body);
        let out = Rc::new(w.out);
        self.nth.borrow_mut().insert(file, out.clone());
        out
    }

    /// A type's identity by structure, or `None` when it can't be written
    /// one-to-one (then the key must not hold it).
    pub(crate) fn stable_type(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Option<StableType> {
        let env = ProgramEnvironment::from_file(file);
        match ty {
            Type::NominalInstance(_) => {
                let Shape::Instance(class) = self.shape(ty, file) else {
                    return None;
                };
                let args = analyzer_ext::type_args(self.db, &env, ty)?
                    .into_iter()
                    .map(|a| self.stable_type(a, file))
                    .collect::<Option<Vec<_>>>()?;
                Some(StableType::Instance {
                    class: self.stable_def(class),
                    args,
                })
            }
            Type::Callable(_) => {
                let (params, ret) = analyzer_ext::callable_parts(self.db, ty)?;
                let params = match params {
                    Some(ps) => Some(
                        ps.into_iter()
                            .map(|p| self.stable_type(p, file))
                            .collect::<Option<Vec<_>>>()?,
                    ),
                    None => None,
                };
                Some(StableType::Callable {
                    params,
                    ret: Box::new(self.stable_type(ret, file)?),
                })
            }
            _ => None,
        }
    }

    /// The name of a module literal type.
    pub(crate) fn module_name(&self, ty: Type<'db>) -> String {
        match ty {
            Type::ModuleLiteral(m) => m.module(self.db).name(self.db).to_string(),
            _ => String::new(),
        }
    }

    pub(crate) fn none_type(&self, file: ProgramFile<'db>) -> Type<'db> {
        Type::none(self.db, &ProgramEnvironment::from_file(file))
    }

    pub(crate) fn is_project(&self, def: Definition<'db>) -> bool {
        let file = def.file(self.db);
        let Some(path) = file.path(self.db).as_system_path() else {
            return false;
        };
        matches!(
            self.db.project().is_file_included(self.db, path),
            IncludeResult::Included { .. }
        )
    }
    pub(crate) fn parsed(&self, def: Definition<'db>) -> ParsedModuleRef {
        parsed_module(self.db, def.python_file(self.db)).load(self.db)
    }
    /// The `def` statement of a function. `parsed` must be `self.parsed(def)`.
    pub(crate) fn function_node<'m>(
        &self,
        def: Definition<'db>,
        parsed: &'m ParsedModuleRef,
    ) -> Option<&'m StmtFunctionDef> {
        match def.kind(self.db) {
            DefinitionKind::Function(f) => Some(f.node(parsed)),
            _ => None,
        }
    }

    /// If `name` refers to `def` statements with decorators, those defs.
    pub(crate) fn decorated_defs(
        &self,
        model: &SemanticModel<'db>,
        name: &ExprName,
    ) -> Option<Vec<Definition<'db>>> {
        let found = definitions_for_name(
            model,
            name.id.as_str(),
            AnyNodeRef::ExprName(name),
            ImportAliasResolution::ResolveAliases,
        );
        let mut defs = Vec::new();
        for r in found {
            let ResolvedDefinition::Definition(def) = r else {
                return None;
            };
            let parsed = self.parsed(def);
            let node = self.function_node(def, &parsed)?; // not a def: not this rule
            if node.decorator_list.is_empty() {
                return None; // undecorated: ty's answer is the def anyway
            }
            defs.push(def);
        }
        (!defs.is_empty()).then_some(defs)
    }

    pub(crate) fn entry(&self, file: File, dotted: &str) -> Option<Definition<'db>> {
        let program_file = self.db.program_file(file);
        let parsed = parsed_module(self.db, program_file.python_file(self.db)).load(self.db);
        let model = SemanticModel::new(self.db, program_file);

        let mut body: &[Stmt] = &parsed.syntax().body;
        let mut parts = dotted.split('.').peekable();

        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            let mut next = None;
            for stmt in body {
                match stmt {
                    Stmt::FunctionDef(f) if last && f.name.as_str() == part => {
                        return Some(f.definition(&model));
                    }
                    Stmt::ClassDef(c) if !last && c.name.as_str() == part => {
                        next = Some(&c.body[..])
                    }
                    Stmt::FunctionDef(f) if !last && f.name.as_str() == part => {
                        next = Some(&f.body[..])
                    }
                    _ => {}
                }
            }
            body = next?;
        }
        None
    }

    /// The definition's file, relative to the project root, with `/`. A file outside the root (library code) keeps its full path.
    pub(crate) fn relative_file(&self, def: Definition<'db>) -> String {
        let path = def.file(self.db).path(self.db);
        let root = self.db.project().root(self.db);
        let shown = match path
            .as_system_path()
            .and_then(|p| p.strip_prefix(root).ok())
        {
            Some(relative) => relative.to_string(),
            None => path.to_string(),
        };
        shown.replace('\\', "/")
    }

    /// Where the definition lives: its file path, then the offset of its `def`.
    /// A stable order for nodes that the walk can reach in any order.
    pub(crate) fn def_position(&self, def: Definition<'db>) -> (String, TextSize) {
        let path = def.file(self.db).path(self.db).to_string();
        let parsed = self.parsed(def);
        // A `def` keeps its node's start: call sites are stored relative to it.
        // Anything else (a class) by its definition's range: it used to be 0 for
        // every class, so sorting classes by position did nothing.
        let start = match self.function_node(def, &parsed) {
            Some(f) => f.range().start(),
            None => def.full_range(self.db, &parsed).range().start(),
        };
        (path, start)
    }

    pub(crate) fn qname(&self, def: Definition<'db>) -> String {
        let parsed = parsed_module(self.db, def.python_file(self.db)).load(self.db);
        qname::qualified_name(self.db, def, &parsed).unwrap_or_else(|| "<unknown>".into())
    }
}

/// The text of the project's files, for the wire: a call site is bytes relative to its function, and the wire wants a line and a column.
pub(crate) struct DbSources<'db>(pub &'db ProjectDatabase);

impl crate::wire::Sources for DbSources<'_> {
    fn text(&self, file: &str) -> Option<String> {
        let path = self.0.project().root(self.0).join(file);
        let file = system_path_to_file(self.0, &path).ok()?;
        Some(source_text(self.0, file).as_str().to_owned())
    }
}

/// A loaded project: the one handle of the public API, so no engine type is in it. Requests read from it and it is not changed by them.
pub struct Project {
    db: ProjectDatabase,
}

/// Why a project could not be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenError {
    /// The path is not a directory we can read.
    NotADirectory(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::NotADirectory(path) => write!(f, "{path} is not a directory"),
        }
    }
}

impl std::error::Error for OpenError {}

impl Project {
    /// Opens the project rooted at `dir`. A broken project configuration never fails it (the defaults are used): only a path that is not
    /// a directory does.
    pub fn open(dir: &str) -> Result<Project, OpenError> {
        let not = || OpenError::NotADirectory(dir.to_owned());
        let absolute = std::fs::canonicalize(dir).map_err(|_| not())?;
        let root = SystemPath::from_std_path(&absolute)
            .filter(|_| absolute.is_dir())
            .ok_or_else(not)?;
        let db = open_project(root).map_err(|_| not())?;
        Ok(Project { db })
    }

    pub(crate) fn db(&self) -> &ProjectDatabase {
        &self.db
    }

    /// A file of the project, by an absolute path or one relative to the root. `None` when there is no such file.
    pub(crate) fn file(&self, path: &str) -> Option<File> {
        let given = std::path::Path::new(path);
        let absolute = if given.is_absolute() {
            std::fs::canonicalize(given).ok()?
        } else {
            let root = self.db.project().root(&self.db);
            std::path::PathBuf::from(root.as_str()).join(given)
        };
        system_path_to_file(&self.db, SystemPath::from_std_path(&absolute)?).ok()
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Code that calls the engine and used to be a file of its own. Rule 4 (only this file names the engine) puts it here, with its tests:
// an engine bump then touches this file and nothing else. The paths other files use are re-exported just below.

#[cfg(test)]
pub(crate) use project::open;
pub(crate) use project::open_project;
#[cfg(test)]
pub(crate) use qname::qualified_name;

mod project {
    #[cfg(test)]
    use ruff_db::files::{File, system_path_to_file};

    use ruff_db::system::{OsSystem, SystemPath, SystemPathBuf};

    use ty_project::{ProjectDatabase, ProjectMetadata};
    #[cfg(test)]
    use ty_python_core::ProgramFile;
    #[cfg(test)]
    use ty_python_semantic::Db as _;

    pub(crate) fn open_project(dir: &SystemPath) -> anyhow::Result<ProjectDatabase> {
        let system = OsSystem::new(dir);
        let metadata = match ProjectMetadata::discover(dir, &system) {
            Ok(m) => m,
            Err(_err) => {
                // Broken config must never take the analyser down (quirk 13).
                // tracing::warn!("project discovery failed: {err}; using defaults");
                ProjectMetadata::new("adhoc", SystemPathBuf::from(dir.as_str()))
            }
        };
        Ok(ProjectDatabase::use_defaults(metadata, system))
    }

    /// Open `path`, returning both handles.
    #[cfg(test)]
    pub(crate) fn open<'db>(
        db: &'db ProjectDatabase,
        path: &SystemPath,
    ) -> anyhow::Result<(File, ProgramFile<'db>)> {
        let file = system_path_to_file(db, path)?;
        Ok((file, db.program_file(file)))
    }
}

mod qname {
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
        for (_file_scope_id, scope) in index.ancestor_scopes(definition.scope(db).file_scope_id(db))
        {
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
}

#[allow(dead_code)] // for `python-cli mro` (planned); its tests are here
mod mro {
    //! `base_classes`: ty's own C3-linearised MRO, via the `analyzer_ext` fork wrapper —
    //! see `experience/08-classes-and-mro/README.md`.
    //!
    //! This used to hand-roll C3 over the public, one-level-only
    //! `type_hierarchy_supertypes`, with a `covering_node` round trip to get back
    //! from that API's `(name, file, range)` to a `Definition`. Both are gone:
    //! `analyzer_ext::class_mro` already returns the linearisation, as `Definition`s
    //! directly, so there is nothing left to merge or to re-locate.
    //!
    //! DECISION — the `None` slot (next to `DECORATORS_IN_RANGE`, object 3): ty's
    //! MRO holds bases it cannot tie to a `Definition` — `Any`, `Unknown`,
    //! `Protocol`, `Generic` — and `analyzer_ext::class_mro` reports those positions as
    //! `None` rather than dropping them, so a later base is never shifted into
    //! the wrong slot. `hierarchy.py`'s `Runner(Protocol)` and `Box(Generic[T])`
    //! both produce one. Jedi reports a real name there (`typing.Protocol`,
    //! `typing.Generic`) because it infers the base expression directly; ty's
    //! `ClassBase` enum does carry that distinction (`Protocol`, `Generic`,
    //! `Dynamic`, …) but `class_mro`'s signature deliberately erases it to stay a
    //! wrapper with no logic (object 8's rule 2) — recovering the Jedi string
    //! would mean matching `ClassBase` in the fork, a `pub(crate)` type `pylspt`
    //! cannot even name. So this module renders a `None` slot as the literal
    //! placeholder below: a known deliberate loss (same shape as `IntBox` ->
    //! `Box`, stripping the specialisation) rather than a match to Jedi's exact
    //! string. Revisit only if a consumer indexes `base_classes` by content
    //! rather than by position.
    use ruff_db::parsed::parsed_module;
    use ty_python_core::definition::Definition;
    use ty_python_semantic::Db;
    use ty_python_semantic::types::Type;
    use ty_python_semantic::types::analyzer_ext;

    use super::qname::qualified_name;

    /// A base ty's MRO places but cannot tie to a definition — `Any`, `Unknown`,
    /// `Protocol`, `Generic`. See the module doc's DECISION.
    pub(crate) const UNRESOLVED_BASE: &str = "<unresolved-base>";

    /// `base_classes` for a class, in ty's C3 order, class itself excluded —
    /// matching the `BaseNode` wire shape: `["pkg.mod.Base", "builtins.object"]`.
    pub(crate) fn base_classes<'db>(db: &'db dyn Db, ty: Type<'db>) -> Vec<String> {
        analyzer_ext::class_mro(db, ty)
            .into_iter()
            .map(|definition| {
                definition
                    .and_then(|d| qname_of(db, d))
                    .unwrap_or_else(|| UNRESOLVED_BASE.to_string())
            })
            .collect()
    }

    /// A `Definition` can live in a different file than the class being asked
    /// about (a base imported from another module), so the `ParsedModuleRef`
    /// `qualified_name` needs has to come from the definition's OWN file, not
    /// the caller's.
    fn qname_of<'db>(db: &'db dyn Db, definition: Definition<'db>) -> Option<String> {
        let parsed = parsed_module(db, definition.python_file(db)).load(db);
        qualified_name(db, definition, &parsed)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::sem::{open, open_project};
        use ruff_db::system::SystemPath;
        use ruff_python_ast::visitor::source_order::{
            SourceOrderVisitor, TraversalSignal, walk_body,
        };
        use ruff_python_ast::{AnyNodeRef, StmtClassDef};
        use ty_python_semantic::{HasType, SemanticModel};

        struct ClassCollector<'a> {
            classes: Vec<&'a StmtClassDef>,
        }

        impl<'a> SourceOrderVisitor<'a> for ClassCollector<'a> {
            fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
                if let AnyNodeRef::StmtClassDef(class_def) = node {
                    self.classes.push(class_def);
                }
                TraversalSignal::Traverse
            }
        }

        /// `base_classes` for every top-level class in `hierarchy.py`, by name.
        fn hierarchy_base_classes() -> std::collections::HashMap<String, Vec<String>> {
            let dir = std::fs::canonicalize("tests/fixtures/classes").unwrap();
            let dir = SystemPath::from_std_path(&dir).unwrap();
            let db = open_project(dir).unwrap();

            let file_path = std::fs::canonicalize("tests/fixtures/classes/hierarchy.py").unwrap();
            let file_path = SystemPath::from_std_path(&file_path).unwrap();
            let (_file, program_file) = open(&db, file_path).unwrap();
            let parsed = parsed_module(&db, program_file.python_file(&db)).load(&db);

            let mut collector = ClassCollector {
                classes: Vec::new(),
            };
            walk_body(&mut collector, &parsed.syntax().body);

            let model = SemanticModel::new(&db, program_file);
            collector
                .classes
                .into_iter()
                .filter_map(|class_def| {
                    let ty = class_def.inferred_type(&model)?;
                    Some((class_def.name.to_string(), base_classes(&db, ty)))
                })
                .collect()
        }

        /// Exercise 08, step 4's headline case: ty's own C3, not a DAG walk —
        /// `Diamond, Left, Right, Base, object`, not `Diamond, Left, Base, Right,
        /// Base, object`. This is `analyzer_ext::class_mro`'s behaviour, observed through
        /// `base_classes`.
        #[test]
        fn diamond_is_c3_linearised_not_depth_first() {
            let bases = hierarchy_base_classes();
            let diamond = &bases["Diamond"];

            assert_eq!(diamond.len(), 4, "{diamond:?}");
            assert!(diamond[0].ends_with(".Left"), "{diamond:?}");
            assert!(diamond[1].ends_with(".Right"), "{diamond:?}");
            assert!(diamond[2].ends_with(".Base"), "{diamond:?}");
            assert_eq!(diamond[3], "builtins.object");
        }

        /// `Protocol`/`Generic` bases carry no `Definition` — the `None` slot
        /// this module's DECISION renders as `UNRESOLVED_BASE`, in position,
        /// rather than dropping (which would shift a later base left by one).
        /// Only the presence of the placeholder is asserted here, not the exact
        /// surrounding sequence — see this module's DECISION doc for why the
        /// exact rendering is a deliberately lossy choice, not a parity target.
        #[test]
        fn protocol_and_generic_bases_are_unresolved_placeholders() {
            let bases = hierarchy_base_classes();

            let runner = &bases["Runner"];
            assert!(runner.contains(&UNRESOLVED_BASE.to_string()), "{runner:?}");

            let box_ = &bases["Box"];
            assert!(box_.contains(&UNRESOLVED_BASE.to_string()), "{box_:?}");
        }

        /// `object` has no base classes — `analyzer_ext::class_mro`'s own MRO is just
        /// `[object]`, and `.skip(1)` leaves nothing, matching
        /// `type_hierarchy_supertypes`'s special case without duplicating its
        /// logic.
        #[test]
        fn plain_class_ends_at_object() {
            let bases = hierarchy_base_classes();
            assert_eq!(bases["Plain"], vec!["builtins.object".to_string()]);
        }
    }
}

#[cfg(test)]
mod attrs_tests {
    //! Tests for the `analyzer_ext::member` and `analyzer_ext::instance_of` wrappers — see
    //! `experience/08-classes-and-mro/README.md`, step 5, and `src/bin/attrs.rs`
    //! for the driver that prints them side by side with the public
    //! `static_member_type_for_attribute`.
    //!
    //! No production code of its own: `analyzer_ext` IS the production surface. This
    //! module exists so those wrappers have a test in `pylspt` rather than in
    //! the fork (object 8's rule — the compiler catches a changed signature on
    //! rebase, only a test catches a changed *answer*).

    #[cfg(test)]
    mod tests {
        use crate::sem::qualified_name;
        use crate::sem::{open, open_project};
        use ruff_db::parsed::{ParsedModuleRef, parsed_module};
        use ruff_db::system::SystemPath;
        use ruff_python_ast::visitor::source_order::{
            SourceOrderVisitor, TraversalSignal, walk_body,
        };
        use ruff_python_ast::{AnyNodeRef, StmtClassDef};
        use ty_project::ProjectDatabase;
        use ty_python_core::ProgramFile;
        use ty_python_semantic::types::ProgramEnvironment;
        use ty_python_semantic::types::analyzer_ext;
        use ty_python_semantic::{HasDefinition, HasType, SemanticModel};

        struct ClassCollector<'a> {
            classes: Vec<&'a StmtClassDef>,
        }

        impl<'a> SourceOrderVisitor<'a> for ClassCollector<'a> {
            fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
                if let AnyNodeRef::StmtClassDef(class_def) = node {
                    self.classes.push(class_def);
                }
                TraversalSignal::Traverse
            }
        }

        /// Opens `attributes.py` in a project rooted at the exercise dir, and
        /// leaks both the db and the parsed module to `'static` — only ever
        /// called from a `#[test]`, which is itself short-lived, so there is
        /// nothing to free.
        fn open_attributes() -> (
            &'static ProjectDatabase,
            ProgramFile<'static>,
            &'static ParsedModuleRef,
        ) {
            let dir = std::fs::canonicalize("tests/fixtures/classes").unwrap();
            let dir = SystemPath::from_std_path(&dir).unwrap();
            let db: &'static ProjectDatabase = Box::leak(Box::new(open_project(dir).unwrap()));

            let file_path = std::fs::canonicalize("tests/fixtures/classes/attributes.py").unwrap();
            let file_path = SystemPath::from_std_path(&file_path).unwrap();
            let (_file, program_file) = open(db, file_path).unwrap();

            let parsed: &'static ParsedModuleRef = Box::leak(Box::new(
                parsed_module(db, program_file.python_file(db)).load(db),
            ));

            (db, program_file, parsed)
        }

        /// `(qualified_name, class's own Type)` for every class in `attributes.py`.
        fn classes_by_name<'db>(
            db: &'db ProjectDatabase,
            program_file: ProgramFile<'db>,
            parsed: &'db ParsedModuleRef,
        ) -> Vec<(String, ty_python_semantic::types::Type<'db>)> {
            let mut collector = ClassCollector {
                classes: Vec::new(),
            };
            walk_body(&mut collector, &parsed.syntax().body);

            let model = SemanticModel::new(db, program_file);
            collector
                .classes
                .into_iter()
                .filter_map(|class_def| {
                    let definition = class_def.definition(&model);
                    let qname = qualified_name(db, definition, parsed)?;
                    let ty = class_def.inferred_type(&model)?;
                    Some((qname, ty))
                })
                .collect()
        }

        /// `analyzer_ext::member` on `Handler`'s instance resolves `.handle` — the
        /// baseline both tools agree on (`attributes.py`'s `safe`/`inherited`
        /// shape).
        #[test]
        fn member_resolves_a_method_on_an_instance() {
            let (db, program_file, parsed) = open_attributes();
            let env = ProgramEnvironment::from_program(program_file.program(db));
            let classes = classes_by_name(db, program_file, parsed);

            let (_, handler_ty) = classes
                .iter()
                .find(|(name, _)| name.ends_with(".Handler"))
                .unwrap();
            let instance = analyzer_ext::instance_of(db, &env, *handler_ty)
                .expect("Handler is a class literal");
            let handle = analyzer_ext::member(db, &env, instance, "handle");
            assert!(handle.is_some(), "Handler instance should resolve .handle");
        }

        /// Step 5's punchline: the same wrapper, given `LoudHandler`'s instance
        /// instead of `Handler`'s, resolves `.handle` to a DIFFERENT method —
        /// the receiver picks the answer, and the caller picks the receiver.
        /// This is what `static_member_type_for_attribute` cannot do for
        /// `dispatch`: it infers the receiver from syntax, so it can only ever
        /// report the union over every construction site.
        #[test]
        fn member_answer_depends_on_which_instance_you_pass() {
            let (db, program_file, parsed) = open_attributes();
            let env = ProgramEnvironment::from_program(program_file.program(db));
            let classes = classes_by_name(db, program_file, parsed);

            let handler_ty = classes
                .iter()
                .find(|(name, _)| name.ends_with(".Handler"))
                .unwrap()
                .1;
            let loud_ty = classes
                .iter()
                .find(|(name, _)| name.ends_with(".LoudHandler"))
                .unwrap()
                .1;

            let handler_instance = analyzer_ext::instance_of(db, &env, handler_ty).unwrap();
            let loud_instance = analyzer_ext::instance_of(db, &env, loud_ty).unwrap();

            let handler_handle =
                analyzer_ext::member(db, &env, handler_instance, "handle").unwrap();
            let loud_handle = analyzer_ext::member(db, &env, loud_instance, "handle").unwrap();

            // Different receivers, different bound methods — displaying to
            // different text is the cheapest behaviour check available without
            // reaching into `FunctionType` internals the fork keeps private.
            assert_ne!(
                handler_handle.display(db, &env).to_string(),
                loud_handle.display(db, &env).to_string(),
            );
        }

        // ---- the other six wrappers: one direct test each. The compiler catches a changed signature after a bump, only a test catches a
        // changed answer, and these answers decide call targets.

        /// What to find in `wrappers.py`.
        enum Want<'w> {
            /// the first use of this name
            Name(&'w str),
            /// the first number literal
            Number,
        }

        struct Finder<'a, 'db, 'w> {
            model: &'a SemanticModel<'db>,
            want: Want<'w>,
            found: Option<ty_python_semantic::types::Type<'db>>,
        }

        impl<'a, 'db, 'w> SourceOrderVisitor<'a> for Finder<'_, 'db, 'w> {
            fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
                if self.found.is_none() {
                    self.found = match (&self.want, node) {
                        (Want::Name(name), AnyNodeRef::ExprName(n)) if n.id.as_str() == *name => {
                            n.inferred_type(self.model)
                        }
                        (Want::Number, AnyNodeRef::ExprNumberLiteral(n)) => {
                            n.inferred_type(self.model)
                        }
                        _ => None,
                    };
                }
                TraversalSignal::Traverse
            }
        }

        fn open_wrappers() -> (
            &'static ProjectDatabase,
            ProgramFile<'static>,
            &'static ParsedModuleRef,
        ) {
            let dir = std::fs::canonicalize("tests/fixtures/classes").unwrap();
            let dir = SystemPath::from_std_path(&dir).unwrap();
            let db: &'static ProjectDatabase = Box::leak(Box::new(open_project(dir).unwrap()));
            let file = std::fs::canonicalize("tests/fixtures/classes/wrappers.py").unwrap();
            let (_file, program_file) =
                open(db, SystemPath::from_std_path(&file).unwrap()).unwrap();
            let parsed: &'static ParsedModuleRef = Box::leak(Box::new(
                parsed_module(db, program_file.python_file(db)).load(db),
            ));
            (db, program_file, parsed)
        }

        fn type_of<'db>(
            db: &'db ProjectDatabase,
            program_file: ProgramFile<'db>,
            parsed: &'db ParsedModuleRef,
            want: Want<'_>,
        ) -> ty_python_semantic::types::Type<'db> {
            let model = SemanticModel::new(db, program_file);
            let mut finder = Finder {
                model: &model,
                want,
                found: None,
            };
            walk_body(&mut finder, &parsed.syntax().body);
            finder.found.expect("the fixture has what is asked for")
        }

        fn shown<'db>(
            db: &'db ProjectDatabase,
            program_file: ProgramFile<'db>,
            ty: ty_python_semantic::types::Type<'db>,
        ) -> String {
            let env = ProgramEnvironment::from_program(program_file.program(db));
            ty.display(db, &env).to_string()
        }

        /// `Literal[3]` is `int`; a type that is not a literal comes back as it was.
        #[test]
        fn widen_gives_a_literals_class_and_leaves_the_rest() {
            let (db, pf, parsed) = open_wrappers();
            let env = ProgramEnvironment::from_program(pf.program(db));
            let three = type_of(db, pf, parsed, Want::Number);
            assert_eq!(shown(db, pf, three), "Literal[3]");
            assert_eq!(shown(db, pf, analyzer_ext::widen(db, &env, three)), "int");
            let n = type_of(db, pf, parsed, Want::Name("n"));
            assert_eq!(analyzer_ext::widen(db, &env, n), n);
        }

        /// A `class` statement binds the class's own type, the same one the class statement infers.
        #[test]
        fn type_of_definition_is_a_classs_own_type() {
            let (db, pf, parsed) = open_wrappers();
            let model = SemanticModel::new(db, pf);
            let mut collector = ClassCollector {
                classes: Vec::new(),
            };
            walk_body(&mut collector, &parsed.syntax().body);
            let class = collector.classes[0];
            assert_eq!(class.name.as_str(), "Box");
            let definition = class.definition(&model);
            let bound = analyzer_ext::type_of_definition(db, definition);
            assert_eq!(Some(bound), class.inferred_type(&model));
        }

        /// A bound method is bound to its receiver; anything else is not a bound method.
        #[test]
        fn bound_method_self_is_the_receiver_and_only_for_a_bound_method() {
            let (db, program_file, parsed) = open_attributes();
            let env = ProgramEnvironment::from_program(program_file.program(db));
            let classes = classes_by_name(db, program_file, parsed);
            let handler = classes
                .iter()
                .find(|(n, _)| n.ends_with(".Handler"))
                .unwrap()
                .1;
            let instance = analyzer_ext::instance_of(db, &env, handler).unwrap();
            let handle = analyzer_ext::member(db, &env, instance, "handle").unwrap();
            assert_eq!(analyzer_ext::bound_method_self(db, handle), Some(instance));
            assert_eq!(analyzer_ext::bound_method_self(db, instance), None);
        }

        /// The type arguments of an instance, in order; none for a class that has none; `None` for what is not a nominal instance.
        #[test]
        fn type_args_are_the_instances_in_order() {
            let (db, pf, parsed) = open_wrappers();
            let env = ProgramEnvironment::from_program(pf.program(db));
            let b = type_of(db, pf, parsed, Want::Name("b"));
            let args =
                analyzer_ext::type_args(db, &env, b).expect("Box[int] is a nominal instance");
            assert_eq!(
                args.iter().map(|t| shown(db, pf, *t)).collect::<Vec<_>>(),
                ["int"]
            );
            let n = type_of(db, pf, parsed, Want::Name("n"));
            assert_eq!(analyzer_ext::type_args(db, &env, n), Some(Vec::new()));
            let cb = type_of(db, pf, parsed, Want::Name("cb"));
            assert_eq!(analyzer_ext::type_args(db, &env, cb), None);
        }

        /// One signature, as (positional parameter types, return type); `...` has no parameter list; a non-callable has no parts.
        #[test]
        fn callable_parts_are_one_signature() {
            let (db, pf, parsed) = open_wrappers();
            let cb = type_of(db, pf, parsed, Want::Name("cb"));
            let (params, ret) = analyzer_ext::callable_parts(db, cb).expect("Callable[[int], str]");
            let params = params.expect("it has a parameter list");
            assert_eq!(
                params.iter().map(|t| shown(db, pf, *t)).collect::<Vec<_>>(),
                ["int"]
            );
            assert_eq!(shown(db, pf, ret), "str");

            let any_cb = type_of(db, pf, parsed, Want::Name("any_cb"));
            let (params, ret) =
                analyzer_ext::callable_parts(db, any_cb).expect("Callable[..., str]");
            assert!(params.is_none(), "`...` has no parameter list");
            assert_eq!(shown(db, pf, ret), "str");

            let n = type_of(db, pf, parsed, Want::Name("n"));
            assert!(analyzer_ext::callable_parts(db, n).is_none());
        }

        /// A `Callable` annotation was not made from a function, and an int is not callable: no definition. (The positive case, a
        /// callable ty made from a function such as `property(f)`, is only exercised indirectly, by the fixture programs.)
        #[test]
        fn callable_definition_is_none_without_a_function_behind_it() {
            let (db, pf, parsed) = open_wrappers();
            let cb = type_of(db, pf, parsed, Want::Name("cb"));
            assert!(analyzer_ext::callable_definition(db, cb).is_none());
            let n = type_of(db, pf, parsed, Want::Name("n"));
            assert!(analyzer_ext::callable_definition(db, n).is_none());
        }
    }
}
