//! `Session::refresh` (BD5, D41, D46): a file the book read, saved, created or deleted, is followed by the next `refresh` and the tree with it; a path an import
//! asked for and did not find is watched, so a file created there is seen; the entry gone is `lost`, reported by every `refresh` while it lasts, and the session
//! recovers by itself when the disk comes right; a file that does not parse is not lost. How many times a book is built is counted in `session.rs`'s unit test.
//!
//! A file is rewritten with a different length where it can be, because a stamp is the length and the modified time and a clock is only so fine; the one test
//! that rewrites to the same length moves the modified time by hand, and the one that does not is the known limit, written down.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use analyzer_testkit::graph::check_invariants;
use bend_analysis::{
    Budget, Cancel, Entry, OpenError, Refreshed, RenderOptions, Session, render, to_text,
};

mod common;
use common::{Program, default_options};

/// What a `Refreshed` says: `Refreshed` is `#[non_exhaustive]`, so a test reads its fields and does not build one.
fn said(refreshed: &Refreshed) -> (bool, Option<OpenError>) {
    (refreshed.reopened, refreshed.lost.clone())
}

fn write(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().expect("a folder")).unwrap();
    fs::write(path, text).unwrap();
}

fn open_in(program: &Program, entry: &str) -> Session {
    Session::open(&program.root, Path::new(entry), default_options()).expect("the program opens")
}

fn open(program: &Program) -> Session {
    open_in(program, "main.bend")
}

/// What `main` of `file` draws, as text, and the counters of the graph: `None` when the session has no such def.
fn tree_in(session: &Session, file: &str) -> Option<String> {
    let entry = Entry::Def {
        qualname: "main",
        occurrence: 0,
    };
    let graph = session.graph(file, &entry, &Budget::default(), &Cancel::new())?;
    check_invariants(&graph);
    let mut text = to_text(&render(
        &graph,
        &graph.entries[0],
        &RenderOptions::default(),
    ));
    for (key, count) in &graph.stats.language {
        text.push_str(&format!("{key}={count}\n"));
    }
    Some(text)
}

fn tree(session: &Session) -> String {
    tree_in(session, "main.bend").expect("main is there")
}

const MAIN: &str = "import ./lib.bend as L\n\ndef main() -> U32:\n  L.first(1)\n";
const LIB: &str = "def first(x: U32) -> U32:\n  x\n\ndef second(x: U32) -> U32:\n  x\n";
/// `MAIN` calling `second`: one character longer.
const MAIN_SECOND: &str = "import ./lib.bend as L\n\ndef main() -> U32:\n  L.second(1)\n";

fn pair() -> Program {
    Program::new(&[("main.bend", MAIN), ("lib.bend", LIB)])
}

/// The modified time of `file` moved by `by`, so that a file rewritten to the same length is still a change to a stamp.
fn touch(root: &Path, file: &str, by: Duration) {
    let path = root.join(file);
    let at = fs::metadata(&path).unwrap().modified().unwrap() + by;
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

#[test]
fn a_changed_file_is_seen_by_the_next_refresh_and_not_before() {
    let program = pair();
    let mut session = open(&program);
    let before = tree(&session);
    assert!(
        before.contains("first") && !before.contains("second"),
        "{before}"
    );

    write(&program.root, "main.bend", MAIN_SECOND);
    assert_eq!(tree(&session), before, "no refresh: the old tree");

    let refreshed = session.refresh();
    assert_eq!(said(&refreshed), (true, None));
    let after = tree(&session);
    assert!(
        after.contains("second") && !after.contains("first"),
        "{after}"
    );
}

#[test]
fn an_imported_file_that_changed_is_seen() {
    let program = pair();
    let mut session = open(&program);
    // `second` now calls `first`: a def of `lib.bend` changes, and `main` only reaches it
    write(&program.root, "main.bend", MAIN_SECOND);
    session.refresh();
    let before = tree(&session);
    assert!(!before.contains("first"), "{before}");

    write(
        &program.root,
        "lib.bend",
        "def first(x: U32) -> U32:\n  x\n\ndef second(x: U32) -> U32:\n  first(x)\n",
    );
    assert!(session.refresh().reopened);
    let after = tree(&session);
    assert!(
        after.contains("second") && after.contains("first"),
        "{after}"
    );
}

#[test]
fn nothing_changed_nothing_is_reopened_and_the_tree_is_the_same() {
    let program = pair();
    let mut session = open(&program);
    let before = tree(&session);
    for _ in 0..3 {
        assert_eq!(said(&session.refresh()), (false, None));
    }
    assert_eq!(tree(&session), before);
}

/// A path an import asked for and did not find is watched: the file created where it looked is seen. The unresolved import is counted until then.
#[test]
fn a_file_created_where_an_import_looked_is_seen() {
    let program = Program::new(&[("main.bend", MAIN)]);
    let mut session = open(&program);
    let before = tree(&session);
    assert!(before.contains("import_missing=1"), "{before}");
    assert!(before.contains("unknown_name=1"), "{before}");
    assert!(!before.contains("first\n"), "{before}");

    write(&program.root, "lib.bend", LIB);
    assert_eq!(tree(&session), before, "no refresh: the old tree");
    assert!(session.refresh().reopened);
    let after = tree(&session);
    assert!(!after.contains("import_missing"), "{after}");
    assert!(after.contains("first"), "{after}");
    assert!(!session.refresh().reopened, "and now nothing changed");
}

/// The same, where the folder the import asks through is itself not there: it is created with the file in it.
#[test]
fn a_file_created_in_a_folder_that_was_not_there_is_seen() {
    let program = Program::new(&[(
        "main.bend",
        "import ./deep/er/lib.bend as L\n\ndef main() -> U32:\n  L.first(1)\n",
    )]);
    let mut session = open(&program);
    assert!(tree(&session).contains("import_missing=1"));
    write(&program.root, "deep/er/lib.bend", LIB);
    assert!(session.refresh().reopened);
    let after = tree(&session);
    assert!(
        !after.contains("import_missing") && after.contains("first"),
        "{after}"
    );
}

/// A deleted imported file is a missing import, counted: not a lost session, and the entry still has its tree.
#[test]
fn a_deleted_imported_file_is_a_missing_import_and_not_lost() {
    let program = pair();
    let mut session = open(&program);
    assert!(!tree(&session).contains("import_missing"));

    fs::remove_file(program.root.join("lib.bend")).unwrap();
    let refreshed = session.refresh();
    assert!(refreshed.reopened);
    assert_eq!(refreshed.lost, None);
    assert_eq!(session.lost(), None);
    let after = tree(&session);
    assert!(after.contains("import_missing=1"), "{after}");
    assert!(after.contains("main"), "{after}");

    // and put back, it is found again
    write(&program.root, "lib.bend", LIB);
    assert!(session.refresh().reopened);
    assert!(!tree(&session).contains("import_missing"));
}

#[test]
fn the_entry_deleted_is_lost_for_as_long_as_it_lasts_and_found_again_when_it_comes_back() {
    let program = pair();
    let mut session = open(&program);
    let before = tree(&session);

    fs::remove_file(program.root.join("main.bend")).unwrap();
    let gone = session.refresh();
    assert_eq!(gone.lost, Some(OpenError::NoSuchFile));
    assert!(!gone.reopened);
    assert_eq!(session.lost(), Some(&OpenError::NoSuchFile));
    assert!(session.book().is_none());
    assert_eq!(
        tree_in(&session, "main.bend"),
        None,
        "a lost session answers None"
    );

    // reported by every refresh, not only the first
    for _ in 0..3 {
        assert_eq!(session.refresh().lost, Some(OpenError::NoSuchFile));
    }
    assert_eq!(session.reload().lost, Some(OpenError::NoSuchFile));

    write(&program.root, "main.bend", MAIN);
    let back = session.refresh();
    assert_eq!(said(&back), (true, None));
    assert_eq!(session.lost(), None);
    assert_eq!(tree(&session), before);
}

/// The entry's folder gone, not only the file: the root is the folder above it.
#[test]
fn the_entry_s_folder_deleted_is_lost_and_recovers() {
    let program = Program::new(&[("app/main.bend", MAIN), ("app/lib.bend", LIB)]);
    let mut session =
        Session::open(&program.root, Path::new("app/main.bend"), default_options()).unwrap();
    let main = |s: &Session| tree_in(s, "app/main.bend");
    let before = main(&session).expect("main is there");

    fs::remove_dir_all(program.root.join("app")).unwrap();
    assert_eq!(session.refresh().lost, Some(OpenError::NoSuchFile));
    assert_eq!(main(&session), None);

    write(&program.root, "app/main.bend", MAIN);
    write(&program.root, "app/lib.bend", LIB);
    let back = session.refresh();
    assert_eq!(said(&back), (true, None));
    assert_eq!(main(&session), Some(before));
}

/// The root itself deleted and made again.
#[test]
fn the_root_deleted_is_lost_and_recovers() {
    let program = pair();
    let mut session = open(&program);
    fs::remove_dir_all(&program.root).unwrap();
    assert_eq!(session.refresh().lost, Some(OpenError::NoSuchFile));
    write(&program.root, "main.bend", MAIN);
    write(&program.root, "lib.bend", LIB);
    assert_eq!(session.refresh().lost, None);
    assert!(tree(&session).contains("first"));
}

/// Never fail: a file that does not parse is in the book with what the parser got. The session is not lost, and the entry's own defs still have a tree.
#[test]
fn a_file_that_does_not_parse_is_not_lost() {
    let program = pair();
    let mut session = open(&program);
    write(
        &program.root,
        "lib.bend",
        "def first(x: U32 -> U32:\n  ((\n",
    );
    let refreshed = session.refresh();
    assert!(refreshed.reopened);
    assert_eq!(refreshed.lost, None);
    assert!(tree_in(&session, "main.bend").is_some());

    // even the entry
    write(&program.root, "main.bend", "def main( -> :\n  @@@\n");
    let refreshed = session.refresh();
    assert_eq!(
        refreshed.lost, None,
        "a syntax error is no reason to lose the book"
    );
    assert!(session.book().is_some());

    write(&program.root, "main.bend", MAIN);
    write(&program.root, "lib.bend", LIB);
    assert!(session.refresh().reopened);
    assert!(tree(&session).contains("first"));
}

/// A file imported from beside or above the root is read all the same (D33), so it is stamped as well.
#[test]
fn a_file_outside_the_root_that_the_entry_imports_is_followed() {
    let program = Program::new(&[
        (
            "app/main.bend",
            "import ../shared.bend as S\n\ndef main() -> U32:\n  S.first(1)\n",
        ),
        ("shared.bend", LIB),
    ]);
    let mut session = Session::open(
        &program.root.join("app"),
        Path::new("main.bend"),
        default_options(),
    )
    .unwrap();
    let before = tree(&session);
    assert!(before.contains("file_outside_root=1"), "{before}");

    write(
        &program.root,
        "shared.bend",
        "def first(x: U32) -> U32:\n  second(x)\n\ndef second(x: U32) -> U32:\n  x\n",
    );
    assert!(session.refresh().reopened);
}

/// What a stamp is: the length and the modified time. Same length and the clock's tick unmoved is not seen (the known limit, D41), the modified time moved
/// by hand is, and `reload` sees the first.
#[test]
fn a_stamp_is_the_length_and_the_modified_time() {
    // `first` and `third` are of one length, so `main` can be rewritten to call the other without its length moving
    let lib = "def first(x: U32) -> U32:\n  x\n\ndef third(x: U32) -> U32:\n  x\n";
    let program = Program::new(&[("main.bend", MAIN), ("lib.bend", lib)]);
    let mut session = open(&program);
    let path: PathBuf = program.root.join("main.bend");
    let original: SystemTime = fs::metadata(&path).unwrap().modified().unwrap();
    let same_length = MAIN.replace("first", "third");
    assert_eq!(same_length.len(), MAIN.len());

    // rewritten to the same length, then the modified time put back: the stamp did not move
    write(&program.root, "main.bend", &same_length);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(original)
        .unwrap();
    assert!(
        !session.refresh().reopened,
        "the known limit: nothing the stamp can see"
    );
    assert!(tree(&session).contains("first"));
    assert!(
        session.reload().reopened,
        "reload builds whatever the stamps say"
    );
    let after = tree(&session);
    assert!(
        after.contains("third") && !after.contains("first"),
        "{after}"
    );

    // the modified time moved: a change, with the same length
    write(&program.root, "main.bend", MAIN);
    touch(&program.root, "main.bend", Duration::from_secs(5));
    assert!(session.refresh().reopened);
    assert!(tree(&session).contains("first"));
}

/// A request in the middle of nothing: `graph` does not look at the disk (D41 (c)), and a refresh that built the book again leaves the old graphs as they were.
#[test]
fn graph_does_not_refresh() {
    let program = pair();
    let mut session = open(&program);
    let held = tree(&session);
    write(&program.root, "main.bend", MAIN_SECOND);
    for _ in 0..3 {
        assert_eq!(tree(&session), held);
    }
    session.refresh();
    assert_ne!(tree(&session), held);
}
