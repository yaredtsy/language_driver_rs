//! `Project::open(root, file, opts)`: which workspace owns a file, and what is project code (RS3 §2).

use std::path::{Path, PathBuf};

use rust_analysis::{LoadOptions, OpenError, Project};

mod common;
use common::{copy_of_fixture, rs_files, snapshot};

/// This repo: it has no `Cargo.toml` of its own and holds five workspaces (the test below opens two: `languages/typescript/` is left out on purpose). Written with `..` on purpose, as a caller writes a path.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..")
}

/// The committed lockfiles must come out of a load as they went in.
fn locks(repo: &Path) -> Vec<Vec<u8>> {
    ["core", "languages/rust"]
        .map(|workspace| std::fs::read(repo.join(workspace).join("Cargo.lock")).unwrap())
        .to_vec()
}

#[test]
fn the_right_one_of_five_workspaces_is_chosen() {
    let repo = repo();
    let before = locks(&repo);
    // (the file as the caller writes it, the manifest found, the folders that hold project files). The Rust workspace has a path dependency on
    // `core/analyzer-core`, which is under the root: loaded as a local crate, so project code (§2's table), and its files are listed.
    // `languages/typescript/` is not opened: its `cargo metadata` needs tsrs's git repository, and Rust's CI must not fetch another language's engine.
    let cases: [(&str, &str, &[&str]); 2] = [
        (
            "core/analyzer-core/src/lib.rs",
            "core/analyzer-core/Cargo.toml",
            &["core/"],
        ),
        (
            "languages/rust/crates/rust-analysis/src/lib.rs",
            "languages/rust/crates/rust-analysis/Cargo.toml",
            &["languages/rust/", "core/"],
        ),
    ];
    for (file, manifest, folders) in cases {
        // the first file relative to the root, the other absolute: both are allowed
        let file_arg = if folders == ["core/"] {
            PathBuf::from(file)
        } else {
            repo.join(file)
        };
        let project = Project::open(&repo, &file_arg, &LoadOptions::default())
            .unwrap_or_else(|e| panic!("{file}: {e:?}"));
        assert_eq!(project.manifest(), manifest);
        let files = project.files();
        assert!(
            files.iter().any(|f| f == file),
            "{file} is not in {} files",
            files.len()
        );
        let stray = files
            .iter()
            .find(|f| !folders.iter().any(|folder| f.starts_with(folder)));
        assert!(
            stray.is_none(),
            "{file}: a file of another workspace is listed: {stray:?}"
        );
        if folders.len() > 1 {
            assert!(
                files.iter().any(|f| f == "core/analyzer-core/src/lib.rs"),
                "{file}: the path dependency's files are project code"
            );
        }
    }
    assert_eq!(
        locks(&repo),
        before,
        "a load changed a committed Cargo.lock"
    );
}

#[test]
fn a_file_in_a_member_finds_the_members_manifest_and_the_workspaces_files() {
    let core = repo().join("core");
    let lock = core.join("Cargo.lock");
    let before = std::fs::read(&lock).unwrap();

    let project = Project::open(
        &core,
        &core.join("analyzer-core/src/lib.rs"),
        &LoadOptions::default(),
    )
    .expect("core/ loads");
    assert_eq!(project.manifest(), "analyzer-core/Cargo.toml");

    // the member's manifest was found, and the load is the whole workspace: both members' files
    let mut expected = Vec::new();
    rs_files(&core, &core, &mut expected);
    expected.sort();
    assert!(
        expected.iter().any(|f| f.starts_with("analyzer-testkit/")),
        "the walk found only {expected:?}"
    );
    assert_eq!(project.files(), expected);
    assert_eq!(
        std::fs::read(&lock).unwrap(),
        before,
        "core/Cargo.lock changed"
    );
}

#[test]
fn a_workspace_root_above_the_root_is_loaded_and_is_not_project_code() {
    let copy = copy_of_fixture("above", "above");
    let before = snapshot(&copy);
    let member = copy.join("member");

    let project = Project::open(&member, &member.join("src/lib.rs"), &LoadOptions::default())
        .expect("the member loads");

    // the manifest is the member's own, below the root; the sibling's file (in the workspace, outside the root) is not listed
    assert_eq!(project.manifest(), "Cargo.toml");
    assert_eq!(project.files(), ["src/lib.rs"]);
    // the workspace above has no lock: the lock rule looks beside *its* manifest, so nothing is written there either
    assert_eq!(
        snapshot(&copy),
        before,
        "the load changed the workspace above the root"
    );

    // the same file with the root at the workspace: now the sibling is under the root, so it is listed. It was loaded both times; the root decides.
    let project = Project::open(
        &copy,
        Path::new("member/src/lib.rs"),
        &LoadOptions::default(),
    )
    .expect("the workspace loads");
    assert_eq!(project.manifest(), "member/Cargo.toml");
    assert_eq!(project.files(), ["member/src/lib.rs", "sibling/src/lib.rs"]);
    assert_eq!(snapshot(&copy), before);
}

#[test]
fn a_file_with_no_cargo_toml_up_to_the_root_is_an_error() {
    // under target/, so a Cargo.toml exists above the root (languages/rust/Cargo.toml): the root is what stops the climb
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no_manifest");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lone.rs"), "pub fn lone() {}\n").unwrap();

    let error = Project::open(&root, Path::new("src/lone.rs"), &LoadOptions::default())
        .err()
        .expect("no manifest");
    assert!(
        matches!(&error, OpenError::Load(message) if message.contains("no Cargo.toml")),
        "{error:?}"
    );
}

#[test]
fn a_file_that_is_not_there_or_not_under_the_root_is_an_error() {
    let core = repo().join("core");
    for file in [
        "analyzer-core/src/no_such_file.rs",
        "../Cargo.toml",
        "../languages/rust/crates/rust-analysis/src/lib.rs",
    ] {
        let error = Project::open(&core, Path::new(file), &LoadOptions::default())
            .err()
            .unwrap_or_else(|| panic!("{file} opened"));
        assert!(matches!(error, OpenError::Load(_)), "{file}: {error:?}");
    }
}
