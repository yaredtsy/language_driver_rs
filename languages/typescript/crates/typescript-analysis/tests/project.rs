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
        Some(OpenError::NotInProject {
            config: "(inferred project)".into(),
            skipped: vec![]
        })
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
    assert_eq!(
        error,
        Some(OpenError::NotInProject {
            config: "tsconfig.json".into(),
            skipped: vec![]
        })
    );
}

#[test]
fn type_errors_do_not_stop_a_project_loading() {
    let project = Project::open(&fixture("type-errors"), Path::new("a.ts")).unwrap();
    assert_eq!(project.files(), ["a.ts"]);
}

#[test]
fn a_repo_declaration_file_is_not_in_files() {
    let project = Project::open(&fixture("with-node-modules"), Path::new("src/main.ts")).unwrap();
    // `src/types.d.ts` is the repo's own and is in the program (a sem test checks that, so its absence here is the rule and not an accident);
    // the packages are in `node_modules`. Whether a call into it resolves as `External` is TS4's (`external/`).
    assert_eq!(project.files(), ["src/main.ts"]);
}

#[test]
fn a_solution_config_hands_each_file_to_the_referenced_config_that_holds_it() {
    let root = fixture("solution");
    // The Vite layout: `tsconfig.json` has `"files": []` and references two configs, one for the app and one for the build script.
    let app = Project::open(&root, Path::new("src/main.ts")).unwrap();
    assert_eq!(app.files(), ["src/main.ts", "src/util.ts"]);
    let node = Project::open(&root, Path::new("vite.config.ts")).unwrap();
    assert_eq!(node.files(), ["vite.config.ts"]);
}

#[test]
fn a_referenced_projects_source_is_in_the_program_not_its_missing_build_output() {
    let root = fixture("references");
    let app = Project::open(&root, Path::new("packages/app/src/main.ts")).unwrap();
    assert_eq!(
        app.files(),
        ["packages/app/src/main.ts", "packages/lib/src/greet.ts"]
    );
}

#[test]
fn a_config_above_holds_a_file_the_nearest_config_leaves_out() {
    let root = fixture("parent-config");
    let inside = Project::open(&root, Path::new("sub/only/in.ts")).unwrap();
    assert_eq!(inside.files(), ["sub/only/in.ts"]);
    // `sub/tsconfig.json` does not include `other.ts`; the root config does, so that is its program
    let outside = Project::open(&root, Path::new("sub/other.ts")).unwrap();
    assert_eq!(outside.files(), ["sub/only/in.ts", "sub/other.ts"]);
}

const GLOBS: [&str; 9] = [
    "**/tsconfig*.json",
    "**/jsconfig.json",
    "**/package.json",
    "**/package-lock.json",
    "**/npm-shrinkwrap.json",
    "**/yarn.lock",
    "**/pnpm-lock.yaml",
    "**/bun.lock",
    "**/bun.lockb",
];

#[test]
fn environment_inputs_are_the_config_and_package_files_and_the_lock_files() {
    let project = Project::open(&fixture("three-files"), Path::new("src/main.ts")).unwrap();
    // the config that was used (`tsconfig.json`) is already matched by a glob, so it is not listed twice
    assert_eq!(project.environment_inputs(), GLOBS);
}

#[test]
fn an_extended_config_with_any_name_is_an_input_too() {
    let project = Project::open(&fixture("extends"), Path::new("src/main.ts")).unwrap();
    let mut expected: Vec<String> = GLOBS.iter().map(|glob| (*glob).to_owned()).collect();
    // `config/shared.json` is not named like a tsconfig, so no glob covers it: editing it changes the program
    expected.push("config/shared.json".into());
    assert_eq!(project.environment_inputs(), expected);
}

/// The engine's spelling of a fixture file: absolute, with `/`, and without the `\\?\` prefix `canonicalize` adds on Windows.
fn engine_path(name: &str) -> String {
    let path = fixture(name).canonicalize().unwrap();
    let path = path.to_string_lossy();
    path.strip_prefix(r"\\?\")
        .unwrap_or(&path)
        .replace('\\', "/")
}

fn globs_then(last: String) -> Vec<String> {
    let mut expected: Vec<String> = GLOBS.iter().map(|glob| (*glob).to_owned()).collect();
    expected.push(last);
    expected
}

#[test]
fn an_extended_config_outside_the_root_is_an_absolute_input() {
    let root = fixture("extends-outside/app");
    let project = Project::open(&root, Path::new("src/main.ts")).unwrap();
    // `../base.json` is not under the root, so there is no relative name for it: the entry is the engine's own spelling
    assert_eq!(
        project.environment_inputs(),
        globs_then(engine_path("extends-outside/base.json"))
    );
}

#[test]
fn a_glob_covers_only_what_is_under_the_root() {
    let root = fixture("extends-outside-named/app");
    let project = Project::open(&root, Path::new("src/main.ts")).unwrap();
    // `../tsconfig.base.json` is named like the first glob, but that glob is relative to the root and cannot reach it: it is listed.
    // `tsconfig.local.json` is under the root, where the glob does cover it: it is not listed a second time.
    assert_eq!(
        project.environment_inputs(),
        globs_then(engine_path("extends-outside-named/tsconfig.base.json"))
    );
}

#[test]
fn a_config_skipped_for_a_syntax_error_is_an_input_so_fixing_it_reloads() {
    let project = Project::open(&fixture("broken-reference"), Path::new("src/main.ts")).unwrap();
    assert!(
        project
            .environment_inputs()
            .contains(&"configs/broken.json".to_owned())
    );
}

#[test]
fn the_repos_own_declaration_files_are_listed_apart_from_files() {
    let project = Project::open(&fixture("with-node-modules"), Path::new("src/main.ts")).unwrap();
    assert_eq!(project.declaration_files(), ["src/types.d.ts"]);
    // not `node_modules` packages, not the bundled libraries, and not in `files()`
    let none = Project::open(&fixture("three-files"), Path::new("src/main.ts")).unwrap();
    assert!(none.declaration_files().is_empty());
}

#[test]
fn not_in_project_says_which_configs_were_skipped_for_not_parsing() {
    let root = fixture("broken-reference");
    let error = Project::open(&root, Path::new("elsewhere/x.ts"))
        .err()
        .unwrap();
    let OpenError::NotInProject { config, skipped } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(config, "tsconfig.json");
    // `configs/broken.json` may have been the one that holds the file; the person is told it could not be read
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert!(
        skipped[0].starts_with("configs/broken.json: "),
        "{skipped:?}"
    );
    assert!(error.to_string().contains("configs/broken.json"), "{error}");
}

#[test]
fn an_imported_json_file_is_in_the_program_but_is_not_project_code() {
    // a real frontend project imports two JSON fixtures: they were listed as project files, and nothing in them can be analyzed
    let project = Project::open(&fixture("json-import"), Path::new("src/main.ts")).unwrap();
    assert_eq!(project.files(), ["src/main.ts"]);
}
