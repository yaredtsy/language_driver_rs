//! A method whose name is computed, called (H14, `docs/03-typescript/10-review-fixes.md` §6.2). The engine's `Node::text` panics on a computed name, and the
//! analysis named an `External` with it: `a[Symbol.iterator]()` on an array stopped a build. The name is now spelled from its text as written, by the rule the
//! outline uses (`typescript_syntax::spell_name`), so `Array.[Symbol.iterator]` is spelled as `K.[Symbol.iterator]` already is.
//!
//! **Written before it was run:** each expected edge is what the design says. `fixtures/computed-names/app` is the root; `outside/` is beside it.

mod common;

use std::path::Path;

use common::{STYLE, call_graph, edges_as_text};
use typescript_analysis::{Cancel, Entry, Options, Project};

fn edges(entry: &str) -> Vec<String> {
    let root = common::tests_dir().join("fixtures/computed-names/app");
    let project = Project::open(&root, Path::new("src/main.ts")).unwrap();
    let graph = call_graph(
        &project,
        "src/main.ts",
        &Entry::Def {
            qualname: entry,
            occurrence: 0,
        },
        &Options::new(STYLE),
        &Cancel::new(),
    )
    .unwrap_or_else(|| panic!("no def `{entry}`"));
    edges_as_text(&graph, &root)
}

fn check(entry: &str, site: &str, what: &str) {
    assert_eq!(
        edges(entry),
        [format!("{entry} | {site} | {what}")],
        "{entry}"
    );
}

#[test]
fn a_library_method_with_a_computed_name_is_an_external_spelled_as_written() {
    check(
        "arrayIterator",
        "[1, 2][Symbol.iterator]",
        "external typescript:lib Array.[Symbol.iterator]",
    );
    check(
        "asyncGenerator",
        "g[Symbol.asyncIterator]",
        "external typescript:lib AsyncGenerator.[Symbol.asyncIterator]",
    );
    check(
        "mapIterator",
        "m[Symbol.iterator]",
        "external typescript:lib Map.[Symbol.iterator]",
    );
    check(
        "disposable",
        "d[Symbol.dispose]",
        "external typescript:lib Disposable.[Symbol.dispose]",
    );
}

#[test]
fn the_same_name_declared_in_the_projects_declaration_file_or_outside_the_root_is_an_external_too()
{
    // not project code (D33): a `.d.ts` of the repo, named by where it is
    check(
        "inProjectDts",
        "i[Symbol.iterator]",
        "external src/shapes.d.ts ProjectDeclared.[Symbol.iterator]",
    );
    // a source file outside the root: named from the root
    check(
        "outsideRoot",
        "i[Symbol.iterator]",
        "external ../outside/iterish.ts OutsideIterish.[Symbol.iterator]",
    );
}

#[test]
fn in_project_code_a_computed_name_is_what_it_was() {
    // a class of the project: its method is a def, named `K.[Symbol.iterator]` (rule 10: unchanged)
    check("viaClass", "k[Symbol.iterator]", "call K.[Symbol.iterator]");
    // an interface of the project: a signature (D43)
    check(
        "inProjectInterface",
        "i[Symbol.iterator]",
        "unresolved function_typed_value",
    );
}
