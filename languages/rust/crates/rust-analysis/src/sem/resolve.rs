//! Step B (RS4 §4): what a site reaches, and whether the answer is open. The engine decides (D7: follow the engine); this file asks, path first and the
//! callable type second, and says what it was answered in the terms of the graph. The substitution the engine returns with a function is dropped here
//! (RC1 keeps it), so layer 1's edges are by construction layer 2's under an empty key.

use ra_ap_hir::{
    AsAssocItem, AssocItemContainer, CallableKind, Function, GenericDef, GenericSubstitution,
    ModuleDef, PathResolution, Type, TypeParam, Variant,
};
use ra_ap_syntax::ast;

use super::bridge::{Bridged, Func, Sem};
use super::origin::{Cause, Item};
use super::sites::{Node, RawSite};

/// The reason a site has no target: what the graph counts as an `Unresolved` edge. Not `warning::Why`, which is why a toolchain was substituted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    /// A call through a closure, a `fn` pointer or a parameter bounded by `Fn*` (D16). Its `open` says which.
    FunctionTypedValue,
    /// A call whose callee the engine could not type: an error, or a name it does not know.
    UnknownCallee,
    /// A method call that is no method and no field of the receiver's type.
    UnknownMethod,
    /// A def of the project that the outline of its file has no def for: the two parses disagree (§5). A bug that must show on the graph.
    DefNotInOutline,
}

impl Reason {
    /// The reason as the counter's key in `Stats.language`.
    pub(crate) fn counter(self) -> &'static str {
        match self {
            Reason::FunctionTypedValue => "function_typed_value",
            Reason::UnknownCallee => "unknown_callee",
            Reason::UnknownMethod => "unknown_method",
            Reason::DefNotInOutline => "def_not_in_outline",
        }
    }
}

/// Why the answer at a site is not one def but a family of them: the place a later layer (RC1 to RC5) would narrow. Layer 1 counts each, so the
/// order of that work rests on the numbers (§4, rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Open {
    /// The trait's own item, reached through a type parameter: `w.write()` with `W: Writer`, `W::make()`.
    TypeParam,
    /// The trait's own item, reached through `Self` in the trait: a default method calling `self.step()`.
    SelfInTrait,
    /// The trait's own item, reached through a parameter of type `impl Trait`.
    ImplTrait,
    /// The trait's own item, reached through an associated type of a parameter (`I::Item: Writer`, then `.write()`): answered by types, as `TypeParam`
    /// is, once RC1 substitutes the key.
    Projection,
    /// The trait's own item, reached through a return type the engine did not see through.
    Opaque,
    /// The trait's own item, reached through `dyn Trait`.
    Dyn,
    /// A call through a closure.
    Closure,
    /// A call through a `fn` pointer.
    FnPointer,
    /// A call through a parameter bounded by `Fn*`, an `impl Fn`, a `&dyn Fn`.
    FnParam,
    /// The trait's own item, and the receiver's type is none of the above and not a concrete type (an unknown type): a case not classified. It must be near zero on real code; if it is not, the classification is missing one, and the number says so before RC0 relies on it.
    Other,
}

impl Open {
    /// The counter's key in `Stats.language`.
    pub(crate) fn counter(self) -> &'static str {
        match self {
            Open::TypeParam => "open_type_param",
            Open::SelfInTrait => "open_self_in_trait",
            Open::ImplTrait => "open_impl_trait",
            Open::Projection => "open_projection",
            Open::Opaque => "open_opaque",
            Open::Dyn => "open_dyn",
            Open::Closure => "open_closure",
            Open::FnPointer => "open_fn_pointer",
            Open::FnParam => "open_fn_param",
            Open::Other => "open_other",
        }
    }
}

/// What a site reaches.
pub(crate) enum Found<'p> {
    /// A function of the project: a node of the graph.
    Def(Func<'p>),
    /// A struct, a union or an enum of the project built here (`S { .. }`, `S(..)`, `Enum::V(..)`: the class of a variant is its enum).
    Construct(Func<'p>),
    /// Not entered: a def of a crate that is not the project's, or of the project's that the walk does not enter (§5).
    External {
        package: String,
        name: String,
        cause: Cause,
        /// A construction and not a call: `Some(x)`, `Range { .. }`. There is no body to walk, so it is not counted as one that was not walked
        /// (`macro_generated_target`, `outside_root`).
        construct: bool,
    },
    Unknown(Reason),
}

/// The answer at one site: what it reaches, and whether that is the whole truth.
pub(crate) struct Callee<'p> {
    pub(crate) found: Found<'p>,
    pub(crate) open: Option<Open>,
}

/// How the receiver (or the qualifier) of a trait's own item dispatches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dispatch {
    /// The type is known: the impl is what runs, or the trait's default body for it.
    Concrete,
    Open(Open),
    /// Not concrete and not one of the open kinds: an error type.
    Unclassified,
}

impl<'p> Sem<'p> {
    /// What the site reaches. `f` is the def whose body the site is in.
    pub(crate) fn callee(&self, _f: Func<'p>, site: &RawSite) -> Callee<'p> {
        match site.node() {
            Node::Method(call) => self.method_callee(call),
            Node::Call(call) => self.call_callee(call),
            Node::Record(record) => self.record_callee(record),
        }
    }

    fn method_callee(&self, call: &ast::MethodCallExpr) -> Callee<'p> {
        // a function, or a field of function type; only the function is a method
        let Some(function) = self
            .sema()
            .resolve_method_call_fallback(call)
            .and_then(|(found, _subst)| found.left())
        else {
            return Callee {
                found: Found::Unknown(Reason::UnknownMethod),
                open: None,
            };
        };
        let open = self.trait_item(function).and_then(|_| {
            let receiver = call.receiver()?;
            let ty = self.sema().type_of_expr(&receiver)?.original();
            self.open_of(self.dispatch_of(&ty))
        });
        Callee {
            found: self.function_found(function),
            open,
        }
    }

    fn call_callee(&self, call: &ast::CallExpr) -> Callee<'p> {
        let Some(callee) = call.expr() else {
            return unknown(Reason::UnknownCallee);
        };
        let path = match &callee {
            ast::Expr::PathExpr(expr) => expr.path(),
            _ => None,
        };
        // the path first: name resolution does not need the signature, which names types the load may not know, and it is the more specific answer
        // (`<Json as Make>::make()` is the impl's `make` by path and the trait's by type)
        if let Some(path) = &path
            && let Some((resolved, subst)) = self.sema().resolve_path_with_subst(path)
        {
            match resolved {
                PathResolution::Def(ModuleDef::Function(function)) => {
                    return self.path_function(function, path, subst.as_ref());
                }
                PathResolution::Def(ModuleDef::Adt(adt)) => {
                    return self.constructed(Item::Adt(adt));
                }
                PathResolution::Def(ModuleDef::EnumVariant(variant)) => {
                    return self.constructed_variant(Variant::EnumVariant(variant));
                }
                // `Self(1, 2)`, a local, a const holding a function, nothing: the callable type says what it is
                _ => {}
            }
        }
        let Some(callable) = self.sema().resolve_expr_as_callable(&callee) else {
            return unknown(Reason::UnknownCallee);
        };
        match callable.kind() {
            CallableKind::Function(function) => match &path {
                Some(path) => self.path_function(function, path, None),
                None => Callee {
                    found: self.function_found(function),
                    open: None,
                },
            },
            CallableKind::TupleStruct(s) => self.constructed(Item::Adt(s.into())),
            CallableKind::TupleEnumVariant(v) => self.constructed_variant(Variant::EnumVariant(v)),
            CallableKind::Closure(_) => function_typed(Open::Closure),
            CallableKind::FnPtr => function_typed(Open::FnPointer),
            CallableKind::FnImpl(_) => function_typed(Open::FnParam),
        }
    }

    fn record_callee(&self, record: &ast::RecordExpr) -> Callee<'p> {
        // `hir::Variant` covers a struct, a union and an enum variant, and answers `Self { .. }` too
        match self.sema().resolve_variant(record.clone()) {
            Some(variant) => self.constructed_variant(variant),
            None => unknown(Reason::UnknownCallee),
        }
    }

    /// A function reached by a path: the open question is asked of the path's qualifier (`W::make()`), or, for a qualifier that is a trait, of the
    /// substitution's `Self`.
    fn path_function(
        &self,
        function: Function,
        path: &ast::Path,
        subst: Option<&GenericSubstitution<'p>>,
    ) -> Callee<'p> {
        let open = self
            .trait_item(function)
            .and_then(|_| self.open_of(self.qualifier_dispatch(path, subst)));
        Callee {
            found: self.function_found(function),
            open,
        }
    }

    /// The trait whose own item this function is: the declaration, or its default body. `None` for an impl's function and a free one.
    fn trait_item(&self, function: Function) -> Option<()> {
        let db = self.loaded().db();
        match function.as_assoc_item(db)?.container(db) {
            AssocItemContainer::Trait(_) => Some(()),
            AssocItemContainer::Impl(_) => None,
        }
    }

    /// `Open` for a trait's own item; `None` when the type is known, so the answer is the whole truth.
    fn open_of(&self, dispatch: Dispatch) -> Option<Open> {
        match dispatch {
            Dispatch::Concrete => None,
            Dispatch::Open(open) => Some(open),
            Dispatch::Unclassified => Some(Open::Other),
        }
    }

    /// How a path's qualifier dispatches: `W` in `W::make()`, the type in `<W as Make>::make()`, `Self` in a trait. A path with no qualifier names the
    /// function itself, not a type to dispatch on.
    fn qualifier_dispatch(
        &self,
        path: &ast::Path,
        subst: Option<&GenericSubstitution<'p>>,
    ) -> Dispatch {
        let Some(qualifier) = path.qualifier() else {
            return Dispatch::Concrete;
        };
        // `<W as Make>::make`: the qualifier is the `<…>` and holds the type
        if let Some(anchor) = qualifier
            .segment()
            .and_then(|segment| segment.type_anchor())
            && let Some(ty) = anchor.ty()
        {
            return match self.sema().resolve_type(&ty) {
                Some(ty) => self.dispatch_of(&ty),
                None => Dispatch::Unclassified,
            };
        }
        match self.sema().resolve_path(&qualifier) {
            Some(PathResolution::TypeParam(param)) => self.param_dispatch(param),
            // a type, an impl's `Self`, an alias: known
            Some(PathResolution::SelfType(_)) => Dispatch::Concrete,
            Some(PathResolution::Def(
                ModuleDef::Adt(_) | ModuleDef::TypeAlias(_) | ModuleDef::BuiltinType(_),
            )) => Dispatch::Concrete,
            // a trait path (`Writer::write(w)`, `Default::default()`) says nothing about the type: it is the substitution's `Self`
            Some(PathResolution::Def(ModuleDef::Trait(_))) => match self.substituted_self(subst) {
                Some(ty) => self.dispatch_of(&ty),
                None => Dispatch::Unclassified,
            },
            // a module, or nothing: not a type we can say
            _ => Dispatch::Unclassified,
        }
    }

    /// The type the engine substituted for `Self` in a trait's item: the entry of `GenericSubstitution::types` named `Self`.
    fn substituted_self(&self, subst: Option<&GenericSubstitution<'p>>) -> Option<Type<'p>> {
        subst?
            .types(self.loaded().db())
            .into_iter()
            .find(|(name, _)| name.as_str() == "Self")
            .map(|(_, ty)| ty)
    }

    /// How a type dispatches, looking through references and boxes (auto-deref): the first type on the way that is a parameter, a `dyn`, an opaque
    /// type or one the engine could not make out decides.
    fn dispatch_of(&self, ty: &Type<'_>) -> Dispatch {
        let db = self.loaded().db();
        for step in ty.autoderef(db) {
            if let Some(param) = step.as_type_param(db) {
                return self.param_dispatch(param);
            }
            if step.as_dyn_trait().is_some() {
                return Dispatch::Open(Open::Dyn);
            }
            if step.as_impl_traits(db).is_some() {
                return Dispatch::Open(Open::Opaque);
            }
            if step.as_associated_type_parent_trait(db).is_some() {
                return Dispatch::Open(Open::Projection);
            }
            if step.is_unknown() {
                return Dispatch::Unclassified;
            }
        }
        Dispatch::Concrete
    }

    /// A type parameter is `Self` in a trait, `impl Trait` in argument position (the engine's implicit one), or a named one. `Self` is implicit too,
    /// so it is asked first.
    fn param_dispatch(&self, param: TypeParam) -> Dispatch {
        let db = self.loaded().db();
        if matches!(param.parent(db), GenericDef::Trait(_)) && param.name(db).as_str() == "Self" {
            Dispatch::Open(Open::SelfInTrait)
        } else if param.is_implicit(db) {
            Dispatch::Open(Open::ImplTrait)
        } else {
            Dispatch::Open(Open::TypeParam)
        }
    }

    fn function_found(&self, function: Function) -> Found<'p> {
        match self.bridge(Item::Function(function)) {
            Bridged::Func(func) => Found::Def(func),
            Bridged::External {
                package,
                name,
                cause,
            } => Found::External {
                package,
                name,
                cause,
                construct: false,
            },
            Bridged::NotInOutline => Found::Unknown(Reason::DefNotInOutline),
        }
    }

    /// A construction of a struct or union.
    fn constructed(&self, class: Item) -> Callee<'p> {
        self.construction(self.bridge(class))
    }

    /// A construction of a variant: of the enum that owns it when it is the project's, named with its variant when it is not.
    fn constructed_variant(&self, variant: Variant) -> Callee<'p> {
        self.construction(self.bridge_variant(variant))
    }

    fn construction(&self, bridged: Bridged<'p>) -> Callee<'p> {
        let found = match bridged {
            Bridged::Func(func) => Found::Construct(func),
            Bridged::External {
                package,
                name,
                cause,
            } => Found::External {
                package,
                name,
                cause,
                construct: true,
            },
            Bridged::NotInOutline => Found::Unknown(Reason::DefNotInOutline),
        };
        Callee { found, open: None }
    }
}

fn unknown<'p>(reason: Reason) -> Callee<'p> {
    Callee {
        found: Found::Unknown(reason),
        open: None,
    }
}

/// A call through a value of function type (D16): unresolved, and `open` says which kind of value.
fn function_typed<'p>(open: Open) -> Callee<'p> {
    Callee {
        found: Found::Unknown(Reason::FunctionTypedValue),
        open: Some(open),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::sem::attached;
    use crate::sem::testing::{STYLE, disk_outline, disk_text, policy};

    /// What a callee is, as one reviewable line: `Def src/shapes.rs:Rec::get#1`, `Construct …`, `External alloc string::String::len`, `Unknown unknown_callee`.
    fn found<'p>(sem: &Sem<'p>, callee: &Callee<'p>) -> String {
        let def = |func: Func<'p>| {
            let info = sem.describe(func);
            let occurrence = if info.occurrence > 0 {
                format!("#{}", info.occurrence)
            } else {
                String::new()
            };
            format!("{}:{}{occurrence}", info.file, info.qualname)
        };
        let mut line = match &callee.found {
            Found::Def(func) => format!("Def {}", def(*func)),
            Found::Construct(func) => format!("Construct {}", def(*func)),
            Found::External {
                package,
                name,
                cause,
                construct,
            } => {
                let cause = match cause {
                    Cause::Dependency => String::new(),
                    other => format!(" ({other:?})"),
                };
                let what = if *construct {
                    "External construct"
                } else {
                    "External"
                };
                format!("{what} {package} {name}{cause}")
            }
            Found::Unknown(reason) => format!("Unknown {}", reason.counter()),
        };
        if let Some(open) = callee.open {
            line.push_str(&format!(
                " open={}",
                open.counter().trim_start_matches("open_")
            ));
        }
        line
    }

    /// One module's sites with what each reaches, as a reviewable text (RS4 §9 step 3's proof).
    fn render(sem: &Sem<'_>, root: &Path, file: &str) -> String {
        let text = disk_text(root, file);
        let outline = disk_outline(root, file);
        let mut out = String::new();
        for (def, occurrence) in outline.defs.iter().zip(rust_syntax::occurrences(&outline)) {
            let Some(func) = sem.entry(file, &def.qualname, occurrence) else {
                continue;
            };
            let sites = sem.sites(func);
            if sites.list.is_empty() {
                continue;
            }
            out.push_str(&def.qualname);
            if occurrence > 0 {
                out.push_str(&format!("#{occurrence}"));
            }
            out.push('\n');
            for site in &sites.list {
                let at = site.span();
                let written = &text
                    [(def.range.start + at.start) as usize..(def.range.start + at.end) as usize];
                out.push_str(&format!(
                    "  {:?} {written:?} -> {}\n",
                    site.kind(),
                    found(sem, &sem.callee(func, site))
                ));
            }
        }
        out
    }

    #[test]
    fn what_every_site_of_every_policy_module_reaches() {
        let (loaded, root) = policy("callee_snapshots");
        let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/callees");
        attached(&loaded, STYLE, |sem| {
            for file in loaded.files() {
                let name = format!("{}.txt", file.replace('/', "__"));
                analyzer_testkit::expect_file(&snapshots.join(name), &render(sem, &root, &file));
            }
        });
    }

    /// §4's `open` column, in the one place a reader looks for it: each kind of open answer, the receiver or qualifier that causes it, and the answers that
    /// are not open (a concrete type, an impl's method, what the engine sees through).
    #[test]
    fn open_row_by_row() {
        let (loaded, _root) = policy("callee_open");
        attached(&loaded, STYLE, |sem| {
            let opens = |file: &str, qualname: &str| -> Vec<Option<Open>> {
                let func = sem.entry(file, qualname, 0).unwrap();
                sem.sites(func)
                    .list
                    .iter()
                    .map(|site| sem.callee(func, site).open)
                    .collect()
            };
            let dispatch = |qualname: &str| opens("src/dispatch.rs", qualname);
            // the trait's own item, by what the receiver or the qualifier is
            assert_eq!(dispatch("generic_by_reference"), [Some(Open::TypeParam)]);
            assert_eq!(dispatch("generic_by_value"), [Some(Open::TypeParam)]);
            assert_eq!(
                dispatch("static_through_a_parameter"),
                [Some(Open::TypeParam)]
            );
            assert_eq!(
                dispatch("qualified_through_a_parameter"),
                [Some(Open::TypeParam)]
            );
            assert_eq!(dispatch("Step::twice"), [Some(Open::SelfInTrait); 2]);
            assert_eq!(dispatch("Step::pair"), [Some(Open::SelfInTrait); 2]);
            assert_eq!(dispatch("impl_argument"), [Some(Open::ImplTrait)]);
            assert_eq!(dispatch("dyn_reference"), [Some(Open::Dyn)]);
            assert_eq!(dispatch("boxed_dyn"), [Some(Open::Dyn)]);
            assert_eq!(
                dispatch("impl_return_of_a_parameter"),
                [None, Some(Open::Opaque)],
                "the hidden type depends on the function's own parameter: the engine answers the declaration"
            );
            // a value of function type
            assert_eq!(dispatch("closure_value"), [Some(Open::Closure)]);
            assert_eq!(dispatch("fn_pointer"), [Some(Open::FnPointer)]);
            assert_eq!(dispatch("fn_pointer_field"), [Some(Open::FnPointer)]);
            assert_eq!(dispatch("fn_bound"), [Some(Open::FnParam)]);
            assert_eq!(dispatch("dyn_fn"), [Some(Open::FnParam)]);
            // a trait-qualified path is asked of the substitution's `Self`: a parameter is open, a concrete type is the impl
            assert_eq!(
                dispatch("trait_path_through_a_parameter"),
                [Some(Open::TypeParam)]
            );
            assert_eq!(
                dispatch("trait_path_through_an_iterator"),
                [Some(Open::TypeParam)]
            );
            assert_eq!(
                dispatch("default_through_a_parameter"),
                [Some(Open::TypeParam)]
            );
            assert_eq!(dispatch("trait_path_on_a_concrete_type"), [None]);
            assert_eq!(dispatch("default_on_a_concrete_type"), [None]);
            // an associated type of a parameter is answered by types, as the parameter is: its own kind, so `other` keeps meaning "a case is missing"
            assert_eq!(
                dispatch("projection_receiver"),
                [Some(Open::TypeParam), None, Some(Open::Projection)]
            );
            // not open: a concrete receiver (the impl's method, and a default method of the trait for a concrete type), a return-position
            // `impl Trait` the engine sees through, a qualified path to the impl
            assert_eq!(dispatch("concrete"), [None, None]);
            assert_eq!(dispatch("impl_return"), [None, None]);
            assert_eq!(dispatch("qualified_concrete"), [None]);
            // an unknown callee and an unknown method are not open: they are unresolved
            assert_eq!(dispatch("unknown_callee"), [None, None]);
            assert_eq!(dispatch("unknown_method"), [None]);
        });
    }

    /// `Found` for the kinds of site and of target that the snapshots show only as text: a construction of a project type and of an external one, and a
    /// reason for each way a site has no target.
    #[test]
    fn found_kinds() {
        let (loaded, _root) = policy("callee_found");
        attached(&loaded, STYLE, |sem| {
            let first = |file: &str, qualname: &str| {
                let func = sem.entry(file, qualname, 0).unwrap();
                let site = sem.sites(func).list[0].clone();
                sem.callee(func, &site).found
            };
            assert!(matches!(
                first("src/calls.rs", "constructions"),
                Found::Construct(_)
            ));
            assert!(matches!(
                first("src/calls.rs", "operators"),
                Found::External {
                    construct: true,
                    ..
                }
            ));
            assert!(matches!(
                first("src/external.rs", "std_calls"),
                Found::External {
                    construct: false,
                    cause: Cause::Dependency,
                    ..
                }
            ));
            assert!(matches!(
                first("src/lib.rs", "calls_generated"),
                Found::External {
                    cause: Cause::MacroFile,
                    ..
                }
            ));
            assert!(matches!(
                first("src/dispatch.rs", "closure_value"),
                Found::Unknown(Reason::FunctionTypedValue)
            ));
            assert!(matches!(
                first("src/dispatch.rs", "unknown_callee"),
                Found::Unknown(Reason::UnknownCallee)
            ));
            assert!(matches!(
                first("src/dispatch.rs", "unknown_method"),
                Found::Unknown(Reason::UnknownMethod)
            ));
        });
    }

    /// RS4 §5's table of external names, one row each: a trait's own item, an inherent impl of an ADT, of a builtin type and of a slice, a trait impl with and
    /// without arguments, a built-in derive's, and a free function. Impls on builtin types and the several trait impls of one type must not share a name.
    #[test]
    fn external_names_follow_the_table() {
        let (loaded, _root) = policy("callee_names");
        attached(&loaded, STYLE, |sem| {
            let externals = |file: &str, qualname: &str| -> Vec<String> {
                let func = sem.entry(file, qualname, 0).unwrap();
                sem.sites(func)
                    .list
                    .iter()
                    .filter_map(|site| match sem.callee(func, site).found {
                        Found::External { package, name, .. } => Some(format!("{package} {name}")),
                        _ => None,
                    })
                    .collect()
            };
            assert_eq!(
                externals("src/external.rs", "std_naming"),
                [
                    // a trait impl with an argument: not the same name as `From<char>` or `From<String>`
                    "alloc string::<String as From<&str>>::from",
                    // an inherent impl of a slice: the self type as displayed
                    "core slice::[T]::len",
                    // free
                    "core mem::swap",
                ]
            );
            assert_eq!(
                externals("src/external.rs", "std_calls"),
                [
                    "alloc string::String::new",
                    "alloc string::String::push",
                    "alloc string::String::len",
                    "alloc vec::Vec::new",
                    // an external variant keeps its name
                    "core option::Option::Some",
                    "core result::Result::Ok",
                    "core ops::range::Range",
                    // a trait impl on a builtin type
                    "core default::<u8 as Default>::default",
                ]
            );
            // a trait's own item
            assert_eq!(
                externals("src/dispatch.rs", "trait_path_through_an_iterator"),
                ["core iter::traits::iterator::Iterator::next"]
            );
        });
    }
}
