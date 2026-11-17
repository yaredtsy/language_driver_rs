//! RS4 §8, "the oracle": the engine's own call hierarchy (`ide::Analysis::outgoing_calls`) as the definition of "follow the engine" (D7), executable. For
//! every function (and const and static) of a workspace, every range it reports is a site of ours with the same target, **except four classes**, each
//! recognised by a rule and not by a position, and each counted: any other difference fails.
//!
//! 1. **a nested item**: the oracle walks every descendant of a body, a nested `fn`'s too; ours are the nested def's own sites (§3).
//! 2. **a tuple variant**: the oracle's target is the variant, ours is the enum that owns it (a variant is not a def, §5).
//! 3. **a def the project does not write**: ours is `External` with cause `MacroFile` or `OtherCrate` (a function a macro made, a built-in derive's method),
//!    where the oracle's target is the macro call or the derive in a file of the project (§5).
//! 4. **an impl's method of the oracle's trait function**: the oracle takes the callable type, the trait's declaration; we take the path first, and name
//!    resolution answers the impl (`Writer::write(&json)`, `<Json as Make>::make()`, §4 rule 3).
//!
//! The oracle reports calls and method calls only: a record, a hand-over and a macro's arguments are ours alone, so what it reports is a subset of ours.

use std::path::Path;

use analyzer_core::outline::DefKind;
use ra_ap_hir::{AsAssocItem, AssocItemContainer};
use ra_ap_ide::{AnalysisHost, CallHierarchyConfig, FilePosition, RaFixtureConfig};
use ra_ap_syntax::TextSize;

use super::resolve::Found;
use super::testing::{STYLE, disk_outline};
use super::{Item, Loaded, attached};

/// How many ranges of each kind the oracle reported on one workspace.
#[derive(Debug, Default, PartialEq, Eq)]
struct Tally {
    /// A site of ours with the same target.
    same: usize,
    nested_item: usize,
    tuple_variant: usize,
    generated_target: usize,
    trait_method: usize,
}

/// Ask the oracle about every function, const and static of every file of `loaded`, and compare each answer with our sites. Panics, naming the range, on a
/// difference that is not one of the four classes.
fn compare(loaded: &Loaded, root: &Path) -> Tally {
    let host = AnalysisHost::with_database(loaded.db().clone());
    let analysis = host.analysis();
    let config = CallHierarchyConfig {
        exclude_tests: false,
        ra_fixture: RaFixtureConfig::default(),
    };
    let mut tally = Tally::default();
    attached(loaded, STYLE, |sem| {
        for file in loaded.files() {
            let outline = disk_outline(root, &file);
            let id = loaded
                .vfs()
                .iter()
                .find(|(_, path)| loaded.root_relative(path).as_deref() == Some(file.as_str()))
                .map(|(id, _)| id)
                .expect("a file of the load");
            for (def, occurrence) in outline.defs.iter().zip(rust_syntax::occurrences(&outline)) {
                let has_body = matches!(&def.kind, DefKind::Function | DefKind::Method)
                    || matches!(&def.kind, DefKind::Other(what) if what == "const" || what == "static");
                let Some(func) = sem
                    .entry(&file, &def.qualname, occurrence)
                    .filter(|_| has_body)
                else {
                    continue;
                };
                let position = FilePosition {
                    file_id: id,
                    offset: TextSize::from(def.name_range.start),
                };
                let items = analysis
                    .outgoing_calls(&config, position)
                    .expect("not cancelled")
                    .unwrap_or_default();
                let sites = sem.sites(func);
                for item in items {
                    let nav_file =
                        loaded.root_relative(loaded.vfs().file_path(item.target.file_id));
                    let focus = item
                        .target
                        .focus_range
                        .map(|range| (u32::from(range.start()), u32::from(range.end())));
                    for range in &item.ranges {
                        let (start, end) =
                            (u32::from(range.range.start()), u32::from(range.range.end()));
                        let at = format!(
                            "{file}:{} {start}..{end} -> {}",
                            def.qualname, item.target.name
                        );
                        // 1. inside a nested item: some other def of the outline holds the range and is itself inside this def
                        let nested = outline.defs.iter().any(|inner| {
                            inner.range != def.range
                                && def.range.start <= inner.range.start
                                && inner.range.end <= def.range.end
                                && inner.range.start <= start
                                && end <= inner.range.end
                        });
                        if nested {
                            tally.nested_item += 1;
                            continue;
                        }
                        let ours = sites
                            .list
                            .iter()
                            .find(|site| {
                                def.range.start + site.span().start == start
                                    && def.range.start + site.span().end == end
                            })
                            .unwrap_or_else(|| {
                                panic!("{at}: the oracle reports a range that is no site of ours")
                            });
                        let callee = sem.callee(func, ours);
                        let same_def = |target| {
                            let (target_file, name) = sem.name_of(target);
                            nav_file.as_deref() == Some(target_file.as_str())
                                && focus == Some((name.start, name.end))
                        };
                        match &callee.found {
                            Found::Def(target) | Found::Construct(target) if same_def(*target) => {
                                tally.same += 1
                            }
                            // both outside the project
                            Found::External { .. } if nav_file.is_none() => tally.same += 1,
                            // 3. what no file of the project writes, where the oracle points at the macro call or the derive
                            Found::External { cause, .. }
                                if nav_file.is_some()
                                    && matches!(
                                        cause,
                                        super::Cause::MacroFile | super::Cause::OtherCrate
                                    ) =>
                            {
                                tally.generated_target += 1;
                            }
                            // 2. a tuple variant: the oracle's target is a variant that is inside our target, the enum
                            Found::Construct(target)
                                if nav_file.as_deref() == Some(sem.name_of(*target).0.as_str())
                                    && format!("{:?}", item.target.kind) == "Some(Variant)"
                                    && focus.is_some_and(|(from, to)| {
                                        let whole = sem.describe(*target).range;
                                        whole.start <= from && to <= whole.end
                                    }) =>
                            {
                                tally.tuple_variant += 1;
                            }
                            // 4. an impl's method of the oracle's trait function
                            Found::Def(target)
                                if is_impl_of_their_trait_method(
                                    sem,
                                    loaded,
                                    *target,
                                    nav_file.as_deref(),
                                    focus,
                                ) =>
                            {
                                tally.trait_method += 1;
                            }
                            other => panic!(
                                "{at}: not one of the four classes (ours {}, the oracle's file {nav_file:?} {focus:?} {:?})",
                                match other {
                                    Found::Def(_) => "Def",
                                    Found::Construct(_) => "Construct",
                                    Found::External { .. } => "External",
                                    Found::Unknown(_) => "Unknown",
                                },
                                item.target.kind
                            ),
                        }
                    }
                }
            }
        }
    });
    tally
}

/// Whether `ours` is a method of an impl of a trait, and the oracle's target (a file and the range of its name) is that trait's function of the same name.
fn is_impl_of_their_trait_method<'p>(
    sem: &super::Sem<'p>,
    loaded: &Loaded,
    ours: super::Func<'p>,
    nav_file: Option<&str>,
    focus: Option<(u32, u32)>,
) -> bool {
    let (Some(nav_file), Some((from, to))) = (nav_file, focus) else {
        return false;
    };
    let db = loaded.db();
    let Item::Function(ours) = ours.item() else {
        return false;
    };
    let Some(theirs) = sem.function_at(nav_file, analyzer_core::text::Span::new(from, to)) else {
        return false;
    };
    let (Some(ours_item), Some(theirs_item)) = (ours.as_assoc_item(db), theirs.as_assoc_item(db))
    else {
        return false;
    };
    match (ours_item.container(db), theirs_item.container(db)) {
        (AssocItemContainer::Impl(implementation), AssocItemContainer::Trait(declaring)) => {
            implementation.trait_(db) == Some(declaring) && ours.name(db) == theirs.name(db)
        }
        _ => false,
    }
}

#[test]
fn the_engines_call_hierarchy_agrees_with_our_sites_on_the_policy_workspace() {
    let (loaded, root) = super::testing::policy("oracle_policy");
    let tally = compare(&loaded, &root);
    // The count of each class is pinned: a fixture that adds a row of a class changes it on purpose, and a new kind of difference is not counted, it fails
    // in `compare`. The spike's "10 differences" were the three classes the plan named; the nested items, which it called allowed, make 13.
    assert_eq!(
        (
            tally.nested_item,
            tally.tuple_variant,
            tally.generated_target,
            tally.trait_method
        ),
        // `nested_items`' three, `E::V` and `Shape::Pair`, `Made`, `generated`, `by_macro` twice and a derive's `clone` twice, and `<Json as Make>::make()` and
        // `Writer::write(&json)`
        (3, 2, 6, 2)
    );
    assert!(tally.same > 100, "{tally:?}");
}
