//! Loading a Cargo workspace into the engine's database. The only file that names `ra_ap_load_cargo`, `ra_ap_project_model` and `ra_ap_vfs`.

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use ra_ap_ide_db::RootDatabase;
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice, load_workspace};
use ra_ap_project_model::{CargoConfig, ProjectManifest, ProjectWorkspace, RustLibSource};
use ra_ap_vfs::{AbsPathBuf, Vfs};

/// A loaded workspace: the engine's database and the table of its files. It owns both, so dropping it frees everything.
pub(crate) struct Loaded {
    #[expect(dead_code, reason = "read by the walks of RS3 step 3 and RS4")]
    db: RootDatabase,
    vfs: Vfs,
    /// Absolute, spelled as the engine spells the paths it reports (`std::path::absolute`, never `canonicalize`: on Windows that adds a `\\?\` prefix).
    root: PathBuf,
}

impl Loaded {
    /// The `.rs` files of the load that sit under the root, relative to it with `/`, sorted. The standard library and the registry's crates
    /// are loaded too, and are somewhere else on disk, so the root test is what leaves them out.
    pub(crate) fn files(&self) -> Vec<String> {
        let mut files: Vec<String> = self
            .vfs
            .iter()
            .filter_map(|(_, path)| {
                let path: &Path = path.as_path()?.as_ref();
                if path.extension()? != "rs" {
                    return None;
                }
                relative(&self.root, path)
            })
            .collect();
        files.sort();
        files
    }
}

/// The one place a path becomes root-relative: `path` below `root`, components joined with `/`. `None` when it is not below, or not UTF-8.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let below = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = below.components().map(|c| c.as_os_str().to_str()).collect();
    Some(parts?.join("/"))
}

/// `path` without its `.` and `..` components, by text alone (no symlink is followed, which is how the engine spells the paths it reports).
/// `std::path::absolute` keeps `..`, and then no loaded file would start with the root.
fn clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Loads the Cargo workspace of `manifest` with std, `cfg(test)` on, and the three expensive switches off (build scripts, proc macros, cache
/// prefill). `Err` carries the engine's message; the ladder that turns most of these into a warning is RS3 step 7.
pub(crate) fn load(manifest: &Path) -> Result<Loaded, String> {
    let manifest = std::path::absolute(manifest)
        .map(|p| clean(&p))
        .map_err(|e| format!("{}: {e}", manifest.display()))?;
    let root = manifest
        .parent()
        .ok_or_else(|| format!("{}: has no folder", manifest.display()))?
        .to_path_buf();
    let abs = AbsPathBuf::try_from(manifest.to_str().ok_or("the manifest path is not UTF-8")?)
        .map_err(|p| format!("{p}: not an absolute path"))?;
    let manifest = ProjectManifest::from_manifest_file(abs).map_err(|e| format!("{e:#}"))?;

    let config = CargoConfig {
        // `RustLibSource::Discover` would run `rustup component add rust-src` when the source is missing (S-RS3); `Path` only looks.
        sysroot: sysroot_dir(&root).map(RustLibSource::Path),
        // without it `#[cfg(test)]` code is inactive and no call inside it resolves (S-RS2)
        set_test: true,
        ..CargoConfig::default()
    };
    let workspace =
        ProjectWorkspace::load(manifest, &config, &|_| {}).map_err(|e| format!("{e:#}"))?;
    let no_server = LoadCargoConfig {
        load_out_dirs_from_check: false,
        with_proc_macro_server: ProcMacroServerChoice::None,
        prefill_caches: false,
        num_worker_threads: 1,
        proc_macro_processes: 0,
    };
    let (db, vfs, _no_proc_macro_client) =
        load_workspace(workspace, &config.extra_env, &no_server).map_err(|e| format!("{e:#}"))?;
    Ok(Loaded { db, vfs, root })
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
