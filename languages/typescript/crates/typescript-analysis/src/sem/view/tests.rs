//! The views are total (`docs/03-typescript/10-review-fixes.md` §6.2 (d)): every cast is tried on every node of every bundled library file and of every
//! fixture, and on success every accessor of the view is called. A kind a view lets through that the engine's accessor does not handle panics here, before a
//! user's file does; the same test is the drift check of an engine bump.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::scratch::{scratch, write};

use super::*;

/// Tries every cast on `node` and calls every accessor of each view that succeeded. Nothing here may panic.
fn touch(node: P<Node>, text: &str) {
    use std::hint::black_box as seen;
    if let Some(view) = HasExpression::cast(node) {
        seen(view.expression());
    }
    if let Some(view) = Wrapper::cast(node) {
        seen(view.inner());
    }
    if let Some(view) = CallOrNew::cast(node) {
        seen(view.callee());
        seen(view.arguments());
    }
    if let Some(view) = Initialized::cast(node) {
        seen(view.initializer());
    }
    if let Some(view) = Members::cast(node) {
        seen(view.members());
    }
    if let Some(view) = Statements::cast(node) {
        seen(view.statements());
    }
    if let Some(view) = Elements::cast(node) {
        seen(view.elements());
    }
    if let Some(view) = Properties::cast(node) {
        seen(view.properties());
    }
    if let Some(view) = JsxTag::cast(node) {
        seen(view.tag_name());
    }
    if let Some(view) = NameToken::cast(node) {
        seen(view.value());
    }
    if let Some(view) = Binary::cast(node) {
        seen(view.left());
        seen(view.right());
        seen(view.operator());
    }
    if let Some(view) = ElementAccess::cast(node) {
        seen(view.argument());
    }
    if let Some(view) = TaggedTemplate::cast(node) {
        seen(view.tag());
    }
    if let Some(view) = Conditional::cast(node) {
        seen(view.condition());
    }
    if let Some(view) = Binding::cast(node) {
        seen(view.is_rest());
    }
    if let Some(view) = TypeReference::cast(node) {
        seen(view.type_name());
    }
    if let Some(view) = StaticBlock::cast(node) {
        seen(view.body());
    }
    // the spelling reads text only: any node, whatever its kind
    seen(spelled(text, node));
}

/// Every node of `file`, with how many there were.
fn walk(file: P<SourceFile>, kinds: &mut HashSet<Kind>) -> u64 {
    let text = file.text();
    let mut count = 0;
    let mut pending = vec![file.as_node()];
    while let Some(node) = pending.pop() {
        count += 1;
        kinds.insert(node.kind());
        touch(node, text);
        node.for_each_child(&mut |child| {
            pending.push(child);
            false
        });
    }
    count
}

/// A source file to open a fixture from: the first TypeScript or JavaScript file under it that is not a declaration file and not in `node_modules`.
fn candidates(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "node_modules") {
                candidates(&path, found);
            }
        } else if path
            .to_str()
            .is_some_and(|name| !name.ends_with(".d.ts") && Dialect::from_path(name).is_some())
        {
            found.push(path);
        }
    }
}

/// Walks the program of `root`, opened from the first of its files that opens: the libraries once (`done` holds the names of those walked), the rest of the
/// program every time. Returns the nodes walked, and how many of them were in a library.
fn walk_project(
    root: &Path,
    done: &mut HashSet<String>,
    kinds: &mut HashSet<Kind>,
) -> Option<(u64, u64)> {
    let mut files = Vec::new();
    candidates(root, &mut files);
    for file in files.iter().take(5) {
        let Ok(sem) = Sem::open(root, file) else {
            continue;
        };
        let (mut nodes, mut library_nodes) = (0, 0);
        for file in sem.files() {
            let is_library = sem.package_of(file).as_deref() == Some(BUNDLED_LIBS);
            if is_library && !done.insert(file.node.file_name().to_owned()) {
                continue;
            }
            let walked = walk(file.node, kinds);
            nodes += walked;
            if is_library {
                library_nodes += walked;
            }
        }
        return Some((nodes, library_nodes));
    }
    None
}

#[test]
fn every_cast_and_every_accessor_of_every_view_is_total_on_the_libraries_and_the_fixtures() {
    let started = std::time::Instant::now();
    let mut done = HashSet::new();
    let mut kinds = HashSet::new();

    // every library the engine ships that a config can ask for, in one program
    let libs = scratch("view-libs");
    write(
        libs.join("tsconfig.json"),
        r#"{ "compilerOptions": { "lib": ["esnext", "dom", "dom.iterable", "webworker", "scripthost"], "types": [] }, "include": ["a.ts"] }"#,
    );
    write(libs.join("a.ts"), "export const a = 1;\n");
    let (_, library_nodes) =
        walk_project(&libs, &mut done, &mut kinds).expect("the libraries project opens");
    assert!(
        library_nodes > 150_000,
        "only {library_nodes} nodes of libraries were walked"
    );

    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut roots: Vec<PathBuf> = Vec::new();
    for folder in ["fixtures", "../../../../scenarios"] {
        if let Ok(entries) = std::fs::read_dir(tests.join(folder)) {
            roots.extend(entries.flatten().map(|entry| entry.path()));
        }
    }
    roots.push(tests.join("policy"));
    roots.sort();
    let mut projects = 0;
    let mut nodes = library_nodes;
    for root in roots.iter().filter(|root| root.is_dir()) {
        // a scenario keeps its TypeScript in a folder of its own
        let root = if root.join("typescript").is_dir() {
            root.join("typescript")
        } else {
            root.clone()
        };
        if let Some((walked, _)) = walk_project(&root, &mut done, &mut kinds) {
            projects += 1;
            nodes += walked;
        }
    }
    eprintln!(
        "views total: {nodes} nodes in {projects} fixture projects and the libraries, {:?}",
        started.elapsed()
    );
    assert!(projects > 20, "only {projects} fixture projects opened");

    // a kind of a view that no node of the walk has was never tried: the walk would say nothing about it
    for (view, listed) in [
        ("HasExpression", HasExpression::KINDS),
        ("Wrapper", Wrapper::KINDS),
        ("CallOrNew", CallOrNew::KINDS),
        ("Initialized", Initialized::KINDS),
        ("Members", Members::KINDS),
        ("Statements", Statements::KINDS),
        ("Elements", Elements::KINDS),
        ("Properties", Properties::KINDS),
        ("JsxTag", JsxTag::KINDS),
        ("NameToken", NameToken::KINDS),
        ("Binary", Binary::KINDS),
        ("ElementAccess", ElementAccess::KINDS),
        ("TaggedTemplate", TaggedTemplate::KINDS),
        ("Conditional", Conditional::KINDS),
        ("Binding", Binding::KINDS),
        ("TypeReference", TypeReference::KINDS),
        ("StaticBlock", StaticBlock::KINDS),
    ] {
        let missing: Vec<&Kind> = listed
            .iter()
            .filter(|kind| !kinds.contains(*kind))
            .collect();
        assert!(
            missing.is_empty(),
            "{view}: no node of {missing:?} in the walk"
        );
    }
}

/// Every view's kinds are distinct and each cast answers for exactly its own kinds.
#[test]
fn a_cast_answers_for_its_kinds_and_for_no_other() {
    // one node of a kind in a view and one that is in none: a tiny program
    let root = scratch("view-casts");
    write(
        root.join("a.ts"),
        "const x = f(1);\nclass C { m = 1; }\nlet y = (x as number)!;\n",
    );
    let sem = Sem::open(&root, &root.join("a.ts")).unwrap();
    let file = sem.files().find(|file| sem.is_project_code(*file)).unwrap();
    let mut kinds = HashSet::new();
    let mut pending = vec![file.node.as_node()];
    while let Some(node) = pending.pop() {
        kinds.insert(node.kind());
        let call = CallOrNew::cast(node).is_some();
        assert_eq!(call, CallOrNew::KINDS.contains(&node.kind()));
        let wrapper = Wrapper::cast(node).is_some();
        assert_eq!(wrapper, Wrapper::KINDS.contains(&node.kind()));
        node.for_each_child(&mut |child| {
            pending.push(child);
            false
        });
    }
    assert!(kinds.contains(&Kind::CallExpression) && kinds.contains(&Kind::NonNullExpression));
}
