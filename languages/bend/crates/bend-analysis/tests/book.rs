//! The book (docs/08-bend/03 §2): one fixture folder per rule under `tests/fixtures/book/<case>/`, each opened and compared with a reviewed snapshot in
//! `tests/snapshots/book/<case>.book` (the files in load order with their namespaces, the tables sorted by `ns:name`, the counters), and the rule as plain
//! assertions beside it. A case's entry is its `main.bend`, and its root the folder, unless `OPENED` says otherwise.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use analyzer_testkit::expect_file;
use bend_analysis::{AliasTarget, Book, DeclId, DefKind, Resolved};

const FIXTURES: &str = "tests/fixtures/book";
const SNAPSHOTS: &str = "tests/snapshots/book";

/// The cases that are opened from a folder inside them: (case, root from the case, entry from the root).
const OPENED: &[(&str, &str, &str)] = &[("outside_root", "app", "main.bend")];

fn open(case: &str) -> Book {
    let (root, entry) = OPENED
        .iter()
        .find(|(c, ..)| *c == case)
        .map_or((".", "main.bend"), |&(_, root, entry)| (root, entry));
    Book::open(&Path::new(FIXTURES).join(case).join(root), Path::new(entry))
        .unwrap_or_else(|| panic!("{case}: the book did not open"))
}

fn id(book: &Book, qualified: &str) -> DeclId {
    let found: Vec<DeclId> = book
        .declared()
        .into_iter()
        .filter(|(name, _)| name == qualified)
        .map(|(_, id)| id)
        .collect();
    assert_eq!(found.len(), 1, "{qualified} is declared once in the table");
    found[0]
}

/// What `name` written in the file at `file` (from the root) means.
fn resolve(book: &Book, file: &str, name: &str) -> Resolved {
    let at = book
        .file_at(Path::new(file))
        .unwrap_or_else(|| panic!("{file} is not in the book"));
    book.resolve(at, name)
}

fn counter(book: &Book, key: &str) -> u64 {
    book.stats().language.get(key).copied().unwrap_or(0)
}

fn decl_text(id: DeclId) -> String {
    format!("{}:{}", id.file().index(), id.def())
}

/// Everything a reviewer wants to see about a book, with no absolute path in it.
fn show(book: &Book) -> String {
    let mut out = String::new();
    out.push_str("files, in load order\n");
    for (at, file) in book.files().iter().enumerate() {
        let aliases: Vec<String> = file
            .aliases
            .iter()
            .map(|a| match a.target {
                AliasTarget::File(f) => format!("{} -> #{}", a.name, f.index()),
                AliasTarget::Hub => format!("{} -> hub", a.name),
                AliasTarget::Missing => format!("{} -> missing", a.name),
                _ => format!("{} -> ?", a.name),
            })
            .collect();
        let _ = writeln!(
            out,
            "  #{at} {}  ns={:?}  {}  [{}]",
            file.relative,
            file.namespace,
            if file.in_root { "in root" } else { "OUTSIDE" },
            aliases.join(", ")
        );
    }
    let _ = writeln!(out, "imports Base: {}", book.imports_base());
    out.push_str("decls (file:def)\n");
    for d in book.decls() {
        let kind = match &d.kind {
            DefKind::Other(name) => name.clone(),
            other => format!("{other:?}").to_lowercase(),
        };
        let proves = d
            .law
            .map_or(String::new(), |law| format!("  proves {}", decl_text(law)));
        let broken = if book.def(d.id).is_some_and(|def| def.syntax_error) {
            "  SYNTAX ERROR"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "  {} {} {kind}{proves}{broken}",
            decl_text(d.id),
            d.qualified
        );
    }
    out.push_str("tlds, sorted\n");
    for (name, id) in book.declared() {
        let _ = writeln!(out, "  {name} -> {}", decl_text(id));
    }
    out.push_str("ctrs, sorted\n");
    for (name, id) in book.constructors() {
        let _ = writeln!(out, "  {name} -> {}", decl_text(id));
    }
    out.push_str("counters\n");
    for (key, n) in &book.stats().language {
        let _ = writeln!(out, "  {key}: {n}");
    }
    out
}

#[test]
fn every_case_equals_its_snapshot() {
    let mut cases: Vec<String> = fs::read_dir(FIXTURES)
        .expect("fixtures")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    cases.sort();
    assert!(cases.len() >= 13, "the cases: {cases:?}");
    for case in cases {
        let path = Path::new(SNAPSHOTS).join(format!("{case}.book"));
        expect_file(&path, &show(&open(&case)));
    }
}

#[test]
fn an_alias_names_the_file_and_the_namespace_is_from_the_entrys_folder() {
    let book = open("alias");
    let ns: Vec<&str> = book.files().iter().map(|f| f.namespace.as_str()).collect();
    // each file after the files it imports; the entry is last, and its namespace is empty, as Bend's is
    assert_eq!(ns, ["lib", "sub/deep", ""]);
    assert_eq!(book.entry().index(), 2);
    assert!(book.files().iter().all(|f| f.in_root));
    assert_eq!(
        resolve(&book, "main.bend", "L.double"),
        Resolved::Project(id(&book, "lib:double"))
    );
    assert_eq!(
        resolve(&book, "main.bend", "D.answer"),
        Resolved::Project(id(&book, "sub/deep:answer"))
    );
    // the entry's own def has the bare name, and a file's own def is found without an alias
    assert_eq!(
        resolve(&book, "main.bend", "main"),
        Resolved::Project(id(&book, "main"))
    );
    // an alias the file did not write means nothing there
    assert_eq!(resolve(&book, "lib.bend", "L.double"), Resolved::Unknown);
    // Base's, through `import Base`
    assert_eq!(resolve(&book, "main.bend", "U32.add"), Resolved::Base);
    assert_eq!(resolve(&book, "main.bend", "Nope.add"), Resolved::Unknown);
    assert!(book.stats().language.is_empty());
}

#[test]
fn a_diamond_loads_the_shared_file_once_and_both_aliases_agree() {
    let book = open("diamond");
    let shared: Vec<&str> = book
        .files()
        .iter()
        .filter(|f| f.relative == "shared.bend")
        .map(|f| f.relative.as_str())
        .collect();
    assert_eq!(shared.len(), 1);
    assert_eq!(book.files().len(), 4);
    let one = Resolved::Project(id(&book, "shared:one"));
    assert_eq!(resolve(&book, "a.bend", "S.one"), one);
    assert_eq!(resolve(&book, "b.bend", "T.one"), one);
    assert_eq!(counter(&book, "import_cycle"), 0);
    assert!(book.stats().language.is_empty());
}

#[test]
fn a_cycle_is_counted_and_not_followed_twice() {
    let book = open("cycle");
    // main, a and b, once each: b imports a (being read) and main (being read)
    assert_eq!(book.files().len(), 3);
    assert_eq!(counter(&book, "import_cycle"), 2);
    // the alias of a cycle still names its file, so what is declared there resolves
    assert_eq!(
        resolve(&book, "b.bend", "A.a"),
        Resolved::Project(id(&book, "a:a"))
    );
    assert_eq!(
        resolve(&book, "b.bend", "M.main"),
        Resolved::Project(id(&book, "main"))
    );
    assert_eq!(counter(&book, "import_missing"), 0);
}

#[test]
fn a_missing_import_is_counted_and_its_alias_means_nothing() {
    let book = open("missing");
    assert_eq!(counter(&book, "import_missing"), 1);
    assert_eq!(book.files().len(), 2);
    assert_eq!(resolve(&book, "main.bend", "N.gone"), Resolved::Unknown);
    // the rest of the book is whole
    assert_eq!(
        resolve(&book, "main.bend", "L.here"),
        Resolved::Project(id(&book, "lib:here"))
    );
    assert_eq!(resolve(&book, "main.bend", "U32.add"), Resolved::Base);
    let main = &book.files()[book.entry().index()];
    assert_eq!(main.aliases[0].target, AliasTarget::Missing);
}

#[test]
fn a_hub_import_is_never_fetched_nor_looked_up() {
    let book = open("hub");
    assert_eq!(book.files().len(), 1);
    // once per import line
    assert_eq!(counter(&book, "hub_import"), 2);
    assert_eq!(counter(&book, "import_missing"), 0);
    assert_eq!(
        resolve(&book, "main.bend", "P.f"),
        Resolved::Hub("P".to_owned())
    );
    assert_eq!(
        resolve(&book, "main.bend", "U.g"),
        Resolved::Hub("U".to_owned())
    );
    // a name with no dot is not through an alias
    assert_eq!(resolve(&book, "main.bend", "P"), Resolved::Unknown);
}

#[test]
fn a_def_that_fills_an_aliased_law_is_that_law_and_the_table_means_the_proof() {
    let book = open("law_alias");
    let ns: Vec<&str> = book.files().iter().map(|f| f.namespace.as_str()).collect();
    assert_eq!(ns, ["code", "LAWS", ""]);
    let law = book
        .decls()
        .iter()
        .find(|d| {
            d.qualified == "LAWS:sorted" && matches!(&d.kind, DefKind::Other(k) if k == "law")
        })
        .expect("the law");
    let proof = book
        .decls()
        .iter()
        .find(|d| d.qualified == "LAWS:sorted" && d.kind == DefKind::Function)
        .expect("the proof, named in the law's namespace though it is written in main.bend");
    // the proof is written in the entry file and says which law it fills
    assert_eq!(proof.id.file(), book.entry());
    assert_eq!(proof.law, Some(law.id));
    assert_eq!(law.law, None);
    // a call through the alias means the proof (the code), from the proof's own file and from any that has the alias
    assert_eq!(
        resolve(&book, "main.bend", "Laws.sorted"),
        Resolved::Project(proof.id)
    );
    // with no proof, the law is what the name means
    let lonely = id(&book, "LAWS:lonely");
    assert_eq!(lonely.file().index(), 1);
    assert_eq!(
        resolve(&book, "main.bend", "Laws.lonely"),
        Resolved::Project(lonely)
    );
    // the table lists the name once, as the proof
    let names: Vec<String> = book.declared().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names.iter().filter(|n| *n == "LAWS:sorted").count(), 1);
    assert!(book.stats().language.is_empty());
}

#[test]
fn a_duplicate_declaration_keeps_the_first_and_is_counted() {
    let book = open("duplicate");
    // `twice` twice in main.bend, and a law proved by two files
    assert_eq!(counter(&book, "duplicate_declaration"), 2);
    let twice: Vec<&bend_analysis::DeclEntry> = book
        .decls()
        .iter()
        .filter(|d| d.qualified == "twice")
        .collect();
    assert_eq!(twice.len(), 2, "both are in the outline");
    assert_eq!(
        resolve(&book, "main.bend", "twice"),
        Resolved::Project(twice[0].id)
    );
    // the first proof in load order fills the law; the second is a def with the law's name that the table does not mean
    let p1 = book
        .decls()
        .iter()
        .find(|d| d.qualified == "lib:ok" && d.law.is_some() && d.id.file().index() == 1)
        .expect("the first proof");
    assert_eq!(resolve(&book, "p1.bend", "L.ok"), Resolved::Project(p1.id));
    assert_eq!(resolve(&book, "p2.bend", "L.ok"), Resolved::Project(p1.id));
    let p2 = book
        .decls()
        .iter()
        .find(|d| d.qualified == "lib:ok" && d.id.file().index() == 2)
        .expect("the second proof is in the outline");
    assert_eq!(p2.law, None);
}

#[test]
fn a_name_base_has_is_declared_by_the_project_and_counted() {
    let book = open("base_name");
    // `def Nat.add` and the constructor `Some`: bend's `parse_fresh` refuses both, we count them
    assert_eq!(counter(&book, "duplicate_declaration"), 2);
    // the project's own def wins over Base's, from its own file
    assert_eq!(
        resolve(&book, "main.bend", "Nat.add"),
        Resolved::Project(id(&book, "Nat.add"))
    );
    let ctrs = book.constructors();
    let some = ctrs.iter().find(|(n, _)| n == "Some").expect("Some");
    assert_eq!(some.1, id(&book, "Opt"));
    let at = book.entry();
    assert_eq!(book.ctor(at, "Some"), Resolved::Project(id(&book, "Opt")));
    // a name Base has and the project does not
    assert_eq!(book.ctor(at, "None"), Resolved::Base);
    assert_eq!(resolve(&book, "main.bend", "U32.add"), Resolved::Base);
    assert_eq!(resolve(&book, "main.bend", "Nat.mul"), Resolved::Base);
    // a name the project has and Base does not
    assert_eq!(
        book.ctor(at, "Nothing"),
        Resolved::Project(id(&book, "Opt"))
    );
}

#[test]
fn base_means_something_only_when_the_book_imports_it() {
    let book = open("no_base");
    assert!(!book.imports_base());
    assert_eq!(resolve(&book, "main.bend", "Nat.add"), Resolved::Unknown);
    assert_eq!(book.ctor(book.entry(), "Some"), Resolved::Unknown);
}

#[test]
fn a_file_above_the_root_is_loaded_and_counted() {
    let book = open("outside_root");
    assert_eq!(counter(&book, "file_outside_root"), 1);
    let lib = book
        .files()
        .iter()
        .find(|f| !f.in_root)
        .expect("the file outside");
    assert_eq!(lib.relative, "../shared/lib.bend");
    assert_eq!(lib.namespace, "../shared/lib");
    assert!(lib.path.is_absolute());
    // its defs are the project's: a def with a path outside the root
    assert_eq!(
        resolve(&book, "main.bend", "Lib.b"),
        Resolved::Project(id(&book, "../shared/lib:b"))
    );
    // the entry must be under the root
    let root = Path::new(FIXTURES).join("outside_root/app");
    assert!(Book::open(&root, Path::new("../shared/lib.bend")).is_none());
}

#[test]
fn a_file_with_a_syntax_error_is_in_the_book_with_its_outline() {
    let book = open("syntax_error");
    let main = &book.files()[book.entry().index()];
    let names: Vec<(&str, bool)> = main
        .outline
        .defs
        .iter()
        .map(|d| (d.qualname.as_str(), d.syntax_error))
        .collect();
    assert_eq!(names, [("first", false), ("broken", true), ("last", false)]);
    assert!(!main.outline.syntax_errors.is_empty());
    // what is declared after the broken def is still declared, and the broken def is a name too
    assert_eq!(
        resolve(&book, "main.bend", "first"),
        Resolved::Project(id(&book, "first"))
    );
    assert_eq!(
        resolve(&book, "main.bend", "broken"),
        Resolved::Project(id(&book, "broken"))
    );
}

#[test]
fn a_constructor_resolves_to_the_def_of_its_type() {
    let book = open("ctors");
    let shape = id(&book, "lib:Shape");
    let at = book.entry();
    assert_eq!(book.ctor(at, "L.Circle"), Resolved::Project(shape));
    assert_eq!(book.ctor(at, "L.Square"), Resolved::Project(shape));
    assert_eq!(book.ctor(at, "Dot"), Resolved::Project(id(&book, "Local")));
    // not through the file that does not have it, and a constructor is not a def
    assert_eq!(book.ctor(at, "L.Triangle"), Resolved::Unknown);
    assert_eq!(book.resolve(at, "L.Circle"), Resolved::Unknown);
    assert_eq!(book.decls().len(), 3);
}

#[test]
fn a_name_declared_below_its_use_still_resolves() {
    let book = open("forward");
    assert_eq!(
        resolve(&book, "main.bend", "b"),
        Resolved::Project(id(&book, "b"))
    );
}

#[test]
fn every_decl_is_found_by_its_id_and_the_files_by_their_path() {
    for case in ["alias", "law_alias", "diamond", "ctors"] {
        let book = open(case);
        for d in book.decls() {
            assert_eq!(book.decl(d.id), Some(d));
            assert!(book.def(d.id).is_some());
        }
        for (at, file) in book.files().iter().enumerate() {
            assert_eq!(
                book.file_at(&file.path)
                    .map(bend_analysis::FileIndex::index),
                Some(at)
            );
            assert_eq!(file.outline.defs.len(), {
                book.decls()
                    .iter()
                    .filter(|d| d.id.file().index() == at)
                    .count()
            });
        }
    }
}

#[test]
fn an_entry_that_is_not_there_gives_nothing() {
    let root = Path::new(FIXTURES).join("alias");
    assert!(Book::open(&root, Path::new("nope.bend")).is_none());
    assert!(Book::open(&root.join("no_such_folder"), Path::new("main.bend")).is_none());
    // a folder is not a file
    assert!(Book::open(&root, Path::new("sub")).is_none());
}

#[test]
fn the_entry_may_be_given_by_its_absolute_path() {
    let root = Path::new(FIXTURES).join("alias");
    let absolute = fs::canonicalize(root.join("main.bend")).expect("fixture");
    let book = Book::open(&root, &absolute).expect("opens");
    assert_eq!(book.files().len(), 3);
}

#[test]
fn opening_twice_gives_the_same_book() {
    for case in ["alias", "cycle", "law_alias", "duplicate"] {
        assert_eq!(show(&open(case)), show(&open(case)));
    }
}
