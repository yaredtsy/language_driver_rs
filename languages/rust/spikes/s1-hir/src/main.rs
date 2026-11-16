use std::path::PathBuf;

fn rss_mb() -> u64 {
    let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse::<u64>().unwrap_or(0) / 1024
}

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("root"));
    let abs = std::fs::canonicalize(&root).unwrap();
    let (db, vfs) = s1_hir::load(&root, Some(ra_ap_project_model::RustLibSource::Discover)).expect("load");
    eprintln!("rss after load: {} MB", rss_mb());
    for round in ["first", "second (a new Semantics on the same database)"] {
        let t = std::time::Instant::now();
        let c = ra_ap_hir::attach_db(&db, || s1_hir::walk(&db, &vfs, &abs));
        eprintln!("{round} walk: {:?}, rss {} MB", t.elapsed(), rss_mb());
        println!("files {} calls {}/{} method_calls {}/{}", c.files, c.call_resolved, c.calls, c.method_resolved, c.method_calls);
    }
    println!("vfs files: {}", vfs.iter().count());
}
