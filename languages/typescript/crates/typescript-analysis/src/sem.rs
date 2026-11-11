//! The only file that imports the engine (`tsrs_*`). Everything else asks `Sem` and gets plain data.
//!
//! A `Sem` is one loaded program: the tsconfig that owns a file, every file it pulls in, parsed and bound. It follows `tsrs_lint`'s
//! `engine.rs` (`create_program`) and the memory pattern measured in S-TS3 (`ENGINE.md`): build inside one region, give the loaders
//! a region set, and free the program on drop with that region entered. The region alone is not enough: the `Program` and its processed
//! files are `Box::leak`ed on the heap, outside it (60 opens grew RSS to 349 MB without the free, 24 MB with it: `tests/memory.rs`).

#![cfg_attr(
    not(feature = "__proof"),
    expect(
        unreachable_pub,
        reason = "`Sem` and `File` are `pub` only for `proof`, which exists with the feature `__proof`"
    )
)]

use std::cell::Cell;
use std::collections::{BTreeSet, HashSet, VecDeque};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tsrs_ast::SourceFile;
use tsrs_compiler::{Program, ProgramOptions, ProgramRegions};
use tsrs_core::arena::Region;
use tsrs_core::{
    CompilerOptions, JsxEmit, ModuleKind, ModuleResolutionKind, P, ScriptTarget, Tristate, tspath,
};
use tsrs_tsoptions::{ParseConfigHost, ParsedCommandLine};
use tsrs_vfs::{FS, bundled, cachedvfs, osvfs};

use typescript_syntax::Dialect;

use crate::OpenError;

/// First chunk of the region a program lives in: the 1 MB `tsrs_lint` uses for one program.
const CHUNK: usize = 1 << 20;

/// Where tsrs reads config files from: the real disk, with the bundled `lib.*.d.ts` mounted.
struct ConfigHost {
    fs: Arc<dyn FS>,
    cwd: String,
}

impl ParseConfigHost for ConfigHost {
    fn fs(&self) -> &dyn FS {
        &*self.fs
    }
    fn get_current_directory(&self) -> &str {
        &self.cwd
    }
}

/// A source file of a `Sem`'s program. tsrs's `P<SourceFile>` is `'static` and the compiler cannot see the program being freed, so this adds a
/// lifetime: a `File<'p>` can only come from a `&'p Sem` (`PhantomData<&'p Sem>` stores nothing and borrows the `Sem`), and `Sem::drop`'s
/// `unsafe` rests on that. `lib.rs`'s `proof` module has the compile-fail proofs: it cannot outlive its `Sem`, and it cannot cross threads.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct File<'p> {
    node: P<SourceFile>,
    _sem: PhantomData<&'p Sem>,
    /// A raw pointer is neither `Send` nor `Sync`, so neither is `File`: a handle stays on the thread that asked for it (`proof` pins this).
    _one_thread: PhantomData<*const ()>,
}

pub struct Sem {
    root: PathBuf,
    /// The program is freed by `Drop`. A `P<…>` taken from it must not outlive `self`.
    program: &'static Program,
    /// Every config file that fed this program: the ones the owner search read (a broken one too) and the ones they extend. Relative to the root
    /// where they are under it, else as the engine spells them. Sorted.
    config_files: Vec<String>,
    /// The config whose program this is (relative to the root), or `None` for an inferred project.
    config_file: Option<String>,
    /// `Cell` is `Send` but not `Sync`, so neither is a `Sem` shared by reference: one checker answers one question at a time, and nothing
    /// here makes concurrent questions safe (`proof` pins this). Moving a `Sem` to another thread stays allowed.
    _not_shared: PhantomData<Cell<()>>,
    // Fields drop in this order, after `Drop::drop` has freed the program: the loaders' regions, then the program's own.
    _loaders: Arc<ProgramRegions>,
    _region: Region,
}

impl Sem {
    /// Loads the program of the `tsconfig.json` (or `jsconfig.json`) nearest above `file`, looking no further up than `root`; with none, an
    /// inferred project built from `file` alone.
    /// `file` is relative to `root`, or absolute. A program that loads with type errors is not an error.
    pub fn open(root: &Path, file: &Path) -> Result<Sem, OpenError> {
        let root = root.canonicalize().map_err(|_| OpenError::NoSuchFile)?;
        let file = root
            .join(file)
            .canonicalize()
            .map_err(|_| OpenError::NoSuchFile)?;
        if !file.is_file() {
            return Err(OpenError::NoSuchFile);
        }
        let cwd = to_engine_path(&root);
        let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(cachedvfs::from(osvfs::fs())));
        let config_host = ConfigHost {
            fs: Arc::clone(&fs),
            cwd: cwd.clone(),
        };

        let region = Region::new_scratch(CHUNK);
        let loaders = Arc::new(ProgramRegions::default());
        let mut config_inputs = BTreeSet::new();
        let (program, config_name) = {
            let _scope = region.enter();
            let mut skipped = Vec::new();
            let owner = find_owner(&root, &file, &config_host, &mut config_inputs, &mut skipped)?;
            let (config_name, mut config) = match owner {
                Owner::Config(name, config) => (Some(name), *config),
                Owner::NoneHolds(name) => {
                    return Err(OpenError::NotInProject {
                        config: relative_to(&root, &name),
                        skipped,
                    });
                }
                Owner::NoConfig => (None, inferred_config(&file, &config_host)),
            };
            // One checker answers every question, so the same project gives the same answers in the same order (rule 9). Not
            // `single_threaded`: that also loads files one at a time, 3.5 times slower on a 1,500-file program (ENGINE.md).
            let compiler_options = config
                .compiler_options()
                .map(|options| (*options).clone())
                .unwrap_or_default();
            config.parsed_config.compiler_options = Some(P::new(CompilerOptions {
                checkers: Some(1),
                ..compiler_options
            }));
            let host = tsrs_compiler::new_compiler_host(
                &cwd,
                fs,
                &bundled::default_lib_path(),
                None,
                None,
                None,
            );
            let mut options = ProgramOptions::new(P::new(config), host);
            // A referenced project's source stands in for its build output (its `.d.ts`, often not built): as `tsrs_lint` does for a config.
            options.use_source_of_project_reference = config_name.is_some();
            // The checkers' memory goes back with the program (S-TS3).
            options.checker_regions = true;
            options.regions = Some(Arc::clone(&loaders));
            let program = tsrs_compiler::new_program(options);
            // Load and bind only: calls are resolved on demand, so nothing here type-checks the whole program.
            program.bind_source_files();
            (program, config_name)
        };

        let config_files = config_inputs
            .iter()
            .map(|name| relative_to(&root, name))
            .collect();
        let config_file = config_name.as_ref().map(|name| relative_to(&root, name));
        let sem = Sem {
            root,
            program,
            config_files,
            config_file,
            _not_shared: PhantomData,
            _loaders: loaders,
            _region: region,
        };
        if sem
            .program
            .get_source_file(&to_engine_path(&file))
            .is_none()
        {
            return Err(OpenError::NotInProject {
                config: match &config_name {
                    Some(name) => relative_to(&sem.root, name),
                    None => INFERRED.into(),
                },
                skipped: Vec::new(),
            });
        }
        Ok(sem)
    }

    /// Every file of the program: the project's, `node_modules`, and the bundled libraries. Ask `is_project_code` to tell them apart.
    pub(crate) fn files(&self) -> impl Iterator<Item = File<'_>> {
        self.program.get_source_files().iter().map(|&node| File {
            node,
            _sem: PhantomData,
            _one_thread: PhantomData,
        })
    }

    /// The file at `relative` (to the root, with `/`; `./` and `..` are folded), if the program holds it.
    #[cfg_attr(
        not(any(test, feature = "__proof")),
        expect(dead_code, reason = "TS4 finds an entry with it")
    )]
    pub fn file(&self, relative: &str) -> Option<File<'_>> {
        let name = tspath::get_normalized_absolute_path(relative, &to_engine_path(&self.root));
        let node = self.program.get_source_file(&name)?;
        Some(File {
            node,
            _sem: PhantomData,
            _one_thread: PhantomData,
        })
    }

    /// The file's path relative to the root, with `/`; `None` for a file outside the root, such as the bundled libraries.
    pub(crate) fn relative_path(&self, file: File<'_>) -> Option<String> {
        let root = to_engine_path(&self.root);
        let relative = file.node.file_name().strip_prefix(root.as_str())?;
        Some(relative.strip_prefix('/')?.to_owned())
    }

    /// Code of the project itself: a TypeScript or JavaScript file (`Dialect::from_path`: an imported `.json` is in the program but is not code),
    /// not in `node_modules`, not a declaration file (`.d.ts`, `.d.mts`, `.d.cts`), not a bundled library.
    pub(crate) fn is_project_code(&self, file: File<'_>) -> bool {
        let name = file.node.file_name();
        Dialect::from_path(name).is_some()
            && !self
                .program
                .is_source_file_default_library(file.node.path())
            && !tspath::is_declaration_file_name(name)
            && package_in_node_modules(name).is_none()
            && !self.program.is_source_file_from_external_library(file.node)
    }

    /// The config whose program this is, relative to the root; `None` for an inferred project.
    pub(crate) fn config_file(&self) -> Option<&str> {
        self.config_file.as_deref()
    }

    /// The config files that fed this program (see the field). What `environment_inputs` adds to its fixed globs.
    pub(crate) fn config_files(&self) -> &[String] {
        &self.config_files
    }

    /// A declaration file of the repo itself: under the root, not in `node_modules`, not a bundled library. Not project code (nothing in it is
    /// analyzed), but editing one changes how calls resolve, so a cache must know about it.
    pub(crate) fn is_repo_declaration_file(&self, file: File<'_>) -> bool {
        tspath::is_declaration_file_name(file.node.file_name())
            && self.package_of(file).is_none()
            && self.relative_path(file).is_some()
    }

    /// The package a file belongs to, for an `External` edge: `"lodash"`, `"@scope/pkg"`, or `"typescript:lib"` for a bundled library. `None` for
    /// the project's own files, declaration files included. `@types/x` is named `x`, the package it describes.
    pub(crate) fn package_of(&self, file: File<'_>) -> Option<String> {
        if self
            .program
            .is_source_file_default_library(file.node.path())
        {
            return Some(BUNDLED_LIBS.into());
        }
        package_in_node_modules(file.node.file_name())
    }
}

impl Drop for Sem {
    /// The only `unsafe` in the workspace: tsrs's contract for freeing a program (`tsrs_lint` does the same in `ProgramOwner::drop`).
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        let _scope = self._region.enter();
        // SAFETY: `open` made this program and shared nothing of it with another (no `update_program`); `Sem` holds no checker handle and
        // hands none out, and nothing reads `self.program` after this. "Hands none out" is true only while `Sem` has no handle type: the
        // first one (`File<'p>`, TS3 step 4) must carry `PhantomData<&'p Sem>`, so the compiler checks that none outlives this drop.
        unsafe { tsrs_compiler::free_unshared_program(self.program) };
    }
}

/// What `package_of` says for a bundled `lib.*.d.ts` (`typescript:lib`: the engine's own copy of TypeScript's libraries).
const BUNDLED_LIBS: &str = "typescript:lib";

/// The package a path inside `node_modules` belongs to, from the **last** `node_modules/` in it (pnpm nests them:
/// `node_modules/.pnpm/a@1/node_modules/a/…` is `a`). `@scope/pkg` keeps its scope; `@types/x` is `x` and `@types/scope__x` is `@scope/x`, the
/// packages they describe.
fn package_in_node_modules(path: &str) -> Option<String> {
    let (_, inside) = path.rsplit_once("/node_modules/")?;
    let mut parts = inside.split('/');
    let first = parts.next().filter(|part| !part.is_empty())?;
    if !first.starts_with('@') {
        return Some(first.to_owned());
    }
    let name = parts.next().filter(|part| !part.is_empty())?;
    if first == "@types" {
        return Some(match name.split_once("__") {
            Some((scope, name)) => format!("@{scope}/{name}"),
            None => name.to_owned(),
        });
    }
    Some(format!("{first}/{name}"))
}

/// What `NotInProject` names when there was no config: the program was inferred from the file alone.
const INFERRED: &str = "(inferred project)";

/// Reads a `tsconfig.json` / `jsconfig.json`: its options, its files and its references.
fn load_config(path: &Path, host: &ConfigHost) -> Result<ParsedCommandLine, OpenError> {
    let (config, diagnostics) = tsrs_tsoptions::get_parsed_command_line_of_config_file(
        &to_engine_path(path),
        None,
        None,
        host,
        None,
    );
    if let Some(first) = diagnostics.first() {
        return Err(OpenError::ConfigError(first.localize()));
    }
    let config =
        config.ok_or_else(|| OpenError::ConfigError("the config file could not be read".into()))?;
    // A config with a syntax error still parses, its errors kept on its own source file (as `tsrs_lint`'s `find_tsconfig` reads them).
    if let Some(first) = config
        .config_file
        .and_then(|file| file.source_file.diagnostics().first().copied())
    {
        return Err(OpenError::ConfigError(first.localize()));
    }
    Ok(config)
}

/// The program of a folder with no config: `file` alone is the root, and its imports pull in the rest. The options are `tsrs_lint`'s
/// for files no config claims, except that nothing is emitted (`no_emit`) and a `.js` file is read, never checked (`check_js` off).
fn inferred_config(file: &Path, host: &ConfigHost) -> ParsedCommandLine {
    tsrs_tsoptions::new_parsed_command_line(
        P::new(CompilerOptions {
            allow_js: Tristate::True,
            check_js: Tristate::False,
            no_emit: Tristate::True,
            module: ModuleKind::ES_NEXT,
            module_resolution: ModuleResolutionKind::BUNDLER,
            target: ScriptTarget::ES2022,
            jsx: JsxEmit::REACT_JSX,
            allow_importing_ts_extensions: Tristate::True,
            es_module_interop: Tristate::True,
            resolve_json_module: Tristate::True,
            ..Default::default()
        }),
        vec![to_engine_path(file)],
        Vec::new(),
        tspath::ComparePathsOptions {
            current_directory: host.cwd.clone(),
            use_case_sensitive_file_names: host.fs.use_case_sensitive_file_names(),
        },
    )
}

/// The nearest `tsconfig.json`, else `jsconfig.json`, in `dir` or above it, up to `root`: the editor's upward search, bounded by the
/// repository.
fn find_config(root: &Path, dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .take_while(|dir| dir.starts_with(root))
        .find_map(|dir| {
            ["tsconfig.json", "jsconfig.json"]
                .iter()
                .map(|name| dir.join(name))
                .find(|path| path.is_file())
        })
}

/// What the search for the config that owns a file found.
enum Owner {
    /// The config whose program holds the file, as engine path and parsed.
    Config(String, Box<ParsedCommandLine>),
    /// There are configs above the file but none of them, nor any they reference, holds it. The nearest one's engine path.
    NoneHolds(String),
    /// No config above the file, up to the root.
    NoConfig,
}

/// Which config owns `file`, as `tsrs_lint`'s `find_tsconfig` (and tsserver) decide, but bounded by `root`: the nearest config, then the configs it
/// references (a solution config has `"files": []` and only references: the Vite layout), then the config above that, and so on; the first whose
/// file list holds the file. A `composite` config must hold it by its own patterns. `disableSolutionSearching` stops the climb.
///
/// Every config it reads goes into `inputs` (see `Sem::config_files`).
///
/// A syntax error in the **nearest** config is the person's to fix (`ConfigError`). One in a config only reached by a reference or a climb is
/// skipped, as `tsrs_lint` does (it may have nothing to do with this file), and listed in `skipped` for `NotInProject` to carry.
fn find_owner(
    root: &Path,
    file: &Path,
    host: &ConfigHost,
    inputs: &mut BTreeSet<String>,
    skipped: &mut Vec<String>,
) -> Result<Owner, OpenError> {
    let Some(nearest) = file.parent().and_then(|dir| find_config(root, dir)) else {
        return Ok(Owner::NoConfig);
    };
    let nearest = to_engine_path(&nearest);
    let file_name = to_engine_path(file);
    let file_path = tspath::to_path(
        &file_name,
        &host.cwd,
        host.fs.use_case_sensitive_file_names(),
    );

    let mut visited = HashSet::new();
    let mut next = Some(nearest.clone());
    while let Some(config_name) = next {
        let mut queue = VecDeque::from([config_name.clone()]);
        let mut disable_solution_searching = false;
        while let Some(name) = queue.pop_front() {
            if !visited.insert(name.clone()) {
                continue;
            }
            // read, so a change to it can change the answer; a broken one too, because fixing it would
            inputs.insert(name.clone());
            let config = match load_config(Path::new(&name), host) {
                Ok(config) => config,
                Err(error) if name == nearest => return Err(error),
                Err(error) => {
                    // never silent: `NotInProject` lists these, as the likely cause of "not in project"
                    let reason = match error {
                        OpenError::ConfigError(reason) => reason,
                        other => other.to_string(),
                    };
                    skipped.push(format!("{}: {reason}", relative_to(root, &name)));
                    continue;
                }
            };
            inputs.extend(config.extended_source_files());
            if name == config_name {
                disable_solution_searching = config
                    .compiler_options()
                    .is_some_and(|options| options.disable_solution_searching.is_true());
            }
            let holds = !config.file_names().is_empty()
                && (config
                    .compiler_options()
                    .is_none_or(|options| !options.composite.is_true())
                    || config.possibly_matches_file_name(&file_name))
                && config.file_names_by_path().contains_key(&file_path);
            if holds {
                return Ok(Owner::Config(name, Box::new(config)));
            }
            queue.extend(config.resolved_project_reference_paths().iter().cloned());
        }
        if disable_solution_searching {
            break;
        }
        // the config above: look from the parent of this config's directory
        next = Path::new(&config_name)
            .parent()
            .and_then(Path::parent)
            .and_then(|dir| find_config(root, dir))
            .map(|path| to_engine_path(&path));
    }
    Ok(Owner::NoneHolds(nearest))
}

/// An engine path made relative to `root` (the config's name in `NotInProject`).
fn relative_to(root: &Path, engine_path: &str) -> String {
    let root = to_engine_path(root);
    engine_path
        .strip_prefix(root.as_str())
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(engine_path)
        .to_owned()
}

/// tsrs spells every path absolute, with `/` as the separator.
fn to_engine_path(path: &Path) -> String {
    tspath::normalize_slashes(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three_files() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/three-files")
    }

    #[test]
    fn the_program_has_one_checker() {
        let sem = Sem::open(&three_files(), Path::new("src/main.ts")).unwrap();
        assert_eq!(sem.program.options().checkers, Some(1));
    }

    #[test]
    fn the_program_lists_its_files_in_the_same_order_every_time() {
        // Files are parsed on several threads, so the order the program ends with must not depend on which finished first.
        let names = || -> Vec<String> {
            let sem = Sem::open(&three_files(), Path::new("src/main.ts")).unwrap();
            sem.program
                .get_source_files()
                .iter()
                .map(|file| file.file_name().to_owned())
                .collect()
        };
        let first = names();
        assert!(
            first.len() > 3,
            "the bundled libraries are in the program too"
        );
        for _ in 0..30 {
            assert_eq!(names(), first);
        }
    }

    fn with_node_modules() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/with-node-modules")
    }

    /// (relative path or bundled, is project code, package) of every file in the program, sorted.
    fn classified(sem: &Sem) -> Vec<(String, bool, Option<String>)> {
        let mut all: Vec<_> = sem
            .files()
            .map(|file| {
                (
                    sem.relative_path(file)
                        .unwrap_or_else(|| "(outside root)".into()),
                    sem.is_project_code(file),
                    sem.package_of(file),
                )
            })
            .collect();
        all.sort();
        all
    }

    #[test]
    fn project_code_is_told_apart_from_packages_declarations_and_libraries() {
        let sem = Sem::open(&with_node_modules(), Path::new("src/main.ts")).unwrap();
        let all = classified(&sem);
        let under_root: Vec<_> = all
            .iter()
            .filter(|(path, ..)| path != "(outside root)")
            .collect();
        let row = |path: &str, project: bool, package: Option<&str>| {
            (path.to_owned(), project, package.map(str::to_owned))
        };
        assert_eq!(
            under_root,
            [
                &row(
                    "node_modules/@scope/other/index.d.ts",
                    false,
                    Some("@scope/other")
                ),
                &row("node_modules/@types/fake/index.d.ts", false, Some("fake")),
                &row("node_modules/pkg/index.d.ts", false, Some("pkg")),
                &row("src/main.ts", true, None),
                &row("src/types.d.ts", false, None),
            ]
        );
        // everything else is a bundled library: not project code, one package for all of them
        let libs: Vec<_> = all
            .iter()
            .filter(|(path, ..)| path == "(outside root)")
            .collect();
        assert!(libs.len() > 50, "{} libraries", libs.len());
        assert!(
            libs.iter()
                .all(|(_, project, package)| !project
                    && package.as_deref() == Some("typescript:lib"))
        );
    }

    #[test]
    fn a_repo_declaration_file_is_in_the_program_but_is_not_project_code() {
        let sem = Sem::open(&with_node_modules(), Path::new("src/main.ts")).unwrap();
        let types = sem
            .file("src/types.d.ts")
            .expect("main.ts imports it, so the program holds it");
        assert!(!sem.is_project_code(types));
        assert_eq!(sem.package_of(types), None);
    }

    #[test]
    fn an_imported_json_file_is_held_by_the_program_and_is_not_project_code() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json-import");
        let sem = Sem::open(&root, Path::new("src/main.ts")).unwrap();
        let data = sem.file("src/data.json").expect("main.ts imports it");
        assert!(!sem.is_project_code(data));
        assert!(!sem.is_repo_declaration_file(data));
    }

    #[test]
    fn a_file_is_found_by_its_relative_path() {
        let sem = Sem::open(&with_node_modules(), Path::new("src/main.ts")).unwrap();
        let file = sem.file("./src/../src/main.ts").unwrap();
        assert_eq!(sem.relative_path(file), Some("src/main.ts".into()));
        assert!(sem.file("src/gone.ts").is_none());
    }

    #[test]
    fn a_package_is_named_from_the_last_node_modules_in_the_path() {
        let name = |path: &str| package_in_node_modules(path);
        assert_eq!(
            name("/r/node_modules/lodash/fp/index.js"),
            Some("lodash".into())
        );
        assert_eq!(
            name("/r/node_modules/@scope/pkg/a.js"),
            Some("@scope/pkg".into())
        );
        assert_eq!(
            name("/r/node_modules/@types/node/fs.d.ts"),
            Some("node".into())
        );
        assert_eq!(
            name("/r/node_modules/@types/babel__core/i.d.ts"),
            Some("@babel/core".into())
        );
        assert_eq!(
            name("/r/node_modules/.pnpm/react@18/node_modules/react/index.js"),
            Some("react".into())
        );
        assert_eq!(name("/r/src/node_modules_like/a.ts"), None);
        assert_eq!(name("/r/node_modules/"), None);
    }
}
