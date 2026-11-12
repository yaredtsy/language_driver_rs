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
    use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal, walk_body};
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
