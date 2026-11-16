//! `LoadOptions`, `Project::skipped` and `Project::environment_inputs` (RS3 §2, §3). What the switches do to the engine's answers is in the
//! unit tests of `sem/origin.rs` (they need a call walk); here is what the public type says.

use rust_analysis::{LoadOptions, Project};

mod common;
use common::{copy_of_fixture, snapshot};

/// `LoadOptions` is `#[non_exhaustive]`: no struct literal outside the crate.
#[allow(clippy::field_reassign_with_default)]
fn options(build_scripts: bool, proc_macros: bool) -> LoadOptions {
    let mut opts = LoadOptions::default();
    opts.build_scripts = build_scripts;
    opts.proc_macros = proc_macros;
    opts
}

const GLOBS: [&str; 5] = [
    "**/Cargo.toml",
    "**/Cargo.lock",
    "**/rust-toolchain",
    "**/rust-toolchain.toml",
    "**/.cargo/config.toml",
];

#[test]
fn both_switches_are_off_by_default() {
    let opts = LoadOptions::default();
    assert!(!opts.build_scripts && !opts.proc_macros);
}

/// The `derive` fixture has a package with a `build.rs` and a proc-macro crate; `scripts` has a `build.rs`; `lock` has neither.
#[test]
fn what_the_switches_leave_out_is_counted_by_name() {
    let derive = copy_of_fixture("derive", "switches_derive");
    let open_derive = |opts: &LoadOptions| {
        Project::open(&derive, &derive.join("app/src/lib.rs"), opts).expect("derive loads")
    };
    // both off: both counted, sorted by name
    assert_eq!(
        open_derive(&options(false, false)).skipped(),
        [("build_scripts_skipped", 1), ("proc_macros_skipped", 1)]
    );
    // scripts on: only the proc macro is left out
    assert_eq!(
        open_derive(&options(true, false)).skipped(),
        [("proc_macros_skipped", 1)]
    );
    // proc macros on: they are built by the build run, so the scripts ran too and nothing is left out
    assert_eq!(open_derive(&options(false, true)).skipped(), []);
    assert_eq!(open_derive(&options(true, true)).skipped(), []);

    // a workspace with a `build.rs` and no proc macro: the second counter is not listed at all
    let scripts = copy_of_fixture("scripts", "switches_scripts");
    let open_scripts = |opts: &LoadOptions| {
        Project::open(&scripts, &scripts.join("src/lib.rs"), opts).expect("scripts loads")
    };
    assert_eq!(
        open_scripts(&options(false, false)).skipped(),
        [("build_scripts_skipped", 1)]
    );
    assert_eq!(open_scripts(&options(true, false)).skipped(), []);

    // neither: nothing is left out, so nothing is listed
    let lock = copy_of_fixture("lock", "switches_lock");
    let project = Project::open(
        &lock.join("app"),
        &lock.join("app/src/lib.rs"),
        &LoadOptions::default(),
    )
    .expect("app loads");
    assert_eq!(project.skipped(), []);
}

#[test]
fn the_environment_is_the_globs_then_the_exact_files_outside_the_root() {
    // root = a member folder of a workspace above it, and that workspace has a sibling: three manifests are outside the root
    let copy = copy_of_fixture("above", "switches_above");
    let member = copy.join("member");
    let project = Project::open(&member, &member.join("src/lib.rs"), &LoadOptions::default())
        .expect("the member loads");

    let spelled = copy.to_str().unwrap().replace('\\', "/");
    let mut expected: Vec<String> = GLOBS.iter().map(|g| (*g).to_owned()).collect();
    // no `Cargo.lock` is there, and a load makes none; `member/Cargo.toml` is under the root, a glob covers it
    expected.push(format!("{spelled}/Cargo.toml"));
    expected.push(format!("{spelled}/sibling/Cargo.toml"));
    assert_eq!(project.environment_inputs(), expected);
}

#[test]
fn build_rs_is_watched_when_build_scripts_ran() {
    let scripts = copy_of_fixture("scripts", "switches_env");
    let open = |opts: &LoadOptions| {
        Project::open(&scripts, &scripts.join("src/lib.rs"), opts)
            .expect("scripts loads")
            .environment_inputs()
    };
    let off = open(&options(false, false));
    assert_eq!(off, GLOBS);
    // under the root, so a glob covers every file: only the glob is added
    let on = open(&options(true, false));
    let mut expected: Vec<&str> = GLOBS.to_vec();
    expected.push("**/build.rs");
    assert_eq!(on, expected);
    // proc macros run the build scripts as well
    assert_eq!(open(&options(false, true)), expected);
}

/// Running the build scripts is a `cargo check`: it compiles into `target/rust-analyzer` (not into the user's own `target/` build) and must not write
/// a `Cargo.lock` into the project (RS3 §5, D38: the lock redirect covers it).
#[test]
fn a_build_run_writes_only_under_target() {
    let copy = copy_of_fixture("derive", "switches_writes");
    let outside_target = |dir: &std::path::Path| {
        let mut files = snapshot(dir);
        files.retain(|path, _| !path.starts_with("target/"));
        files
    };
    let before = outside_target(&copy);
    assert!(before.keys().all(|path| !path.ends_with("Cargo.lock")));

    Project::open(&copy, &copy.join("app/src/lib.rs"), &options(true, true)).expect("derive loads");

    assert_eq!(
        outside_target(&copy),
        before,
        "the build run changed the project"
    );
    assert!(
        copy.join("target/rust-analyzer").is_dir(),
        "the build did not go to target/rust-analyzer"
    );
}

/// A `build.rs` that panics makes the build run fail. That is not a failed load: the project loads as if the scripts had not run, and both
/// counters say what was cut, the proc macro's too when it was asked for (its dylib is made by the same run).
#[test]
fn a_build_that_fails_is_not_a_failed_load() {
    let copy = copy_of_fixture("broken", "switches_broken");
    let open = |opts: &LoadOptions| {
        Project::open(&copy, &copy.join("app/src/lib.rs"), opts)
            .expect("a failed build still loads")
    };
    let both = [("build_scripts_skipped", 1), ("proc_macros_skipped", 1)];

    let scripts = open(&options(true, false));
    assert_eq!(scripts.skipped(), both);
    assert!(scripts.files().iter().any(|f| f == "app/src/lib.rs"));

    let proc_macros = open(&options(false, true));
    assert_eq!(proc_macros.skipped(), both);
    assert!(proc_macros.files().iter().any(|f| f == "app/src/lib.rs"));

    // the same fixture with the build not asked for says the same: nothing differs between "off" and "failed"
    assert_eq!(open(&options(false, false)).skipped(), both);
}

/// A path dependency that is not a member of the workspace is project code (§4), so its `build.rs` counts too: `app` has none, `dep` is outside
/// the workspace folder and has one.
#[test]
fn a_path_dependencys_build_script_is_counted() {
    let copy = copy_of_fixture("pathdep", "switches_pathdep");
    let app = copy.join("ws/app");
    let project =
        Project::open(&copy, &app.join("src/lib.rs"), &options(false, false)).expect("app loads");
    assert_eq!(project.skipped(), [("build_scripts_skipped", 1)]);
}
