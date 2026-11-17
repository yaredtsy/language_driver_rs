//! Scenarios: the same small program in every language, with the tree it must give.
//!
//! ```text
//! scenarios/two-callers/
//! ├── python/ typescript/ rust/        the program, once per language
//! ├── expected.default.tree            what a default resolver gives
//! └── expected.precise.tree            what a context-sensitive solver gives
//! ```
//!
//! A scenario where both levels agree has one `expected.tree`.
//!
//! A language whose names differ (`Counter::bump` is `Counter.bump` elsewhere) or whose program has no counterpart for a row (Rust has no constructor
//! function) keeps its own tree next to the shared one, named after its folder: `expected.default.rust.tree`, or `expected.rust.tree` for a scenario with one
//! level. It is looked for **first**, so the shared tree stays what every other language is held to.

use std::fs;
use std::path::{Path, PathBuf};

use analyzer_core::graph::Graph;
use analyzer_core::render::{RenderOptions, render, to_text};

use crate::{expect_file, graph};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Default,
    Precise,
}

impl Level {
    fn file(self) -> &'static str {
        match self {
            Level::Default => "expected.default.tree",
            Level::Precise => "expected.precise.tree",
        }
    }
}

/// How one language runs a scenario.
pub struct Subject {
    /// The folder inside a scenario that holds this language's program: `"python"`, `"typescript"`, `"rust"`.
    pub folder: &'static str,
    /// The `scenarios/` directory, relative to the test's working directory (a crate root).
    pub scenarios: &'static str,
    /// Build the graph of the program in that folder. Its first entry is the scenario's entry (`main`).
    pub graph: fn(program: &Path) -> Option<Graph>,
}

/// Run every scenario that has a program in this language's folder. Returns how many ran.
///
/// A scenario without that folder is skipped (it is not written in this language yet); a folder the language cannot
/// analyze is a failure, and so is a tree that does not match.
pub fn run(subject: &Subject, level: Level) -> usize {
    let mut ran = 0;
    for dir in sorted_dirs(Path::new(subject.scenarios)) {
        let program = dir.join(subject.folder);
        if !program.is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let graph = (subject.graph)(&program)
            .unwrap_or_else(|| panic!("scenario {name}: no graph for {}", program.display()));
        graph::check_invariants(&graph);
        let entry = graph
            .entries
            .first()
            .unwrap_or_else(|| panic!("scenario {name}: the graph has no entry"));
        let actual = to_text(&render(&graph, entry, &RenderOptions::default()));
        let expected = expected_tree(&dir, subject.folder, level);
        expect_file(&expected, &actual);
        ran += 1;
    }
    ran
}

/// The tree a language's graph is compared with, most specific first: its own tree for the level, its own single tree, the shared tree for the level,
/// the shared single tree. None of them existing is the shared tree for the level, which is the file a blessed run writes.
fn expected_tree(scenario: &Path, folder: &str, level: Level) -> PathBuf {
    let own = |name: &str| name.replacen(".tree", &format!(".{folder}.tree"), 1);
    [
        scenario.join(own(level.file())),
        scenario.join(own("expected.tree")),
        scenario.join(level.file()),
        scenario.join("expected.tree"),
    ]
    .into_iter()
    .find(|p| p.is_file())
    .unwrap_or_else(|| scenario.join(level.file()))
}

fn sorted_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toy;

    const TOY: Subject = Subject {
        folder: "toy",
        scenarios: "tests/scenarios",
        graph: toy::scenario_graph,
    };

    /// A language's own tree is looked for before the shared one, and only the language that has one is held to it.
    #[test]
    fn a_languages_own_tree_comes_before_the_shared_one() {
        let dir = std::env::temp_dir().join(format!("testkit-own-tree-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let touch = |name: &str| {
            let path = dir.join(name);
            fs::write(&path, "x\n").unwrap();
            path
        };
        // nothing written: the shared tree for the level, the file a blessed run creates
        assert_eq!(
            expected_tree(&dir, "rust", Level::Default),
            dir.join("expected.default.tree")
        );
        let shared = touch("expected.tree");
        assert_eq!(expected_tree(&dir, "rust", Level::Default), shared);
        // a language's own single tree beats it, for that language only
        let own = touch("expected.rust.tree");
        assert_eq!(expected_tree(&dir, "rust", Level::Default), own);
        assert_eq!(expected_tree(&dir, "python", Level::Default), shared);
        // a level of the shared tree beats the shared single tree, and a language's own level tree beats everything
        let shared_level = touch("expected.default.tree");
        assert_eq!(expected_tree(&dir, "python", Level::Default), shared_level);
        assert_eq!(expected_tree(&dir, "python", Level::Precise), shared);
        let own_level = touch("expected.default.rust.tree");
        assert_eq!(expected_tree(&dir, "rust", Level::Default), own_level);
        assert_eq!(expected_tree(&dir, "rust", Level::Precise), own);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn toy_scenarios_match_their_trees() {
        assert_eq!(run(&TOY, Level::Default), 2);
    }

    #[test]
    #[should_panic(expected = "differs")]
    fn a_wrong_tree_fails() {
        // the `precise` files do not exist for the toy, so write a deliberately wrong one through a scratch scenario
        let dir = std::env::temp_dir().join(format!("testkit-wrong-tree-{}", std::process::id()));
        let prog = dir.join("only").join("toy");
        fs::create_dir_all(&prog).unwrap();
        fs::write(prog.join("main.toy"), "main: a\na:\n").unwrap();
        fs::write(
            dir.join("only").join("expected.default.tree"),
            "main\n  b\n",
        )
        .unwrap();
        let scratch = Box::leak(dir.to_string_lossy().into_owned().into_boxed_str());
        let result = std::panic::catch_unwind(|| {
            run(
                &Subject {
                    scenarios: scratch,
                    ..TOY
                },
                Level::Default,
            )
        });
        fs::remove_dir_all(&dir).ok();
        std::panic::resume_unwind(result.unwrap_err());
    }
}
