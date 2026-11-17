//! What the unit tests of `sem` share: the policy fixture, loaded, and the outline of one of its files read from disk.

use std::path::{Path, PathBuf};

use analyzer_core::edit::{MarkerStyle, Placement};
use analyzer_core::outline::Outline;
use rust_syntax::Edition;

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
