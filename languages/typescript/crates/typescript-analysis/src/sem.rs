//! The only file that imports the engine (`tsrs_*`). Everything else asks `Sem` and gets plain data.
//!
//! TS3 grows this into the loaded program (`docs/03-typescript/05-ts3-project.md` §5). For now it only names the four heavy
//! crates, so the build proves they resolve and link.

// `as _` names a crate without using an item from it: the dependency is checked and linked, and later steps replace these lines
// with the real imports.
use tsrs_checker as _;
use tsrs_compiler as _;
use tsrs_tsoptions as _;
use tsrs_vfs as _;
