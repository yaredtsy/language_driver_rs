//! The corpus test of BD2 (docs/08-bend/03 §1): every file of the Bend corpus that Bend's own loader accepts has an outline whose defs are Bend's
//! declarations without the constructors, in order, with the same names and kinds, and that is well formed. It also takes the measurement of
//! 05 §4 [check] 5: how many declarations have a comment block directly above them, and how many have one separated by a blank line. And of BD2 step 2:
//! a marker planned on the defs of every accepted file and the file's own, all applied, reads back, and changes nothing Bend sees.
//!
//! Ignored by default: it needs the bend clone and the lists that `scripts/bend-corpus.sh` writes.
//!
//!     scripts/bend-corpus.sh ../language_clone/bend
//!     BEND_CLONE=../language_clone/bend cargo test -p bend-syntax --release --test corpus -- --ignored --nocapture
//!
//! `BEND_CORPUS_OUT` names the folder of the lists (default `languages/bend/spikes/out`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use analyzer_core::edit::apply;
use bend_syntax::{DefKind, MarkerStyle, outline, plan_header_marker, plan_marker};

const ID: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: bend_syntax::Placement::Above,
};

fn read_list(out: &Path, file: &str) -> String {
    fs::read_to_string(out.join(file))
        .unwrap_or_else(|_| panic!("{file}: run scripts/bend-corpus.sh first"))
}

/// Bend names a def that fills a law of an imported file by the law's resolved name, `<namespace>:<name>`; the text says `<Alias>.<name>`.
fn same_name(ours: &str, bends: &str) -> bool {
    match bends.rsplit_once(':') {
        None => ours == bends,
        Some((_, name)) => ours.split_once('.').is_some_and(|(_, rest)| rest == name),
    }
}

/// Whether the text before `at` ends with a line that is only a `#` comment, `blanks` empty lines before `at`.
fn comment_above(text: &str, at: usize, blanks: usize) -> bool {
    let mut lines = text[..at].rsplit('\n');
    // the piece after the last newline is the indentation of the line `at` is on
    lines.next();
    for _ in 0..blanks {
        if lines.next().is_none_or(|l| !l.trim().is_empty()) {
            return false;
        }
    }
    lines
        .next()
        .is_some_and(|l| l.trim_start().starts_with('#'))
}

#[derive(Default)]
struct Tally {
    decls: usize,
    doc: usize,
    one_blank: usize,
    one_blank_not_header: usize,
    two_blanks: usize,
}

#[test]
#[ignore = "needs BEND_CLONE and the lists of scripts/bend-corpus.sh"]
fn every_accepted_file_has_an_outline_of_bends_declarations() {
    let clone =
        PathBuf::from(std::env::var("BEND_CLONE").expect("BEND_CLONE: the path to the bend clone"));
    let out = std::env::var("BEND_CORPUS_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spikes/out"));
    let accepted = read_list(&out, "parse-accepted.txt");
    let tsv = read_list(&out, "parse-decls.tsv");
    let mut expected: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    for line in tsv.lines() {
        let col: Vec<&str> = line.split('\t').collect();
        if col[1] != "ctr" {
            expected.entry(col[0]).or_default().push((col[1], col[2]));
        }
    }

    let mut problems: Vec<String> = Vec::new();
    let mut by_folder: BTreeMap<String, Tally> = BTreeMap::new();
    let (mut files, mut defs) = (0usize, 0usize);
    for file in accepted.lines() {
        let text = fs::read_to_string(clone.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let o = outline(&text, ID);
        files += 1;
        defs += o.defs.len();
        if !o.is_well_formed() {
            problems.push(format!("{file}: the outline is not well formed"));
        }
        if let Some(e) = o.syntax_errors.first() {
            problems.push(format!("{file}: a syntax error: {}", e.message));
            continue;
        }
        let want = expected.get(file).map_or(&[][..], Vec::as_slice);
        let kind = |k: &DefKind| match k {
            DefKind::Enum => "type",
            DefKind::Function => "def",
            DefKind::Other(_) => "law",
            _ => "?",
        };
        let agrees = |d: &bend_syntax::Def, (k, n): &(&str, &str)| {
            kind(&d.kind) == *k && same_name(&d.qualname, n)
        };
        if o.defs.len() != want.len() || !o.defs.iter().zip(want).all(|(d, w)| agrees(d, w)) {
            let at = o
                .defs
                .iter()
                .zip(want)
                .position(|(d, w)| !agrees(d, w))
                .unwrap_or(o.defs.len().min(want.len()));
            problems.push(format!(
                "{file}: defs differ at #{at}: we have {:?}, Bend has {:?} ({} against {})",
                o.defs.get(at).map(|d| (&d.qualname, &d.kind)),
                want.get(at),
                o.defs.len(),
                want.len()
            ));
            continue;
        }
        // the measurement: what is directly above each declaration (the range starts at its doc when there is one)
        let tally = by_folder
            .entry(file.split('/').next().unwrap_or("").to_string())
            .or_default();
        for d in &o.defs {
            tally.decls += 1;
            let start = d.range.start as usize;
            if text[start..].starts_with('#') {
                tally.doc += 1;
            } else if comment_above(&text, start, 1) {
                tally.one_blank += 1;
                // a block that is the very first thing of the file is a header, which the file marker owns, not a doc
                let first_line_end = text.find('\n').unwrap_or(text.len());
                let header = text[..first_line_end].starts_with('#')
                    && !text[..start]
                        .lines()
                        .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'));
                if !header {
                    tally.one_blank_not_header += 1;
                }
            } else if comment_above(&text, start, 2) {
                tally.two_blanks += 1;
            }
        }
    }
    println!("corpus: {files} files, {defs} defs");
    println!(
        "{:<8} {:>9} {:>10} {:>11} {:>14} {:>8}",
        "folder", "decls", "doc above", "1 blank", "1 blank (no hdr)", "2 blanks"
    );
    let mut total = Tally::default();
    for (folder, t) in &by_folder {
        println!(
            "{folder:<8} {:>9} {:>10} {:>11} {:>14} {:>8}",
            t.decls, t.doc, t.one_blank, t.one_blank_not_header, t.two_blanks
        );
        total.decls += t.decls;
        total.doc += t.doc;
        total.one_blank += t.one_blank;
        total.one_blank_not_header += t.one_blank_not_header;
        total.two_blanks += t.two_blanks;
    }
    println!(
        "{:<8} {:>9} {:>10} {:>11} {:>14} {:>8}",
        "all",
        total.decls,
        total.doc,
        total.one_blank,
        total.one_blank_not_header,
        total.two_blanks
    );
    assert!(
        problems.is_empty(),
        "{} problem(s); the first 40:\n{}",
        problems.len(),
        problems
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Which defs of a file take a marker in the test below. Every def of a file of at most 300; of a larger one (the generated `bench/` files have up to
/// 70,000) a spread of about 40 and the last, because `plan_marker` outlines the whole file on every call.
fn sample(len: usize) -> Vec<usize> {
    if len <= 300 {
        return (0..len).collect();
    }
    let mut picked: Vec<usize> = (0..len).step_by(len / 40).collect();
    picked.push(len - 1);
    picked
}

/// A marker planned on the defs of every accepted file, and the file's own, applied together: the file's marker and every planned def's read back, no
/// other def changed, and the defs of the new text are Bend's own list again (a marker line is a comment: it never changes what Bend sees).
#[test]
#[ignore = "needs BEND_CLONE and the lists of scripts/bend-corpus.sh"]
fn a_marker_planned_on_the_defs_of_every_accepted_file_reads_back() {
    let clone =
        PathBuf::from(std::env::var("BEND_CLONE").expect("BEND_CLONE: the path to the bend clone"));
    let out = std::env::var("BEND_CORPUS_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spikes/out"));
    let accepted = read_list(&out, "parse-accepted.txt");
    let tsv = read_list(&out, "parse-decls.tsv");
    let mut expected: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    for line in tsv.lines() {
        let col: Vec<&str> = line.split('\t').collect();
        if col[1] != "ctr" {
            expected.entry(col[0]).or_default().push((col[1], col[2]));
        }
    }
    let mut problems: Vec<String> = Vec::new();
    let (mut files, mut defs, mut planned, mut headers, mut already) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    for file in accepted.lines() {
        let text = fs::read_to_string(clone.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let before = outline(&text, ID);
        files += 1;
        defs += before.defs.len();
        let mut edits = Vec::new();
        let header = plan_header_marker(&text, "FILE", ID);
        let planned_header = header.is_ok();
        if let Ok(edit) = header {
            headers += 1;
            edits.push(edit);
        }
        let mut chosen = Vec::new();
        for i in sample(before.defs.len()) {
            match plan_marker(&text, &before.defs[i], &format!("m{i}"), ID) {
                Ok(edit) => {
                    edits.push(edit);
                    chosen.push(i);
                    planned += 1;
                }
                Err(bend_syntax::Skip::AlreadyMarked { .. }) => already += 1,
                Err(e) => problems.push(format!(
                    "{file}: def {i} ({}) refused: {e}",
                    before.defs[i].qualname
                )),
            }
        }
        let Some(marked) = apply(&text, &edits) else {
            problems.push(format!("{file}: the edits do not apply together"));
            continue;
        };
        let after = outline(&marked, ID);
        if !after.is_well_formed() || !after.syntax_errors.is_empty() {
            problems.push(format!(
                "{file}: the marked text is not well formed or has a syntax error"
            ));
            continue;
        }
        if planned_header && after.file_marker.as_deref() != Some("FILE") {
            problems.push(format!(
                "{file}: the file marker does not read back: {:?}",
                after.file_marker
            ));
        }
        if !planned_header && after.file_marker != before.file_marker {
            problems.push(format!("{file}: the file marker changed"));
        }
        // the same defs as Bend's list
        let want = expected.get(file).map_or(&[][..], Vec::as_slice);
        let kind = |k: &DefKind| match k {
            DefKind::Enum => "type",
            DefKind::Function => "def",
            DefKind::Other(_) => "law",
            _ => "?",
        };
        if after.defs.len() != want.len()
            || !after
                .defs
                .iter()
                .zip(want)
                .all(|(d, (k, n))| kind(&d.kind) == *k && same_name(&d.qualname, n))
        {
            problems.push(format!(
                "{file}: after marking, the defs are not Bend's list"
            ));
            continue;
        }
        for (i, (a, b)) in after.defs.iter().zip(&before.defs).enumerate() {
            let want_marker = if chosen.contains(&i) {
                Some(format!("m{i}"))
            } else {
                b.marker.clone()
            };
            if a.marker != want_marker || a.qualname != b.qualname || a.params != b.params {
                problems.push(format!(
                    "{file}: def {i} ({}) after marking: marker {:?}, wanted {want_marker:?}",
                    b.qualname, a.marker
                ));
                break;
            }
        }
    }
    println!(
        "marked corpus: {files} files, {defs} defs, {planned} markers planned and read back ({already} already marked), {headers} file markers"
    );
    assert!(
        problems.is_empty(),
        "{} problem(s); the first 40:\n{}",
        problems.len(),
        problems
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
