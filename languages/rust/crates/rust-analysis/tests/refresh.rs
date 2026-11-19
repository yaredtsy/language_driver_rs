//! `Session::refresh`, the fast path (RS5 §7 step 3): a file saved, created or deleted on disk is followed by the next `refresh`, and the graph with it. Each
//! test has a copy of the `refresh` fixture and a load of its own (`common::one_load`). TypeScript's cases are
//! `languages/typescript/crates/typescript-analysis/tests/session_refresh.rs`; what needs a counter or a hook inside the engine module (nothing changed, a file
//! saved during a refresh, no folder above the root) is a unit test of `sem/refresh.rs`.
//!
//! The fixture: `entry` (`src/lib.rs`) calls `callee::callee`, which calls `first`; `callee.rs` also has `second`. A file is rewritten with a different
//! length, because a stamp is the length and the modified time and a clock is only so fine.

use std::path::{Path, PathBuf};

use analyzer_testkit::graph::check_invariants;
use rust_analysis::{
    Budget, Cancel, Entry, LoadOptions, MarkerStyle, Options, RenderOptions, Session, render,
    to_text,
};

mod common;
use common::{copy_of_fixture, one_load};

fn write(root: &Path, file: &str, text: &str) {
    common::write(&root.join(file), text);
}

fn open(name: &str) -> (PathBuf, Session) {
    let root = copy_of_fixture("refresh", name);
    let session = Session::open(
        &root,
        Path::new("src/lib.rs"),
        &LoadOptions::default(),
        Options::new(MarkerStyle::docstring("ID")),
    )
    .expect("the fixture loads");
    (root, session)
}

/// The tree of `entry`, as text.
fn entry_tree(session: &Session) -> String {
    let entry = Entry::Def {
        qualname: "entry",
        occurrence: 0,
    };
    let graph = session
        .graph("src/lib.rs", &entry, &Budget::default(), &Cancel::new())
        .expect("`entry` is there");
    check_invariants(&graph);
    to_text(&render(
        &graph,
        &graph.entries[0],
        &RenderOptions::default(),
    ))
}

fn files(session: &Session) -> Vec<String> {
    session.project().files()
}

const LIB: &str = "mod callee;\nmod nested;\n\npub fn entry() {\n    callee::callee();\n}\n";

/// `callee` calls `second` where the fixture has `first`.
const CALLEE_SECOND: &str =
    "pub fn callee() {\n    second();\n}\n\npub fn first() {}\n\npub fn second() {}\n";

/// The callee's file is rewritten: the graph follows, after `refresh` and not before. (The pin of "not before": a session shows the disk as it was when
/// it last looked, and the driver's call to `refresh` is what moves it.)
#[test]
fn a_rewritten_callee_file_is_followed_by_refresh_and_not_without_it() {
    let _load = one_load();
    let (root, mut session) = open("refresh_rewritten");
    let before = entry_tree(&session);
    assert!(
        before.contains("first") && !before.contains("second"),
        "{before}"
    );

    write(&root, "src/callee.rs", CALLEE_SECOND);
    assert_eq!(entry_tree(&session), before, "no refresh: the old graph");

    let refreshed = session.refresh();
    assert!(!refreshed.reopened, "a saved file is no reload");
    assert_eq!(refreshed.lost, None);
    let after = entry_tree(&session);
    assert!(
        after.contains("second") && !after.contains("first"),
        "{after}"
    );
}

/// A file a `mod` names is created, with the `mod` line saved in the same refresh: the new file is part of the project, and the graph reaches it.
#[test]
fn a_file_a_mod_names_is_created_with_its_mod_line() {
    let _load = one_load();
    let (root, mut session) = open("refresh_created");
    assert!(!files(&session).contains(&"src/extra.rs".to_owned()));

    write(
        &root,
        "src/extra.rs",
        "pub fn extra() {\n    crate::callee::second();\n}\n",
    );
    write(
        &root,
        "src/lib.rs",
        &format!("mod extra;\n{LIB}").replace(
            "callee::callee();",
            "callee::callee();\n    extra::extra();",
        ),
    );
    session.refresh();

    assert!(
        files(&session).contains(&"src/extra.rs".to_owned()),
        "{:?}",
        files(&session)
    );
    let tree = entry_tree(&session);
    assert!(tree.contains("extra") && tree.contains("second"), "{tree}");
}

/// The file is created in one refresh and the `mod` line saved in a later one: the file is in the `Vfs` but no module, then it is a module.
#[test]
fn a_file_created_before_its_mod_line_joins_when_the_line_is_saved() {
    let _load = one_load();
    let (root, mut session) = open("refresh_mod_later");

    write(&root, "src/extra.rs", "pub fn extra() {}\n");
    session.refresh();
    assert!(
        !files(&session).contains(&"src/extra.rs".to_owned()),
        "no `mod` names it yet"
    );

    write(&root, "src/lib.rs", &format!("mod extra;\n{LIB}"));
    session.refresh();
    assert!(files(&session).contains(&"src/extra.rs".to_owned()));
}

/// A file in a folder that did not exist: the folder is new, so it is found by listing its parent and listed in turn.
#[test]
fn a_file_in_a_new_folder_is_found() {
    let _load = one_load();
    let (root, mut session) = open("refresh_new_folder");

    write(&root, "src/deeper/mod.rs", "pub mod leaf;\n");
    write(&root, "src/deeper/leaf.rs", "pub fn leaf() {}\n");
    write(&root, "src/lib.rs", &format!("mod deeper;\n{LIB}"));
    session.refresh();

    let files = files(&session);
    assert!(files.contains(&"src/deeper/mod.rs".to_owned()), "{files:?}");
    assert!(
        files.contains(&"src/deeper/leaf.rs".to_owned()),
        "{files:?}"
    );
}

/// The callee's file is deleted: the call has nowhere to go. And a folder is removed with its files.
#[test]
fn a_callee_file_that_is_deleted_is_gone_from_the_graph() {
    let _load = one_load();
    let (root, mut session) = open("refresh_deleted");
    assert!(files(&session).contains(&"src/callee.rs".to_owned()));
    assert!(files(&session).contains(&"src/nested/mod.rs".to_owned()));
    let before = entry_tree(&session);

    std::fs::remove_file(root.join("src/callee.rs")).unwrap();
    std::fs::remove_dir_all(root.join("src/nested")).unwrap();
    session.refresh();

    assert!(!files(&session).contains(&"src/callee.rs".to_owned()));
    assert!(!files(&session).contains(&"src/nested/mod.rs".to_owned()));
    let after = entry_tree(&session);
    assert_ne!(after, before);
    assert!(!after.contains("first"), "{after}");

    // and written back: it is there again
    write(&root, "src/callee.rs", CALLEE_SECOND);
    session.refresh();
    assert!(files(&session).contains(&"src/callee.rs".to_owned()));
    assert!(entry_tree(&session).contains("second"));
}

/// A session moves to a worker thread (RS5 §2), so it must be `Send`: checked when this compiles.
#[test]
fn a_session_is_send() {
    fn is_send<T: Send>() {}
    is_send::<Session>();
}
