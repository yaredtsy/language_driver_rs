//! SLOW (as far as Bend has a slow side): the book, and from BD4 the call sites and the call graph. The `sem` folder is the only code that imports the
//! engine (`bend-parser`, the parser this repo writes for Bend, BD-D1).
//!
//! A [`Book`] is the file you asked about and what it imports, with the tables that say what a name means. Nothing in it needs a type, so opening one
//! is milliseconds. Text in, data out: opening reads files and writes none, nothing is fetched, and `~/.bend` is never read.
//!
//! ```no_run
//! use std::path::Path;
//! use bend_analysis::{Book, Resolved};
//!
//! let book = Book::open(Path::new("demos/proof_insertion_sort"), Path::new("PROOF.bend")).unwrap();
//! let main = book.files().last().unwrap(); // the entry file is loaded last
//! assert_eq!(main.namespace, "");
//! // `Spec.LE` in a file that wrote `import ./SPEC.bend as Spec` is the def `SPEC:LE`
//! assert!(matches!(book.resolve(book.entry(), "Spec.LE"), Resolved::Project(_)));
//! ```

mod base_names;
mod book;
mod paths;
mod sem;

pub use analyzer_core::graph::Stats;
pub use analyzer_core::outline::{Def, DefKind, Outline};
pub use book::{Alias, AliasTarget, Book, DeclEntry, DeclId, FileEntry, FileIndex, Resolved};
