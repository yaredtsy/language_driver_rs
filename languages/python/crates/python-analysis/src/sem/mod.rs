use analyzer_core::edit::MarkerStyle;
use ruff_db::files::File;
use ruff_python_ast::{Alias, ExprRef};
use ruff_text_size::TextSize;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) use ty_project::ProjectDatabase;
use ty_python_core::ProgramFile;
use ty_python_core::scope::FileScopeId;
use ty_python_semantic::HasType;
use ty_python_semantic::types::analyzer_ext;

pub(crate) use ruff_python_ast::name::Name;
pub(crate) use ty_python_core::definition::{Definition, DefinitionKind};
pub(crate) use ty_python_semantic::types::{Type, TypeDefinition};
pub(crate) use ty_python_semantic::{ProgramEnvironment, SemanticModel};

// The sem module is this folder: only its files name the engine (rule 4).
mod defs;
#[allow(dead_code)] // for `python-cli mro` (planned); its tests are here
mod mro;
mod project;
mod qname;
mod scope;

use defs::NamedDefs;
pub(crate) use project::DbSources;
pub use project::{OpenError, Project, Refreshed};
#[cfg(test)]
pub(crate) use project::{open, open_project};
#[cfg(test)]
pub(crate) use qname::qualified_name;

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
    pub(crate) fn class_type(&self, class: Definition<'db>) -> Type<'db> {
        analyzer_ext::type_of_definition(self.db, class)
    }
    /// An object of the class a type expression names (`Handler` → a `Handler`).
    /// Only a class-ish type; a union is the caller's to split (`shape`).
    pub(crate) fn instance_of(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Option<Type<'db>> {
        let env = ProgramEnvironment::from_file(file);
        analyzer_ext::instance_of(self.db, &env, ty)
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

    /// The def the engine found past something it could not see (D28): `ty` is an intersection whose positive parts are exactly one
    /// function, bound method or class, and `Unknown` beside it. That is how the engine writes a method found after a base class it
    /// cannot resolve: if that base defines the name, its own runs instead. The one part is returned; anything else is `None`.
    pub(crate) fn likely_def(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Option<Type<'db>> {
        // Negative parts say what the value is not; they do not change which def was found, so only the positive ones are read.
        let Type::Intersection(parts) = ty else {
            return None;
        };
        let (mut def, mut unknown) = (None, false);
        for part in parts.iter_positive(self.db) {
            match self.shape(part, file) {
                Shape::Unknown => unknown = true,
                Shape::Function(_) | Shape::BoundMethod { .. } | Shape::Class(_)
                    if def.is_none() =>
                {
                    def = Some(part)
                }
                _ => return None,
            }
        }
        def.filter(|_| unknown)
    }

    pub(crate) fn none_type(&self, file: ProgramFile<'db>) -> Type<'db> {
        Type::none(self.db, &ProgramEnvironment::from_file(file))
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

#[cfg(test)]
mod likely_def_tests {
    //! `Sem::likely_def` on the engine's own types, in the `11-likely-callee` project: the one reader of how the engine writes "found
    //! past an unknown base" (D28).
    use super::{Sem, Type, open, open_project};
    use analyzer_core::edit::MarkerStyle;
    use ruff_db::parsed::parsed_module;
    use ruff_db::system::SystemPath;
    use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal, walk_body};
    use ruff_python_ast::{AnyNodeRef, ExprRef};
    use ty_project::ProjectDatabase;
    use ty_python_core::ProgramFile;
    use ty_python_semantic::types::ProgramEnvironment;

    const DIR: &str = "tests/fixtures/11-likely-callee/python";

    /// The project, leaked to `'static`: only a `#[test]` calls it, which is short-lived.
    fn project() -> &'static ProjectDatabase {
        let dir = std::fs::canonicalize(DIR).unwrap();
        let dir = SystemPath::from_std_path(&dir).unwrap();
        Box::leak(Box::new(open_project(dir).unwrap()))
    }

    /// The engine's type of the first attribute `.attr` in `file`.
    fn attribute_type<'db>(
        db: &'db ProjectDatabase,
        sem: &Sem<'db>,
        file: &str,
        attr: &str,
    ) -> (Type<'db>, ProgramFile<'db>) {
        struct Finder<'a> {
            attr: &'a str,
            found: Option<ruff_python_ast::ExprAttribute>,
        }
        impl<'n> SourceOrderVisitor<'n> for Finder<'_> {
            fn enter_node(&mut self, node: AnyNodeRef<'n>) -> TraversalSignal {
                if let AnyNodeRef::ExprAttribute(a) = node
                    && self.found.is_none()
                    && a.attr.as_str() == self.attr
                {
                    self.found = Some(a.clone());
                }
                TraversalSignal::Traverse
            }
        }
        let path = std::fs::canonicalize(format!("{DIR}/{file}")).unwrap();
        let (_, pf) = open(db, SystemPath::from_std_path(&path).unwrap()).unwrap();
        let parsed = parsed_module(db, pf.python_file(db)).load(db);
        let mut finder = Finder { attr, found: None };
        walk_body(&mut finder, &parsed.syntax().body);
        let found = finder.found.expect("the fixture has the attribute");
        (sem.type_of(&sem.model(pf), ExprRef::from(&found)), pf)
    }

    fn shown<'db>(db: &'db ProjectDatabase, pf: ProgramFile<'db>, ty: Type<'db>) -> String {
        ty.display(db, &ProgramEnvironment::from_file(pf))
            .to_string()
    }

    /// A classmethod found past the unknown base: the engine's type is the bound method `&` `Unknown`, and the bound method is the part.
    #[test]
    fn the_part_of_a_def_found_past_an_unknown_base_is_the_def() {
        let db = project();
        let sem = Sem::new(db, MarkerStyle::docstring("ID"));
        let (ty, pf) = attribute_type(db, &sem, "classmethod.py", "find");
        assert_eq!(
            shown(db, pf, ty),
            "(bound method <class 'Model'>.find() -> Unknown) & Unknown"
        );
        let part = sem.likely_def(ty, pf).expect("one def beside Unknown");
        assert_eq!(
            shown(db, pf, part),
            "bound method <class 'Model'>.find() -> Unknown"
        );
        let def = sem.callee_definition(part, pf).expect("a bound method");
        assert_eq!(sem.qname(def), "base.Mixin.find");
    }

    /// A method found before the unknown base is exact, and `Unknown` alone has no part.
    #[test]
    fn an_exact_def_and_unknown_have_no_part() {
        let db = project();
        let sem = Sem::new(db, MarkerStyle::docstring("ID"));
        let (ty, pf) = attribute_type(db, &sem, "own.py", "own");
        assert_eq!(shown(db, pf, ty), "bound method Model.own() -> Unknown");
        assert!(sem.likely_def(ty, pf).is_none());
        assert!(sem.likely_def(Type::unknown(), pf).is_none());
    }
}
