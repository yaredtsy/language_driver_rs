//! One body, read once: the sites of a def, in the solver's terms. `sem` reads the tree (it is the only file that touches tsrs nodes) and hands back
//! plain spans; this file puts them where the core wants them. Targets come with `Lang` (TS4 steps 3 and 4): until then every site is only a site.

use analyzer_core::graph::Site;
use typescript_syntax::MarkerStyle;

use crate::sem::{Func, Sem};

/// The sites of `func`'s body in source order, each a byte range **relative to the start of the def** (`Def::range.start`), so an edit above the
/// def does not move them. `None` for a def with no node in the program's tree (`FileDefs::nodes`: a bug in one of the two parses): the caller says so
/// on the graph and counts it (`def_without_node`) instead of reporting a body with no calls.
pub(crate) fn sites(sem: &Sem, func: Func<'_>, style: MarkerStyle<'_>) -> Option<Vec<Site>> {
    let spans = sem.call_sites(func, style)?;
    let start = sem.def(func, style).range.start;
    Some(
        spans
            .into_iter()
            .map(|span| {
                Site::new(
                    span.start.saturating_sub(start),
                    span.end.saturating_sub(start),
                )
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use typescript_syntax::Placement;

    use super::*;

    const STYLE: MarkerStyle<'static> = MarkerStyle {
        label: "ID",
        placement: Placement::Docstring,
    };

    #[test]
    fn sites_are_relative_to_the_start_of_the_def_and_in_source_order() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sites");
        let sem = Sem::open(&root, Path::new("src/sites.ts")).unwrap();
        let text = std::fs::read_to_string(root.join("src/sites.ts")).unwrap();
        let plain = sem.entry("src/sites.ts", "plain", 0, STYLE).unwrap();
        let start = sem.def(plain, STYLE).range.start;
        let found = sites(&sem, plain, STYLE).unwrap();
        // adding the def's start gives the callee back in the file
        let callees: Vec<&str> = found
            .iter()
            .map(|site| &text[(start + site.start) as usize..(start + site.end) as usize])
            .collect();
        assert_eq!(
            callees,
            ["f", "a.b", "a?.b", "obj.x.y", "a[\"key\"]", "(g)"]
        );
        assert!(found.windows(2).all(|pair| pair[0] < pair[1]), "{found:?}");
        // the first site is inside the body, not at the def's first byte
        assert!(found[0].start > 0);
    }

    #[test]
    fn moving_a_def_does_not_change_its_sites() {
        // the same function under a different amount of text above it: the relative sites are the same
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sites");
        let sem = Sem::open(&root, Path::new("src/sites.ts")).unwrap();
        let at = |qualname: &str| {
            let func = sem.entry("src/sites.ts", qualname, 0, STYLE).unwrap();
            sites(&sem, func, STYLE).unwrap()
        };
        // `Widget.method` and `table.load` both hold exactly `f();` as their body, at different places in the file and under different headers
        let method = at("Widget.method");
        let load = at("table.load");
        assert_eq!(method.len(), 1);
        assert_eq!(load.len(), 1);
        assert_eq!(method[0].end - method[0].start, load[0].end - load[0].start);
    }
}
