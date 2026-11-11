//! The only file that imports the engine (`tsrs_*`). Everything else asks `Sem` and gets plain data.
//!
//! A `Sem` is one loaded program: the tsconfig that owns a file, every file it pulls in, parsed and bound. It follows `tsrs_lint`'s
//! `engine.rs` (`create_program`) and the memory pattern measured in S-TS3 (`ENGINE.md`): build inside one region, give the loaders
//! a region set. Freeing the program itself (`free_unshared_program`, an `unsafe fn`) is TS3 step 5; until then dropping a `Sem` drops
//! its region, which returns what was allocated in it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tsrs_compiler::{Program, ProgramOptions, ProgramRegions};
use tsrs_core::arena::Region;
use tsrs_core::{P, tspath};
use tsrs_tsoptions::ParseConfigHost;
use tsrs_vfs::{FS, bundled, cachedvfs, osvfs};

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

pub(crate) struct Sem {
    root: PathBuf,
    /// The program lives in `_region`. A `P<…>` taken from it must not outlive `self`.
    program: &'static Program,
    // Fields drop in this order: the loaders' regions, then the program's own.
    _loaders: Arc<ProgramRegions>,
    _region: Region,
}

impl Sem {
    /// Loads the program of the `tsconfig.json` (or `jsconfig.json`) nearest above `file`, looking no further up than `root`.
    /// `file` is relative to `root`, or absolute. A program that loads with type errors is not an error.
    pub(crate) fn open(root: &Path, file: &Path) -> Result<Sem, OpenError> {
        let root = root.canonicalize().map_err(|_| OpenError::NoSuchFile)?;
        let file = root
            .join(file)
            .canonicalize()
            .map_err(|_| OpenError::NoSuchFile)?;
        if !file.is_file() {
            return Err(OpenError::NoSuchFile);
        }
        let config_path = find_config(&root, &file).ok_or_else(|| {
            OpenError::ConfigError("no tsconfig.json or jsconfig.json above the file".into())
        })?;

        let cwd = to_engine_path(&root);
        let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(cachedvfs::from(osvfs::fs())));
        let config_host = ConfigHost {
            fs: Arc::clone(&fs),
            cwd: cwd.clone(),
        };

        let region = Region::new_scratch(CHUNK);
        let loaders = Arc::new(ProgramRegions::default());
        let program = {
            let _scope = region.enter();
            let (config, diagnostics) = tsrs_tsoptions::get_parsed_command_line_of_config_file(
                &to_engine_path(&config_path),
                None,
                None,
                &config_host,
                None,
            );
            if let Some(first) = diagnostics.first() {
                return Err(OpenError::ConfigError(first.localize()));
            }
            let config = config.ok_or_else(|| {
                OpenError::ConfigError("the config file could not be read".into())
            })?;
            // A config with a syntax error still parses, its errors kept on its own source file (as `tsrs_lint`'s `find_tsconfig` reads them).
            if let Some(first) = config
                .config_file
                .and_then(|file| file.source_file.diagnostics().first().copied())
            {
                return Err(OpenError::ConfigError(first.localize()));
            }
            let host = tsrs_compiler::new_compiler_host(
                &cwd,
                fs,
                &bundled::default_lib_path(),
                None,
                None,
                None,
            );
            let mut options = ProgramOptions::new(P::new(config), host);
            // The checkers' memory goes back with the program (S-TS3).
            options.checker_regions = true;
            options.regions = Some(Arc::clone(&loaders));
            let program = tsrs_compiler::new_program(options);
            // Load and bind only: calls are resolved on demand, so nothing here type-checks the whole program.
            program.bind_source_files();
            program
        };

        let sem = Sem {
            root,
            program,
            _loaders: loaders,
            _region: region,
        };
        if sem
            .program
            .get_source_file(&to_engine_path(&file))
            .is_none()
        {
            return Err(OpenError::ConfigError(format!(
                "{} does not include the file",
                config_path
                    .strip_prefix(&sem.root)
                    .unwrap_or(&config_path)
                    .display()
            )));
        }
        Ok(sem)
    }

    /// The program's source files that live under the root, relative to it with `/`, sorted. The bundled libraries and anything outside the
    /// root are left out.
    pub(crate) fn files(&self) -> Vec<String> {
        let root = to_engine_path(&self.root);
        let mut files: Vec<String> = self
            .program
            .get_source_files()
            .iter()
            .filter_map(|file| {
                let relative = file
                    .file_name()
                    .strip_prefix(root.as_str())?
                    .strip_prefix('/')?;
                Some(relative.to_owned())
            })
            .collect();
        files.sort();
        files
    }
}

/// The nearest `tsconfig.json`, else `jsconfig.json`, from the file's directory up to `root`: the editor's upward search, bounded by
/// the repository.
fn find_config(root: &Path, file: &Path) -> Option<PathBuf> {
    file.ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(root))
        .find_map(|dir| {
            ["tsconfig.json", "jsconfig.json"]
                .iter()
                .map(|name| dir.join(name))
                .find(|path| path.is_file())
        })
}

/// tsrs spells every path absolute, with `/` as the separator.
fn to_engine_path(path: &Path) -> String {
    tspath::normalize_slashes(&path.to_string_lossy())
}
