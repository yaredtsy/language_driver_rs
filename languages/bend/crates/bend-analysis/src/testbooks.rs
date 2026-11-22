//! The books the in-crate tests run over: the fixtures and the scenarios, and (with `BEND_CLONE`) the 16 books of the sweep.

use std::fs;
use std::path::{Path, PathBuf};

use crate::book::Book;

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

/// The book of each demo of the clone `BEND_CLONE` names, rooted at the demo's folder, with the file it was opened at: its `main.bend`, its `walkers_test.bend` when it
/// has no `main.bend`, and its `PROOF.bend` when it has one.
pub(crate) fn demo_books() -> Vec<(String, Book)> {
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
    let mut out = Vec::new();
    for folder in &folders {
        for entry in ["main.bend", "walkers_test.bend", "PROOF.bend"] {
            if folder.join(entry).is_file() {
                let book = Book::open(folder, Path::new(entry)).expect("the book opens");
                out.push((format!("{}/{entry}", folder.display()), book));
            }
        }
    }
    assert!(out.len() >= 16, "the 16 books: {}", out.len());
    out
}
