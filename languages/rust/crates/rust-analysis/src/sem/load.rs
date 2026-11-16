//! Loading a Cargo workspace into the engine's database. The only file that names `ra_ap_load_cargo`, `ra_ap_project_model` and `ra_ap_vfs`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::{LoadOptions, paths, toolchain};

use ra_ap_ide_db::RootDatabase;
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice, load_workspace};
use ra_ap_project_model::{
    CargoConfig, ProjectManifest, ProjectWorkspace, ProjectWorkspaceKind, RustLibSource,
    TargetDirectoryConfig, TargetKind,
};
use ra_ap_vfs::{AbsPathBuf, Vfs, VfsPath};

/// A loaded workspace: the engine's database and the table of its files. It owns both, so dropping it frees everything.
pub(crate) struct Loaded {
    db: RootDatabase,
    vfs: Vfs,
    /// The project had no `Cargo.lock` and has one now: cargo wrote it into the user's folder (the toolchain does not honour the redirect, §5 option A).
    #[expect(
        dead_code,
        reason = "becomes the LockfileWritten warning in RS3 step 5"
    )]
    wrote_lockfile: bool,
    /// The caller's root (absolute, clean): `files()` lists what is under it and nothing else, wherever the workspace's own root manifest is.
    root: PathBuf,
    /// Why the build scripts did not run (or ran with an error), when they were asked for: the message of the engine's `cargo check`.
    #[expect(dead_code, reason = "becomes a warning in RS3 step 5")]
    build_error: Option<String>,
    /// What this load left out, by name (RS3 §3). Only what was off and cut something.
    skipped: Vec<(&'static str, u64)>,
    /// Manifests (and the lock) that fed the load and sit outside the root, as engine text: no glob relative to the root covers them.
    outside_inputs: Vec<String>,
}

impl Loaded {
    /// The `.rs` files of the load that sit under the root, relative to it with `/`, sorted. The standard library and the registry's crates
    /// are loaded too, and are somewhere else on disk, so the root test is what leaves them out.
    pub(crate) fn files(&self) -> Vec<String> {
        let mut files: Vec<String> = self
            .vfs
            .iter()
            .filter_map(|(_, path)| {
                if path.as_path()?.extension()? != "rs" {
                    return None;
                }
                self.root_relative(path)
            })
            .collect();
        files.sort();
        files
    }

    /// The one place a path the engine reports becomes a path relative to the root, with `/`: `None` for a path that is not a file on disk
    /// (a virtual path), not under the root, or not UTF-8. Compared as engine text (`paths::relative`), never with `Path::starts_with`.
    pub(super) fn root_relative(&self, path: &VfsPath) -> Option<String> {
        paths::relative(&self.root, path.as_path()?.as_ref())
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
pub(crate) fn load(root: &Path, manifest: &Path, opts: &LoadOptions) -> Result<Loaded, String> {
    let dir = manifest
        .parent()
        .ok_or_else(|| format!("{}: has no folder", manifest.display()))?;
    let abs = AbsPathBuf::try_from(manifest.to_str().ok_or("the manifest path is not UTF-8")?)
        .map_err(|p| format!("{p}: not an absolute path"))?;
    let manifest_path = manifest;
    let manifest = ProjectManifest::from_manifest_file(abs).map_err(|e| format!("{e:#}"))?;

    let mut config = CargoConfig {
        // `RustLibSource::Discover` would run `rustup component add rust-src` when the source is missing (S-RS3); `Path` only looks.
        sysroot: sysroot_dir(dir).map(RustLibSource::Path),
        // without it `#[cfg(test)]` code is inactive and no call inside it resolves (S-RS2)
        set_test: true,
        ..CargoConfig::default()
    };

    let lock = workspace_dir(dir, manifest_path).join("Cargo.lock");
    let had_lock = lock.exists();
    // `redirect` is dropped (its folder deleted) when this function returns, after the last cargo has run, on every path out of it
    let redirect = if !had_lock && toolchain::honours_lockfile_path(toolchain::cargo_version(dir)) {
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

    let mut workspace =
        ProjectWorkspace::load(manifest, &config, &|_| {}).map_err(|e| format!("{e:#}"))?;

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
    // each proc macro the database knows holds the server's pool (an `Arc`), so the client may be dropped here
    let (db, vfs, _proc_macro_client) =
        load_workspace(workspace, &config.extra_env, &switches).map_err(|e| format!("{e:#}"))?;
    Ok(Loaded {
        db,
        vfs,
        wrote_lockfile: !had_lock && lock.exists(),
        root: root.to_path_buf(),
        build_error,
        skipped,
        outside_inputs,
    })
}

/// §3: what the switches that did not act cut, by name. `build_scripts_skipped`: the packages of the workspace that have a build script, when the
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
        build_scripts += u64::from(script && cargo[package].is_member);
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
fn workspace_dir(dir: &Path, manifest: &Path) -> PathBuf {
    let located = Command::new("cargo")
        .args([
            "locate-project",
            "--workspace",
            "--message-format",
            "plain",
            "--manifest-path",
        ])
        .arg(manifest)
        .current_dir(dir)
        .env("RUSTUP_AUTO_INSTALL", "0")
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

/// The toolchain's sysroot as `rustc --print sysroot` says in `dir` (so a `rust-toolchain.toml` there counts). `RUSTUP_AUTO_INSTALL=0`: a
/// toolchain that is named and not installed must fail here, not be downloaded.
fn sysroot_dir(dir: &Path) -> Option<AbsPathBuf> {
    let out = Command::new("rustc")
        .args(["--print", "sysroot"])
        .current_dir(dir)
        .env("RUSTUP_AUTO_INSTALL", "0")
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
