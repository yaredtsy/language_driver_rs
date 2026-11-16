use std::path::PathBuf;
fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("root"));
    let abs = std::fs::canonicalize(&root).unwrap();
    let (db, vfs) = s1_hir::load(&root, Some(ra_ap_project_model::RustLibSource::Discover)).expect("load");
    let c = ra_ap_hir::attach_db(&db, || s1_hir::walk(&db, &vfs, &abs));
    let host = ra_ap_ide::AnalysisHost::with_database(db);
    let analysis = host.analysis();
    // call the oracle on the first file, offset 0: forces linking outgoing_calls and its callees
    let (fid, _) = vfs.iter().next().unwrap();
    let cfg = ra_ap_ide::CallHierarchyConfig { exclude_tests: false, ra_fixture: ra_ap_ide::RaFixtureConfig::default() };
    let r = analysis.outgoing_calls(&cfg, ra_ap_ide::FilePosition { file_id: fid, offset: 0.into() });
    println!("files {} calls {}/{} oracle {:?}", c.files, c.call_resolved, c.calls, r.is_ok());
}
