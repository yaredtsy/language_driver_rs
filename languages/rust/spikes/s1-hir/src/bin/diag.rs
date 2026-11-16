//! what does project_model say about the sysroot and the packages?
use ra_ap_project_model::{CargoConfig, ProjectManifest, ProjectWorkspace, ProjectWorkspaceKind, RustLibSource};
fn main() {
    let root = std::fs::canonicalize(std::env::args().nth(1).unwrap()).unwrap();
    let abs = ra_ap_vfs::AbsPathBuf::assert_utf8(root);
    let manifest = ProjectManifest::discover_single(&abs).expect("manifest");
    let mut cfg = CargoConfig { sysroot: if std::env::var_os("NO_SYSROOT").is_some() { None } else { Some(RustLibSource::Discover) }, set_test: true, ..Default::default() };
    if let Some(p) = std::env::var_os("LOCKFILE_VIA_EXTRA_ENV") {
        cfg.extra_env.insert("CARGO_RESOLVER_LOCKFILE_PATH".into(), Some(p.to_string_lossy().into_owned()));
    }
    let t = std::time::Instant::now();
    let ws = ProjectWorkspace::load(manifest, &cfg, &|m| eprintln!("progress: {m}")).expect("ws");
    eprintln!("ProjectWorkspace::load in {:?}", t.elapsed());
    let s = &ws.sysroot;
    println!("sysroot root: {:?}", s.root());
    println!("rust_lib_src_root: {:?}", s.rust_lib_src_root());
    println!("is_rust_lib_src_empty: {}", s.is_rust_lib_src_empty());
    println!("sysroot error: {:?}", s.error());
    println!("sysroot metadata_error: {:?}", s.metadata_error());
    println!("sysroot packages: {}", s.num_packages());
    println!("workspace packages: {}", ws.n_packages());
    if let ProjectWorkspaceKind::Cargo { error, .. } = &ws.kind {
        println!("cargo metadata error (retry with --no-deps worked): {:?}", error.as_ref().map(|e| format!("{e:#}")));
    }
}
