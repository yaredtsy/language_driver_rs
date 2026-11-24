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

/// Which tree each scenario is held to.
#[derive(Debug, Clone, Copy)]
pub enum Levels {
    /// Every scenario at one level: what `run(subject, Level::Default)` has always meant.
    All(Level),
    /// The scenarios named here (by folder name) at `Level::Precise`, every other at `Level::Default`. A name that is not a scenario with a
    /// program in the subject's folder fails the run: a stale or misspelt list would otherwise hold nothing to the precise tree.
    PreciseFor(&'static [&'static str]),
}

impl From<Level> for Levels {
    fn from(level: Level) -> Self {
        Levels::All(level)
    }
}

impl Levels {
    fn of(self, scenario: &str) -> Level {
        match self {
            Levels::All(level) => level,
            Levels::PreciseFor(names) if names.contains(&scenario) => Level::Precise,
            Levels::PreciseFor(_) => Level::Default,
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
/// analyze is a failure, and so is a tree that does not match. `levels` says which tree each scenario is held to: a bare [`Level`] holds them all to
/// it, [`Levels::PreciseFor`] holds the named ones to the precise tree.
pub fn run(subject: &Subject, levels: impl Into<Levels>) -> usize {
    let levels = levels.into();
    let mut ran = Vec::new();
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
        let expected = expected_tree(&dir, subject.folder, levels.of(&name));
        expect_file(&expected, &actual);
        ran.push(name);
    }
    if let Levels::PreciseFor(names) = levels {
        for name in names {
            assert!(
                ran.iter().any(|ran| ran == name),
                "scenario {name} is listed as precise but has no {} program",
                subject.folder
            );
        }
    }
    ran.len()
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

    /// A scratch `Subject` over scenarios written under a directory of the process id, and that directory.
    fn scratch(tag: &str, scenarios: &[(&str, &[(&str, &str)])]) -> (Subject, PathBuf) {
        let dir = std::env::temp_dir().join(format!("testkit-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for (name, files) in scenarios {
            let prog = dir.join(name).join("toy");
            fs::create_dir_all(&prog).unwrap();
            fs::write(prog.join("main.toy"), "main: x\nx:\n").unwrap();
            for (file, text) in *files {
                fs::write(dir.join(name).join(file), text).unwrap();
            }
        }
        let leaked = Box::leak(dir.to_string_lossy().into_owned().into_boxed_str());
        (
            Subject {
                scenarios: leaked,
                ..TOY
            },
            dir,
        )
    }

    const RIGHT: &str = "main\n  x\n";
    const WRONG: &str = "main\n  wrong\n";

    #[test]
    fn levels_are_chosen_per_scenario() {
        let (subject, dir) = scratch(
            "levels",
            &[
                (
                    "a",
                    &[
                        ("expected.precise.tree", RIGHT),
                        ("expected.default.tree", WRONG),
                    ],
                ),
                (
                    "b",
                    &[
                        ("expected.default.tree", RIGHT),
                        ("expected.precise.tree", WRONG),
                    ],
                ),
            ],
        );
        let mixed = std::panic::catch_unwind(|| run(&subject, Levels::PreciseFor(&["a"])));
        let all_default = std::panic::catch_unwind(|| run(&subject, Level::Default));
        let all_precise = std::panic::catch_unwind(|| run(&subject, Level::Precise));
        fs::remove_dir_all(&dir).ok();
        assert_eq!(mixed.unwrap(), 2);
        // each level alone meets a wrong file
        assert!(all_default.is_err());
        assert!(all_precise.is_err());
    }

    #[test]
    #[should_panic(expected = "missing is listed as precise")]
    fn a_scenario_listed_as_precise_must_exist() {
        let (subject, dir) = scratch("listed", &[("a", &[("expected.tree", RIGHT)])]);
        let result =
            std::panic::catch_unwind(|| run(&subject, Levels::PreciseFor(&["a", "missing"])));
        fs::remove_dir_all(&dir).ok();
        std::panic::resume_unwind(result.unwrap_err());
    }
}
