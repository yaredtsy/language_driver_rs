//! E1 (D20): every row of `graph.defs` names a def the outline of its file has: the `occurrence`th def with that `qualname`, and it is the same
//! def (same kind, same range). The one naming rule a request and an answer share, so a row that is wrong is the bug.

use std::path::{Path, PathBuf};

use typescript_analysis::{Cancel, Entry, MarkerStyle, Options, Placement, Project, call_graph};
use typescript_syntax::Dialect;

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A shared scenario written in TypeScript: `scenarios/<name>/typescript`, repo-relative.
fn scenario(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../scenarios")
        .join(name)
        .join("typescript")
}

#[test]
fn every_def_row_of_a_graph_names_a_def_of_its_files_outline() {
    let mut rows = 0;
    for (dir, root, file, entry) in [
        ("resolve", fixture("resolve"), "src/main.ts", "callers"),
        (
            "method-on-class",
            scenario("method-on-class"),
            "main.ts",
            "main",
        ),
        (
            "construct-marked-class",
            scenario("construct-marked-class"),
            "main.ts",
            "main",
        ),
    ] {
        let Ok(project) = Project::open(&root, Path::new(file)) else {
            continue;
        };
        let entry = Entry {
            qualname: entry,
            occurrence: 0,
        };
        let Some(graph) = call_graph(&project, file, &entry, &Options::new(STYLE), &Cancel::new())
        else {
            continue;
        };
        for d in &graph.defs {
            let Ok(text) = std::fs::read_to_string(root.join(&d.file)) else {
                continue; // a declaration file the project does not hold as text under its root
            };
            let dialect = Dialect::from_path(&d.file).expect("a def row is in a script file");
            let outline = typescript_syntax::outline(&text, dialect, STYLE);
            let same_name: Vec<_> = outline
                .defs
                .iter()
                .filter(|o| o.qualname == d.qualname)
                .collect();
            let o = same_name.get(usize::from(d.occurrence)).unwrap_or_else(|| {
                panic!(
                    "{dir}: `{}` occurrence {} is not in the outline of {} ({} defs of that name)",
                    d.qualname,
                    d.occurrence,
                    d.file,
                    same_name.len()
                )
            });
            assert_eq!(
                (&o.kind, o.range),
                (&d.kind, d.range),
                "{dir}: `{}` occurrence {} is another def than the row says",
                d.qualname,
                d.occurrence
            );
            rows += 1;
        }
    }
    assert!(rows > 5, "only {rows} rows were checked");
}
