//! What the unit tests of `sem` share: the policy fixture, loaded, and the outline of one of its files read from disk.

use std::path::{Path, PathBuf};

use analyzer_core::edit::{MarkerStyle, Placement};
use analyzer_core::outline::Outline;
use rust_syntax::Edition;

use super::Sem;
use super::common;
use super::load::{Loaded, load};
use crate::LoadOptions;

pub(crate) const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

/// A fresh copy of `fixtures/policy/` under `target/`, loaded as cargo's workspace of one package. Its root is the copy.
pub(crate) fn policy(name: &str) -> (Loaded, PathBuf) {
    let copy = crate::paths::absolute(&common::copy_of_fixture("policy", name)).unwrap();
    let manifest = copy.join("Cargo.toml");
    let loaded = load(&copy, Some(&manifest), &manifest, &LoadOptions::default())
        .expect("the policy fixture loads");
    (loaded, copy)
}

/// The text of a file on disk, as it is: a byte-order mark stays, and the engine's offsets and the outline's count it.
pub(crate) fn disk_text(root: &Path, file: &str) -> String {
    std::fs::read_to_string(root.join(file)).unwrap()
}

/// `rust-syntax`'s outline of a file's text on disk, in the edition of the fixture.
pub(crate) fn disk_outline(root: &Path, file: &str) -> Outline {
    rust_syntax::outline(&disk_text(root, file), Edition::E2024, STYLE)
}

/// One module's site lists, as a reviewable text: every def that can be an entry, with its sites in order. A site is its kind, its span relative to the
/// def's start, the text there, and the closure it is in. What is lost to an expansion the engine could not make is printed too.
pub(crate) fn render_sites(sem: &Sem<'_>, root: &Path, file: &str) -> String {
    let text = disk_text(root, file);
    let outline = disk_outline(root, file);
    let mut out = String::new();
    for (def, occurrence) in outline.defs.iter().zip(rust_syntax::occurrences(&outline)) {
        let Some(func) = sem.entry(file, &def.qualname, occurrence) else {
            continue;
        };
        out.push_str(&def.qualname);
        if occurrence > 0 {
            out.push_str(&format!("#{occurrence}"));
        }
        out.push('\n');
        let sites = sem.sites(func);
        for site in &sites.list {
            let at = site.span();
            let from = (def.range.start + at.start) as usize;
            let to = (def.range.start + at.end) as usize;
            out.push_str(&format!(
                "  {:?} {}..{} {:?}",
                site.kind(),
                at.start,
                at.end,
                &text[from..to]
            ));
            if let Some(closure) = site.closure() {
                out.push_str(&format!(" closure {closure}"));
            }
            out.push('\n');
        }
        if sites.macro_not_expanded > 0 {
            out.push_str(&format!(
                "  macro_not_expanded {}\n",
                sites.macro_not_expanded
            ));
        }
    }
    out
}
