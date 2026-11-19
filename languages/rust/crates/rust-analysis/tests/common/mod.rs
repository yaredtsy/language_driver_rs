//! Helpers shared by the test files of this crate. Each file under `tests/` is its own program, so each says `mod common;`.
#![allow(dead_code, reason = "each test program uses some of these")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

/// Every file under `dir` with its text (lossy: all of them are text, and a failed comparison is then readable), by path relative to `dir`
/// with `/`.
pub(crate) fn snapshot(dir: &Path) -> BTreeMap<String, String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let parts: Vec<_> = path
                    .strip_prefix(base)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_str().unwrap())
                    .collect();
                out.insert(
                    parts.join("/"),
                    String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

pub(crate) fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Where a test may make folders: cargo's scratch folder under `target/` for an integration test, and the same place by name for a unit test (cargo
/// sets `CARGO_TARGET_TMPDIR` only for integration tests). Inside the repo's folder tree on purpose: `rust-toolchain.toml` still selects the toolchain.
pub(crate) fn tmp_root() -> PathBuf {
    match option_env!("CARGO_TARGET_TMPDIR") {
        Some(dir) => PathBuf::from(dir),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp"),
    }
}

/// A fresh copy of `tests/fixtures/<fixture>` under `target/`, named `name`. A load that writes into the project then leaves nothing in the
/// repo, and the copy is inside the repo's folder tree, so `rust-toolchain.toml` still selects the toolchain.
pub(crate) fn copy_of_fixture(fixture: &str, name: &str) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let to = tmp_root().join(name);
    let _ = std::fs::remove_dir_all(&to);
    copy_dir(&from, &to);
    to
}

/// Every `.rs` file under `dir`, relative to `base` with `/`, found by walking the disk (not through the engine), skipping `target/`.
pub(crate) fn rs_files(base: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                rs_files(base, &path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            let parts: Vec<_> = path
                .strip_prefix(base)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_str().unwrap())
                .collect();
            out.push(parts.join("/"));
        }
    }
}

/// Every entry of the first-trees snapshot: a function, a method, a const's initializer, a struct and an enum (a construction with nothing to enter),
/// recursion, a project macro defined elsewhere, an `impl Trait` the engine sees through, one it does not, a built-in derive, a std call, a duplicate marker.
pub(crate) const POLICY_ENTRIES: [(&str, &str, u16); 40] = [
    ("src/calls.rs", "plain_calls", 0),
    ("src/calls.rs", "method_chain", 0),
    ("src/calls.rs", "constructions", 0),
    ("src/calls.rs", "closures", 0),
    ("src/calls.rs", "nested_items", 0),
    ("src/calls.rs", "macro_from_another_module", 0),
    ("src/calls.rs", "recurse_a", 0),
    ("src/calls.rs", "FROM_CONST", 0),
    ("src/calls.rs", "Wrapper", 0),
    ("src/macros.rs", "std_arguments", 0),
    ("src/macros.rs", "project_wrapping_external", 0),
    ("src/dispatch.rs", "generic_by_value", 0),
    ("src/dispatch.rs", "impl_return", 0),
    ("src/dispatch.rs", "impl_return_of_a_parameter", 0),
    ("src/dispatch.rs", "fn_pointer", 0),
    ("src/shapes.rs", "derived", 0),
    ("src/shapes.rs", "E", 0),
    ("src/lib.rs", "Writer::twice", 0),
    ("src/lib.rs", "uses_the_twins", 0),
    ("src/external.rs", "std_calls", 0),
    ("src/external.rs", "std_naming", 0),
    // constructions (step 5): a marked struct, a tuple struct and an enum as entries, and what builds them
    ("src/constructions.rs", "MarkedStruct", 0),
    ("src/constructions.rs", "Tuple", 0),
    ("src/constructions.rs", "Shape", 0),
    ("src/constructions.rs", "build_variants", 0),
    ("src/constructions.rs", "build_external", 0),
    ("src/constructions.rs", "build_generated", 0),
    // hand-overs (step 6): named, a method path, called and passed, constructors, an external, a local, through a parameter
    ("src/passed.rs", "named", 0),
    ("src/passed.rs", "Parser::run", 0),
    ("src/passed.rs", "called_and_passed", 0),
    ("src/passed.rs", "constructors", 0),
    ("src/passed.rs", "externals", 0),
    ("src/passed.rs", "locals", 0),
    ("src/passed.rs", "through_a_parameter", 0),
    // macros (step 7): a project macro around an external one, two project levels, an unknown macro, a derive, a function a macro made
    ("src/macros.rs", "project_macro_around_external", 0),
    ("src/macros.rs", "argument_through_external", 0),
    ("src/macros.rs", "two_project_levels", 0),
    ("src/macros.rs", "unknown_in_unknown", 0),
    ("src/macros.rs", "derived_methods", 0),
    ("src/macros.rs", "calls_made_functions", 0),
];

/// (file, qualname, what the program is about). The entries are the callers, and the generics, callbacks and trait objects they reach.
pub(crate) const CONTEXT_ENTRIES: [(&str, &str); 14] = [
    // a generic called from two callers
    ("src/generic.rs", "both"),
    ("src/generic.rs", "emit"),
    // a callback: a `fn` pointer, a parameter bounded by `Fn`, a closure
    ("src/callback.rs", "use_callbacks"),
    ("src/callback.rs", "apply"),
    ("src/callback.rs", "apply_generic"),
    // a `dyn` in a local, and as a parameter
    ("src/dyn_local.rs", "dyn_in_a_local"),
    ("src/dyn_param.rs", "call_dyn"),
    // a factory: a trait object, and an `impl Trait` the engine sees through
    ("src/factory.rs", "use_factory"),
    ("src/factory.rs", "use_impl_factory"),
    // a struct holding a `Box<dyn Writer>`
    ("src/holder.rs", "use_holder"),
    ("src/holder.rs", "Holder::run"),
    ("src/holder.rs", "Holder"),
    // a recursive generic
    ("src/recursive.rs", "start"),
    ("src/recursive.rs", "countdown"),
];

/// The `open_*` counts every entry of `section` (`policy` or `context`) had before D44 (step 9b), frozen in `tests/snapshots/trees/open_counts.txt` from the
/// snapshots of the commit before it: one line per entry, `<file> <qualname>: <kind> <n>, …` or `none`. D44 changes the edge of a required trait method and
/// not the count of its `open` kind, and this is the proof.
pub(crate) fn frozen_open_counts(section: &str) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/trees/open_counts.txt"),
    )
    .expect("the frozen counts");
    let mut lines = text
        .lines()
        .skip_while(|line| *line != format!("# {section}").as_str());
    lines.next();
    lines
        .take_while(|line| !line.starts_with('#'))
        .map(|line| {
            let (entry, counts) = line.split_once(": ").expect("`<entry>: <counts>`");
            (entry.to_string(), counts.to_string())
        })
        .collect()
}

/// A breakdown's `open` counts (`Breakdown::open`) in the form of `frozen_open_counts`.
pub(crate) fn open_counts_of(open: &BTreeMap<String, u64>) -> String {
    if open.is_empty() {
        return "none".to_string();
    }
    open.iter()
        .map(|(kind, count)| format!("{kind} {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

static OWN_LOAD: Mutex<()> = Mutex::new(());

/// Taken first by a test that loads a project of its own, and held to its end: one such load at a time in a test program. A load of the engine reads the
/// standard library, which is most of what it costs (about 500 MB), and the test harness runs as many tests at once as the machine has cores, so eight
/// loads at once were 5 GB. The tests that ask the same of one fixture share a load instead (`shared`).
pub(crate) fn one_load() -> MutexGuard<'static, ()> {
    OWN_LOAD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The one `T` of a test program that every test asking for it uses, made by `make` the first time and used by one test at a time (a failed test does not
/// poison it for the others). The cell is the caller's, a `static` of its own type.
pub(crate) fn shared<T>(
    cell: &'static OnceLock<Mutex<T>>,
    make: impl FnOnce() -> T,
) -> MutexGuard<'static, T> {
    cell.get_or_init(|| Mutex::new(make()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}
