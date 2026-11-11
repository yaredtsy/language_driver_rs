//! Loading a project: which tsconfig is chosen and which files the program holds. One fixture folder per case under `tests/fixtures/`.

use std::path::{Path, PathBuf};

use typescript_analysis::{OpenError, Project};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn a_tsconfig_above_the_file_lists_its_three_files() {
    let project = Project::open(&fixture("three-files"), Path::new("src/main.ts")).unwrap();
    assert_eq!(
        project.files(),
        ["src/main.ts", "src/math.ts", "src/util.ts"]
    );
}

#[test]
fn the_tsconfig_is_found_from_any_file_of_the_project() {
    let project = Project::open(&fixture("three-files"), Path::new("src/util.ts")).unwrap();
    assert_eq!(project.files().len(), 3);
}

#[test]
fn an_absolute_file_path_is_accepted() {
    let root = fixture("three-files");
    let project = Project::open(&root, &root.join("src/main.ts")).unwrap();
    assert_eq!(project.files().len(), 3);
}

#[test]
fn a_missing_file_is_no_such_file() {
    let error = Project::open(&fixture("three-files"), Path::new("src/gone.ts")).err();
    assert_eq!(error, Some(OpenError::NoSuchFile));
}

#[test]
fn opening_twice_in_one_process_works() {
    let first = Project::open(&fixture("three-files"), Path::new("src/main.ts")).unwrap();
    let second = Project::open(&fixture("three-files"), Path::new("src/main.ts")).unwrap();
    drop(first);
    assert_eq!(second.files().len(), 3);
}

#[test]
fn a_folder_with_no_config_is_an_inferred_project_of_the_file_and_its_imports() {
    let project = Project::open(&fixture("plain-js"), Path::new("index.js")).unwrap();
    // `unused.js` is not imported, so it is not in the program.
    assert_eq!(project.files(), ["index.js", "lib/util.js"]);
}

#[test]
fn an_inferred_project_follows_require_in_a_js_file() {
    let project = Project::open(&fixture("plain-js-require"), Path::new("main.js")).unwrap();
    assert_eq!(project.files(), ["dep.js", "main.js"]);
}

#[test]
fn an_inferred_project_of_a_ts_file_loads_too() {
    let project = Project::open(&fixture("no-config"), Path::new("a.ts")).unwrap();
    assert_eq!(project.files(), ["a.ts"]);
}

#[test]
fn a_file_that_is_not_source_has_no_program() {
    let error = Project::open(&fixture("no-config"), Path::new("notes.txt")).err();
    assert_eq!(
        error,
        Some(OpenError::NotInProject("(inferred project)".into()))
    );
}

#[test]
fn a_config_that_cannot_be_parsed_is_a_config_error() {
    let error = Project::open(&fixture("broken-config"), Path::new("a.ts")).err();
    assert!(
        matches!(error, Some(OpenError::ConfigError(_))),
        "{error:?}"
    );
}

#[test]
fn a_file_the_config_does_not_include_is_not_in_project() {
    let root = fixture("excluded-file");
    assert!(Project::open(&root, Path::new("src/a.ts")).is_ok());
    let error = Project::open(&root, Path::new("b.ts")).err();
    assert_eq!(error, Some(OpenError::NotInProject("tsconfig.json".into())));
}

#[test]
fn type_errors_do_not_stop_a_project_loading() {
    let project = Project::open(&fixture("type-errors"), Path::new("a.ts")).unwrap();
    assert_eq!(project.files(), ["a.ts"]);
}

#[test]
fn files_lists_project_code_only() {
    let project = Project::open(&fixture("with-node-modules"), Path::new("src/main.ts")).unwrap();
    // `src/types.d.ts` is in the program but is a declaration file; the packages are in `node_modules`
    assert_eq!(project.files(), ["src/main.ts"]);
}
