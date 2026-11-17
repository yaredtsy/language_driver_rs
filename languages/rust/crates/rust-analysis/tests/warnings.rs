//! `Project::warnings` (RS3 §6): what a load could not do as asked, and said so. The rows a test can make on any machine; the ones that need a
//! toolchain this machine may not have are in `toolchains.rs`.

use std::path::Path;
use std::process::Command;

use rust_analysis::{LoadOptions, OpenError, Project, Warning};

mod common;
use common::copy_of_fixture;

/// The folder name of the sysroot the project's toolchain has, when its standard library source is missing; `None` when the source is there.
/// Read the way a user would: `rustc --print sysroot` in the folder, and the source where rustup puts it (or `RUST_SRC_PATH`).
fn toolchain_without_rust_src(dir: &Path) -> Option<String> {
    let out = Command::new("rustc")
        .args(["--print", "sysroot"])
        .current_dir(dir)
        .output()
        .unwrap();
    let sysroot = std::path::PathBuf::from(String::from_utf8(out.stdout).unwrap().trim());
    let from_env =
        std::env::var("RUST_SRC_PATH").is_ok_and(|path| Path::new(&path).join("core").exists());
    let present = from_env || sysroot.join("lib/rustlib/src/rust/library").exists();
    (!present).then(|| sysroot.file_name().unwrap().to_str().unwrap().to_owned())
}

fn codes(project: &Project) -> Vec<&'static str> {
    project.warnings().iter().map(Warning::code).collect()
}

/// A complete toolchain says nothing; one without `rust-src` says `NoStdSource` and names its folder. Which of the two this machine is, the test
/// reads from the disk, so it checks the right thing on both (CI's `rust-toolchain.toml` has no `rust-src`; a developer's usually has).
#[test]
fn a_missing_rust_src_is_a_warning_and_a_complete_toolchain_is_silent() {
    let copy = copy_of_fixture("lock", "warnings_lock");
    let app = copy.join("app");
    let project =
        Project::open(&app, &app.join("src/lib.rs"), &LoadOptions::default()).expect("app loads");
    match toolchain_without_rust_src(&app) {
        None => assert_eq!(project.warnings(), []),
        Some(toolchain) => {
            assert_eq!(project.warnings(), [Warning::NoStdSource { toolchain }]);
            assert_eq!(project.warnings()[0].code(), "no_std_source");
        }
    }
}

/// `.cargo/config.toml` makes cargo offline and `rs3-no-such-crate` is in no cache: `cargo metadata` fails and the engine goes on without
/// dependencies. The load succeeds, and says so.
#[test]
fn an_unfetchable_dependency_is_a_warning_and_not_an_error() {
    let copy = copy_of_fixture("offline", "warnings_offline");
    let project = Project::open(&copy, &copy.join("app/src/lib.rs"), &LoadOptions::default())
        .expect("a load without dependencies is still a load");

    let mut expected = vec!["dependencies_missing"];
    if toolchain_without_rust_src(&copy).is_some() {
        expected.push("no_std_source");
        expected.sort_unstable();
    }
    assert_eq!(codes(&project), expected);
    let Some(Warning::DependenciesMissing { message }) = project
        .warnings()
        .iter()
        .find(|w| w.code() == "dependencies_missing")
    else {
        panic!("{:?}", project.warnings());
    };
    assert!(
        message.contains("rs3-no-such-crate"),
        "cargo's message is kept: {message}"
    );
    assert!(project.files().iter().any(|f| f == "app/src/lib.rs"));
}

#[test]
fn a_file_that_is_not_there_is_the_one_error() {
    let copy = copy_of_fixture("lock", "warnings_missing");
    let error = Project::open(&copy, Path::new("app/src/nope.rs"), &LoadOptions::default())
        .err()
        .expect("no such file");
    assert_eq!(error, OpenError::NotUnderRoot);
}

/// A `build.rs` that panics fails the build run. The load succeeds, and says the build failed, with cargo's message.
#[test]
fn a_failed_build_is_a_warning_and_not_an_error() {
    let copy = copy_of_fixture("broken", "warnings_broken");
    let file = copy.join("app/src/lib.rs");
    let project = Project::open(&copy, &file, &options_with_build_scripts())
        .expect("a failed build still loads");

    let mut expected = vec!["build_scripts_failed"];
    if toolchain_without_rust_src(&copy).is_some() {
        expected.push("no_std_source");
        expected.sort_unstable();
    }
    assert_eq!(codes(&project), expected);
    let Some(Warning::BuildScriptsFailed { message }) = project
        .warnings()
        .iter()
        .find(|w| w.code() == "build_scripts_failed")
    else {
        panic!("{:?}", project.warnings());
    };
    assert!(!message.is_empty());

    // not asked for: not run, not failed, nothing to say
    let quiet = Project::open(&copy, &file, &LoadOptions::default()).expect("loads");
    assert!(
        quiet
            .warnings()
            .iter()
            .all(|w| w.code() != "build_scripts_failed")
    );
}

/// `LoadOptions` is `#[non_exhaustive]`: no struct literal outside the crate.
#[allow(clippy::field_reassign_with_default)]
fn options_with_build_scripts() -> LoadOptions {
    let mut opts = LoadOptions::default();
    opts.build_scripts = true;
    opts
}
