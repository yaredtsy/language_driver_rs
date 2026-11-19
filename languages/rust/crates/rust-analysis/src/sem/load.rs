//! Loading a Cargo workspace into the engine's database. The only file that names `ra_ap_load_cargo`, `ra_ap_project_model` and `ra_ap_vfs`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::manifest::{self, CrateSpec, DiskFs, Layout};
use crate::toolchain::Tool;
use crate::warning::Warning;
use crate::{LoadOptions, paths, toolchain};

use ra_ap_hir::{Semantics, attach_db};
use ra_ap_ide_db::RootDatabase;
use ra_ap_load_cargo::{
    LoadCargoConfig, ProcMacroServerChoice, ProjectFolders, SourceRootConfig, load_workspace,
};
use ra_ap_project_model::project_json::{ProjectJson, ProjectJsonData};
use ra_ap_project_model::{
    CargoConfig, ProjectManifest, ProjectWorkspace, ProjectWorkspaceKind, RustLibSource,
    TargetDirectoryConfig, TargetKind,
};
use ra_ap_vfs::loader::Entry;
use ra_ap_vfs::{AbsPathBuf, Vfs, VfsPath};

/// What the load was told to read, kept because `load_workspace` builds it and does not give it back: a later look at the disk (`refresh.rs`) needs both.
pub(super) struct Folders {
    /// Which file sets are the project's and which a dependency's or the sysroot's, and the roots a set of files is split into.
    pub(super) roots: SourceRootConfig,
    /// The folders the load listed, each with its extensions, the roots it includes and what it leaves out (`target/`).
    pub(super) listed: Vec<Entry>,
}

impl Folders {
    /// What the engine's own load makes of `workspace`. Built from a borrow, before `load_workspace` takes the workspace and from the same workspace it
    /// loads, so the two agree.
    fn of(workspace: &ProjectWorkspace) -> Folders {
        let folders = ProjectFolders::new(std::slice::from_ref(workspace), &[], None);
        Folders {
            roots: folders.source_root_config,
            listed: folders.load,
        }
    }
}

/// A loaded workspace: the engine's database and the table of its files. It owns both, so dropping it frees everything.
pub(crate) struct Loaded {
    db: RootDatabase,
    vfs: Vfs,
    folders: Folders,
    /// Cargo wrote the project's `Cargo.lock`: the project had none and has one now, or its lock changed (the toolchain does not honour the redirect,
    /// §5 option A).
    wrote_lockfile: bool,
    /// The toolchain (the name of its folder) whose standard library source is not installed: the load went on without `std` (S-RS3).
    no_std_source: Option<String>,
    /// Cargo's message when `cargo metadata` failed and the engine went on with `--no-deps` (offline, a dependency that cannot be fetched).
    dependencies_error: Option<String>,
    /// `ToolchainSubstituted` or `OldToolchain`, when the project's toolchain was not used as it was (`toolchain::decide`).
    toolchain_warning: Option<Warning>,
    /// The ladder (RS3 §6, D37): cargo was asked (rungs 1 and 2), or this crate's own reader built the crate graph (rung 3) or the file stands alone
    /// (rung 4).
    rung: Rung,
    /// No `cargo` at all (rungs 3 and 4).
    no_toolchain: bool,
    /// Cargo is there and could not load the project, and our reader did (rung 3): why, in one line.
    cargo_failed: Option<String>,
    /// No `Cargo.toml` up to the root (rung 4).
    no_manifest: bool,
    /// The reason a `Cargo.toml` could not be read by cargo or by the reader (rung 4).
    manifest_unreadable: Option<String>,
    /// The caller's root (absolute, clean): `files()` lists what is under it and nothing else, wherever the workspace's own root manifest is.
    root: PathBuf,
    /// Why the build scripts did not run (or ran with an error), when they were asked for: the message of the engine's `cargo check`.
    build_error: Option<String>,
    /// What this load left out, by name (RS3 §3). Only what was off and cut something.
    skipped: Vec<(&'static str, u64)>,
    /// Manifests (and the lock) that fed the load and sit outside the root, as engine text: no glob relative to the root covers them.
    outside_inputs: Vec<String>,
    /// Declared last: dropped after the database (fields drop in order), so a count of one means one database.
    #[cfg(test)]
    _alive: alive::Alive,
}

/// How many loads are alive on this thread, how many were ever made, and the most that were alive at once: a test that says "the old database is dropped
/// before the new one is built" reads the peak. Counted in a field of `Loaded`, whose `Drop` runs when the load (and its database) is freed.
#[cfg(test)]
pub(crate) mod alive {
    use std::cell::Cell;

    thread_local! {
        pub(crate) static ALIVE: Cell<u32> = const { Cell::new(0) };
        pub(crate) static PEAK: Cell<u32> = const { Cell::new(0) };
        pub(crate) static MADE: Cell<u32> = const { Cell::new(0) };
    }

    pub(super) struct Alive;

    impl Alive {
        pub(super) fn new() -> Alive {
            MADE.with(|n| n.set(n.get() + 1));
            let now = ALIVE.with(|n| {
                n.set(n.get() + 1);
                n.get()
            });
            PEAK.with(|peak| peak.set(peak.get().max(now)));
            Alive
        }
    }

    impl Drop for Alive {
        fn drop(&mut self) {
            ALIVE.with(|n| n.set(n.get() - 1));
        }
    }
}

/// How far down the ladder a load went (RS3 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rung {
    /// `cargo metadata` answered, with or without dependencies (rungs 1 and 2).
    Cargo,
    /// Our reader built the crate graph from `Cargo.toml` (rung 3).
    Reader,
    /// The file alone (rung 4).
    Lone,
}

impl Loaded {
    /// The engine's database and files for a load that has nothing to say yet: every warning fact is empty. The caller fills what it knows.
    fn from_engine(
        db: RootDatabase,
        vfs: Vfs,
        folders: Folders,
        root: &Path,
        rung: Rung,
    ) -> Loaded {
        Loaded {
            db,
            vfs,
            folders,
            wrote_lockfile: false,
            no_std_source: None,
            dependencies_error: None,
            toolchain_warning: None,
            rung,
            no_toolchain: false,
            cargo_failed: None,
            no_manifest: false,
            manifest_unreadable: None,
            root: root.to_path_buf(),
            build_error: None,
            skipped: Vec::new(),
            outside_inputs: Vec::new(),
            #[cfg(test)]
            _alive: alive::Alive::new(),
        }
    }

    pub(crate) fn rung(&self) -> Rung {
        self.rung
    }

    pub(crate) fn no_toolchain(&self) -> bool {
        self.no_toolchain
    }

    pub(crate) fn cargo_failed(&self) -> Option<&str> {
        self.cargo_failed.as_deref()
    }

    pub(crate) fn no_manifest(&self) -> bool {
        self.no_manifest
    }

    pub(crate) fn manifest_unreadable(&self) -> Option<&str> {
        self.manifest_unreadable.as_deref()
    }

    /// The `.rs` files of the load that sit under the root and belong to some crate's module tree, relative to the root with `/`, sorted. The standard
    /// library and the registry's crates are loaded too, and are somewhere else on disk, so the root test is what leaves them out; a file next to a
    /// crate that no `mod` reaches (and no `#[path]` or `include!`) is left out because the engine has no module for it.
    pub(crate) fn files(&self) -> Vec<String> {
        // the engine's own answer to "which crate is this file in": a file that is no module of any crate (a stray `.rs` next to one, a fixture) has none
        attach_db(&self.db, || {
            let sema = Semantics::new(&self.db);
            let mut files: Vec<String> = self
                .vfs
                .iter()
                .filter_map(|(id, path)| {
                    if path.as_path()?.extension()? != "rs" {
                        return None;
                    }
                    let relative = self.root_relative(path)?;
                    sema.file_to_module_def(id)?;
                    Some(relative)
                })
                .collect();
            files.sort();
            files
        })
    }

    /// The one place a path the engine reports becomes a path relative to the root, with `/`: `None` for a path that is not a file on disk
    /// (a virtual path), not under the root, or not UTF-8. Compared as engine text (`paths::relative`), never with `Path::starts_with`.
    pub(super) fn root_relative(&self, path: &VfsPath) -> Option<String> {
        paths::relative(&self.root, path.as_path()?.as_ref())
    }

    pub(crate) fn wrote_lockfile(&self) -> bool {
        self.wrote_lockfile
    }

    pub(crate) fn no_std_source(&self) -> Option<&str> {
        self.no_std_source.as_deref()
    }

    pub(crate) fn dependencies_error(&self) -> Option<&str> {
        self.dependencies_error.as_deref()
    }

    pub(crate) fn toolchain_warning(&self) -> Option<&Warning> {
        self.toolchain_warning.as_ref()
    }

    pub(crate) fn build_error(&self) -> Option<&str> {
        self.build_error.as_deref()
    }

    pub(crate) fn skipped(&self) -> &[(&'static str, u64)] {
        &self.skipped
    }

    pub(crate) fn outside_inputs(&self) -> &[String] {
        &self.outside_inputs
    }

    pub(super) fn db(&self) -> &RootDatabase {
        &self.db
    }

    pub(super) fn vfs(&self) -> &Vfs {
        &self.vfs
    }

    pub(super) fn folders(&self) -> &Folders {
        &self.folders
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    /// The engine's database and files to write to, with what the load was told to read: what `refresh.rs` needs to follow the disk.
    pub(super) fn follow(&mut self) -> (&mut RootDatabase, &mut Vfs, &Folders, &Path) {
        (&mut self.db, &mut self.vfs, &self.folders, &self.root)
    }
}

/// Why `load_cargo` did not give a `Loaded`.
enum Failed {
    /// Cargo could not load the workspace: it is not there, or it cannot read the manifest. The ladder goes on (RS3 §6).
    Cargo {
        /// Cargo's reason, in one line (`cargo_reason`).
        reason: String,
        cargo_present: bool,
        chosen: Option<String>,
    },
    /// Something that no rung can get past: a path the engine cannot be given, or the engine's own load failing.
    Cannot(String),
}

impl From<String> for Failed {
    fn from(message: String) -> Failed {
        Failed::Cannot(message)
    }
}

impl From<&str> for Failed {
    fn from(message: &str) -> Failed {
        Failed::Cannot(message.to_owned())
    }
}

/// The edition of a file that has no manifest to say: the latest, as `rust-syntax` falls back to (`Edition::LATEST`). `rustc` itself would say 2015.
const LONE_EDITION: &str = "2024";

/// Loads what is at `file`, down the ladder of RS3 §6 (D37). `manifest` is the nearest `Cargo.toml` between the file and the root, if there is one.
///
/// 1. and 2. cargo is asked (`load_cargo`, with or without dependencies);
/// 3. cargo is missing, or cannot read the manifest: our own reader builds the crate graph from the `Cargo.toml` files (`NoToolchain` when there is no
///    cargo), and there is no standard library;
/// 4. no manifest, or none that either can read: the file alone, as a one-file crate with its `mod` children (`NoManifest`, `ManifestUnreadable`).
///
/// `Err` is only for what no rung can get past (see `Failed::Cannot`).
pub(crate) fn load(
    root: &Path,
    manifest: Option<&Path>,
    file: &Path,
    opts: &LoadOptions,
) -> Result<Loaded, String> {
    let Some(manifest) = manifest else {
        let mut loaded = load_lone(root, file, None)?;
        loaded.no_manifest = true;
        return Ok(loaded);
    };
    match load_cargo(root, manifest, opts) {
        Ok(loaded) => Ok(loaded),
        Err(Failed::Cannot(message)) => Err(message),
        Err(Failed::Cargo {
            reason,
            cargo_present,
            chosen,
        }) => {
            let chosen = chosen.as_deref();
            match manifest::read(&DiskFs, manifest) {
                Ok(layout) => {
                    let mut loaded = load_layout(root, layout, chosen)?;
                    // no cargo is `NoToolchain`; a cargo that is there and failed is `CargoFailed`: the load is the reader's either way
                    loaded.no_toolchain = !cargo_present;
                    loaded.cargo_failed = cargo_present.then_some(reason);
                    Ok(loaded)
                }
                Err(reason) => {
                    let mut loaded = load_lone(root, file, chosen)?;
                    loaded.manifest_unreadable = Some(reason);
                    loaded.no_toolchain = !cargo_present;
                    Ok(loaded)
                }
            }
        }
    }
}

/// The engine's error for a workspace cargo could not load, as the one line a warning can carry: what cargo itself said (after "`cargo metadata` exited
/// with an error:"), first non-empty line; or, when cargo never ran, the engine's own first line.
fn cargo_reason(error: &str) -> String {
    let said = error
        .split_once("exited with an error:")
        .map_or(error, |(_, said)| said);
    said.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("cargo failed")
        .to_owned()
}

/// Rung 3: the crate graph our reader made, handed to the engine as a project description. No sysroot: the engine asks `rustc` for the target's cfg
/// and data layout, and falls back to nothing when it cannot (ENGINE.md, "Rung 3").
fn load_layout(root: &Path, layout: Layout, toolchain: Option<&str>) -> Result<Loaded, String> {
    let mut manifests = layout.manifests.clone();
    manifests.push(layout.workspace_dir.join("Cargo.lock"));
    let mut loaded = load_json(root, &layout.crates, Rung::Reader, toolchain)?;
    loaded.skipped = if layout.skipped > 0 {
        vec![("layout_skipped", layout.skipped)]
    } else {
        Vec::new()
    };
    loaded.outside_inputs = outside_of(root, manifests);
    Ok(loaded)
}

/// Rung 4: `file` is the root of a crate of its own, and its `mod` children come with it (they are files below its folder).
fn load_lone(root: &Path, file: &Path, toolchain: Option<&str>) -> Result<Loaded, String> {
    let dir = file
        .parent()
        .ok_or_else(|| format!("{}: has no folder", file.display()))?;
    let stem = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or("the file name is not UTF-8")?;
    let spec = CrateSpec {
        name: stem.replace('-', "_"),
        root_module: file.to_path_buf(),
        edition: LONE_EDITION,
        deps: Vec::new(),
        package_dir: dir.to_path_buf(),
        cfg: manifest::host_cfgs(),
        is_proc_macro: false,
    };
    load_json(root, &[spec], Rung::Lone, toolchain)
}

fn load_json(
    root: &Path,
    crates: &[CrateSpec],
    rung: Rung,
    toolchain: Option<&str>,
) -> Result<Loaded, String> {
    let utf8 = |path: &Path| {
        path.to_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("{}: not UTF-8", path.display()))
    };
    let mut described = Vec::new();
    for spec in crates {
        let deps: Vec<_> = spec
            .deps
            .iter()
            .map(|(name, at)| serde_json::json!({ "crate": at, "name": name }))
            .collect();
        described.push(serde_json::json!({
            "display_name": spec.name,
            "root_module": utf8(&spec.root_module)?,
            "edition": spec.edition,
            "deps": deps,
            // every package the reader loads is the user's own code (a member, or a path dependency): §4's rule on the file decides what is under the root
            "is_workspace_member": true,
            "cfg": spec.cfg,
            "env": {},
            "is_proc_macro": spec.is_proc_macro,
            // the whole package: a `#[path]` module outside `src/` is otherwise not loaded
            "source": { "include_dirs": [utf8(&spec.package_dir)?], "exclude_dirs": [utf8(&spec.package_dir.join("target"))?] },
        }));
    }
    let data: ProjectJsonData = serde_json::from_value(serde_json::json!({ "crates": described }))
        .map_err(|e| format!("the crate graph: {e}"))?;
    let base = AbsPathBuf::try_from(root.to_str().ok_or("the root is not UTF-8")?)
        .map_err(|p| format!("{p}: not an absolute path"))?;
    let project = ProjectJson::new(None, &base, data);

    let mut config = CargoConfig {
        sysroot: None,
        set_test: true,
        ..CargoConfig::default()
    };
    if let Some(toolchain) = toolchain {
        config
            .extra_env
            .insert("RUSTUP_TOOLCHAIN".to_owned(), Some(toolchain.to_owned()));
    }
    let workspace = ProjectWorkspace::load_inline(project, &config, &|_| {});
    let switches = LoadCargoConfig {
        load_out_dirs_from_check: false,
        with_proc_macro_server: ProcMacroServerChoice::None,
        prefill_caches: false,
        num_worker_threads: 1,
        proc_macro_processes: 0,
    };
    let folders = Folders::of(&workspace);
    let (db, vfs, _no_client) =
        load_workspace(workspace, &config.extra_env, &switches).map_err(|e| format!("{e:#}"))?;
    Ok(Loaded::from_engine(db, vfs, folders, root, rung))
}

/// Loads the Cargo workspace of `manifest` (absolute, clean, inside `root`) with std and `cfg(test)` on. Build scripts, proc macros and cache
/// prefill are off unless `opts` turns the first two on (§3); what is off and cuts something is counted in `Loaded::skipped`. `Err` carries the engine's message; the ladder that turns most of these into a warning is RS3 step 7.
///
/// Cargo finds the workspace root, which may be above `root`: it is loaded because cargo needs all of it for one member, and `root` stays the limit
/// of what is the user's project (`Loaded::files`).
///
/// A load must not write into the project (RS3 §5, D38). With no `Cargo.lock`, cargo would write one, so on a toolchain that honours it
/// (`toolchain::honours_lockfile_path`) cargo is told to write it into a folder that is deleted before this returns. On an older toolchain the
/// load goes on and `Loaded::wrote_lockfile` says what happened.
fn load_cargo(root: &Path, manifest: &Path, opts: &LoadOptions) -> Result<Loaded, Failed> {
    let dir = manifest
        .parent()
        .ok_or_else(|| format!("{}: has no folder", manifest.display()))?;
    let abs = AbsPathBuf::try_from(manifest.to_str().ok_or("the manifest path is not UTF-8")?)
        .map_err(|p| format!("{p}: not an absolute path"))?;
    let manifest_path = manifest;
    let manifest = ProjectManifest::from_manifest_file(abs).map_err(|e| format!("{e:#}"))?;

    // The toolchain of the project may be unusable as it is (not installed) or too old (RS3 §6): the choice is made once, and what it picks is put in
    // `RUSTUP_TOOLCHAIN` for every tool below, ours and the engine's. Nothing is ever installed.
    let decision = toolchain::decide(toolchain::probe(dir, None), toolchain::MINIMUM, || {
        toolchain::installed(dir)
    });
    let chosen = decision.toolchain.as_deref();

    let sysroot = sysroot_dir(dir, chosen);
    let mut config = CargoConfig {
        // `RustLibSource::Discover` would run `rustup component add rust-src` when the source is missing (S-RS3); `Path` only looks.
        sysroot: sysroot.clone().map(RustLibSource::Path),
        // without it `#[cfg(test)]` code is inactive and no call inside it resolves (S-RS2)
        set_test: true,
        ..CargoConfig::default()
    };
    if let Some(toolchain) = chosen {
        // the engine leaves a `RUSTUP_TOOLCHAIN` of `extra_env` alone (`Sysroot::tool`)
        config
            .extra_env
            .insert("RUSTUP_TOOLCHAIN".to_owned(), Some(toolchain.to_owned()));
    }

    let lock = workspace_dir(dir, manifest_path, chosen).join("Cargo.lock");
    // the lock's bytes before the load: a toolchain that ignores the redirect (below 1.97) writes a missing lock, and one below 1.82 also rewrites a
    // stale one in place
    let lock_before = std::fs::read(&lock).ok();
    let had_lock = lock_before.is_some();
    // `redirect` is dropped (its folder deleted) when this function returns, after the last cargo has run, on every path out of it
    let redirect =
        if !had_lock && toolchain::honours_lockfile_path(toolchain::cargo_version(dir, chosen)) {
            TempDir::new()
        } else {
            None
        };
    // a temporary folder whose path is not UTF-8 cannot be written into the (String) environment: go on as A, the folder is deleted at the end
    if let Some(path) = redirect.as_ref().and_then(|dir| lock_in(&dir.0)) {
        config
            .extra_env
            .insert("CARGO_RESOLVER_LOCKFILE_PATH".to_owned(), Some(path));
    }

    let mut workspace = match ProjectWorkspace::load(manifest, &config, &|_| {}) {
        Ok(workspace) => workspace,
        // cargo could not be asked, or cannot read the manifest, and the `--no-deps` retry failed too: the next rung may still load it
        Err(error) => {
            return Err(Failed::Cargo {
                reason: cargo_reason(&format!("{error:#}")),
                cargo_present: toolchain::cargo_present(dir, chosen),
                chosen: chosen.map(str::to_owned),
            });
        }
    };

    // What the engine knows and does not say: the standard library source missing (`load_workspace_at` hides it) and a `cargo metadata` that failed
    // and was retried without dependencies (S-RS3). Both are read before the load goes on.
    let no_std_source = no_std_toolchain(
        sysroot.as_ref().map(AsRef::<Path>::as_ref),
        workspace.sysroot.error().is_some(),
    );
    let dependencies_error = match &workspace.kind {
        ProjectWorkspaceKind::Cargo {
            error: Some(error), ..
        } => Some(format!("{error:#}")),
        _ => None,
    };

    // A proc macro is a dylib that the build step (`cargo check`) builds, so asking for proc macros runs it too. It runs inside the lock
    // redirect above, and into `target/rust-analyzer`, not into the user's own `target/` build.
    //
    // A build that fails (a `build.rs` that panics, or one cargo cannot start) is not a failed load: the project loads as if the scripts had not
    // run, they are counted as skipped, and the message is kept for step 5's warning. A run that reports an error still has what it built, and
    // that is used.
    let mut build_error = None;
    let build_scripts_ran = if opts.build_scripts || opts.proc_macros {
        config.target_dir_config = TargetDirectoryConfig::UseSubdirectory;
        match workspace.run_build_scripts(&config, &|_| {}) {
            Ok(scripts) => {
                build_error = scripts.error().map(str::to_owned);
                workspace.set_build_scripts(scripts);
            }
            Err(error) => build_error = Some(format!("{error:#}")),
        }
        build_error.is_none()
    } else {
        false
    };
    // a proc macro needs the dylib that the build made: a build that did not succeed leaves the server with nothing to expand
    let proc_macros_ran = opts.proc_macros && build_scripts_ran;
    let skipped = skipped(&workspace, build_scripts_ran, proc_macros_ran);
    let outside_inputs = outside_inputs(&workspace, root);

    let switches = LoadCargoConfig {
        load_out_dirs_from_check: build_scripts_ran,
        with_proc_macro_server: if proc_macros_ran {
            ProcMacroServerChoice::Sysroot
        } else {
            ProcMacroServerChoice::None
        },
        prefill_caches: false,
        num_worker_threads: 1,
        // a pool of none has no process to ask
        proc_macro_processes: usize::from(proc_macros_ran),
    };
    // after the build scripts: their output folders are roots too
    let folders = Folders::of(&workspace);
    // each proc macro the database knows holds the server's pool (an `Arc`), so the client may be dropped here
    let (db, vfs, _proc_macro_client) =
        load_workspace(workspace, &config.extra_env, &switches).map_err(|e| format!("{e:#}"))?;
    let mut loaded = Loaded::from_engine(db, vfs, folders, root, Rung::Cargo);
    loaded.wrote_lockfile =
        lock_written(lock_before.as_deref(), std::fs::read(&lock).ok().as_deref());
    loaded.no_std_source = no_std_source;
    loaded.dependencies_error = dependencies_error;
    loaded.toolchain_warning = decision.warning;
    loaded.build_error = build_error;
    loaded.skipped = skipped;
    loaded.outside_inputs = outside_inputs;
    Ok(loaded)
}

/// §3: what the switches that did not act cut, by name. `build_scripts_skipped`: the local packages (members and path dependencies: project code, §4) that have a build script, when the
/// scripts did not run (off, or the run failed). `proc_macros_skipped`: 1 when any crate of the dependency graph is a proc macro and the server did
/// not run (off, or the build that makes its dylib failed). A switch that is
/// on, or that cuts nothing, is not listed.
fn skipped(
    workspace: &ProjectWorkspace,
    build_scripts_ran: bool,
    proc_macros_ran: bool,
) -> Vec<(&'static str, u64)> {
    let ProjectWorkspaceKind::Cargo { cargo, .. } = &workspace.kind else {
        return Vec::new();
    };
    let mut build_scripts = 0;
    let mut any_proc_macro = false;
    for package in cargo.packages() {
        let targets = cargo[package]
            .targets
            .iter()
            .map(|&target| cargo[target].kind);
        let (mut script, mut proc_macro) = (false, false);
        for kind in targets {
            script |= kind == TargetKind::BuildScript;
            proc_macro |= kind
                == TargetKind::Lib {
                    is_proc_macro: true,
                };
        }
        build_scripts += u64::from(script && cargo[package].is_local);
        any_proc_macro |= proc_macro;
    }
    let mut skipped = Vec::new();
    if !build_scripts_ran && build_scripts > 0 {
        skipped.push(("build_scripts_skipped", build_scripts));
    }
    if !proc_macros_ran && any_proc_macro {
        skipped.push(("proc_macros_skipped", 1));
    }
    skipped.sort_unstable_by_key(|(name, _)| *name);
    skipped
}

/// The manifests of the workspace's own packages (members and path dependencies), the workspace's root manifest and the lock beside it, that sit
/// outside `root`: engine text, absolute (`paths::engine_text`). Sorted, each once. Inside the root a glob covers them.
fn outside_inputs(workspace: &ProjectWorkspace, root: &Path) -> Vec<String> {
    let ProjectWorkspaceKind::Cargo { cargo, .. } = &workspace.kind else {
        return Vec::new();
    };
    let root_manifest: &Path = cargo.manifest_path().as_ref();
    let mut files: Vec<PathBuf> = vec![root_manifest.with_file_name("Cargo.lock")];
    files.push(root_manifest.to_path_buf());
    for package in cargo.packages().filter(|&p| cargo[p].is_local) {
        files.push(AsRef::<Path>::as_ref(&cargo[package].manifest).to_path_buf());
    }
    outside_of(root, files)
}

/// Of `files`, those that are files and sit outside `root`: as engine text, absolute, sorted, each once. Inside the root a glob covers them.
fn outside_of(root: &Path, files: Vec<PathBuf>) -> Vec<String> {
    let mut inputs: Vec<String> = files
        .into_iter()
        .filter(|file| paths::relative(root, file).is_none() && file.is_file())
        .filter_map(|file| paths::engine_text(&file))
        .collect();
    inputs.sort();
    inputs.dedup();
    inputs
}

/// The folder of the workspace's root manifest, where cargo keeps `Cargo.lock`: `cargo locate-project --workspace` says (a member's folder is
/// not it). Falls back to `dir`, the manifest's folder, when cargo cannot say.
fn workspace_dir(dir: &Path, manifest: &Path, toolchain: Option<&str>) -> PathBuf {
    let located = toolchain::command(Tool::Cargo, dir, toolchain)
        .args([
            "locate-project",
            "--workspace",
            "--message-format",
            "plain",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok());
    located
        .as_deref()
        .and_then(|text| Path::new(text.trim()).parent())
        .map_or_else(|| dir.to_path_buf(), Path::to_path_buf)
}

/// Where cargo is told to write the lock for a folder `dir`: `dir/Cargo.lock`, as the text of an environment value. `None` when the path is not UTF-8.
fn lock_in(dir: &Path) -> Option<String> {
    dir.join("Cargo.lock").to_str().map(str::to_owned)
}

/// An empty folder in the system's temporary folder, deleted when the value is dropped. (`Drop` is Rust's destructor: it runs when the owner goes
/// out of scope, however that happens, an early `return` or `?` included. One use does not justify the `tempfile` crate.)
struct TempDir(PathBuf);

impl TempDir {
    /// `None` when no folder could be made: the load then goes on without the redirect.
    fn new() -> Option<TempDir> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        (0..16).find_map(|_| {
            let name = format!(
                "rust-analysis-lock-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(name);
            // `create_dir` fails when the folder exists, so a folder of another process is never taken
            std::fs::create_dir(&path).ok().map(|()| TempDir(path))
        })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Did the load write the project's `Cargo.lock`? The bytes of the lock before and after (`None`: no file): a lock that appeared, or one that is
/// not what it was. A lock that is gone is not a write, and neither is no lock before and after. (Below 1.82 cargo rewrites a stale lock in place;
/// from 1.82 the engine works on a copy of an existing one.)
fn lock_written(before: Option<&[u8]>, after: Option<&[u8]>) -> bool {
    match (before, after) {
        (None, Some(_)) => true,
        (Some(before), Some(after)) => before != after,
        (_, None) => false,
    }
}

/// A load without `std` is never silent (RS3 §6): the toolchain to name in `NoStdSource`, when the standard library is not there. `sysroot` is
/// the toolchain's folder as `rustc --print sysroot` said, `None` when it said nothing (no `rustc`): the engine then has no sysroot at all, so
/// no `std`, and the toolchain is called `unknown`. With a sysroot, `std` is missing when the engine could not load its source.
fn no_std_toolchain(sysroot: Option<&Path>, source_missing: bool) -> Option<String> {
    match sysroot {
        None => Some("unknown".to_owned()),
        Some(dir) if source_missing => Some(
            dir.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown")
                .to_owned(),
        ),
        Some(_) => None,
    }
}

/// The toolchain's sysroot as `rustc --print sysroot` says in `dir` (so a `rust-toolchain.toml` there counts). `RUSTUP_AUTO_INSTALL=0`: a
/// toolchain that is named and not installed must fail here, not be downloaded.
fn sysroot_dir(dir: &Path, toolchain: Option<&str>) -> Option<AbsPathBuf> {
    let out = toolchain::command(Tool::Rustc, dir, toolchain)
        .args(["--print", "sysroot"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    AbsPathBuf::try_from(String::from_utf8(out.stdout).ok()?.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargos_reason_is_one_line() {
        let ran = "Failed to load the project at /w/Cargo.toml: Failed to read Cargo metadata from Cargo.toml file /w/Cargo.toml, None: Failed to run `cd \"/w\" && \"cargo\" \"metadata\"`: `cargo metadata` exited with an error: error: failed to parse manifest at `/w/Cargo.toml`\n\nCaused by:\n  dependency (foo) specified without providing a local path\n";
        assert_eq!(
            cargo_reason(ran),
            "error: failed to parse manifest at `/w/Cargo.toml`"
        );
        // cargo never ran: the engine's first line
        let never = "Failed to load the project at /w/Cargo.toml: Failed to run `cargo`: No such file or directory (os error 2)";
        assert!(cargo_reason(never).starts_with("Failed to load the project"));
        assert_eq!(cargo_reason(""), "cargo failed");
        assert_eq!(
            cargo_reason("exited with an error:\n\n  \n"),
            "cargo failed"
        );
    }

    #[test]
    fn a_lock_that_appeared_or_changed_is_a_write() {
        let old = Some(&b"version = 3\n"[..]);
        let new = Some(&b"version = 3\n[[package]]\n"[..]);
        assert!(lock_written(None, new), "a lock appeared");
        assert!(
            lock_written(old, new),
            "a stale lock was rewritten in place"
        );
        assert!(!lock_written(old, old), "a lock that is as it was");
        assert!(!lock_written(None, None), "no lock before and after");
        assert!(
            !lock_written(old, None),
            "a lock that is gone was not written"
        );
        assert!(
            !lock_written(Some(b""), Some(b"")),
            "an empty lock is a lock, and unchanged"
        );
    }

    #[test]
    fn a_load_without_std_names_its_toolchain_or_says_unknown() {
        let dir = Path::new("/opt/rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu");
        // a sysroot whose source is there: nothing to say
        assert_eq!(no_std_toolchain(Some(dir), false), None);
        // its source is missing: the folder's name is the toolchain
        assert_eq!(
            no_std_toolchain(Some(dir), true).as_deref(),
            Some("1.99.1-x86_64-unknown-linux-gnu")
        );
        // no sysroot at all (no rustc): no std either, whatever `source_missing` says, and no name to give
        assert_eq!(no_std_toolchain(None, false).as_deref(), Some("unknown"));
        assert_eq!(no_std_toolchain(None, true).as_deref(), Some("unknown"));
        // a sysroot path with no last component
        assert_eq!(
            no_std_toolchain(Some(Path::new("/")), true).as_deref(),
            Some("unknown")
        );
    }

    #[test]
    fn the_lock_goes_inside_the_folder() {
        let path = lock_in(Path::new("redirect")).expect("a UTF-8 path is text");
        assert_eq!(Path::new(&path), Path::new("redirect").join("Cargo.lock"));
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_is_not_utf8_is_no_redirect() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(lock_in(Path::new(OsStr::from_bytes(b"/tmp/\xff"))), None);
    }
}
