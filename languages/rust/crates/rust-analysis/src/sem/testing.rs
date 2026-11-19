//! What the unit tests of `sem` share: the policy fixture, loaded, and the outline of one of its files read from disk.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use analyzer_core::edit::{MarkerStyle, Placement};
use analyzer_core::outline::Outline;
use rust_syntax::Edition;

use super::Sem;
use super::common;
use super::load::{Loaded, load};
use crate::{LoadOptions, Options, Session};

pub(crate) const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

/// The policy fixture, loaded as cargo's workspace of one package, once for the whole test program and used by one test at a time (see
/// `common::one_load` for why). Its root is the copy it was loaded from, which no test writes to.
pub(crate) fn policy() -> (MutexGuard<'static, Loaded>, PathBuf) {
    static POLICY: OnceLock<Fixture> = OnceLock::new();
    shared(&POLICY, "policy")
}

/// The context fixture, shared the same way.
pub(crate) fn context() -> (MutexGuard<'static, Loaded>, PathBuf) {
    static CONTEXT: OnceLock<Fixture> = OnceLock::new();
    shared(&CONTEXT, "context")
}

struct Fixture {
    root: PathBuf,
    loaded: Mutex<Loaded>,
}

fn shared(
    cell: &'static OnceLock<Fixture>,
    fixture: &str,
) -> (MutexGuard<'static, Loaded>, PathBuf) {
    let fixture = cell.get_or_init(|| {
        let root = crate::paths::absolute(&common::copy_of_fixture(
            fixture,
            &format!("shared_{fixture}"),
        ))
        .unwrap();
        let manifest = root.join("Cargo.toml");
        let loaded = load(&root, Some(&manifest), &manifest, &LoadOptions::default())
            .unwrap_or_else(|e| panic!("the {fixture} fixture loads: {e}"));
        Fixture {
            root,
            loaded: Mutex::new(loaded),
        }
    });
    (
        fixture
            .loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
        fixture.root.clone(),
    )
}

/// The text of a file on disk, as it is: a byte-order mark stays, and the engine's offsets and the outline's count it.
pub(crate) fn disk_text(root: &Path, file: &str) -> String {
    std::fs::read_to_string(root.join(file)).unwrap()
}

/// `rust-syntax`'s outline of a file's text on disk, in the edition of the fixture.
pub(crate) fn disk_outline(root: &Path, file: &str) -> Outline {
    rust_syntax::outline(&disk_text(root, file), Edition::E2024, STYLE)
}

/// One module's site lists, as a reviewable text: every def that can be an entry, with its sites in order. A site is its kind, its span relative to the
/// def's start, the text there, and the closure it is in. What is lost to an expansion the engine could not make is printed too.
pub(crate) fn render_sites(sem: &Sem<'_>, root: &Path, file: &str) -> String {
    let text = disk_text(root, file);
    let outline = disk_outline(root, file);
    let mut out = String::new();
    for (def, occurrence) in outline.defs.iter().zip(rust_syntax::occurrences(&outline)) {
        let Some(func) = sem.entry(file, &def.qualname, occurrence) else {
            continue;
        };
        out.push_str(&def.qualname);
        if occurrence > 0 {
            out.push_str(&format!("#{occurrence}"));
        }
        out.push('\n');
        let sites = sem.sites(func);
        for site in &sites.list {
            let at = site.span();
            let from = (def.range.start + at.start) as usize;
            let to = (def.range.start + at.end) as usize;
            out.push_str(&format!(
                "  {:?} {}..{} {:?}",
                site.kind(),
                at.start,
                at.end,
                &text[from..to]
            ));
            if let Some(closure) = site.closure() {
                out.push_str(&format!(" closure {closure}"));
            }
            out.push('\n');
        }
        if sites.macro_not_expanded > 0 {
            out.push_str(&format!(
                "  macro_not_expanded {}\n",
                sites.macro_not_expanded
            ));
        }
    }
    out
}

/// Copies `from` to a folder of the tests' scratch space (no `target/` and no `.git`): what is measured is saved to, and a measurement must not touch the
/// repo.
pub(crate) fn copy_of_workspace(from: &Path, name: &str) -> PathBuf {
    let to = common::tmp_root().join(name);
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

pub(crate) fn timed<R>(f: impl FnOnce() -> R) -> (R, Duration) {
    let started = Instant::now();
    let result = f();
    (result, started.elapsed())
}

/// The process's peak physical footprint, from `vmmap -summary` (macOS): what the machine counts, which `ps`'s RSS leaves out (compressed memory).
pub(crate) fn peak_footprint() -> String {
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

/// What the ignored measurements are run on, from the environment: `RS_MEASURE_ROOT` (a folder, copied: default `core/` of this repo),
/// `RS_MEASURE_FILE` (the file the session is opened from, relative to the root: default `analyzer-core/src/lib.rs`) and `RS_MEASURE_ENTRY`
/// (`<file>::<qualname>`, the def asked for: default `analyzer-core/src/wire.rs::wire`).
pub(crate) struct Measured {
    pub(crate) from: PathBuf,
    pub(crate) open_file: String,
    pub(crate) file: String,
    pub(crate) qualname: String,
}

pub(crate) fn measured() -> Measured {
    let var =
        |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.to_owned());
    let from = std::env::var("RS_MEASURE_ROOT").map_or_else(
        |_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../core"),
        PathBuf::from,
    );
    let entry = var("RS_MEASURE_ENTRY", "analyzer-core/src/wire.rs::wire");
    let (file, qualname) = entry
        .split_once("::")
        .expect("RS_MEASURE_ENTRY is <file>::<qualname>");
    Measured {
        from,
        open_file: var("RS_MEASURE_FILE", "analyzer-core/src/lib.rs"),
        file: file.to_owned(),
        qualname: qualname.to_owned(),
    }
}

/// Opens a session on `root` for a measurement, and says what the load was: its warnings are printed, and a load that fell down the ladder (no cargo, a
/// manifest cargo could not read: no `std`, no dependencies) fails the test, because its numbers are not the numbers of a workspace.
pub(crate) fn open_measured(root: &Path, file: &str) -> (Session, Duration) {
    let (session, took) = timed(|| {
        Session::open(
            root,
            Path::new(file),
            &LoadOptions::default(),
            Options::new(STYLE),
        )
        .expect("the workspace loads")
    });
    let project = session.project().expect("a project");
    println!("MEASURE root {}", root.display());
    for warning in project.warnings() {
        println!("MEASURE warning {}: {warning}", warning.code());
    }
    let rung = project.loaded().rung();
    assert_eq!(
        rung,
        super::Rung::Cargo,
        "the load fell down the ladder ({rung:?}, warnings {:?}): its numbers are not a workspace's",
        project.warnings()
    );
    (session, took)
}
