//! `Project::open` on a real workspace: this repo's `core/`, a virtual manifest with two members.

use std::path::{Path, PathBuf};

use rust_analysis::Project;

/// Every `.rs` file under `dir`, relative to `base` with `/`, found by walking the disk (not through the engine), skipping `target/`.
fn rs_files(base: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                rs_files(base, &path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            let parts: Vec<_> = path
                .strip_prefix(base)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_str().unwrap())
                .collect();
            out.push(parts.join("/"));
        }
    }
}

#[test]
fn opens_core_and_lists_its_rs_files() {
    // `..` on purpose: a manifest path as a caller writes it, not as the engine spells it
    let core: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../core");
    let project = Project::open(&core.join("Cargo.toml")).expect("core/ loads");

    let mut expected = Vec::new();
    rs_files(&core, &core, &mut expected);
    expected.sort();
    assert!(expected.len() > 5, "the walk found only {expected:?}");
    assert_eq!(project.files(), expected);
}
