//! RS4 §8, "no `std`": the policy workspace at rung 3 (no `cargo`, no `rustc`, an empty `HOME`: the crate graph is read from `Cargo.toml` by this crate and the
//! engine has no standard library). A worker, for the reason of `origin.rs`'s rung-3 test: `cargo test` itself puts the toolchain on the path of its children,
//! and this crate may not call `set_var`.

use std::collections::BTreeMap;
use std::path::Path;

use super::testing::{STYLE, render_sites};
use super::{Rung, attached, common};
use crate::{
    Cancel, Entry, LoadOptions, MarkerStyle, Options, Project, RenderOptions, Warning, breakdown,
    call_graph, render, to_text,
};

/// The defs whose calls are written in the arguments of a macro of `std` (`println!`, `vec!`, `assert!`), directly or through a project macro. Without `std`
/// those macros are not defined, so they do not expand: what is written in their arguments is lost, counted `macro_not_expanded`. The one place a rung-3 site
/// list may differ from rung 1's in what it holds (`method_statement` only loses the count: its `vec!` holds no call).
const STD_MACROS: [&str; 6] = [
    "std_arguments",
    "project_wrapping_external",
    "project_macro_around_external",
    "argument_through_external",
    "two_project_levels",
    "method_statement",
];

/// The defs that hand over a path of `std` (`Some`, `String::from`). Without `std` the path names nothing, so it is no site (§3: a hand-over is a site only
/// when name resolution says the path is a function or a tuple constructor), and its count (`constructor_passed`, `external_passed`) is not made. **A
/// difference from the plan's "the site list is rung 1's except inside `std`'s macros"**, found by this test.
const STD_PATHS: [(&str, &str); 2] = [("constructors", "Some"), ("externals", "String::from")];

/// A snapshot as its blocks: a def's name, and the lines under it.
fn blocks(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current = String::new();
    for line in text.lines() {
        if line.starts_with("  ") {
            out.entry(current.clone())
                .or_default()
                .push(line.to_owned());
        } else {
            current = line.to_owned();
            out.entry(current.clone()).or_default();
        }
    }
    out
}

#[test]
fn the_policy_workspace_without_std() {
    let nothing = common::tmp_root().join("no_std_tools");
    let _ = std::fs::remove_dir_all(&nothing);
    std::fs::create_dir_all(&nothing).unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "sem::nostd::worker_policy_without_std",
            "--nocapture",
        ])
        .env_clear()
        .env("RS_WORKER", "1")
        .env("PATH", &nothing)
        .env("HOME", &nothing)
        .env("USERPROFILE", &nothing);
    // a Windows process needs its system folder to start
    for key in ["SystemRoot", "TMP", "TEMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let out = command.output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    println!("{text}");
    assert!(
        out.status.success() && text.contains("1 passed"),
        "the worker failed:\n{text}"
    );
}

#[test]
#[ignore = "run by the_policy_workspace_without_std"]
fn worker_policy_without_std() {
    let _load = crate::sem::common::one_load();
    if std::env::var_os("RS_WORKER").is_none() {
        return;
    }
    let copy = common::copy_of_fixture("policy", "nostd_policy");
    let root = crate::paths::absolute(&copy).unwrap();
    let project = Project::open(&root, &root.join("src/lib.rs"), &LoadOptions::default())
        .expect("a load with no cargo is a load");
    assert_eq!(project.warnings(), [Warning::NoToolchain]);
    assert_eq!(project.loaded().rung(), Rung::Reader);

    // the site lists: rung 1's, except where std's macros lose what is in their arguments, and where a std path handed over names nothing
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/sites");
    attached(project.loaded(), STYLE, |sem| {
        for file in project.files() {
            let name = format!("{}.txt", file.replace('/', "__"));
            let rung1 = blocks(&std::fs::read_to_string(snapshots.join(name)).unwrap());
            let rung3 = blocks(&render_sites(sem, &root, &file));
            assert_eq!(
                rung1.keys().collect::<Vec<_>>(),
                rung3.keys().collect::<Vec<_>>(),
                "{file}: the same defs are entries"
            );
            for (def, lines) in &rung1 {
                let found = &rung3[def];
                let counted =
                    |lines: &[String]| lines.iter().any(|l| l.contains("macro_not_expanded"));
                if STD_MACROS.contains(&def.as_str()) {
                    // what remains is some of what was there, and the loss is counted
                    assert!(counted(found), "{file}:{def}: the loss is counted");
                    assert!(
                        found
                            .iter()
                            .filter(|l| !l.contains("macro_not_expanded"))
                            .all(|l| lines.contains(l)),
                        "{file}:{def}: {found:?}"
                    );
                } else if let Some((_, path)) = STD_PATHS.iter().find(|(d, _)| d == def) {
                    let without: Vec<String> = lines
                        .iter()
                        .filter(|l| {
                            !(l.trim_start().starts_with("Passed")
                                && l.ends_with(&format!("\"{path}\"")))
                        })
                        .cloned()
                        .collect();
                    assert_eq!(*found, without, "{file}:{def}: only the std path is gone");
                } else {
                    assert_eq!(found, lines, "{file}:{def}: the site list of rung 1");
                }
            }
        }
    });

    // the trees: the first-trees snapshot is rung 1's, and the same entries are asked again
    let snapshot = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/trees/policy.txt");
    let opts = Options::new(MarkerStyle::docstring("ID"));
    let ask = |file: &str, qualname: &str| {
        call_graph(
            &project,
            file,
            &Entry::Def {
                qualname,
                occurrence: 0,
            },
            &opts,
            &Cancel::new(),
        )
        .unwrap_or_else(|| panic!("{file}:{qualname}"))
    };
    let mut asked = 0;
    for section in std::fs::read_to_string(snapshot)
        .unwrap()
        .split("=== ")
        .skip(1)
    {
        let (head, rest) = section.split_once('\n').unwrap();
        let (file, qualname) = head.split_once(' ').unwrap();
        let rung1 = rest.split_once("\nbreakdown:\n").unwrap().0.trim_end();
        let graph = ask(file, qualname);
        let rung3 = to_text(&render(
            &graph,
            &graph.entries[0],
            &RenderOptions::default(),
        ));
        let rung3 = rung3.trim_end();
        // the load says what it lacks, in every graph
        assert_eq!(
            graph.stats.language["warning_no_toolchain"], 1,
            "{qualname}"
        );
        if STD_MACROS.contains(&qualname) {
            // a prefix of the tree, and the loss counted
            assert!(
                rung1.starts_with(rung3),
                "{qualname}: {rung3:?} in {rung1:?}"
            );
            assert!(
                graph.stats.language["macro_not_expanded"] >= 1,
                "{qualname}"
            );
        } else {
            assert_eq!(
                rung3, rung1,
                "{file}:{qualname}: the project's edges are rung 1's"
            );
        }
        asked += 1;
    }
    assert!(asked >= 40, "the snapshot's entries were asked: {asked}");

    // what the std calls become: unresolved, never an `External` (there is no std to name) and never dropped
    let chain = breakdown(&ask("src/calls.rs", "method_chain"));
    assert_eq!((chain.calls, chain.externals, chain.unresolved), (1, 0, 2));
    assert_eq!(chain.reasons["unknown_method"], 2);
    let naming = breakdown(&ask("src/external.rs", "std_naming"));
    assert_eq!((naming.externals, naming.unresolved), (0, 3));
    // `std`'s macros are not defined: nothing in their arguments is a site, and the number says so
    let macros = ask("src/macros.rs", "std_arguments");
    assert_eq!(macros.stats.language["macro_not_expanded"], 3);
    assert_eq!(breakdown(&macros).macro_not_expanded, 3);
    assert_eq!(breakdown(&macros).warnings, ["no_toolchain"]);
}
