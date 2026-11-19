//! `Session::refresh` and `Session::reload` (`docs/03-typescript/11-step6-text-and-disk.md` §7, cases 1 to 12 and 14; case 13, a file rewritten during a rebuild,
//! needs a hook inside the crate and is `session.rs`'s own test).
//!
//! **Written before it was run.** Each test writes a small project to a folder of its own, asks, changes the disk, and asks again. Every expected list is in
//! the test, not copied from a run. An edge is `from | callee text | what it is`; an unresolved edge does not say why (these cases are about *whether* a
//! call resolves, not about the engine's reason), and the list is sorted, so the order the graph holds the edges in is not asserted.
//!
//! A rewrite changes the file's **length**: two writes of the same length within the clock's resolution read as no change (design §6), and a test that
//! relied on the modified time alone would fail now and then. The one case that rests on a folder's modified time alone (a file created that nothing
//! imports, case 5) waits a moment first.
//!
//! The folders are all made once, before any test asks for one: a session stamps the folders **above** its project too, so a folder created beside it by
//! a test running in parallel would move a stamp and fail a "nothing changed" for nothing.

mod common;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use common::STYLE;
use typescript_analysis::{
    Budget, Cancel, EdgeTo, Entry, Graph, OpenError, Options, Session, Sources,
};

/// One folder per test.
const CASES: [&str; 14] = [
    "rewrite",
    "created",
    "deleted",
    "outside",
    "new-file",
    "unchanged",
    "paths",
    "node-modules",
    "lost",
    "back",
    "config",
    "reload",
    "lock",
    "reload-lost",
];

fn folder(case: &str) -> PathBuf {
    static MADE: OnceLock<PathBuf> = OnceLock::new();
    let parent = MADE.get_or_init(|| {
        let parent = Path::new(env!("CARGO_TARGET_TMPDIR")).join("session-refresh");
        for case in CASES {
            std::fs::create_dir_all(parent.join(case)).unwrap();
        }
        parent
    });
    assert!(CASES.contains(&case), "`{case}` is not in CASES");
    let dir = parent.join(case);
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            std::fs::remove_dir_all(path).unwrap();
        } else {
            std::fs::remove_file(path).unwrap();
        }
    }
    dir
}

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const TSCONFIG: &str = r#"{ "compilerOptions": { "strict": true }, "include": ["src"] }"#;

const MAIN: &str = "import { callee } from \"./lib\";\nexport function main() {\n  callee();\n}\n";
const LIB: &str = "export function callee() {\n  first();\n}\nexport function first() {}\nexport function second() {}\n";
/// `LIB` with `callee` calling `second`: one character longer.
const LIB_SECOND: &str = "export function callee() {\n  second();\n}\nexport function first() {}\nexport function second() {}\n";

/// The project most cases start from: `main` calls `callee` of `./lib`, which calls `first`.
fn small(case: &str) -> PathBuf {
    let root = folder(case);
    write(&root, "tsconfig.json", TSCONFIG);
    write(&root, "src/main.ts", MAIN);
    write(&root, "src/lib.ts", LIB);
    root
}

fn open(root: &Path) -> Session {
    Session::open(root, Path::new("src/main.ts"), Options::new(STYLE)).unwrap()
}

fn graph(session: &Session) -> Option<Graph> {
    let entry = Entry::Def {
        qualname: "main",
        occurrence: 0,
    };
    session.graph("src/main.ts", &entry, &Budget::default(), &Cancel::new())
}

/// The edges of `main`'s graph, sorted, with the callee text read from the **program's** text of the file and not from the disk: after a change the program
/// may still hold the old text, and the edges must be read against it.
fn edges(session: &Session) -> Vec<String> {
    let graph = graph(session).expect("main is there");
    let project = session.project().expect("the session has a program");
    let name = |id: &typescript_analysis::StableDef| graph.def(id).unwrap().qname.clone();
    let mut lines: Vec<String> = graph
        .edges
        .iter()
        .map(|edge| {
            let from = graph.def(&edge.from.func).unwrap();
            let text = project.text(&from.file).unwrap();
            let callee = &text[(from.range.start + edge.site.start) as usize
                ..(from.range.start + edge.site.end) as usize];
            let to = match &edge.to {
                EdgeTo::Context(id, _) => format!("call {}", name(&id.func)),
                EdgeTo::External(external, _) => {
                    format!("external {} {}", external.package, external.name)
                }
                EdgeTo::Unresolved(_) => "unresolved".to_owned(),
                _ => unreachable!("an edge kind this test does not know"),
            };
            format!("{} | {callee} | {to}", from.qname)
        })
        .collect();
    lines.sort();
    lines
}

fn lines(expected: &[&str]) -> Vec<String> {
    expected.iter().map(|line| (*line).to_owned()).collect()
}

/// Case 12: after a rebuild, the session answers what a session opened now on the same disk answers.
fn assert_as_fresh_as_open(session: &Session, root: &Path) {
    let fresh = open(root);
    assert!(graph(session).is_some(), "the session has no graph");
    assert_eq!(graph(session), graph(&fresh), "not what a fresh open says");
    assert_eq!(
        session.project().unwrap().files(),
        fresh.project().unwrap().files()
    );
}

/// Let the folder clock move before a change that is told only by a folder's modified time.
fn let_the_clock_move() {
    std::thread::sleep(Duration::from_millis(50));
}

// 1
#[test]
fn a_file_rewritten_is_seen_by_the_next_refresh_and_not_before() {
    let root = small("rewrite");
    let mut session = open(&root);
    let old = lines(&["callee | first | call first", "main | callee | call callee"]);
    let new = lines(&[
        "callee | second | call second",
        "main | callee | call callee",
    ]);
    assert_eq!(edges(&session), old);

    write(&root, "src/lib.ts", LIB_SECOND);
    // pinned: the session keeps the program it has until it is asked to look, which is what lets the driver choose when a rebuild happens
    assert_eq!(edges(&session), old);

    let refreshed = session.refresh();
    assert!(refreshed.reopened);
    assert_eq!(refreshed.lost, None);
    assert_eq!(edges(&session), new);
    assert_as_fresh_as_open(&session, &root);
}

// 2
#[test]
fn an_import_of_a_file_created_later_resolves_after_a_refresh() {
    let root = folder("created");
    write(&root, "tsconfig.json", TSCONFIG);
    write(
        &root,
        "src/main.ts",
        "import { made } from \"./made\";\nexport function main() {\n  made();\n}\n",
    );
    let mut session = open(&root);
    assert_eq!(edges(&session), lines(&["main | made | unresolved"]));

    write(&root, "src/made.ts", "export function made() {}\n");
    let refreshed = session.refresh();
    assert!(refreshed.reopened);
    assert_eq!(edges(&session), lines(&["main | made | call made"]));
    assert_as_fresh_as_open(&session, &root);
}

// 3
#[test]
fn a_callee_file_deleted_makes_the_call_unresolved_and_the_session_stays() {
    let root = small("deleted");
    let mut session = open(&root);
    assert_eq!(edges(&session).len(), 2);

    std::fs::remove_file(root.join("src/lib.ts")).unwrap();
    let refreshed = session.refresh();
    assert!(refreshed.reopened);
    assert_eq!(
        refreshed.lost, None,
        "the file the session came from is still there"
    );
    assert_eq!(edges(&session), lines(&["main | callee | unresolved"]));
    assert_as_fresh_as_open(&session, &root);
}

// 4
#[test]
fn a_file_outside_the_root_that_the_program_read_is_followed() {
    let case = folder("outside");
    let root = case.join("app");
    write(&root, "tsconfig.json", TSCONFIG);
    write(
        &root,
        "src/main.ts",
        "import { helper } from \"../../shared/lib\";\nexport function main() {\n  helper();\n}\n",
    );
    write(&case, "shared/lib.ts", "export function helper() {}\n");
    write(&case, "shared/other.ts", "export function helper() {}\n");
    let mut session = open(&root);
    assert_eq!(
        edges(&session),
        lines(&["main | helper | external ../shared/lib.ts helper"])
    );

    // the same name, now declared in another file: the external is named by the file that declares it
    write(
        &case,
        "shared/lib.ts",
        "export { helper } from \"./other\";\n",
    );
    let refreshed = session.refresh();
    assert!(refreshed.reopened);
    assert_eq!(
        edges(&session),
        lines(&["main | helper | external ../shared/other.ts helper"])
    );
    assert_as_fresh_as_open(&session, &root);
}

// 5
#[test]
fn a_new_source_file_under_an_include_folder_is_in_files_after_a_refresh() {
    let root = small("new-file");
    let mut session = open(&root);
    assert_eq!(
        session.project().unwrap().files(),
        ["src/lib.ts", "src/main.ts"]
    );

    let_the_clock_move();
    write(&root, "src/extra.ts", "export function extra() {}\n");
    assert_eq!(
        session.project().unwrap().files(),
        ["src/lib.ts", "src/main.ts"]
    );
    let refreshed = session.refresh();
    assert!(refreshed.reopened);
    assert_eq!(
        session.project().unwrap().files(),
        ["src/extra.ts", "src/lib.ts", "src/main.ts"]
    );
    assert_as_fresh_as_open(&session, &root);
}

// 6
#[test]
fn nothing_changed_is_no_rebuild_and_the_same_graph() {
    let root = small("unchanged");
    let mut session = open(&root);
    let before = graph(&session).unwrap();
    for _ in 0..3 {
        let refreshed = session.refresh();
        assert!(!refreshed.reopened);
        assert_eq!(refreshed.lost, None);
        assert_eq!(graph(&session).unwrap(), before);
    }
}

// 7
#[test]
fn a_changed_paths_mapping_in_the_tsconfig_is_followed() {
    let root = folder("paths");
    let config = |target: &str| {
        format!(
            r#"{{ "compilerOptions": {{ "strict": true, "module": "esnext", "moduleResolution": "bundler", "paths": {{ "@lib": ["./src/{target}"] }} }}, "include": ["src"] }}"#
        )
    };
    write(&root, "tsconfig.json", &config("lib.ts"));
    write(
        &root,
        "src/main.ts",
        "import { callee } from \"@lib\";\nexport function main() {\n  callee();\n}\n",
    );
    write(&root, "src/lib.ts", LIB);
    write(&root, "src/alternative.ts", LIB_SECOND);
    let mut session = open(&root);
    assert_eq!(
        edges(&session),
        lines(&["callee | first | call first", "main | callee | call callee"])
    );

    write(&root, "tsconfig.json", &config("alternative.ts"));
    let refreshed = session.refresh();
    assert!(refreshed.reopened);
    assert_eq!(
        edges(&session),
        lines(&[
            "callee | second | call second",
            "main | callee | call callee"
        ])
    );
    assert_as_fresh_as_open(&session, &root);
}

/// `main` calls `ext` and `other` of the package `pkg`, which has no `other` yet.
fn with_a_package(case: &str) -> PathBuf {
    let root = folder(case);
    write(&root, "tsconfig.json", TSCONFIG);
    write(
        &root,
        "src/main.ts",
        "import { ext, other } from \"pkg\";\nexport function main() {\n  ext();\n  other();\n}\n",
    );
    write(
        &root,
        "node_modules/pkg/package.json",
        r#"{ "name": "pkg", "version": "1.0.0", "types": "index.d.ts" }"#,
    );
    write(
        &root,
        "node_modules/pkg/index.d.ts",
        "export declare function ext(): void;\n",
    );
    root
}

const PKG_BEFORE: [&str; 2] = ["main | ext | external pkg ext", "main | other | unresolved"];
const PKG_AFTER: [&str; 2] = [
    "main | ext | external pkg ext",
    "main | other | external pkg other",
];
const PKG_EDITED: &str =
    "export declare function ext(): void;\nexport declare function other(): void;\n";

// 8
#[test]
fn a_file_in_node_modules_edited_by_hand_is_seen_by_reload_and_not_by_refresh() {
    let root = with_a_package("node-modules");
    let mut session = open(&root);
    assert_eq!(edges(&session), lines(&PKG_BEFORE));

    write(&root, "node_modules/pkg/index.d.ts", PKG_EDITED);
    // a known limit, pinned (design §6): nothing under `node_modules` is stamped, and no folder was added
    let refreshed = session.refresh();
    assert!(!refreshed.reopened);
    assert_eq!(edges(&session), lines(&PKG_BEFORE));

    let reloaded = session.reload();
    assert!(reloaded.reopened);
    assert_eq!(reloaded.lost, None);
    assert_eq!(edges(&session), lines(&PKG_AFTER));
    assert_as_fresh_as_open(&session, &root);
}

// 9
#[test]
fn the_file_the_session_came_from_deleted_loses_the_program_and_says_so_every_time() {
    let root = small("lost");
    let mut session = open(&root);

    std::fs::remove_file(root.join("src/main.ts")).unwrap();
    for _ in 0..3 {
        let refreshed = session.refresh();
        assert!(!refreshed.reopened);
        assert_eq!(refreshed.lost, Some(OpenError::NoSuchFile));
        assert!(session.project().is_none());
        assert!(graph(&session).is_none());
    }
    // `reload` says it too
    let reloaded = session.reload();
    assert_eq!(reloaded.lost, Some(OpenError::NoSuchFile));
}

// 10
#[test]
fn a_lost_session_comes_back_when_the_file_does() {
    let root = small("back");
    let mut session = open(&root);
    std::fs::remove_file(root.join("src/main.ts")).unwrap();
    assert_eq!(session.refresh().lost, Some(OpenError::NoSuchFile));
    assert!(graph(&session).is_none());

    write(&root, "src/main.ts", MAIN);
    let refreshed = session.refresh();
    assert!(refreshed.reopened);
    assert_eq!(refreshed.lost, None);
    assert_eq!(
        edges(&session),
        lines(&["callee | first | call first", "main | callee | call callee"])
    );
    assert_as_fresh_as_open(&session, &root);
}

// 11
#[test]
fn a_tsconfig_saved_with_a_syntax_error_loses_the_program_until_it_is_fixed() {
    let root = small("config");
    let mut session = open(&root);

    write(&root, "tsconfig.json", "{ \"compilerOptions\": ");
    for _ in 0..2 {
        let refreshed = session.refresh();
        assert!(!refreshed.reopened);
        assert!(
            matches!(refreshed.lost, Some(OpenError::ConfigError(_))),
            "{:?}",
            refreshed.lost
        );
        assert!(graph(&session).is_none());
    }

    write(&root, "tsconfig.json", TSCONFIG);
    let refreshed = session.refresh();
    assert!(refreshed.reopened);
    assert_eq!(refreshed.lost, None);
    assert_eq!(
        edges(&session),
        lines(&["callee | first | call first", "main | callee | call callee"])
    );
    assert_as_fresh_as_open(&session, &root);
}

/// `reload` rebuilds when no stamp moved, and a lost session that `reload` cannot help says why again.
#[test]
fn reload_rebuilds_whatever_the_stamps_say() {
    let root = small("reload");
    let mut session = open(&root);
    let before = graph(&session).unwrap();
    let reloaded = session.reload();
    assert!(reloaded.reopened);
    assert_eq!(reloaded.lost, None);
    assert_eq!(graph(&session).unwrap(), before);
}

/// A rebuild that fails and then one that works: nothing of the lost program is kept, and the session is the same as a new one.
#[test]
fn reload_of_a_lost_session_that_has_its_file_back_rebuilds() {
    let root = small("reload-lost");
    let mut session = open(&root);
    std::fs::remove_file(root.join("src/main.ts")).unwrap();
    assert_eq!(session.reload().lost, Some(OpenError::NoSuchFile));
    write(&root, "src/main.ts", MAIN);
    let reloaded = session.reload();
    assert!(reloaded.reopened);
    assert_eq!(reloaded.lost, None);
    assert_as_fresh_as_open(&session, &root);
}

// 14
#[test]
fn an_install_that_rewrites_the_lock_file_beside_node_modules_is_seen_by_refresh() {
    let root = with_a_package("lock");
    write(&root, "package-lock.json", "{}\n");
    let mut session = open(&root);
    assert_eq!(edges(&session), lines(&PKG_BEFORE));

    // what an install does: the package changes and the lock file is written
    write(&root, "node_modules/pkg/index.d.ts", PKG_EDITED);
    write(&root, "package-lock.json", "{ \"lockfileVersion\": 3 }\n");
    let refreshed = session.refresh();
    assert!(refreshed.reopened);
    assert_eq!(refreshed.lost, None);
    assert_eq!(edges(&session), lines(&PKG_AFTER));
    assert_as_fresh_as_open(&session, &root);
}
