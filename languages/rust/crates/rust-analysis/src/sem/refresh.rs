//! Following the disk (RS5 §3): which `.rs` files of the project moved since the last look, and what to tell the engine's database about them. The fast
//! path: nothing is loaded again, and salsa recomputes only what read a file that changed.
//!
//! What is watched: every `.rs` file the `Vfs` holds that is not a library's (a dependency's or the sysroot's), and each folder that holds one, from the
//! file's folder up to its package root and never above the project root. A folder's modified time moves when an entry is created or deleted in it, so that is
//! how a new file is found: the folder is listed again, the way the load listed it (same extensions, same excludes).
//!
//! What makes the workspace load again instead (`Disk::Reload`): an environment input that moved (a manifest, a lock, a toolchain or cargo config file, a
//! `build.rs` when build scripts ran), or a `.rs` file created or deleted where cargo finds a target by itself (`targets.rs`). Those change the crate
//! graph; this module only detects them.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use ra_ap_ide_db::{ChangeWithProcMacros, RootDatabase};
use ra_ap_vfs::loader::Entry;
use ra_ap_vfs::{AbsPath, AbsPathBuf, Change, ChangeKind, Vfs, VfsPath};

use super::load::{Folders, Loaded};
use super::stamps::{Stamp, Stamps};
use super::targets::should_refresh_for_change;
use crate::paths;

/// What `follow_disk` did.
pub(crate) enum Disk {
    /// The changes of the disk are in the database (possibly none: a file touched and not changed).
    Applied,
    /// The crate graph may have changed: nothing was read or applied, and the workspace must be loaded again.
    Reload,
}

impl Loaded {
    /// What to watch after a load, stamped now. `inputs` are `Project::environment_inputs()` (globs relative to the root, and exact absolute paths);
    /// `opened` is the file the session was opened from; `before` is what was stamped just before the load, or nothing for the first load.
    ///
    /// A path `before` knew keeps its stamp from before the load; a path stamped for the first time is read once more and handed to the `Vfs`, because
    /// the load read it before any stamp existed: a file saved while the load ran would otherwise have a stamp that describes the new text while the
    /// database holds the old. The `Vfs` compares the bytes, so a file that did not change costs a read and a hash and applies nothing.
    pub(crate) fn watch(&mut self, inputs: &[String], opened: &str, before: &Stamps) -> Stamps {
        #[cfg(test)]
        run_after_load();

        let (mut fresh, files) = self.stamp();
        for path in self.input_paths(inputs) {
            fresh.add_input(&path);
        }
        fresh.add_kept(opened);
        let mut stamps = fresh.keeping(before);

        let new: BTreeSet<String> = files
            .into_iter()
            .filter(|file| !before.contains(file))
            .collect();
        let (db, vfs, folders, _) = self.follow();
        apply(db, vfs, folders, &new);
        stamps.forget_missing();
        stamps
    }

    /// The stamps of what the load read, now: each non-library `.rs` file and the folders above it (see the module's doc); and the files, as engine text.
    fn stamp(&self) -> (Stamps, Vec<String>) {
        let folders = self.folders();
        let includes = includes(folders);
        let mut stamps = Stamps::default();
        let mut files = Vec::new();
        for (_, path) in self.vfs().iter() {
            let Some(file) = path.as_path() else { continue };
            if file.extension() != Some("rs") || folders.roots.path_is_library(path) {
                continue;
            }
            let Some(text) = paths::engine_text(file.as_ref()) else {
                continue;
            };
            stamps.add(&text);
            files.push(text);
            let Some(dir) = file.parent() else { continue };
            watch_folders(&mut stamps, dir.as_ref(), &includes, self.root());
        }
        files.sort();
        // the folders from the project root down to each package root: a package created in one of them is seen when the folder changes
        let root = self.root();
        for dir in std::iter::once(root.to_path_buf()).chain(package_roots(folders, root)) {
            for above in dir.ancestors() {
                if paths::relative(root, above).is_none() {
                    break;
                }
                if let Some(text) = paths::engine_text(above) {
                    stamps.add_scout(&text);
                }
            }
        }
        (stamps, files)
    }

    /// The paths `inputs` name, as engine text. A glob (`**/Cargo.toml`) is resolved now, for the load: the name in the project root and in each package
    /// root under it (it may be missing: its creation is then seen), and in any other folder of the project where the load found a file of that name. An
    /// exact path (a manifest outside the root) is as it is.
    fn input_paths(&self, inputs: &[String]) -> BTreeSet<String> {
        let (folders, root) = (self.folders(), self.root());
        let suffixes: Vec<&str> = inputs
            .iter()
            .filter_map(|input| input.strip_prefix("**/"))
            .collect();
        let names: Vec<&str> = suffixes
            .iter()
            .filter_map(|suffix| suffix.rsplit('/').next())
            .collect();
        let mut found: BTreeSet<String> = inputs
            .iter()
            .filter(|input| !input.starts_with("**/"))
            .cloned()
            .collect();

        let mut dirs: Vec<PathBuf> = vec![root.to_path_buf()];
        dirs.extend(package_roots(folders, root));
        for dir in &dirs {
            for suffix in &suffixes {
                found.extend(paths::engine_text(&dir.join(suffix)));
            }
        }
        for (_, path) in self.vfs().iter() {
            let Some(file) = path.as_path() else { continue };
            if !file.file_name().is_some_and(|name| names.contains(&name))
                || folders.roots.path_is_library(path)
                || paths::relative(root, file.as_ref()).is_none()
            {
                continue;
            }
            let Some(text) = paths::engine_text(file.as_ref()) else {
                continue;
            };
            if suffixes
                .iter()
                .any(|suffix| text.ends_with(&format!("/{suffix}")))
            {
                found.insert(text);
            }
        }
        found
    }

    /// Looks at the disk and tells the database what changed, in one transaction. `known` is what the last look saw and `now` what the disk says of the
    /// same paths (taken by the caller, before anything here reads); `now` also gets the paths found here, stamped before they are read, and is what the
    /// next look compares with.
    ///
    /// A saved file is read and given to the `Vfs`; a created or deleted one also splits the files into source roots again (the module tree looks a `mod`
    /// up in the roots); a file that is not UTF-8 has no text, as after the load. **Stamp first, read second**: every stamp here is taken before the
    /// bytes it describes are read, so a file saved in between has a newer stamp than the one kept, and the next look reads it again.
    ///
    /// An input that moved, a created or deleted `.rs` file where cargo finds a target by itself, or a package created below the root (a new folder, under
    /// one on the way to a package root, that holds an input file), is `Disk::Reload`: nothing is read.
    pub(crate) fn follow_disk(
        &mut self,
        known: &Stamps,
        now: &mut Stamps,
        inputs: &[String],
    ) -> Disk {
        let moved: Vec<String> = known
            .iter()
            .filter(|&(path, stamp)| now.get(path) != Some(stamp))
            .map(|(path, _)| path.to_owned())
            .collect();
        if moved.iter().any(|path| known.is_input(path)) {
            return Disk::Reload;
        }

        // a folder on the way from the root to a package root changed: a package may have been created in it
        let suffixes: Vec<&str> = inputs
            .iter()
            .filter_map(|input| input.strip_prefix("**/"))
            .collect();
        let mut scouted: HashSet<PathBuf> = HashSet::new();
        for dir in moved.iter().filter(|path| known.is_scout(path)) {
            if scout_folder(dir, &suffixes, now, &mut scouted) {
                return Disk::Reload;
            }
        }

        let (db, vfs, folders, _) = self.follow();
        let mut to_read: BTreeSet<String> = BTreeSet::new();
        let mut to_list: Vec<String> = Vec::new();
        let mut deleted: Vec<&str> = Vec::new();
        let mut created: Vec<String> = Vec::new();
        for path in &moved {
            if known.get(path).is_some_and(Stamp::is_dir) {
                to_list.push(path.clone());
            } else {
                to_read.insert(path.clone());
                if now.get(path) == Some(Stamp::Missing) {
                    deleted.push(path);
                }
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
                // what the last look knew is left to its own stamp (not `now`: scouting above stamped the new folders it walked, and they are listed here)
                if known.contains(&text) {
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
                    created.push(text.clone());
                    to_read.insert(text);
                }
            }
        }

        let target_changed = |path: &str, kind| {
            AbsPathBuf::try_from(path).is_ok_and(|abs| should_refresh_for_change(&abs, kind, &[]))
        };
        if created
            .iter()
            .any(|path| target_changed(path, ChangeKind::Create))
            || deleted
                .iter()
                .any(|path| target_changed(path, ChangeKind::Delete))
        {
            return Disk::Reload;
        }

        apply(db, vfs, folders, &to_read);
        now.forget_missing();
        Disk::Applied
    }
}

/// Reads each of `paths` (engine text) and gives it to the `Vfs`, then hands what changed to the database in one transaction. A file that cannot be
/// read is gone, as the load reads it; one that is not UTF-8 has no text.
fn apply(db: &mut RootDatabase, vfs: &mut Vfs, folders: &Folders, paths: &BTreeSet<String>) {
    for path in paths {
        let Ok(abs) = AbsPathBuf::try_from(path.as_str()) else {
            continue;
        };
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

/// The package roots under the project root: the folders the load listed for the project's own packages (not a dependency's or the sysroot's), as paths,
/// leaving out what is under a `target/` or `.git`.
fn package_roots(folders: &Folders, root: &Path) -> Vec<PathBuf> {
    includes(folders)
        .into_iter()
        .filter(|include| {
            paths::relative(root, Path::new(include)).is_some_and(|below| {
                !below
                    .split('/')
                    .any(|name| name == "target" || name == ".git")
            })
        })
        .filter(|include| {
            AbsPathBuf::try_from(include.as_str())
                .is_ok_and(|abs| !folders.roots.path_is_library(&VfsPath::from(abs)))
        })
        .map(PathBuf::from)
        .collect()
}

/// Whether `text` (engine text) is a file the load reads as an environment input: its path ends with the name a glob of the inputs gives (`Cargo.toml`,
/// `.cargo/config.toml`).
fn is_input_file(text: &str, suffixes: &[&str]) -> bool {
    suffixes
        .iter()
        .any(|suffix| text.ends_with(&format!("/{suffix}")))
}

/// A folder on the way from the root to a package root changed. Whether a file in it is an input, or a folder in it that was not watched holds one
/// below it (`holds_input`). A folder that is not watched is a new one: it is stamped now, before it is looked in, so a later change in it is seen.
fn scout_folder(
    dir: &str,
    suffixes: &[&str],
    now: &mut Stamps,
    visited: &mut HashSet<PathBuf>,
) -> bool {
    #[cfg(test)]
    SCOUTED.with(|n| n.set(n.get() + 1));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let Some(text) = paths::engine_text(&entry.path()) else {
            continue;
        };
        let Ok(meta) = std::fs::metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() {
            if !skipped_folder(&entry.file_name()) && !now.contains(&text) {
                let cycle =
                    std::fs::canonicalize(entry.path()).is_ok_and(|real| !visited.insert(real));
                if !cycle && holds_input(&text, suffixes, now, visited) {
                    return true;
                }
            }
        } else if is_input_file(&text, suffixes) {
            return true;
        }
    }
    false
}

/// Whether `dir` (a folder that was not watched) or any folder below it holds an input file. Every folder walked is stamped as a scout, before it is read.
fn holds_input(
    dir: &str,
    suffixes: &[&str],
    now: &mut Stamps,
    visited: &mut HashSet<PathBuf>,
) -> bool {
    let mut to_walk = vec![dir.to_owned()];
    while let Some(dir) = to_walk.pop() {
        now.add_scout(&dir);
        #[cfg(test)]
        SCOUTED.with(|n| n.set(n.get() + 1));
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(text) = paths::engine_text(&entry.path()) else {
                continue;
            };
            let Ok(meta) = std::fs::metadata(entry.path()) else {
                continue;
            };
            if meta.is_dir() {
                let cycle =
                    std::fs::canonicalize(entry.path()).is_ok_and(|real| !visited.insert(real));
                if !skipped_folder(&entry.file_name()) && !cycle {
                    to_walk.push(text);
                }
            } else if is_input_file(&text, suffixes) {
                return true;
            }
        }
    }
    false
}

/// The folders no package is looked for in: the build's output and the version control's.
fn skipped_folder(name: &std::ffi::OsStr) -> bool {
    name == "target" || name == ".git"
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
    /// How many folders were read looking for a new package (`scout_folder`, `holds_input`): a refresh with nothing changed must leave it alone.
    pub(crate) static SCOUTED: Cell<u32> = const { Cell::new(0) };
    /// A test hook, run once by the next `apply`, after the reads: a file saved here is saved after the database was given the old text, and the stamp
    /// kept for it is the one taken before. Nothing else can land a change there.
    static AFTER_READ: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    /// A test hook, run once by the next `watch`, before it stamps anything: a file saved here is saved after the load read it and before it has a stamp.
    static AFTER_LOAD: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn after_the_next_read(hook: impl FnOnce() + 'static) {
    AFTER_READ.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
pub(crate) fn after_the_next_load(hook: impl FnOnce() + 'static) {
    AFTER_LOAD.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
fn run_after_load() {
    if let Some(hook) = AFTER_LOAD.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
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
    use crate::sem::load::alive;
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

    fn made() -> u32 {
        alive::MADE.with(Cell::get)
    }

    /// Makes the manifest longer: a save of an environment input.
    fn save_manifest(root: &Path) {
        grow(&root.join("Cargo.toml"));
    }

    /// Nothing moved: no load. A manifest saved: one load, and at no moment two databases alive, because the old project is dropped before the new one is
    /// built. `reload` is the same. A session dropped frees its database.
    #[test]
    fn a_reload_drops_the_old_database_first_and_nothing_moved_loads_nothing() {
        let _load = one_load();
        let root = copy_of_fixture("refresh", "refresh_one_database");
        alive::PEAK.with(|peak| peak.set(alive::ALIVE.with(Cell::get)));
        let started = made();
        let mut session = open(&root, "src/lib.rs");
        assert_eq!(made(), started + 1);
        assert_eq!(alive::ALIVE.with(Cell::get), 1);

        for _ in 0..3 {
            assert!(!session.refresh().reopened);
        }
        assert_eq!(made(), started + 1, "nothing moved: nothing loaded");

        save_manifest(&root);
        let refreshed = session.refresh();
        assert!(refreshed.reopened, "an environment input moved");
        assert_eq!(made(), started + 2);
        assert!(session.reload().reopened);
        assert_eq!(made(), started + 3);
        assert!(!session.refresh().reopened, "and then nothing moved again");

        assert_eq!(
            alive::PEAK.with(Cell::get),
            1,
            "one database alive at most, through two reloads"
        );
        assert_eq!(alive::ALIVE.with(Cell::get), 1);
        drop(session);
        assert_eq!(
            alive::ALIVE.with(Cell::get),
            0,
            "dropping the session frees it"
        );
    }

    /// A lost session has no database at all, loads nothing while nothing moved, and loads when a path it watches moves.
    #[test]
    fn a_lost_session_holds_no_database_and_loads_only_when_a_stamp_moves() {
        let _load = one_load();
        let root = copy_of_fixture("refresh", "refresh_lost_counts");
        let mut session = open(&root, "src/lib.rs");
        let lib = root.join("src/lib.rs");
        let text = std::fs::read_to_string(&lib).unwrap();
        std::fs::remove_file(&lib).unwrap();

        let before = made();
        assert!(
            session.refresh().lost.is_some(),
            "the file it was opened from is gone"
        );
        assert_eq!(made(), before, "the load failed before a database was made");
        assert_eq!(alive::ALIVE.with(Cell::get), 0, "and the old one is gone");
        for _ in 0..3 {
            assert!(session.refresh().lost.is_some());
        }
        assert_eq!(made(), before, "nothing moved: nothing attempted");

        write(&lib, &text);
        let back = session.refresh();
        assert!(back.reopened && back.lost.is_none());
        assert_eq!(made(), before + 1);
    }

    /// A file saved after the load read it and before it has a stamp: the stamp describes the new text, so the load's text is read once more, and the graph
    /// after `open` has the new text with no `refresh`. (Without the re-read the stamp matches the disk, the database holds the old text, and nothing
    /// would ever notice.)
    #[test]
    fn a_file_saved_between_the_load_and_the_stamps_is_in_the_graph_after_open() {
        let _load = one_load();
        let root = copy_of_fixture("refresh", "refresh_open_window");
        let callee = root.join("src/callee.rs");
        let saved = std::fs::read_to_string(&callee)
            .unwrap()
            .replace("first();", "second();");
        after_the_next_load(move || write(&callee, &saved));

        let mut session = open(&root, "src/lib.rs");
        let (_, names) = tree(&session, "src/lib.rs", "entry");
        assert!(
            names.iter().any(|name| name.ends_with("second")),
            "{names:?}"
        );
        assert!(
            !names.iter().any(|name| name.ends_with("first")),
            "{names:?}"
        );

        let reads_before = reads();
        assert!(!session.refresh().reopened);
        assert_eq!(
            reads(),
            reads_before,
            "and the stamp matches: nothing moved"
        );
    }

    /// The same window in a reload: a path the session knew keeps the stamp taken before the load, so a file saved during it is read by the next look; one
    /// that was not known is read once more at once.
    #[test]
    fn a_file_saved_during_a_reload_is_seen_by_the_next_refresh() {
        let _load = one_load();
        let root = copy_of_fixture("refresh", "refresh_reload_window");
        let mut session = open(&root, "src/lib.rs");
        let callee = root.join("src/callee.rs");
        let saved = std::fs::read_to_string(&callee)
            .unwrap()
            .replace("first();", "second();");
        let calls_second = |session: &Session| {
            tree(session, "src/lib.rs", "entry")
                .1
                .iter()
                .any(|name| name.ends_with("second"))
        };

        save_manifest(&root);
        after_the_next_load(move || write(&callee, &saved));
        assert!(session.refresh().reopened);
        assert!(
            !calls_second(&session),
            "`callee.rs` was known: its stamp is from before the load, and the load read the old text"
        );
        let second = session.refresh();
        assert!(!second.reopened, "a source file saved is no reload");
        assert!(calls_second(&session), "the next look read it");
    }

    fn scouted() -> u32 {
        SCOUTED.with(Cell::get)
    }

    /// Nothing changed in a workspace with members: no reload and no folder read looking for a package. A new folder under `crates/` is read (and is no
    /// reload when it holds no manifest).
    #[test]
    fn nothing_changed_walks_no_folder_and_a_new_folder_is_walked_once() {
        let _load = one_load();
        let root = copy_of_fixture("members", "refresh_scout_counts");
        let mut session = open(&root, "crates/a/src/lib.rs");
        let crates =
            crate::paths::engine_text(&crate::paths::absolute(&root).unwrap().join("crates"))
                .unwrap();
        assert!(
            session.stamps().is_scout(&crates),
            "`crates/`, between the root and a package root, is a scout"
        );

        let (made_before, scouted_before) = (made(), scouted());
        for _ in 0..3 {
            assert!(!session.refresh().reopened);
        }
        assert_eq!(
            scouted(),
            scouted_before,
            "nothing moved: no folder was walked"
        );
        assert_eq!(made(), made_before);

        write(&root.join("crates/notes/readme.md"), "not a crate\n");
        assert!(!session.refresh().reopened);
        assert!(scouted() > scouted_before, "the new folder was walked");
        let walked = scouted();
        assert!(!session.refresh().reopened);
        assert_eq!(scouted(), walked, "and it is not walked again");
    }

    /// Copies `from` to a folder of the tests' scratch space (no `target/` and no `.git`): what is measured is saved to, and a measurement must not touch
    /// the repo.
    fn copy_of_workspace(from: &Path, name: &str) -> PathBuf {
        let to = crate::sem::common::tmp_root().join(name);
        let _ = std::fs::remove_dir_all(&to);
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                if entry.file_name() == "target" || entry.file_name() == ".git" {
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
        copy(from, &to);
        crate::paths::absolute(&to).unwrap()
    }

    fn timed<R>(f: impl FnOnce() -> R) -> (R, Duration) {
        let started = Instant::now();
        let result = f();
        (result, started.elapsed())
    }

    /// The process's peak physical footprint, from `vmmap -summary` (macOS): what the machine counts, which `ps`'s RSS leaves out (compressed memory).
    fn peak_footprint() -> String {
        let pid = std::process::id().to_string();
        match std::process::Command::new("vmmap")
            .args(["-summary", &pid])
            .output()
        {
            Ok(out) => String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|line| line.contains("Physical footprint"))
                .collect::<Vec<_>>()
                .join(" | "),
            Err(_) => "no `vmmap` here".to_owned(),
        }
    }

    /// RS5 steps 3 and 4 `[measure]`, on a copy of a workspace: what a refresh costs when nothing changed (and how many files and folders it stamps), when
    /// one file was saved, and what the request after it costs; the first and second request for one entry; and what a reload costs, the next request
    /// after it, and the process's peak footprint. Printed, to be written into `ENGINE.md`.
    ///
    /// The workspace comes from the environment: `RS_MEASURE_ROOT` (a folder, copied: default `core/` of this repo), `RS_MEASURE_FILE` (the file the
    /// session is opened from, relative to the root: default `analyzer-core/src/lib.rs`) and `RS_MEASURE_ENTRY` (`<file>::<qualname>`, the def asked
    /// for: default `analyzer-core/src/wire.rs::wire`). The load's warnings are printed, and a load that fell down the ladder (no cargo, a manifest cargo
    /// could not read: no `std`, no dependencies) fails the test: its numbers are not the numbers of a workspace.
    ///
    /// ```text
    /// RS_MEASURE_ROOT=/path RS_MEASURE_FILE=a/src/lib.rs RS_MEASURE_ENTRY=a/src/lib.rs::main \
    ///   cargo test -p rust-analysis --lib -j2 -- --ignored --nocapture --test-threads=2 refresh::tests::measure
    /// ```
    #[test]
    #[ignore = "a measurement: run by hand with --ignored --nocapture"]
    fn measure_refresh_and_reload() {
        let _load = one_load();
        let var =
            |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.to_owned());
        let from = std::env::var("RS_MEASURE_ROOT").map_or_else(
            |_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../core"),
            PathBuf::from,
        );
        let open_file = var("RS_MEASURE_FILE", "analyzer-core/src/lib.rs");
        let entry = var("RS_MEASURE_ENTRY", "analyzer-core/src/wire.rs::wire");
        let (file, qualname) = entry
            .split_once("::")
            .expect("RS_MEASURE_ENTRY is <file>::<qualname>");
        let root = copy_of_workspace(&from, "refresh_measure");

        let (mut session, opened) = timed(|| open(&root, &open_file));
        let project = session.project().expect("a project");
        println!("MEASURE root {}", from.display());
        for warning in project.warnings() {
            println!("MEASURE warning {}: {warning}", warning.code());
        }
        let rung = project.loaded().rung();
        assert_eq!(
            rung,
            crate::sem::Rung::Cargo,
            "the load fell down the ladder ({rung:?}, warnings {:?}): its numbers are not a workspace's",
            project.warnings()
        );
        let (files, folders) = session.stamps().counts();
        println!(
            "MEASURE open {opened:?}; stamped {files} paths that are files and {folders} folders"
        );

        let (first, first_time) = timed(|| tree(&session, file, qualname));
        let (second, second_time) = timed(|| tree(&session, file, qualname));
        assert_eq!(first, second);
        println!("MEASURE request for `{qualname}`: first {first_time:?}, second {second_time:?}");

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

        // one saved file that is not the entry's, then the entry's own
        let other = session
            .project()
            .unwrap()
            .files()
            .into_iter()
            .find(|name| name != file)
            .expect("a second file");
        let applied = applies();
        grow(&root.join(&other));
        let (_, refresh_time) = timed(|| session.refresh());
        assert_eq!(
            applies(),
            applied + 1,
            "the saved file reached the database"
        );
        let (_, next_time) = timed(|| tree(&session, file, qualname));
        let (_, again_time) = timed(|| tree(&session, file, qualname));
        println!(
            "MEASURE one saved file ({other}): refresh {refresh_time:?}, next request {next_time:?}, the one after {again_time:?}"
        );
        grow(&root.join(file));
        let (_, refresh_time) = timed(|| session.refresh());
        assert_eq!(applies(), applied + 2, "and the second one");
        let (_, next_time) = timed(|| tree(&session, file, qualname));
        println!(
            "MEASURE one saved file ({file}, the entry's own): refresh {refresh_time:?}, next request {next_time:?}"
        );

        // a reload, by `refresh` (a manifest saved) and by `reload`
        let manifest = root.join(session.project().unwrap().manifest());
        let mut text = std::fs::read_to_string(&manifest).unwrap();
        text.push_str("\n# saved\n");
        write(&manifest, &text);
        let made = alive::MADE.with(Cell::get);
        let (refreshed, reload_time) = timed(|| session.refresh());
        assert!(refreshed.reopened, "a manifest saved is a reload");
        assert_eq!(alive::MADE.with(Cell::get), made + 1);
        let (_, next_time) = timed(|| tree(&session, file, qualname));
        let (_, again_time) = timed(|| tree(&session, file, qualname));
        println!(
            "MEASURE reload by refresh (a manifest saved): {reload_time:?}, next request {next_time:?}, the one after {again_time:?}"
        );
        let (reloaded, reload_time) = timed(|| session.reload());
        assert!(reloaded.reopened);
        println!(
            "MEASURE reload(): {reload_time:?}; databases alive at most {} at once",
            alive::PEAK.with(Cell::get)
        );
        println!("MEASURE process {}", peak_footprint());
    }
}
