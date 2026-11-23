//! One file per case under `tests/fixtures/`, each with a reviewed snapshot of the tree's Debug print under `tests/snapshots/` (spans are folded to
//! `start..end "the text"`, see `common::debug_print`). `ANALYZER_BLESS=1` writes the snapshots; read `git diff` before committing them.

mod common;

use bend_parser::{Decl, parse};

#[test]
fn every_fixture_matches_its_snapshot() {
    let fixtures = common::fixtures();
    assert!(
        fixtures.len() >= 30,
        "the fixtures went missing: {}",
        fixtures.len()
    );
    for (name, text) in fixtures {
        let tree = parse(&text);
        let bad = common::invariants(&text, &tree);
        assert!(bad.is_empty(), "{name}: {}", bad.join("\n"));
        common::expect_file(
            &common::snapshots_dir().join(format!("{name}.tree")),
            &common::render(&tree, &text),
        );
    }
}

#[test]
fn the_same_text_gives_the_same_tree() {
    for (name, text) in common::fixtures() {
        assert_eq!(parse(&text), parse(&text), "{name}");
    }
}

/// A fixture named `recover_*` is there for an error; any other must read with none (the `recover_*` files say what the rules are).
#[test]
fn only_the_recovery_fixtures_have_errors() {
    for (name, text) in common::fixtures() {
        let tree = parse(&text);
        if name.starts_with("recover_") {
            assert!(
                !tree.errors.is_empty(),
                "{name}: a recovery fixture without an error"
            );
        } else {
            assert!(tree.errors.is_empty(), "{name}: {:?}", tree.errors);
        }
    }
}

/// The broken declaration keeps its name and is flagged; the ones after it are read as if nothing had happened (BD-D10).
#[test]
fn a_broken_declaration_does_not_hide_the_next_ones() {
    let text = std::fs::read_to_string(common::fixtures_dir().join("recover_def_header.bend"))
        .expect("fixture");
    let tree = parse(&text);
    let flags: Vec<(&str, bool)> = tree
        .decls
        .iter()
        .map(|d| (d.name().text.as_str(), d.syntax_error()))
        .collect();
    assert_eq!(flags, [("ok1", false), ("broken", true), ("ok2", false)]);
    let Decl::Def(broken) = &tree.decls[1] else {
        panic!("a def")
    };
    assert_eq!(
        broken.params.len(),
        1,
        "the parameters read before the error are kept"
    );
    assert!(broken.doc.is_some(), "so is the doc above it");
    assert!(broken.body.is_none(), "no body was read");
}
