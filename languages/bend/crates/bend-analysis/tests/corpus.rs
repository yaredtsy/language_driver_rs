//! BD3 against Bend's own corpus (docs/08-bend/03 §2, §5 step 3). Ignored by default: these need the bend clone and the lists and the Ref oracle's output
//! that two scripts write.
//!
//!     scripts/bend-corpus.sh ../language_clone/bend                              # the lists: which files the loader accepts, what it sees in them
//!     bun scripts/bend-refs.ts ../language_clone/bend languages/bend/spikes/out  # the Ref oracle, for every demo entry
//!     BEND_CLONE=../language_clone/bend cargo test -p bend-analysis --release --test corpus -- --ignored --nocapture
//!
//! `BEND_CORPUS_OUT` names the folder of the lists (default `languages/bend/spikes/out`).
//!
//! - `every_ref_the_oracle_prints_resolves`: Bend's own loader leaves a `Ref` for every name that is a def; each one, found again by the name written at its
//!   span, resolves in our book to the def Bend says it is (never `Unknown`).
//! - `every_accepted_entry_opens_with_bends_names`: for every file the loader accepts, the book of that file has the entry's declarations with Bend's
//!   names and kinds, in order, and counts no cycle and no missing import.
//! - `every_ref_the_oracle_prints_is_accounted_for` (BD4): each Ref that is in a def's body falls in a place the walk read: a site (a call, a construct, an
//!   external, a passed edge, an unresolved one), a term it skipped as a type, or one it counted or knows to be data. The first ten that fall in none are printed.
//! - `demos_counters`: what the walk counted over every def of each demo's largest book.
//! - `context_layer_on_the_demos` (BC step 7): the 16 books of the sweep asked whole at `main`, at `Default` and at `Precise`; what each of the 55 sites of layer 1 is
//!   in every context of its def, the classes, the cost of the layer, and why `io_rollback_netcode` has more contexts. Run it with `--release` (it times requests).
//! - `measure`: time and bytes of opening (a) each demo, (b) the largest book of the corpus by file count, (c) `base.bend` alone.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bend_analysis::{
    Book, Cancel, ContextId, DeclEntry, DefKind, EdgeTo, FileIndex, Graph, Limits, MarkerStyle,
    Options, Precision, Resolved, StableDef, Walked, WalkedKind, call_graph, walked,
};

/// The repo's root, which a relative `BEND_CLONE` is from (a test runs in its crate's folder).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..")
}

fn clone_dir() -> PathBuf {
    let given =
        PathBuf::from(std::env::var_os("BEND_CLONE").expect(
            "BEND_CLONE names the bend clone (../language_clone/bend, from the repo's root)",
        ));
    repo_root().join(given)
}

fn out_dir() -> PathBuf {
    std::env::var_os("BEND_CORPUS_OUT").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spikes/out"),
        PathBuf::from,
    )
}

fn read_list(file: &str) -> String {
    fs::read_to_string(out_dir().join(file)).unwrap_or_else(|_| {
        panic!("{file}: run scripts/bend-corpus.sh (and scripts/bend-refs.ts) first")
    })
}

/// A book of a file of the clone, rooted at the file's folder (what a person who opens one file asks for).
fn open_entry(entry: &Path) -> Option<Book> {
    Book::open(entry.parent()?, Path::new(entry.file_name()?))
}

fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

#[test]
#[ignore = "needs BEND_CLONE and the Ref oracle's output (scripts/bend-refs.ts)"]
fn every_ref_the_oracle_prints_resolves() {
    let clone = clone_dir();
    let refs = out_dir().join("refs");
    let mut tables: Vec<PathBuf> = fs::read_dir(&refs)
        .unwrap_or_else(|_| panic!("{}: run scripts/bend-refs.ts first", refs.display()))
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "tsv"))
        .collect();
    tables.sort();
    assert!(!tables.is_empty(), "no oracle output in {}", refs.display());

    let (mut total, mut by_name, mut sugar, mut project, mut base) = (0, 0, 0, 0, 0);
    let mut mismatches: Vec<String> = Vec::new();
    for table in &tables {
        // `demos__proof_insertion_sort__PROOF.bend.tsv` is `demos/proof_insertion_sort/PROOF.bend`
        let stem = table
            .file_stem()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        let entry = clone.join(stem.split("__").collect::<PathBuf>());
        let dir = entry.parent().expect("folder").to_path_buf();
        let book = open_entry(&entry).unwrap_or_else(|| panic!("{stem}: the book did not open"));
        let mut units: HashMap<FileIndex, Vec<u16>> = HashMap::new();
        for line in fs::read_to_string(table).expect("oracle table").lines() {
            let cells: Vec<&str> = line.split('\t').collect();
            let [file, _def, callee, span] = cells[..] else {
                panic!("{stem}: a line of four cells: {line}");
            };
            total += 1;
            let Some(at) = book.file_at(&dir.join(file)) else {
                mismatches.push(format!("{stem}: {file} is not in the book"));
                continue;
            };
            let text = &book.files()[at.index()].text;
            let text16 = units
                .entry(at)
                .or_insert_with(|| text.encode_utf16().collect());
            let (beg, end) = span.split_once('-').expect("beg-end");
            let (beg, end): (usize, usize) = (beg.parse().expect("beg"), end.parse().expect("end"));
            let written = text16
                .get(beg..end)
                .map(String::from_utf16_lossy)
                .unwrap_or_default();
            // a Ref that Bend makes up (`a + b : T`, `a[i]`, `3n+p`) has the sugar's span: the name is what Bend resolved, without its namespace
            let name = if is_name(&written) {
                by_name += 1;
                written
            } else {
                sugar += 1;
                callee
                    .rsplit_once(':')
                    .map_or(callee, |(_, n)| n)
                    .to_owned()
            };
            let ours = book.resolve(at, &name);
            let ok = match &ours {
                Resolved::Project(id) => {
                    project += 1;
                    book.decl(*id).is_some_and(|d| d.qualified == callee)
                }
                Resolved::Base => {
                    base += 1;
                    !callee.contains(':')
                }
                _ => false,
            };
            if !ok {
                mismatches.push(format!(
                    "{stem}: {file} {span} `{name}` -> Bend {callee:?}, ours {ours:?}"
                ));
            }
        }
    }
    println!(
        "oracle: {} tables, {total} refs: {by_name} found by the name written at the span, {sugar} made up by sugar; \
         ours: {project} Project, {base} Base, {} mismatches",
        tables.len(),
        mismatches.len()
    );
    for m in mismatches.iter().take(25) {
        println!("  {m}");
    }
    assert!(
        mismatches.is_empty(),
        "{} refs do not resolve as Bend's do",
        mismatches.len()
    );
}

/// The names Bend sees in each accepted file, from `parse-decls.tsv`: `(kind, name)` in order.
fn bends_decls() -> BTreeMap<String, Vec<(String, String)>> {
    let mut by_file: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for line in read_list("parse-decls.tsv").lines() {
        let mut cells = line.split('\t');
        let (Some(file), Some(kind), Some(name)) = (cells.next(), cells.next(), cells.next())
        else {
            continue;
        };
        by_file
            .entry(file.to_owned())
            .or_default()
            .push((kind.to_owned(), name.to_owned()));
    }
    by_file
}

fn kind_word(kind: &DefKind) -> &'static str {
    match kind {
        DefKind::Enum => "type",
        DefKind::Function => "def",
        DefKind::Other(k) if k == "law" => "law",
        _ => "?",
    }
}

#[test]
#[ignore = "needs BEND_CLONE and the lists of scripts/bend-corpus.sh"]
fn every_accepted_entry_opens_with_bends_names() {
    let clone = clone_dir();
    let bends = bends_decls();
    // a file with no declaration (imports and comments only) is not in the list
    let none: Vec<(String, String)> = Vec::new();
    let accepted = read_list("parse-accepted.txt");
    let (mut entries, mut declarations, mut files, mut ctors) = (0, 0, 0, 0);
    let mut bad: Vec<String> = Vec::new();
    for rel in accepted.lines() {
        let Some(book) = open_entry(&clone.join(rel)) else {
            bad.push(format!("{rel}: the book did not open"));
            continue;
        };
        entries += 1;
        files += book.files().len();
        let entry = book.entry();
        let ours: Vec<(&str, &str)> = book
            .decls()
            .iter()
            .filter(|d| d.id.file() == entry)
            .map(|d: &DeclEntry| (kind_word(&d.kind), d.qualified.as_str()))
            .collect();
        let bends_here = bends.get(rel).unwrap_or(&none);
        let theirs: Vec<(&str, &str)> = bends_here
            .iter()
            .filter(|(kind, _)| kind != "ctr")
            .map(|(k, n)| (k.as_str(), n.as_str()))
            .collect();
        declarations += ours.len();
        if ours != theirs {
            let at = ours
                .iter()
                .zip(&theirs)
                .position(|(a, b)| a != b)
                .unwrap_or(ours.len().min(theirs.len()));
            bad.push(format!(
                "{rel}: {} defs, Bend sees {}; first difference at {at}: ours {:?}, Bend's {:?}",
                ours.len(),
                theirs.len(),
                ours.get(at),
                theirs.get(at)
            ));
        }
        // the constructors of the entry's own types: bare names (every other file's have a namespace)
        let mut own_ctors: Vec<String> = book
            .constructors()
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| !name.contains(':'))
            .collect();
        own_ctors.sort();
        let mut their_ctors: Vec<String> = bends_here
            .iter()
            .filter(|(kind, _)| kind == "ctr")
            .map(|(_, n)| n.clone())
            .collect();
        their_ctors.sort();
        ctors += own_ctors.len();
        if own_ctors != their_ctors {
            bad.push(format!(
                "{rel}: constructors differ: {} ours, {} Bend's",
                own_ctors.len(),
                their_ctors.len()
            ));
        }
        for key in [
            "import_cycle",
            "import_missing",
            "hub_import",
            "import_no_alias",
            "duplicate_declaration",
        ] {
            if let Some(n) = book.stats().language.get(key) {
                bad.push(format!("{rel}: Bend accepts it, and we count {key} = {n}"));
            }
        }
    }
    println!(
        "accepted entries: {entries}, {files} files opened, {declarations} entry declarations and {ctors} constructors as Bend names them, {} problems",
        bad.len()
    );
    for b in bad.iter().take(25) {
        println!("  {b}");
    }
    assert!(bad.is_empty());
    assert!(entries > 1700, "the lists name {entries} accepted files");

    // the files the loader refuses open all the same, and tests/import is where the cycles and the misses are: what the book counted
    let all = read_list("files.txt");
    let mut refused = 0;
    let mut counters: BTreeMap<String, u64> = BTreeMap::new();
    for rel in all.lines().filter(|r| !accepted.lines().any(|a| a == *r)) {
        let book =
            open_entry(&clone.join(rel)).unwrap_or_else(|| panic!("{rel}: the book did not open"));
        refused += 1;
        for (key, n) in &book.stats().language {
            *counters.entry(key.clone()).or_default() += n;
        }
        if rel.starts_with("tests/import/") && !book.stats().language.is_empty() {
            println!("  {rel}: {:?}", book.stats().language);
        }
    }
    println!("entries the loader refuses, opened: {refused}; counters over them: {counters:?}");
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times[times.len() / 2]
}

/// How long `open` takes, the median of `runs`.
fn time_open(entry: &Path, runs: usize) -> Duration {
    median(
        (0..runs)
            .map(|_| {
                let start = Instant::now();
                let book = open_entry(entry).expect("opens");
                let took = start.elapsed();
                std::hint::black_box(book);
                took
            })
            .collect(),
    )
}

/// Bytes held, from lengths and `size_of` (the crate forbids unsafe code, so there is no counting allocator): (texts, outlines, tables).
fn sizes(book: &Book) -> (usize, usize, usize) {
    let texts = book.files().iter().map(|f| f.text.len()).sum();
    let outlines = book
        .files()
        .iter()
        .flat_map(|f| &f.outline.defs)
        .map(|d| {
            size_of::<bend_analysis::Def>()
                + d.qualname.len()
                + d.name.len()
                + d.params
                    .iter()
                    .map(|p| size_of::<String>() + p.len())
                    .sum::<usize>()
                + d.marker.as_ref().map_or(0, String::len)
        })
        .sum();
    let entry = size_of::<String>() + size_of::<bend_analysis::DeclId>();
    let tables = book
        .decls()
        .iter()
        .map(|d| size_of::<DeclEntry>() + d.qualified.len())
        .sum::<usize>()
        + book
            .declared()
            .iter()
            .map(|(n, _)| entry + n.len())
            .sum::<usize>()
        + book
            .constructors()
            .iter()
            .map(|(n, _)| entry + n.len())
            .sum::<usize>();
    (texts, outlines, tables)
}

fn row(label: &str, entry: &Path, runs: usize) {
    let book = open_entry(entry).expect("opens");
    let (texts, outlines, tables) = sizes(&book);
    println!(
        "| {label} | {} | {} | {} | {} | {} | {:.2} |",
        book.files().len(),
        book.decls().len(),
        texts,
        outlines,
        tables,
        time_open(entry, runs).as_secs_f64() * 1000.0
    );
}

#[test]
#[ignore = "needs BEND_CLONE; run with --release, one at a time"]
fn measure() {
    let clone = clone_dir();
    println!(
        "| book | files | defs | text bytes | outline bytes | table bytes | open, ms (median) |"
    );
    println!("|---|---|---|---|---|---|---|");
    // (a) each demo: its entry file is the one with the largest book (a `PROOF.bend` where there is one, `walkers_test.bend` for the netcode)
    let mut demos: Vec<PathBuf> = fs::read_dir(clone.join("demos"))
        .expect("demos")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.is_dir())
        .collect();
    demos.sort();
    for demo in &demos {
        let mut best: Option<(usize, PathBuf)> = None;
        for file in fs::read_dir(demo)
            .expect("demo")
            .map(|e| e.expect("entry").path())
        {
            if file.extension().is_some_and(|e| e == "bend") {
                let n = open_entry(&file).expect("opens").files().len();
                if best.as_ref().is_none_or(|(m, _)| n > *m) {
                    best = Some((n, file));
                }
            }
        }
        let (_, entry) = best.expect("a demo has a .bend file");
        let label = format!(
            "demos/{}/{}",
            demo.file_name().expect("name").to_string_lossy(),
            entry.file_name().expect("name").to_string_lossy()
        );
        row(&label, &entry, 30);
    }
    // (b) the largest single book of the corpus, by file count
    let mut largest: Option<(usize, usize, String)> = None;
    for rel in read_list("files.txt").lines() {
        let book = open_entry(&clone.join(rel)).expect("opens");
        let key = (book.files().len(), book.decls().len());
        if largest.as_ref().is_none_or(|(f, d, _)| key > (*f, *d)) {
            largest = Some((key.0, key.1, rel.to_owned()));
        }
    }
    let (_, _, rel) = largest.expect("a corpus");
    row(&format!("largest by files: {rel}"), &clone.join(&rel), 30);
    // (c) base.bend alone
    row("bend2/base.bend", &clone.join("bend2/base.bend"), 100);
}

/// BD5: what a `Session` costs on the largest book of the demos (`demos/app_slash_boss_3d/PROOF.bend`, 5 files): a cold open, a `refresh` with nothing changed,
/// and a `refresh` after the entry file changed. The folder is copied to a scratch folder first: the clone is only ever read.
#[test]
#[ignore = "needs BEND_CLONE; run with --release, one at a time"]
fn session_measure() {
    use bend_analysis::{Budget, Cancel, Entry, Session};

    let demo = clone_dir().join("demos/app_slash_boss_3d");
    let root = std::env::temp_dir().join(format!("bend-session-measure-{}", std::process::id()));
    fs::create_dir_all(&root).expect("scratch folder");
    for file in fs::read_dir(&demo).expect("demo") {
        let file = file.expect("entry").path();
        if file.extension().is_some_and(|e| e == "bend") {
            fs::copy(&file, root.join(file.file_name().expect("name"))).expect("copy");
        }
    }
    let entry = Path::new("PROOF.bend");
    let open = || Session::open(&root, entry, options()).expect("opens");

    // the request: the def of the entry file whose tree is the largest, found once
    let session = open();
    let book = session.book().expect("a book");
    let at = book.file_at(entry).expect("the entry is in the book");
    let outline = &book.file(at).expect("a file").outline;
    let occurrences = bend_syntax::occurrences(outline);
    let defs: Vec<(String, u16)> = outline
        .defs
        .iter()
        .zip(&occurrences)
        .map(|(def, &n)| (def.qualname.clone(), n as u16))
        .collect();
    let summary = (book.files().len(), book.decls().len());
    let ask = |session: &Session, def: usize| {
        let entry = Entry::Def {
            qualname: &defs[def].0,
            occurrence: defs[def].1,
        };
        session.graph("PROOF.bend", &entry, &Budget::default(), &Cancel::new())
    };
    let biggest = (0..defs.len())
        .max_by_key(|&def| ask(&session, def).map_or(0, |g| g.stats.contexts))
        .expect("a def");
    let contexts = ask(&session, biggest).expect("a graph").stats.contexts;
    println!(
        "book: {} files, {} defs; request: `{}`, {contexts} contexts",
        summary.0, summary.1, defs[biggest].0
    );
    drop(session);

    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let cold_open = median(
        (0..30)
            .map(|_| {
                let start = Instant::now();
                std::hint::black_box(open());
                start.elapsed()
            })
            .collect(),
    );
    let cold_request = median(
        (0..30)
            .map(|_| {
                let session = open();
                let start = Instant::now();
                std::hint::black_box(ask(&session, biggest));
                start.elapsed()
            })
            .collect(),
    );
    let mut session = open();
    let nothing = median(
        (0..300)
            .map(|_| {
                let start = Instant::now();
                std::hint::black_box(session.refresh());
                start.elapsed()
            })
            .collect(),
    );
    // one file changed: the entry, rewritten to a text of another length each time (a stamp is the length and the modified time)
    let path = root.join("PROOF.bend");
    let text = fs::read_to_string(&path).expect("PROOF.bend");
    let (mut changed, mut changed_and_asked) = (Vec::new(), Vec::new());
    for run in 0..30 {
        fs::write(&path, format!("{text}{}", "\n".repeat(run % 2 + 1))).expect("rewrite");
        let start = Instant::now();
        assert!(session.refresh().reopened);
        changed.push(start.elapsed());
        let start = Instant::now();
        std::hint::black_box(ask(&session, biggest));
        changed_and_asked.push(start.elapsed());
    }
    let (changed, first_request) = (median(changed), median(changed_and_asked));
    println!(
        "| measure | median, ms |\n|---|---|\n| Session::open (cold: the files read, the tables built) | {:.3} |\n| the first request on a fresh session | {:.3} |\n| refresh, nothing changed | {:.3} |\n| refresh, the entry file changed (the book built again) | {:.3} |\n| the first request after that refresh | {:.3} |",
        ms(cold_open),
        ms(cold_request),
        ms(nothing),
        ms(changed),
        ms(first_request),
    );
    let _ = fs::remove_dir_all(&root);
}

fn options() -> Options {
    Options::new(MarkerStyle::above("ID"))
}

/// The byte offset of each UTF-16 unit of `text` (and one past the end), which is how the oracle's spans are written.
fn byte_of_unit(text: &str) -> Vec<u32> {
    let mut at = Vec::with_capacity(text.len() + 1);
    for (byte, c) in text.char_indices() {
        for _ in 0..c.len_utf16() {
            at.push(byte as u32);
        }
    }
    at.push(text.len() as u32);
    at
}

/// The place that holds the start of `beg..end`: the one that starts last, and of those the one whose end is nearest the Ref's. Bend ends the span of a do
/// statement after the white space that follows it, so an end can only be compared, not required to lie inside; and a do block's closing value has two Refs
/// that start together (the monad's, over the whole value, and the call's inside it).
fn smallest(items: &[Walked], beg: u32, end: u32) -> Option<&Walked> {
    items
        .iter()
        .filter(|w| w.span.start <= beg && beg < w.span.end)
        .min_by_key(|w| (std::cmp::Reverse(w.span.start), w.span.end.abs_diff(end)))
}

fn bucket(w: &Walked) -> String {
    match &w.kind {
        WalkedKind::Call(_) => "call".into(),
        WalkedKind::Passed(_) => "passed".into(),
        WalkedKind::Construct(_) => "construct".into(),
        WalkedKind::External(_) => "external".into(),
        WalkedKind::Unresolved(reason) => format!("unresolved {reason}"),
        WalkedKind::Counted(key) => format!("counted {key}"),
        WalkedKind::Quiet => "quiet (data, or a Base name)".into(),
        _ => "?".into(),
    }
}

#[test]
#[ignore = "needs BEND_CLONE and the Ref oracle's output (scripts/bend-refs.ts)"]
fn every_ref_the_oracle_prints_is_accounted_for() {
    let clone = clone_dir();
    let refs = out_dir().join("refs");
    let mut tables: Vec<PathBuf> = fs::read_dir(&refs)
        .unwrap_or_else(|_| panic!("{}: run scripts/bend-refs.ts first", refs.display()))
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "tsv"))
        .collect();
    tables.sort();
    let mut buckets: BTreeMap<String, usize> = BTreeMap::new();
    let mut none: Vec<String> = Vec::new();
    let mut wrong: Vec<String> = Vec::new();
    // the sites of sugar against the spans of Bend's own Refs: (matched, mismatched) for an operator's token and for an index
    let (mut operator_spans, mut index_spans) = ((0usize, 0usize), (0usize, 0usize));
    let mut span_mismatches: Vec<String> = Vec::new();
    let mut total = 0;
    for table in &tables {
        let stem = table
            .file_stem()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        let entry = clone.join(stem.split("__").collect::<PathBuf>());
        let dir = entry.parent().expect("folder").to_path_buf();
        let book = open_entry(&entry).unwrap_or_else(|| panic!("{stem}: the book did not open"));
        let declared = book.declared();
        let mut units: HashMap<FileIndex, Vec<u32>> = HashMap::new();
        let mut places: HashMap<bend_analysis::DeclId, Vec<Walked>> = HashMap::new();
        for line in fs::read_to_string(table).expect("oracle table").lines() {
            let cells: Vec<&str> = line.split('\t').collect();
            let [file, def, callee, span] = cells[..] else {
                panic!("{stem}: a line of four cells: {line}");
            };
            total += 1;
            let at = book
                .file_at(&dir.join(file))
                .expect("the file is in the book");
            let id = declared
                .iter()
                .find(|(name, _)| name == def)
                .map(|(_, id)| *id)
                .unwrap_or_else(|| panic!("{stem}: no def {def}"));
            let map = units
                .entry(at)
                .or_insert_with(|| byte_of_unit(&book.files()[at.index()].text));
            let (beg, end) = span.split_once('-').expect("beg-end");
            let (beg, end) = (
                map[beg.parse::<usize>().expect("beg")],
                map[end.parse::<usize>().expect("end")],
            );
            let items = places
                .entry(id)
                .or_insert_with(|| walked(&book, id, &options()).expect("a def of the book"));
            // a term the walk skipped as a type holds whatever is in it, whatever the smaller places inside say
            let skipped = items.iter().any(|w| {
                w.kind == WalkedKind::Counted("type_position".into())
                    && w.span.start <= beg
                    && end <= w.span.end
            });
            let found = if skipped {
                *buckets.entry("counted type_position".into()).or_default() += 1;
                continue;
            } else {
                smallest(items, beg, end)
            };
            let Some(found) = found else {
                none.push(format!("{stem}: {file} {def} -> {callee} at {beg}..{end}"));
                continue;
            };
            *buckets.entry(bucket(found)).or_default() += 1;
            // an operator's Ref is on its token, which has no letter and no space in it; an index's is `Array.get` / `Array.set`: the place must be the same bytes
            let text = &book.files()[at.index()].text[beg as usize..end as usize];
            let operator = !text.is_empty()
                && text
                    .chars()
                    .all(|c| !c.is_alphanumeric() && c != '_' && !c.is_whitespace());
            let index = callee == "Array.get" || callee == "Array.set";
            if operator || index {
                let same = found.span.start == beg && found.span.end == end;
                let tally = if operator {
                    &mut operator_spans
                } else {
                    &mut index_spans
                };
                if same {
                    tally.0 += 1;
                } else {
                    tally.1 += 1;
                    span_mismatches.push(format!(
                        "{stem}: {file} {def} -> {callee} {beg}..{end} ({text:?}), ours {}..{}",
                        found.span.start, found.span.end
                    ));
                }
            }
            // what a place says it reaches is what Bend says
            let agrees = match &found.kind {
                WalkedKind::Call(to) | WalkedKind::Passed(to) => {
                    book.decl(*to).is_some_and(|d| d.qualified == callee)
                }
                WalkedKind::External(name) => {
                    callee == name
                        || callee.ends_with(&format!(":{name}"))
                        || name.ends_with(callee)
                }
                _ => true,
            };
            if !agrees {
                wrong.push(format!(
                    "{stem}: {file} {def} -> {callee} at {beg}..{end}: {:?}",
                    found.kind
                ));
            }
        }
    }
    println!("oracle: {total} refs in {} tables", tables.len());
    for (name, n) in &buckets {
        println!("  {n:6}  {name}");
    }
    println!(
        "  operator tokens: {} spans equal, {} differ; indexes: {} equal, {} differ",
        operator_spans.0, operator_spans.1, index_spans.0, index_spans.1
    );
    for line in span_mismatches.iter().take(10) {
        println!("    {line}");
    }
    println!("  {:6}  in none", none.len());
    for line in none.iter().take(10) {
        println!("    {line}");
    }
    println!(
        "  {:6}  whose place names another def than Bend's",
        wrong.len()
    );
    for line in wrong.iter().take(10) {
        println!("    {line}");
    }
    assert!(
        none.is_empty(),
        "{} refs fall in no place the walk read",
        none.len()
    );
    assert!(wrong.is_empty());
}

#[test]
#[ignore = "needs BEND_CLONE"]
fn demos_counters() {
    let clone = clone_dir();
    let mut demos: Vec<PathBuf> = fs::read_dir(clone.join("demos"))
        .expect("demos")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.is_dir())
        .collect();
    demos.sort();
    let mut totals: BTreeMap<String, usize> = BTreeMap::new();
    for demo in &demos {
        let entry = ["PROOF.bend", "walkers_test.bend"]
            .iter()
            .map(|f| demo.join(f))
            .find(|f| f.is_file())
            .expect("an entry");
        let book = open_entry(&entry).expect("opens");
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for d in book.decls() {
            for w in walked(&book, d.id, &options()).expect("a def") {
                *counts.entry(bucket(&w)).or_default() += 1;
                // a call to a def with no code: the name means a law with no proof (`open_law`)
                if let WalkedKind::Call(to) = &w.kind
                    && book
                        .decl(*to)
                        .is_some_and(|d| matches!(&d.kind, DefKind::Other(k) if k == "law"))
                {
                    *counts.entry("open_law".into()).or_default() += 1;
                }
            }
        }
        println!(
            "{}: {counts:?}",
            demo.file_name().expect("name").to_string_lossy()
        );
        for (k, n) in counts {
            *totals.entry(k).or_default() += n;
        }
    }
    println!("all demos: {totals:#?}");
}

// ---- BC step 7: the context layer on the demos ----

/// The 16 entries of the sweep (`ENGINE.md`, "The sweep"): each demo's `main.bend`, except `io_rollback_netcode`, which has none (its largest book is `walkers_test.bend`).
fn sweep_entries() -> Vec<(String, PathBuf)> {
    let demos = clone_dir().join("demos");
    let mut found = Vec::new();
    let mut dirs: Vec<PathBuf> = fs::read_dir(&demos)
        .expect("demos")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        let entry = ["main.bend", "walkers_test.bend"]
            .iter()
            .map(|f| dir.join(f))
            .find(|f| f.is_file())
            .unwrap_or_else(|| panic!("{name}: no main.bend and no walkers_test.bend"));
        found.push((name, entry));
    }
    found
}

/// What one site is in one context of its def, under `Precise`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Outcome {
    /// A call of this def.
    Drawn(String),
    /// `Unresolved(anonymous_function_called)`: the caller named a lambda, drawn where it is written.
    Lambda,
    /// `Unresolved(function_typed_value)`: nobody named it.
    Unresolved,
}

/// What the callee of a site is, from the def's own parameter list (`~f` is a template).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Callee {
    Template,
    Plain,
    Neither,
}

impl Callee {
    fn word(self) -> &'static str {
        match self {
            Callee::Template => "~",
            Callee::Plain => "plain",
            Callee::Neither => "neither",
        }
    }
}

const CLASSES: [&str; 6] = [
    "drawn in every context",
    "lambda in every context",
    "drawn in some, lambda in others",
    "named in some, unresolved in others",
    "unresolved in every context",
    "def not reached",
];

fn class_of(outcomes: &[Outcome]) -> &'static str {
    let drawn = outcomes
        .iter()
        .filter(|o| matches!(o, Outcome::Drawn(_)))
        .count();
    let lambda = outcomes.iter().filter(|o| **o == Outcome::Lambda).count();
    let open = outcomes.len() - drawn - lambda;
    match (outcomes.len(), drawn, lambda, open) {
        (0, ..) => CLASSES[5],
        (_, _, _, 0) if lambda == 0 => CLASSES[0],
        (_, _, _, 0) if drawn == 0 => CLASSES[1],
        (_, _, _, 0) => CLASSES[2],
        (n, ..) if open == n => CLASSES[4],
        _ => CLASSES[3],
    }
}

/// A def as the graph names it: its qualname, and `#n` for the n-th of one name in its file.
fn def_label(graph: &Graph, id: &StableDef) -> String {
    let def = graph.def(id).expect("a def of the graph");
    match def.occurrence {
        0 => def.qname.clone(),
        n => format!("{}#{n}", def.qname),
    }
}

/// A def for a count of contexts: its name, and its file in brackets when another def of the graph has the same name (`Mesh.of` is in two files of
/// `app_slash_boss_3d`: counting by name would merge them).
fn shown(graph: &Graph, id: &StableDef) -> String {
    let name = def_label(graph, id);
    let same = graph
        .defs
        .iter()
        .filter(|d| def_label(graph, &d.id) == name)
        .count();
    match graph.def(id) {
        Some(def) if same > 1 => format!("{name} [{}]", def.file),
        _ => name,
    }
}

fn edge_kinds(graph: &Graph) -> [u64; 5] {
    let mut kinds = [0; 5];
    for edge in &graph.edges {
        kinds[match &edge.to {
            EdgeTo::Context(..) if edge.passed => 3,
            EdgeTo::Context(..) => 0,
            EdgeTo::Construct { .. } => 1,
            EdgeTo::External(..) => 2,
            EdgeTo::Unresolved(_) => 4,
            _ => 4,
        }] += 1;
    }
    kinds
}

fn counter(graph: &Graph, key: &str) -> u64 {
    graph.stats.language.get(key).copied().unwrap_or(0)
}

fn hex8(digest: &[u8; 16]) -> String {
    digest.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// The request of `main` in the file of `book`, asked `runs` times: the graph, and the median wall time in milliseconds.
fn timed(book: &Book, file: &str, precision: Precision, runs: usize) -> (Graph, f64) {
    let options = options().with_precision(precision);
    let ask = || {
        call_graph(
            book,
            Path::new(file),
            "main",
            &options,
            &Limits::default(),
            &Cancel::new(),
        )
        .expect("a graph of main")
    };
    let graph = ask();
    let times = (0..runs)
        .map(|_| {
            let start = Instant::now();
            std::hint::black_box(ask());
            start.elapsed()
        })
        .collect();
    (graph, median(times).as_secs_f64() * 1000.0)
}

/// The text of `at` bytes from the start of the def, its line and column (1-based, in characters), and the whole line it is on.
fn place(
    book: &Book,
    graph: &Graph,
    def: &StableDef,
    site: (u32, u32),
) -> (String, usize, usize, String) {
    let info = graph.def(def).expect("a def of the graph");
    let file = book
        .files()
        .iter()
        .find(|f| f.relative == info.file)
        .expect("the def's file");
    let (start, end) = (
        (info.range.start + site.0) as usize,
        (info.range.start + site.1) as usize,
    );
    let before = &file.text[..start];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    let text = file.text[start..end].to_owned();
    let whole = file
        .text
        .lines()
        .nth(line - 1)
        .unwrap_or("")
        .trim()
        .to_owned();
    (text, line, col, whole)
}

/// Whether the callee `text` is a template parameter of the def (`~text` in its outline's params), a plain one, or neither.
fn callee_of(book: &Book, graph: &Graph, def: &StableDef, text: &str) -> Callee {
    let info = graph.def(def).expect("a def of the graph");
    let file = book
        .files()
        .iter()
        .find(|f| f.relative == info.file)
        .expect("the def's file");
    let outline = file
        .outline
        .defs
        .iter()
        .find(|d| d.range == info.range && d.qualname == info.qualname)
        .expect("the def in its outline");
    if outline
        .params
        .iter()
        .any(|p| p.strip_prefix('~') == Some(text))
    {
        Callee::Template
    } else if outline
        .params
        .iter()
        .any(|p| p == text || p.strip_prefix(['+', '-']) == Some(text))
    {
        Callee::Plain
    } else {
        Callee::Neither
    }
}

struct SiteRow {
    book: String,
    callee: Callee,
    outcomes: Vec<Outcome>,
}

#[test]
#[ignore = "needs BEND_CLONE; run with --release (it times requests)"]
fn context_layer_on_the_demos() {
    const RUNS: usize = 20;
    let entries = sweep_entries();
    assert_eq!(entries.len(), 16, "the sweep's 16 books");
    let mut rows: Vec<SiteRow> = Vec::new();
    let mut defaults_unresolved: Vec<(String, usize)> = Vec::new();
    println!(
        "# a. the sites of layer 1 (the unresolved edges of the Default graph), each in every context of its def under Precise\n"
    );
    let mut books_report = String::new();
    let mut tops = String::new();
    let mut rollback = String::new();
    // (book, def, contexts) of every def that has more than one context under Precise
    let mut many: Vec<(String, String, usize)> = Vec::new();
    // per book: contexts under Default and Precise, and the layer's five counters under Precise
    let mut layer: Vec<(String, u64, u64, [u64; 5])> = Vec::new();
    for (name, entry) in &entries {
        let file = entry
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        let book = open_entry(entry).expect("opens");
        let (default, default_ms) = timed(&book, &file, Precision::Default, RUNS);
        let (precise, precise_ms) = timed(&book, &file, Precision::Precise, RUNS);

        // the sites: (def, span) of every unresolved edge of the Default graph, each once (one context per def there)
        let mut sites: Vec<(StableDef, (u32, u32))> = default
            .edges
            .iter()
            .filter_map(|e| match &e.to {
                EdgeTo::Unresolved(reason) => {
                    assert_eq!(
                        reason.as_str(),
                        "function_typed_value",
                        "{name}: BC0 saw no other reason"
                    );
                    Some((e.from.func.clone(), (e.site.start, e.site.end)))
                }
                _ => None,
            })
            .collect();
        sites.sort();
        let before = sites.len();
        sites.dedup();
        assert_eq!(
            before,
            sites.len(),
            "{name}: a def has one context under Default"
        );
        defaults_unresolved.push((name.clone(), sites.len()));

        for (def, site) in &sites {
            let contexts = precise
                .contexts
                .iter()
                .filter(|c| &c.id.func == def)
                .count();
            let outcomes: Vec<Outcome> = precise
                .edges
                .iter()
                .filter(|e| &e.from.func == def && (e.site.start, e.site.end) == *site)
                .map(|e| match &e.to {
                    EdgeTo::Context(id, _) if !e.passed => {
                        Outcome::Drawn(def_label(&precise, &id.func))
                    }
                    EdgeTo::Unresolved(r) if r.as_str() == "anonymous_function_called" => {
                        Outcome::Lambda
                    }
                    EdgeTo::Unresolved(_) => Outcome::Unresolved,
                    other => {
                        panic!("{name}: a site that is neither drawn nor unresolved: {other:?}")
                    }
                })
                .collect();
            assert_eq!(
                outcomes.len(),
                contexts,
                "{name}: one edge for the site in each context of its def"
            );
            let (text, line, col, _) = place(&book, &default, def, *site);
            let callee = callee_of(&book, &default, def, &text);
            let label = def_label(&default, def);
            let drawn: Vec<&str> = {
                let mut d: Vec<&str> = outcomes
                    .iter()
                    .filter_map(|o| match o {
                        Outcome::Drawn(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect();
                d.sort();
                d.dedup();
                d
            };
            let count = |f: fn(&Outcome) -> bool| outcomes.iter().filter(|o| f(o)).count();
            println!(
                "{name} | {} {label} {line}:{col} `{text}` | {} | {} | drawn {} lambda {} unresolved {} of {contexts} contexts{}",
                default.def(def).map_or("?", |d| d.file.as_str()),
                callee.word(),
                class_of(&outcomes),
                count(|o| matches!(o, Outcome::Drawn(_))),
                count(|o| *o == Outcome::Lambda),
                count(|o| *o == Outcome::Unresolved),
                if drawn.is_empty() {
                    String::new()
                } else {
                    format!(" | -> {}", drawn.join(", "))
                },
            );
            rows.push(SiteRow {
                book: name.clone(),
                callee,
                outcomes,
            });
        }

        // c. the cost of the layer
        let counters = [
            "anonymous_function_called",
            "key_slot_open",
            "key_slot_local",
            "key_slot_value",
            "key_slot_missing",
        ];
        assert!(
            counters.iter().all(|c| counter(&default, c) == 0),
            "{name}: the layer's counters are Precise's alone"
        );
        layer.push((
            name.clone(),
            default.stats.contexts,
            precise.stats.contexts,
            counters.map(|c| counter(&precise, c)),
        ));
        for (mode, graph, ms) in [
            ("Default", &default, default_ms),
            ("Precise", &precise, precise_ms),
        ] {
            let k = edge_kinds(graph);
            books_report.push_str(&format!(
                "| {name} | {mode} | {} | {} | {} / {} / {} / {} / {} | {} | {:.3} |\n",
                graph.stats.contexts,
                graph.stats.walks,
                k[0],
                k[1],
                k[2],
                k[3],
                k[4],
                counters
                    .iter()
                    .map(|c| counter(graph, c).to_string())
                    .collect::<Vec<_>>()
                    .join(" / "),
                ms
            ));
        }

        // d. the five defs with the most contexts under Precise
        let mut per_def: BTreeMap<StableDef, usize> = BTreeMap::new();
        for c in &precise.contexts {
            *per_def.entry(c.id.func.clone()).or_default() += 1;
        }
        let mut order: Vec<(String, usize)> = per_def
            .iter()
            .map(|(d, n)| (shown(&precise, d), *n))
            .collect();
        order.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        many.extend(
            per_def
                .iter()
                .filter(|(_, n)| **n > 1)
                .map(|(d, n)| (name.clone(), shown(&precise, d), *n)),
        );
        tops.push_str(&format!(
            "{name}: {}\n",
            order
                .iter()
                .take(5)
                .map(|(d, n)| format!("{d} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));

        // e. io_rollback_netcode: which defs have more contexts, and what their keys are made of
        if name == "io_rollback_netcode" {
            let mut before_count: BTreeMap<StableDef, usize> = BTreeMap::new();
            for c in &default.contexts {
                *before_count.entry(c.id.func.clone()).or_default() += 1;
            }
            rollback.push_str(&format!(
                "contexts {} -> {} (Default -> Precise); the defs that have more:\n",
                default.stats.contexts, precise.stats.contexts
            ));
            for (def_id, more) in &per_def {
                let was = before_count.get(def_id).copied().unwrap_or(0);
                if *more <= was {
                    continue;
                }
                rollback.push_str(&format!("  {}: {was} -> {more}\n", shown(&precise, def_id)));
                let mut mine: Vec<&ContextId> = precise
                    .contexts
                    .iter()
                    .map(|c| &c.id)
                    .filter(|id| id.func == *def_id)
                    .collect();
                mine.sort_by_key(|id| id.digest);
                for id in mine {
                    let key = if id.digest == [0; 16] {
                        "open".to_owned()
                    } else {
                        hex8(&id.digest)
                    };
                    // what this context's own sites of layer 1 became, and where it was called from
                    let own: Vec<String> = sites
                        .iter()
                        .filter(|(d, _)| *d == id.func)
                        .map(|(d, site)| {
                            let (text, ..) = place(&book, &default, d, *site);
                            let out: Vec<String> = precise
                                .edges
                                .iter()
                                .filter(|e| e.from == *id && (e.site.start, e.site.end) == *site)
                                .map(|e| match &e.to {
                                    EdgeTo::Context(t, _) => {
                                        format!("calls {}", def_label(&precise, &t.func))
                                    }
                                    EdgeTo::Unresolved(r) => r.as_str().to_owned(),
                                    _ => "?".to_owned(),
                                })
                                .collect();
                            format!("`{text}` {}", out.join("+"))
                        })
                        .collect();
                    rollback.push_str(&format!("    key {key}: own sites [{}]\n", own.join("; ")));
                    let mut from: Vec<String> = precise
                        .edges
                        .iter()
                        .filter(|e| matches!(&e.to, EdgeTo::Context(t, _) if t == id && !e.passed))
                        .map(|e| {
                            let (_, line, _, whole) =
                                place(&book, &precise, &e.from.func, (e.site.start, e.site.end));
                            format!(
                                "{}@{} line {line}: {whole}",
                                def_label(&precise, &e.from.func),
                                if e.from.digest == [0; 16] {
                                    "open".to_owned()
                                } else {
                                    hex8(&e.from.digest)
                                }
                            )
                        })
                        .collect();
                    from.sort();
                    from.dedup();
                    for f in from.iter().take(3) {
                        rollback.push_str(&format!("      called from {f}\n"));
                    }
                }
            }
        }
    }

    // the totals
    let total: usize = defaults_unresolved.iter().map(|(_, n)| n).sum();
    println!(
        "\nunresolved sites under Default: {total} = {}\n",
        defaults_unresolved
            .iter()
            .filter(|(_, n)| *n > 0)
            .map(|(b, n)| format!("{b} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    println!(
        "# b. the classes, by what the callee is (`~` template parameter / plain parameter / neither)\n"
    );
    let table = |label: &str, rows: &[&SiteRow]| {
        println!("{label}");
        println!("| class | ~ | plain | neither | all |\n|---|---|---|---|---|");
        for class in CLASSES {
            let n = |c: Callee| {
                rows.iter()
                    .filter(|r| class_of(&r.outcomes) == class && r.callee == c)
                    .count()
            };
            let all: usize = [Callee::Template, Callee::Plain, Callee::Neither]
                .iter()
                .map(|c| n(*c))
                .sum();
            println!(
                "| {class} | {} | {} | {} | {all} |",
                n(Callee::Template),
                n(Callee::Plain),
                n(Callee::Neither)
            );
        }
        let n = |c: Callee| rows.iter().filter(|r| r.callee == c).count();
        println!(
            "| **all** | {} | {} | {} | {} |\n",
            n(Callee::Template),
            n(Callee::Plain),
            n(Callee::Neither),
            rows.len()
        );
    };
    for (name, _) in entries
        .iter()
        .filter(|(n, _)| rows.iter().any(|r| &r.book == n))
    {
        let mine: Vec<&SiteRow> = rows.iter().filter(|r| &r.book == name).collect();
        table(&format!("{name}:"), &mine);
    }
    let all: Vec<&SiteRow> = rows.iter().collect();
    table("all 16 books:", &all);

    println!(
        "# c. Default against Precise, per book: contexts | bodies walked | edges call / construct / external / passed / unresolved | anonymous_function_called / key_slot_open / key_slot_local / key_slot_value / key_slot_missing | median of {RUNS} requests, ms\n"
    );
    println!(
        "| book | mode | contexts | walks | edges | layer counters | ms |\n|---|---|---|---|---|---|---|"
    );
    print!("{books_report}");
    println!("\n# d. the five defs with the most contexts under Precise\n");
    print!("{tops}");
    println!("\ndefs with more than one context under Precise, in all 16 books: {many:?}");
    println!("\n# e. io_rollback_netcode\n");
    print!("{rollback}");

    // What the code gave on 2026-11-21, explained site by site in the report of BC step 7 and pinned here (the numbers are `ENGINE.md`'s §BC1+BC2).
    // Layer 1: BC0's 55 sites, in four books.
    let found: Vec<(&str, usize)> = defaults_unresolved
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(b, n)| (b.as_str(), *n))
        .collect();
    assert_eq!(
        found,
        [
            ("app_slash_boss_3d", 20),
            ("io_rollback_netcode", 13),
            ("io_tcp_echos", 1),
            ("pure_hvm5_mini", 21)
        ]
    );
    // The classes by what the callee is: every cell not named is 0. BC0 said 28 named (16 at a `~`, 12 at a plain parameter), 0 named in some, 27 in none. Here:
    // the 28 are 16 drawn (all `~`) and 12 known to be a lambda (all plain: continuations written at the call); 3 of BC0's 27 are named in 14 of 15 contexts of
    // `String.word` and `String.word.go` (the callers name a lambda; the open context is reached through a lambda's own binder, `k2 => String.word(R, t, k2)`);
    // 24 stay unresolved in every context (17 plain parameters of `pure_hvm5_mini`'s CPS parser, 7 that are no parameter: BC0's c, d and g).
    let cell = |class: &str, callee: Callee| {
        rows.iter()
            .filter(|r| class_of(&r.outcomes) == class && r.callee == callee)
            .count()
    };
    let pinned = [
        (CLASSES[0], Callee::Template, 16),
        (CLASSES[1], Callee::Plain, 12),
        (CLASSES[3], Callee::Plain, 3),
        (CLASSES[4], Callee::Plain, 17),
        (CLASSES[4], Callee::Neither, 7),
    ];
    for class in CLASSES {
        for callee in [Callee::Template, Callee::Plain, Callee::Neither] {
            let want = pinned
                .iter()
                .find(|(c, k, _)| *c == class && *k == callee)
                .map_or(0, |(_, _, n)| *n);
            assert_eq!(cell(class, callee), want, "{class}, {}", callee.word());
        }
    }
    let named = |callee: Callee| cell(CLASSES[0], callee) + cell(CLASSES[1], callee);
    assert_eq!((named(Callee::Template), named(Callee::Plain)), (16, 12));
    // The cost: only these books have a context or a counter more under Precise: (contexts under Default, under Precise, then the layer's counters
    // `anonymous_function_called`, `key_slot_open`, `key_slot_local`, `key_slot_value`, `key_slot_missing`).
    for (book, default_contexts, precise_contexts, counters) in &layer {
        let want = match book.as_str() {
            "app_slash_boss_3d" => (424, 424, [4, 0, 0, 0, 0]),
            "io_rollback_netcode" => (130, 160, [64, 1, 15, 0, 0]),
            "io_tcp_echos" => (8, 8, [1, 0, 0, 0, 0]),
            "pure_hvm5_mini" => (157, 157, [0, 5, 0, 0, 14]),
            // nothing to resolve: the same contexts, no counter
            _ => (*default_contexts, *default_contexts, [0; 5]),
        };
        assert_eq!(
            (*default_contexts, *precise_contexts, *counters),
            want,
            "{book}"
        );
    }
    // The only defs with more than one context under Precise: the 30 contexts `io_rollback_netcode` has more (130 to 160).
    let many: Vec<(&str, &str, usize)> = many
        .iter()
        .map(|(b, d, n)| (b.as_str(), d.as_str(), *n))
        .collect();
    assert_eq!(
        many,
        [
            ("io_rollback_netcode", "String.word", 15),
            ("io_rollback_netcode", "String.word.go", 15),
            ("io_rollback_netcode", "Bot.frame", 2),
            ("io_rollback_netcode", "Bot.more", 2)
        ]
    );
}
