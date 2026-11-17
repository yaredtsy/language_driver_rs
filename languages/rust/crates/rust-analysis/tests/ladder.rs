//! The ladder's last rungs (RS3 §6, D37) through the public API, on a machine that has cargo: a file with no manifest, a manifest nobody can read, and a
//! manifest only cargo refuses. Rung 3 with no cargo at all, and the calls that resolve on it, are in `sem/origin.rs` (they need a call walk).

use std::path::Path;

use rust_analysis::{LoadOptions, Project, Warning};

mod common;
use common::copy_of_fixture;

fn open(root: &Path, file: &str) -> Project {
    Project::open(root, Path::new(file), &LoadOptions::default())
        .expect("a load that cannot be done as asked is still a load")
}

/// Rung 4: no `Cargo.toml` between the file and the root. The file is the root of a crate, and `mod child;` finds `child.rs` beside it.
#[test]
fn a_file_with_no_manifest_is_a_crate_of_one_file_and_its_mod_children() {
    // the copy is under target/: `languages/rust/Cargo.toml` exists above, and the root is what stops the climb
    let copy = copy_of_fixture("lone", "ladder_lone");
    let project = open(&copy, "main.rs");

    assert_eq!(project.manifest(), "");
    assert_eq!(project.files(), ["child.rs", "main.rs"]);
    assert_eq!(project.warnings(), [Warning::NoManifest]);
    assert_eq!(project.warnings()[0].code(), "no_manifest");
    assert_eq!(project.skipped(), []);
    // nothing to watch but what the next `Cargo.toml` would be: the globs only
    assert!(
        project
            .environment_inputs()
            .iter()
            .all(|input| input.starts_with("**/"))
    );
}

/// Rung 4 again: a `Cargo.toml` that is not TOML. Cargo cannot read it, our reader cannot either, and the file stands alone with the reason said.
#[test]
fn a_manifest_nobody_can_read_leaves_the_file_alone_with_the_reason() {
    let copy = copy_of_fixture("broken_manifest", "ladder_broken");
    let project = open(&copy, "src/lib.rs");

    assert_eq!(
        project.manifest(),
        "Cargo.toml",
        "it was found, it is the reading that failed"
    );
    assert!(project.files().iter().any(|f| f == "src/lib.rs"));
    let [Warning::ManifestUnreadable { message }] = project.warnings() else {
        panic!("{:?}", project.warnings());
    };
    assert!(message.contains("Cargo.toml"), "{message}");
    assert!(!message.contains('\n'), "a warning is one line: {message}");
    assert_eq!(project.warnings()[0].code(), "manifest_unreadable");
}

/// Rung 3 with cargo present: a manifest that is valid TOML and that cargo refuses (a dependency with no source). Our reader reads it; the dependency has
/// no source to load, so it is counted. §6 names no warning for this case (a proposal is with the report), so none is given: the load says
/// `layout_skipped` and nothing else.
#[test]
fn a_manifest_only_cargo_refuses_is_read_by_the_reader_and_counted() {
    let copy = copy_of_fixture("cargo_fails", "ladder_cargo_fails");
    let project = open(&copy, "src/lib.rs");

    assert_eq!(project.manifest(), "Cargo.toml");
    assert_eq!(project.files(), ["src/lib.rs"]);
    assert_eq!(project.skipped(), [("layout_skipped", 1)]);
    // cargo is there, so this is not `NoToolchain`; and the reader's load has no `std` by design, so it is not `NoStdSource` either
    assert_eq!(project.warnings(), []);
    // the manifest that was read is watched, as always
    assert!(
        project
            .environment_inputs()
            .contains(&"**/Cargo.toml".to_owned())
    );
}
