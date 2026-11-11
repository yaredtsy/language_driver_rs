//! `pub` wrappers around otherwise-private `ty_python_semantic` accessors,
//! for `python-analysis` (`languages/python/crates/python-analysis`) — the entire reason this crate is forked
//! rather than pulled from crates.io/git.
//!
//! Lives under `types` (not a sibling of it) because Rust privacy is
//! module-scoped: `crate::types::{ClassBase, ClassLiteral, ClassType}` are
//! only re-exported `pub(crate)` from `types.rs` (`types.rs:118`), so a
//! wrapper declared outside `types` could not even *name* them to call
//! `ClassLiteral::iter_mro`/`Type::member`, let alone call them.
//!
//! Rules for anything added here, because a fork without them is
//! unmaintainable:
//!
//! 1. One `pub fn` per accessor `python-analysis` actually needs — add on demand, not
//!    speculatively.
//! 2. One internal call each. No logic, no reimplemented semantics — that is
//!    the entire point of forking instead of hand-rolling MRO/descriptors/
//!    `__getattr__` a second time, where a second copy would disagree with
//!    ty wherever ty is right.
//! 3. Only genuinely PUBLIC types in a wrapper's signature — `Type<'db>`,
//!    `&dyn Db`, `&ProgramEnvironment<'db>`, plain Rust types. Never
//!    `ClassLiteral`/`ClassBase`/`ClassType` themselves: `python-analysis` could not
//!    write that type name even if the wrapper were `pub`, for the reason
//!    above.
//! 4. A rebase only ever conflicts on this file plus the one `pub mod analyzer_ext;`
//!    line in `types.rs` — keep it that way.
//!
//! `python-analysis` owns one test per wrapper (not here): the compiler catches a
//! signature change on rebase, only a test catches a *behaviour* change.

use super::{ClassBase, ClassType, KnownClass, ProgramEnvironment, Type};
use crate::{Db, types::binding_type};
use ty_python_core::definition::Definition;

/// The MRO of `ty`, without the class itself, as definitions.
///
/// `ClassLiteral::iter_mro` (`class.rs:644`) is `pub(crate)` and yields the
/// class first (`MroIterator::next`, `mro.rs:691-699`) — `.skip(1)` drops it,
/// matching Jedi's `py__mro__()[1:]` and the wire shape's `base_classes`.
///
/// Uses `to_class_type`, not `let Type::ClassLiteral(c) = ty`: `Type` has two
/// class-ish variants, `ClassLiteral` and `GenericAlias` (`Box[int]`), and
/// matching only the first silently returns an empty MRO for every
/// specialised generic.
///
/// `None` holds a base's *position* when ty cannot tie it to a `Definition`
/// (`Any`, `Unknown`, `Protocol`, `Generic`) rather than dropping it — a
/// dropped slot would shift every base after it. What a `None` slot should
/// render as in `base_classes` is `pylspt::mro`'s decision, not this one's.
///
/// Supersedes `pylspt::mro::base_classes`'s hand-rolled C3 over the public,
/// one-level-only `type_hierarchy_supertypes` IDE API — that function
/// existed only because this one was unreachable without the fork.
pub fn class_mro<'db>(db: &'db dyn Db, ty: Type<'db>) -> Vec<Option<Definition<'db>>> {
    let Some(class) = ty.to_class_type(db) else {
        return Vec::new();
    };
    class
        .iter_mro(db)
        .skip(1) // [0] is the class itself
        .map(|base| match base {
            ClassBase::Class(c) => c.class_literal(db).definition(db),
            _ => None,
        })
        .collect()
}

/// `getattr(<value of type ty>, name)`, as ty resolves it — descriptors,
/// properties, metaclasses and `__getattr__` included. `Type::member`
/// (`types.rs:4929`), private. Unlike the public
/// `static_member`/`static_member_type_for_attribute` (`ide_support.rs`),
/// which infers the receiver from AST syntax, this takes the receiver `ty`
/// as an argument — the interpreter (`pylspt`'s call tree, stage 3) picks
/// *which* type that is, per call path; the wrapper only answers what is on
/// it once chosen.
///
/// `None` for "no such attribute" or "could not be determined" — callers
/// should treat both the same way object 5's quirk 13 treats a bad parse:
/// skip, do not panic.
pub fn member<'db>(
    db: &'db dyn Db,
    env: &ProgramEnvironment<'db>,
    ty: Type<'db>,
    name: &str,
) -> Option<Type<'db>> {
    ty.member(db, env, name).ignore_possibly_undefined()
}

/// Turn a class-ish `Type` (`ClassLiteral` or `GenericAlias`) into the type
/// of ITS INSTANCES — `Type::instance` (`types/instance.rs:60`), private.
///
/// Added for `member`'s sake: choosing a receiver by hand (exercise 08 step
/// 5 — "call the wrapper with `LoudHandler`'s type as the receiver") means
/// starting from a class's `Type` (what `inferred_type` on a `StmtClassDef`
/// gives you) and turning it into the instance `member` expects, since
/// `.handle` is looked up on an *instance* of `LoudHandler`, not on the
/// class object itself. Not a class literal → `None`, same posture as
/// `class_mro`.
pub fn instance_of<'db>(
    db: &'db dyn Db,
    env: &ProgramEnvironment<'db>,
    ty: Type<'db>,
) -> Option<Type<'db>> {
    let class = ty.to_class_type(db)?;
    Some(Type::instance(db, env, class))
}

/// A literal's class: `Literal[3]` is `int`, `Literal["x"]` is `str`. Non-literals come back unchanged.
///
/// Added for the call-tree key: the walker never reads a literal's value, so
/// two calls that differ only in a literal share one key.
pub fn widen<'db>(db: &'db dyn Db, env: &ProgramEnvironment<'db>, ty: Type<'db>) -> Type<'db> {
    ty.promote(db, env)
}

/// The function a callable type was made from, when ty kept it: `property(get_h)`
/// holds `get_h` as a `Callable`. `Signature::definition` (`signatures.rs:1133`), pub(crate).
/// Overloads: no single function to walk.
pub fn callable_definition<'db>(db: &'db dyn Db, ty: Type<'db>) -> Option<Definition<'db>> {
    match ty {
        Type::Callable(c) => match c.signatures(db).overloads.as_slice() {
            [sig] => sig.definition(),
            _ => None,
        },
        _ => None,
    }
}

/// What a bound method is bound to: its `__self__`. `BoundMethodType::self_instance`
/// (`types/method.rs:29`), pub(super). Not a bound method → `None`.
pub fn bound_method_self<'db>(db: &'db dyn Db, ty: Type<'db>) -> Option<Type<'db>> {
    match ty {
        Type::BoundMethod(m) => Some(m.self_instance(db)),
        _ => None,
    }
}

/// The type a definition binds: a class's own type for a `class` statement.
/// `binding_type` (`types.rs:235`), pub(crate).   [check] the path
pub fn type_of_definition<'db>(db: &'db dyn Db, def: Definition<'db>) -> Type<'db> {
    binding_type(db, def)
}

/// The type arguments of a nominal instance, in order: `Box[int]` gives `[int]`, a
/// non-generic class gives `[]`. `None` when they can't stand for the type alone:
/// not a nominal instance, a tuple (its elements aren't its arguments), or a
/// materialized specialization. `GenericAlias::specialization` (`class.rs:407`), pub(crate).
pub fn type_args<'db>(
    db: &'db dyn Db,
    env: &ProgramEnvironment<'db>,
    ty: Type<'db>,
) -> Option<Vec<Type<'db>>> {
    let Type::NominalInstance(n) = ty else {
        return None;
    };
    if n.known_class(db) == Some(KnownClass::Tuple) {
        return None;
    }
    match n.class(db, env) {
        ClassType::NonGeneric(_) => Some(Vec::new()),
        ClassType::Generic(alias) => {
            let spec = alias.specialization(db);
            if spec.materialization_kind(db).is_some() {
                return None;
            }
            Some(spec.types(db).to_vec())
        }
    }
}

/// A `Callable` of one signature, as `(positional parameter types, return type)`.
/// The parameters are `None` for `...`. `None` for anything with another shape:
/// overloads, named or keyword-only or variadic parameters, `ParamSpec`, `Concatenate`.
/// `Signature::parameters` (`signatures.rs:1072`), `Parameters::kind`, pub(crate).
pub fn callable_parts<'db>(
    db: &'db dyn Db,
    ty: Type<'db>,
) -> Option<(Option<Vec<Type<'db>>>, Type<'db>)> {
    use super::signatures::{ParameterKind, ParametersKind};
    let Type::Callable(c) = ty else {
        return None;
    };
    let [sig] = c.signatures(db).overloads.as_slice() else {
        return None;
    };
    let ret = sig.return_ty;
    match sig.parameters().kind() {
        ParametersKind::Gradual => Some((None, ret)),
        ParametersKind::Standard => {
            let mut out = Vec::new();
            for p in sig.parameters().as_slice() {
                match p.kind() {
                    ParameterKind::PositionalOnly { name: None, default_type: None } => {
                        out.push(p.annotated_type())
                    }
                    _ => return None,
                }
            }
            Some((Some(out), ret))
        }
        _ => None,
    }
}
