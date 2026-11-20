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
//! - `measure`: time and bytes of opening (a) each demo, (b) the largest book of the corpus by file count, (c) `base.bend` alone.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bend_analysis::{
    Book, DeclEntry, DefKind, FileIndex, MarkerStyle, Options, Resolved, Walked, WalkedKind, walked,
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
