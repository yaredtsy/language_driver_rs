//! The context layer on changes nothing yet (`docs/03-typescript/12-context-layer/05-tc1-the-body-as-data.md` §8, step 4b): every graph made with
//! `Precision::Precise` is the one `Precision::Default` makes, stats included. `Precise` runs twice, the second time on tables already kept.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::{STYLE, call_graph, cases, open_project, tests_dir};
use typescript_analysis::{Cancel, Entry, Options, Precision, Project};
use typescript_syntax::Dialect;

/// One entry to compare: its name, its file, and the def in it (qualname and occurrence); `None` is the file's own code.
type Item = (String, String, Option<(String, u16)>);

/// One project, opened once, and the entries to compare on it.
struct Plan {
    project: Project,
    entries: Vec<Item>,
}

/// The fixtures whose every def and file are compared, and the file each is opened with.
const WHOLE: [(&str, &str); 5] = [
    ("flow", "src/flow.tsx"),
    ("handed-over", "src/main.ts"),
    ("site-kinds", "src/jsx.tsx"),
    ("object-arrows", "src/main.ts"),
    ("anonymous-in-place", "src/main.ts"),
];

#[test]
fn every_graph_is_the_same_with_the_layer_on() {
    let mut plans: BTreeMap<String, Plan> = BTreeMap::new();
    // the cases of the shared list: one project per root, opened on its first case
    for case in cases() {
        let key = case.root.to_string_lossy().into_owned();
        let plan = plans.entry(key).or_insert_with(|| Plan {
            project: open_project(&case),
            entries: Vec::new(),
        });
        plan.entries
            .push((case.name, case.file, Some((case.entry, 0))));
    }
    // every def and the code of every file of the fixtures that read the table
    for (fixture, open_with) in WHOLE {
        let root = tests_dir().join("fixtures").join(fixture);
        let project = Project::open(&root, Path::new(open_with))
            .unwrap_or_else(|error| panic!("{fixture}: {error:?}"));
        let mut entries = Vec::new();
        for name in project.files() {
            let Some(dialect) = Dialect::from_path(&name) else {
                continue;
            };
            let text = std::fs::read_to_string(root.join(&name)).unwrap();
            let outline = typescript_syntax::outline(&text, dialect, STYLE);
            let mut seen: BTreeMap<&str, u16> = BTreeMap::new();
            for def in &outline.defs {
                let occurrence = seen.entry(def.qualname.as_str()).or_default();
                entries.push((
                    format!("{fixture} {name} {} #{occurrence}", def.qualname),
                    name.clone(),
                    Some((def.qualname.clone(), *occurrence)),
                ));
                *occurrence += 1;
            }
            entries.push((format!("{fixture} {name} FILE"), name, None));
        }
        let key = format!("whole {fixture}");
        assert!(plans.insert(key, Plan { project, entries }).is_none());
    }

    let default = Options::new(STYLE);
    let mut precise = Options::new(STYLE);
    precise.precision = Precision::Precise;
    let cancel = Cancel::new();

    let mut compared = 0;
    let mut differ = Vec::new();
    for plan in plans.values() {
        for (name, file, def) in &plan.entries {
            let entry = match def {
                Some((qualname, occurrence)) => Entry::Def {
                    qualname,
                    occurrence: *occurrence,
                },
                None => Entry::File,
            };
            let graph =
                |options: &Options| call_graph(&plan.project, file, &entry, options, &cancel);
            let (off, on, again) = (graph(&default), graph(&precise), graph(&precise));
            compared += 1;
            if off != on || on != again {
                differ.push(format!(
                    "{name}: default == precise {}, precise == precise again {}\n  default {:.300?}\n  precise {:.300?}",
                    off == on,
                    on == again,
                    off,
                    on
                ));
            }
        }
    }
    eprintln!("compared {compared} cases, each as Default, Precise and Precise again");
    assert!(compared > 0);
    assert!(differ.is_empty(), "{} differ: {differ:#?}", differ.len());
}
