//! The proof of V10b and V10d, run: a request worked out the old way and the new way gives the same graph, byte for byte (edges, contexts, digests, every counter),
//! and the same canonical keys, from every def of every fixture, every scenario and (with `BEND_CLONE`) the 16 books of the sweep.

use std::fs;
use std::path::{Path, PathBuf};

use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::Entry;
use analyzer_core::solve::{Budget, Cancel};

use crate::book::Book;
use crate::calls::{Options, Precision, graph_within};
use crate::eval::{Ways, with_ways};

/// Every folder under `dir` that has a `main.bend`: a book is opened at each, rooted there.
pub(crate) fn books(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .expect("a folder")
        .map(|e| e.expect("an entry").path())
        .collect();
    entries.sort();
    if entries.iter().any(|p| p.ends_with("main.bend")) {
        out.push(dir.to_path_buf());
    }
    for path in entries.into_iter().filter(|p| p.is_dir()) {
        books(&path, out);
    }
}

/// The books of the fixtures and the scenarios.
pub(crate) fn fixture_books() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut folders = Vec::new();
    books(&manifest.join("tests/fixtures"), &mut folders);
    books(&manifest.join("../../../../scenarios"), &mut folders);
    assert!(
        folders.len() > 50,
        "the books were found: {}",
        folders.len()
    );
    folders
}

/// The graph and the canonical keys of every def of `book` as an entry, in `ways`, as text: the graph's serialization, then its keys sorted by digest.
fn everything(book: &Book, ways: Ways) -> Vec<String> {
    let options = Options::new(MarkerStyle::above("ID")).with_precision(Precision::Precise);
    let mut out = Vec::new();
    for decl in book.decls() {
        let (Some(file), Some(def)) = (book.file(decl.id.file()), book.def(decl.id)) else {
            continue;
        };
        let occurrence = bend_syntax::occurrences(&file.outline)[decl.id.def()];
        let entry = Entry::Def {
            qualname: &def.qualname,
            occurrence: occurrence as u16,
        };
        let asked = with_ways(ways, || {
            graph_within(
                book,
                &file.relative,
                &entry,
                &options,
                &Budget::default(),
                &Cancel::new(),
            )
        });
        out.push(match asked {
            None => format!("{} {}: none", file.relative, def.qualname),
            Some((graph, keys)) => {
                let mut keys: Vec<_> = keys.into_iter().collect();
                keys.sort_by_key(|(digest, _)| *digest);
                format!(
                    "{} {}: {}\n{}",
                    file.relative,
                    def.qualname,
                    serde_json::to_string(&graph).expect("a graph is data"),
                    serde_json::to_string(&keys).expect("keys are data"),
                )
            }
        });
    }
    out
}

/// The two ways over one book: the first difference as a failure. Returns how many entries were compared.
fn same_both_ways(book: &Book, whose: &str, old: Ways, new: Ways) -> usize {
    let (before, after) = (everything(book, old), everything(book, new));
    assert_eq!(before.len(), after.len(), "{whose}");
    for (a, b) in before.iter().zip(&after) {
        assert_eq!(a, b, "{whose}: the old and the new way differ");
    }
    before.len()
}

/// The way before V10b: a body is read for its sites again by the solver's walk.
pub(crate) const OLD: Ways = Ways {
    keep: false,
    nest: false,
};
/// The way after V10b and before V10d: the sites are kept, a context met for the first time is queued.
pub(crate) const KEPT: Ways = Ways {
    keep: true,
    nest: false,
};
/// The way now.
pub(crate) const NEW: Ways = Ways {
    keep: true,
    nest: true,
};

/// V10b: the sites kept from the evaluation that gave the value are the sites the walk would have read.
#[test]
fn the_sites_kept_are_the_sites_read_on_every_fixture_and_scenario() {
    let compared: usize = fixture_books()
        .iter()
        .map(|folder| {
            let book = Book::open(folder, Path::new("main.bend")).expect("the book opens");
            same_both_ways(&book, &folder.display().to_string(), OLD, NEW)
        })
        .sum();
    assert!(compared > 250, "entries were compared: {compared}");
}

/// V10d: a context met for the first time is evaluated at once, and the graph is the one it was when it was queued.
#[test]
fn a_context_evaluated_at_once_gives_the_graph_it_gave_queued_on_every_fixture_and_scenario() {
    let compared: usize = fixture_books()
        .iter()
        .map(|folder| {
            let book = Book::open(folder, Path::new("main.bend")).expect("the book opens");
            same_both_ways(&book, &folder.display().to_string(), KEPT, NEW)
        })
        .sum();
    assert!(compared > 250, "entries were compared: {compared}");
}

/// The same over the 16 books of the sweep (each demo's `main.bend`, `walkers_test.bend` for the netcode, and `PROOF.bend` where there is one):
/// `BEND_CLONE=../language_clone/bend cargo test -p bend-analysis --release --lib equal -- --ignored`.
#[test]
#[ignore = "needs BEND_CLONE"]
fn the_sites_kept_are_the_sites_read_on_the_demos() {
    let given = std::env::var_os("BEND_CLONE").expect("BEND_CLONE names the bend clone");
    let demos = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../..")
        .join(given)
        .join("demos");
    let mut folders: Vec<PathBuf> = fs::read_dir(&demos)
        .expect("the demos")
        .map(|e| e.expect("a demo").path())
        .collect();
    folders.sort();
    let (mut books, mut compared) = (0, 0);
    for folder in &folders {
        let entries = ["main.bend", "walkers_test.bend", "PROOF.bend"]
            .into_iter()
            .filter(|f| folder.join(f).is_file());
        for entry in entries {
            let book = Book::open(folder, Path::new(entry)).expect("the book opens");
            let whose = format!("{}/{entry}", folder.display());
            compared += same_both_ways(&book, &whose, OLD, NEW);
            same_both_ways(&book, &whose, KEPT, NEW);
            books += 1;
        }
    }
    assert!(books >= 16, "the 16 books: {books}");
    println!("{books} books, {compared} entries, the same both ways");
}
