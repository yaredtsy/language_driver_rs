//! Following the disk (RS5 §3): which `.rs` files of the project moved since the last look, and what to tell the engine's database about them. The fast
//! path: nothing is loaded again, and salsa recomputes only what read a file that changed.
//!
//! What is watched: every `.rs` file the `Vfs` holds that is not a library's (a dependency's or the sysroot's), and each folder that holds one, from the
//! file's folder up to its package root and never above the project root. A folder's modified time moves when an entry is created or deleted in it, so that is
//! how a new file is found: the folder is listed again, the way the load listed it (same extensions, same excludes).
//!
//! Not here yet (step 4): the manifests, locks and toolchain files, and a `.rs` file created or deleted where cargo finds a target by itself. Those change
//! the crate graph, and need a reload, not this.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use ra_ap_ide_db::ChangeWithProcMacros;
use ra_ap_vfs::loader::Entry;
use ra_ap_vfs::{AbsPath, AbsPathBuf, Change, VfsPath};

use super::load::{Folders, Loaded};
use super::stamps::{Stamp, Stamps};
use crate::paths;

impl Loaded {
    /// The stamps of what the load read, now: each non-library `.rs` file and the folders above it (see the module's doc). Taken after the load, so a
    /// file saved while the load ran is not seen until it is saved again.
    pub(crate) fn stamp(&self) -> Stamps {
        let folders = self.folders();
        let includes = includes(folders);
        let mut stamps = Stamps::default();
        for (_, path) in self.vfs().iter() {
            let Some(file) = path.as_path() else { continue };
            if file.extension() != Some("rs") || folders.roots.path_is_library(path) {
                continue;
            }
            let Some(text) = paths::engine_text(file.as_ref()) else {
                continue;
            };
            stamps.add(&text);
            let Some(dir) = file.parent() else { continue };
            watch_folders(&mut stamps, dir.as_ref(), &includes, self.root());
        }
        stamps
    }

    /// Looks at the disk and tells the database what changed, in one transaction. `known` is what the last look saw; `None` when nothing moved (nothing
    /// is read), else the stamps for the next look.
    ///
    /// A saved file is read and given to the `Vfs`; a created or deleted one also splits the files into source roots again (the module tree looks a `mod`
    /// up in the roots); a file that is not UTF-8 has no text, as after the load. **Stamp first, read second**: every stamp here is taken before the
    /// bytes it describes are read, so a file saved in between has a newer stamp than the one kept, and the next look reads it again.
    pub(crate) fn follow_disk(&mut self, known: &Stamps) -> Option<Stamps> {
        let mut now = known.restamped();
        let moved: Vec<&str> = known
            .iter()
            .filter(|&(path, stamp)| now.get(path) != Some(stamp))
            .map(|(path, _)| path)
            .collect();
        if moved.is_empty() {
            return None;
        }

        let (db, vfs, folders, _) = self.follow();
        let mut to_read: BTreeSet<String> = BTreeSet::new();
        let mut to_list: Vec<String> = Vec::new();
        for path in moved {
            if known.get(path).is_some_and(Stamp::is_dir) {
                to_list.push(path.to_owned());
            } else {
                to_read.insert(path.to_owned());
            }
        }

        // A folder that changed is listed again: a file that is new is stamped and read, and a folder that is new is stamped and listed in turn. A
        // folder already watched is left to its own stamp.
        let mut visited: HashSet<PathBuf> = HashSet::new();
        while let Some(dir) = to_list.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(text) = paths::engine_text(&path) else {
                    continue;
                };
                let Ok(abs) = AbsPathBuf::try_from(text.as_str()) else {
                    continue;
                };
                // follows a symlink, as the load's walk does
                let Ok(meta) = std::fs::metadata(&path) else {
                    continue;
                };
                if now.contains(&text) {
                    continue;
                }
                if meta.is_dir() {
                    let cycle =
                        std::fs::canonicalize(&path).is_ok_and(|real| !visited.insert(real));
                    if !cycle && listed(folders, &abs, true) {
                        now.add(&text);
                        to_list.push(text);
                    }
                } else if meta.is_file()
                    && abs.extension() == Some("rs")
                    && listed(folders, &abs, false)
                {
                    now.add(&text);
                    to_read.insert(text);
                }
            }
        }

        for path in &to_read {
            let Ok(abs) = AbsPathBuf::try_from(path.as_str()) else {
                continue;
            };
            // a file that cannot be read is gone, as the load reads it
            let bytes = read(path);
            vfs.set_file_contents(VfsPath::from(abs), bytes);
        }

        #[cfg(test)]
        run_after_read();

        let mut change = ChangeWithProcMacros::default();
        let mut any = false;
        let mut created_or_deleted = false;
        for (id, file) in vfs.take_changes() {
            any = true;
            let text = match file.change {
                Change::Create(bytes, _) => {
                    created_or_deleted = true;
                    String::from_utf8(bytes).ok()
                }
                Change::Modify(bytes, _) => String::from_utf8(bytes).ok(),
                Change::Delete => {
                    created_or_deleted = true;
                    None
                }
            };
            change.change_file(id, text);
        }
        // a file touched and not changed is not a change: the `Vfs` compares the bytes
        if any {
            if created_or_deleted {
                change.set_roots(folders.roots.partition(vfs));
            }
            #[cfg(test)]
            APPLIES.with(|n| n.set(n.get() + 1));
            db.apply_change(change);
        }

        now.forget_missing();
        Some(now)
    }
}

/// The folders the load listed, each include root as engine text: the package roots.
fn includes(folders: &Folders) -> Vec<String> {
    folders
        .listed
        .iter()
        .filter_map(|entry| match entry {
            Entry::Directories(dirs) => Some(dirs),
            Entry::Files(_) => None,
        })
        .flat_map(|dirs| dirs.include.iter())
        .filter_map(|dir| paths::engine_text(dir.as_ref()))
        .collect()
}

/// Stamps `dir` and the folders above it, up to the package root (the deepest include root that holds `dir`) and no higher than the project root: for a
/// folder inside the root, the nearer of the two; for one outside it (a workspace member above, a path dependency beside) the package root, and with
/// none, the folder alone. A folder already stamped has its folders above it stamped too, so the climb stops there.
fn watch_folders(stamps: &mut Stamps, dir: &Path, includes: &[String], root: &Path) {
    let Some(text) = paths::engine_text(dir) else {
        return;
    };
    if stamps.contains(&text) {
        return;
    }
    let package = includes
        .iter()
        .filter(|include| paths::relative(Path::new(include.as_str()), dir).is_some())
        .max_by_key(|include| Path::new(include.as_str()).components().count())
        .map(|include| Path::new(include.as_str()));
    let under_root = paths::relative(root, dir).is_some();
    let limit = match package {
        Some(package) if !under_root || paths::relative(root, package).is_some() => Some(package),
        _ if under_root => Some(root),
        _ => None,
    };
    let Some(limit) = limit else {
        stamps.add(&text);
        return;
    };
    for above in dir.ancestors() {
        if paths::relative(limit, above).is_none() {
            break;
        }
        let Some(text) = paths::engine_text(above) else {
            break;
        };
        if stamps.contains(&text) {
            break;
        }
        stamps.add(&text);
    }
}

/// Whether the load would have listed `path`: a folder it walks into, or a file with an extension it reads, below an include root and not below an
/// exclude (the engine's own rule, `Entry::contains_dir` and `contains_file`).
fn listed(folders: &Folders, path: &AbsPath, dir: bool) -> bool {
    folders.listed.iter().any(|entry| {
        if dir {
            entry.contains_dir(path)
        } else {
            entry.contains_file(path)
        }
    })
}

fn read(path: &str) -> Option<Vec<u8>> {
    #[cfg(test)]
    READS.with(|n| n.set(n.get() + 1));
    std::fs::read(path).ok()
}

#[cfg(test)]
use std::cell::{Cell, RefCell};

#[cfg(test)]
thread_local! {
    /// How many files `follow_disk` read, and how many changes it handed to the database, on this thread: a test that says "nothing was read" or "nothing
    /// was applied" reads them before and after.
    pub(crate) static READS: Cell<u32> = const { Cell::new(0) };
    pub(crate) static APPLIES: Cell<u32> = const { Cell::new(0) };
    /// A test hook, run once by the next `follow_disk` that read something, after the reads: a file saved here is saved after the database was given the old
    /// text, and the stamp kept for it is the one taken before. Nothing else can land a change there.
    static AFTER_READ: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn after_the_next_read(hook: impl FnOnce() + 'static) {
    AFTER_READ.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
fn run_after_read() {
    if let Some(hook) = AFTER_READ.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
}

#[cfg(test)]
mod tests {
    //! What needs a hook or a counter inside `sem` (the rest of RS5 step 3 is in `tests/refresh.rs`, which sees only the public API), and the measures.

    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::sem::common::{copy_of_fixture, one_load, write};
    use crate::{Budget, Cancel, Entry, LoadOptions, MarkerStyle, Options, RenderOptions, Session};

    fn open(root: &Path, file: &str) -> Session {
        Session::open(
            root,
            Path::new(file),
            &LoadOptions::default(),
            Options::new(MarkerStyle::docstring("ID")),
        )
        .expect("the fixture loads")
    }

    fn reads() -> u32 {
        READS.with(Cell::get)
    }

    fn applies() -> u32 {
        APPLIES.with(Cell::get)
    }

    /// The tree of `qualname`'s graph as text, and the names of the defs in it.
    fn tree(session: &Session, file: &str, qualname: &str) -> (String, Vec<String>) {
        let entry = Entry::Def {
            qualname,
            occurrence: 0,
        };
        let graph = session
            .graph(file, &entry, &Budget::default(), &Cancel::new())
            .expect("the entry is there");
        let text = crate::to_text(&crate::render(
            &graph,
            &graph.entries[0],
            &RenderOptions::default(),
        ));
        let names = graph.defs.iter().map(|def| def.qname.clone()).collect();
        (text, names)
    }

    fn grow(file: &Path) {
        let mut text = std::fs::read_to_string(file).unwrap();
        text.push_str("\n// one more line\n");
        write(file, &text);
    }

    /// Nothing changed: no file is read, nothing is handed to the database, the `Vfs` has recorded nothing, and the graph is the same text. A file that
    /// was touched and not changed is read (its stamp moved) and still hands the database nothing, because the `Vfs` compares the bytes.
    #[test]
    fn nothing_changed_reads_nothing_and_applies_nothing() {
        let _load = one_load();
        let root = copy_of_fixture("refresh", "refresh_nothing");
        let mut session = open(&root, "src/lib.rs");
        let before = tree(&session, "src/lib.rs", "entry");

        let (reads_before, applies_before) = (reads(), applies());
        for _ in 0..3 {
            let refreshed = session.refresh();
            assert!(!refreshed.reopened);
            assert_eq!(refreshed.lost, None);
        }
        assert_eq!(reads(), reads_before, "no stamp moved: no file was read");
        assert_eq!(applies(), applies_before, "and nothing was applied");
        assert!(
            session
                .project_mut()
                .loaded_mut()
                .follow()
                .1
                .take_changes()
                .is_empty(),
            "the Vfs recorded no change"
        );
        assert_eq!(tree(&session, "src/lib.rs", "entry"), before);

        // touched: the modified time moves, the bytes do not
        let file = std::fs::File::options()
            .write(true)
            .open(root.join("src/callee.rs"))
            .unwrap();
        file.set_modified(std::time::SystemTime::now() + Duration::from_secs(5))
            .unwrap();
        drop(file);
        session.refresh();
        assert_eq!(reads(), reads_before + 1, "the touched file was read");
        assert_eq!(
            applies(),
            applies_before,
            "the bytes are the same: nothing applied"
        );
        assert_eq!(tree(&session, "src/lib.rs", "entry"), before);
    }

    /// A file saved after the database was given its text is read again by the next refresh, because the stamp kept is the one taken before the read.
    /// (Taking it after would describe the new text while the database holds the old one, and nothing would ever notice.)
    #[test]
    fn a_file_saved_after_it_was_read_is_read_again_by_the_next_refresh() {
        let _load = one_load();
        let root = copy_of_fixture("refresh", "refresh_during");
        let mut session = open(&root, "src/lib.rs");
        let callee = root.join("src/callee.rs");
        let calls_second = |session: &Session| {
            tree(session, "src/lib.rs", "entry")
                .1
                .iter()
                .any(|name| name.ends_with("second"))
        };
        assert!(!calls_second(&session));

        // a first save, so that the next refresh reads `callee.rs`; a second one lands while it does
        let first = std::fs::read_to_string(&callee)
            .unwrap()
            .replace("first()", "first();\n    ");
        write(&callee, &first);
        let saved_again = std::fs::read_to_string(&callee)
            .unwrap()
            .replace("first();", "second();");
        let path = callee.clone();
        after_the_next_read(move || write(&path, &saved_again));
        session.refresh();
        assert!(
            !calls_second(&session),
            "the database holds the text the refresh read: the first save"
        );

        let reads_before = reads();
        session.refresh();
        assert_eq!(reads(), reads_before + 1, "the second save was seen");
        assert!(calls_second(&session));

        let reads_before = reads();
        session.refresh();
        assert_eq!(reads(), reads_before, "and now nothing changed");
    }

    /// The root is `above/member`, the workspace's own folder is `above`: no folder above the root is stamped, and what is outside the root (`sibling`, a
    /// member of the same workspace) is stamped up to its own package root.
    #[test]
    fn no_folder_above_the_root_is_stamped() {
        let _load = one_load();
        let case = crate::paths::absolute(&copy_of_fixture("above", "refresh_above")).unwrap();
        let root = crate::paths::absolute(&case.join("member")).unwrap();
        let session = open(&root, "src/lib.rs");
        let stamps = session.stamps();

        let text = |path: &Path| crate::paths::engine_text(path).unwrap();
        let (root_text, sibling) = (text(&root), text(&case.join("sibling")));
        let paths: Vec<&str> = stamps.paths().collect();
        for path in &paths {
            let above = root_text.starts_with(&format!("{path}/"));
            assert!(!above, "{path} is above the root {root_text}");
        }
        for wanted in [
            format!("{root_text}/src/lib.rs"),
            format!("{root_text}/src"),
            root_text.clone(),
            format!("{sibling}/src/lib.rs"),
            format!("{sibling}/src"),
            sibling.clone(),
        ] {
            assert!(paths.contains(&wanted.as_str()), "{wanted} in {paths:?}");
        }
        assert!(
            !paths.contains(&text(&case).as_str()),
            "the workspace's folder: {paths:?}"
        );
    }

    /// `core/` is copied (no `target/`) and the copy is the project: a saved file must not touch the repo.
    fn copy_of_core(name: &str) -> PathBuf {
        let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../core");
        let to = crate::sem::common::tmp_root().join(name);
        let _ = std::fs::remove_dir_all(&to);
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                if entry.file_name() == "target" {
                    continue;
                }
                let target = to.join(entry.file_name());
                if entry.path().is_dir() {
                    copy(&entry.path(), &target);
                } else {
                    std::fs::copy(entry.path(), &target).unwrap();
                }
            }
        }
        copy(&from, &to);
        crate::paths::absolute(&to).unwrap()
    }

    fn timed<R>(f: impl FnOnce() -> R) -> (R, Duration) {
        let started = Instant::now();
        let result = f();
        (result, started.elapsed())
    }

    /// RS5 step 3 `[measure]`, on a copy of `core/`: what a refresh costs when nothing changed (and how many files and folders it stamps), when one file was
    /// saved, and what the request after it costs; and the first and second request for one entry. Printed, to be written into `ENGINE.md`.
    ///
    /// ```text
    /// cargo test -p rust-analysis --lib -j2 -- --ignored --nocapture --test-threads=2 refresh::tests::measure
    /// ```
    #[test]
    #[ignore = "a measurement: run by hand with --ignored --nocapture"]
    fn measure_refresh_on_core() {
        let _load = one_load();
        let root = copy_of_core("refresh_measure_core");
        let (mut session, opened) = timed(|| open(&root, "analyzer-core/src/lib.rs"));
        let (files, folders) = session.stamps().counts();
        println!("MEASURE open {opened:?}; stamped {files} files and {folders} folders");

        let file = "analyzer-core/src/wire.rs";
        let (first, first_time) = timed(|| tree(&session, file, "wire"));
        let (second, second_time) = timed(|| tree(&session, file, "wire"));
        assert_eq!(first, second);
        println!("MEASURE request for `wire`: first {first_time:?}, second {second_time:?}");

        let mut worst = Duration::ZERO;
        let mut total = Duration::ZERO;
        for _ in 0..50 {
            let (_, took) = timed(|| session.refresh());
            worst = worst.max(took);
            total += took;
        }
        println!(
            "MEASURE refresh, nothing changed: mean {:?}, worst {worst:?} (50 runs)",
            total / 50
        );

        let applied = applies();
        grow(&root.join("analyzer-core/src/text.rs"));
        let (_, refresh_time) = timed(|| session.refresh());
        assert_eq!(
            applies(),
            applied + 1,
            "the saved file reached the database"
        );
        let (_, next_time) = timed(|| tree(&session, file, "wire"));
        let (_, again_time) = timed(|| tree(&session, file, "wire"));
        println!(
            "MEASURE one saved file (text.rs): refresh {refresh_time:?}, next request {next_time:?}, the one after {again_time:?}"
        );

        grow(&root.join("analyzer-core/src/wire.rs"));
        let (_, refresh_time) = timed(|| session.refresh());
        assert_eq!(applies(), applied + 2, "and the second one");
        let (_, next_time) = timed(|| tree(&session, file, "wire"));
        println!(
            "MEASURE one saved file (wire.rs, the entry's own): refresh {refresh_time:?}, next request {next_time:?}"
        );
    }
}
