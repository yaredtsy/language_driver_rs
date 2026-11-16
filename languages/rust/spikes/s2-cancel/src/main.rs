//! S-RS2: how does a cancelled query report itself to a caller outside `ide`?
use std::{panic::AssertUnwindSafe, path::PathBuf, time::{Duration, Instant}};

use ra_ap_ide_db::base_db::salsa::{Cancelled, Database};

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).unwrap());
    let abs = std::fs::canonicalize(&root).unwrap();
    let mode = std::env::args().nth(2).unwrap_or_else(|| "token".into());
    let (mut db, vfs) = s1_hir::load(&root, Some(ra_ap_project_model::RustLibSource::Discover)).unwrap();

    let snap = db.clone();
    let token = snap.cancellation_token();
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let t = Instant::now();
        let r = Cancelled::catch(AssertUnwindSafe(|| {
            ra_ap_hir::attach_db(&snap, || s1_hir::walk(&snap, &vfs, &abs));
        }));
        tx.send(()).ok();
        // a second walk on the same handle after a cancellation
        let again = Cancelled::catch(AssertUnwindSafe(|| {
            ra_ap_hir::attach_db(&snap, || s1_hir::walk(&snap, &vfs, &abs).calls)
        }));
        (r.map_err(|c| format!("{c:?}")), t.elapsed(), again.map_err(|c| format!("{c:?}")))
    });

    std::thread::sleep(Duration::from_millis(400));
    let t = Instant::now();
    match mode.as_str() {
        "token" => token.cancel(),
        "write" => {
            // blocks until every other handle is dropped; readers see PendingWrite
            db.trigger_cancellation();
        }
        _ => {}
    }
    rx.recv_timeout(Duration::from_secs(60)).ok();
    println!("mode={mode}: first walk returned {:?} after the request", t.elapsed());
    let (first, total, again) = handle.join().unwrap();
    println!("first walk: {first:?} (thread total {total:?})");
    println!("second walk on the same handle: {again:?}");
}
