//! S-RS1 / S-RS2 spike: load a Cargo workspace, walk every call of every project file.
use std::{path::Path, time::Instant};

use ra_ap_hir::Semantics;
use ra_ap_ide_db::RootDatabase;
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice, load_workspace_at};
use ra_ap_project_model::{CargoConfig, RustLibSource};
use ra_ap_syntax::{AstNode, ast};
use ra_ap_vfs::Vfs;

pub fn load(root: &Path, sysroot: Option<RustLibSource>) -> anyhow_like::Result<(RootDatabase, Vfs)> {
    let sysroot = if std::env::var_os("NO_SYSROOT").is_some() { None } else { sysroot };
    let mut cargo_config = CargoConfig { sysroot, set_test: std::env::var_os("SET_TEST").is_some(), ..Default::default() };
    if let Some(p) = std::env::var_os("LOCKFILE_VIA_EXTRA_ENV") {
        cargo_config.extra_env.insert("CARGO_RESOLVER_LOCKFILE_PATH".into(), Some(p.to_string_lossy().into_owned()));
    }
    let load_config = LoadCargoConfig {
        load_out_dirs_from_check: false,
        with_proc_macro_server: ProcMacroServerChoice::None,
        prefill_caches: false,
        num_worker_threads: 1,
        proc_macro_processes: 0,
    };
    let t = Instant::now();
    let (db, vfs, _proc) = load_workspace_at(root, &cargo_config, &load_config, &|m| eprintln!("progress: {m}"))
        .map_err(|e| anyhow_like::Error(format!("{e:#}")))?;
    eprintln!("loaded in {:?}", t.elapsed());
    Ok((db, vfs))
}

pub mod anyhow_like {
    #[derive(Debug)]
    pub struct Error(pub String);
    pub type Result<T> = std::result::Result<T, Error>;
}

pub struct Counts {
    pub files: usize,
    pub calls: usize,
    pub call_resolved: usize,
    pub method_calls: usize,
    pub method_resolved: usize,
}

pub fn walk(db: &RootDatabase, vfs: &Vfs, under: &Path) -> Counts {
    let sema = Semantics::new(db);
    let mut c = Counts { files: 0, calls: 0, call_resolved: 0, method_calls: 0, method_resolved: 0 };
    for (file_id, path) in vfs.iter() {
        let Some(p) = path.as_path() else { continue };
        let p: &Path = p.as_ref();
        if !p.starts_with(under) || p.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let Some(efid) = sema.attach_first_edition_opt(file_id) else { continue };
        c.files += 1;
        let file = sema.parse(efid);
        for node in file.syntax().descendants() {
            if let Some(call) = ast::CallExpr::cast(node.clone()) {
                c.calls += 1;
                if let Some(callee) = call.expr()
                    && sema.resolve_expr_as_callable(&callee).is_some()
                {
                    c.call_resolved += 1;
                } else if std::env::var_os("VERBOSE").is_some() {
                    eprintln!("UNRESOLVED call {}: {}", p.display(), call.syntax().text().to_string().lines().next().unwrap_or(""));
                }
            } else if let Some(m) = ast::MethodCallExpr::cast(node) {
                c.method_calls += 1;
                if sema.resolve_method_call_fallback(&m).is_some() {
                    c.method_resolved += 1;
                } else if std::env::var_os("VERBOSE").is_some() {
                    eprintln!("UNRESOLVED method {}: {}", p.display(), m.syntax().text().to_string().lines().next().unwrap_or(""));
                }
            }
        }
    }
    c
}
