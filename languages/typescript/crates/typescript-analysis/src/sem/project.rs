use super::*;

/// What `NotInProject` names when there was no config: the program was inferred from the file alone.
pub(super) const INFERRED: &str = "(inferred project)";

/// Reads a `tsconfig.json` / `jsconfig.json`: its options, its files and its references.
pub(super) fn load_config(path: &Path, host: &ConfigHost) -> Result<ParsedCommandLine, OpenError> {
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
pub(super) fn inferred_config(file: &Path, host: &ConfigHost) -> ParsedCommandLine {
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
pub(super) fn find_config(root: &Path, dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .take_while(|dir| is_within(root, dir))
        .find_map(|dir| {
            ["tsconfig.json", "jsconfig.json"]
                .iter()
                .map(|name| dir.join(name))
                .find(|path| path.is_file())
        })
}

/// Whether `dir` is `root` or below it, compared as the engine spells paths. `root` is canonical (on Windows verbatim: `\\?\D:\repo`), while a directory
/// the climb of `find_owner` rebuilds from a config's engine path is not (`D:/repo/sub`): `Path::starts_with` would call them different places and the climb
/// would find nothing above the nearest config.
pub(super) fn is_within(root: &Path, dir: &Path) -> bool {
    Path::new(&to_engine_path(dir)).starts_with(to_engine_path(root))
}

/// What the search for the config that owns a file found.
pub(super) enum Owner {
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
pub(super) fn find_owner(
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
pub(super) fn relative_to(root: &Path, engine_path: &str) -> String {
    let root = to_engine_path(root);
    engine_path
        .strip_prefix(root.as_str())
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(engine_path)
        .to_owned()
}

/// tsrs spells every path absolute, with `/` as the separator.
pub(super) fn to_engine_path(path: &Path) -> String {
    tspath::normalize_slashes(&without_verbatim_prefix(&path.to_string_lossy()))
}

/// On Windows `canonicalize` answers with a verbatim path (`\\?\D:\repo`, `\\?\UNC\server\share`), which tsrs cannot read: with `/` it becomes
/// `//?/D:/repo`, a path on a server named `?`. This gives the ordinary spelling back (`D:\repo`, `\\server\share`). Any other path is unchanged.
pub(super) fn without_verbatim_prefix(path: &str) -> String {
    match path.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(share) => format!(r"\\{share}"),
            None => rest.to_owned(),
        },
        None => path.to_owned(),
    }
}
