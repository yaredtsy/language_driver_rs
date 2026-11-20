//! The corpus test of BD1 (docs/08-bend/02-phase-1-parsing.md §4): every file of the Bend corpus that Bend's own loader accepts parses with no error, and
//! the declarations found are the ones Bend sees, in order: the same kind and name, the same number of parameters (a type's, a def's, a constructor's
//! fields) and of templates, and each with a span that starts at its keyword and a name that is its span (`common::invariants`).
//!
//! Ignored by default: it needs the bend clone and the lists that `scripts/bend-corpus.sh` writes.
//!
//!     scripts/bend-corpus.sh ../language_clone/bend
//!     BEND_CLONE=../language_clone/bend cargo test -p bend-parser --release --test corpus -- --ignored --nocapture
//!
//! `BEND_CORPUS_OUT` names the folder of the lists (default `languages/bend/spikes/out`).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bend_parser::{Decl, Tree, parse};

/// One declaration as both sides state it: kind, name, parameters (`None` for a law: Bend's count for it is the proof's), templates (a def's, when it
/// has a `->`; `None` is "not stated").
#[derive(Debug, PartialEq)]
struct Entry {
    kind: String,
    name: String,
    params: Option<usize>,
    templates: Option<usize>,
}

/// Our tree in Bend's order: a declaration, then its constructors.
fn entries(decls: &[Decl]) -> Vec<Entry> {
    let entry = |kind: &str, name: &str, params, templates| Entry {
        kind: kind.to_string(),
        name: name.to_string(),
        params,
        templates,
    };
    let mut out = Vec::new();
    for d in decls {
        match d {
            Decl::Type(t) => {
                out.push(entry("type", &t.name.text, Some(t.params.len()), None));
                for c in &t.ctrs {
                    out.push(entry("ctr", &c.name.text, Some(c.fields.len()), None));
                }
            }
            Decl::Law(l) => out.push(entry("law", &l.name.text, None, None)),
            Decl::Def(f) => {
                // a def with no `->` fills a law, and its template count is the law's (`for ~k`): it states none of its own
                let templates = f
                    .ret
                    .is_some()
                    .then(|| f.params.iter().filter(|b| b.template).count());
                out.push(entry("def", &f.name.text, Some(f.params.len()), templates));
            }
            _ => {}
        }
    }
    out
}

/// Bend names a def that fills a law of an imported file by the law's resolved name, `<namespace>:<name>`; the text says `<Alias>.<name>`, with `Alias`
/// one of the file's own import aliases. Every other name is spelled the same by both.
fn same_name(tree: &Tree, ours: &str, bends: &str) -> bool {
    match bends.rsplit_once(':') {
        None => ours == bends,
        Some((_, name)) => ours.split_once('.').is_some_and(|(alias, rest)| {
            rest == name
                && tree
                    .imports
                    .iter()
                    .any(|i| i.alias.as_ref().is_some_and(|a| a.text == alias))
        }),
    }
}

fn read_list(out: &Path, file: &str) -> String {
    fs::read_to_string(out.join(file))
        .unwrap_or_else(|_| panic!("{file}: run scripts/bend-corpus.sh first"))
}

#[test]
#[ignore = "needs BEND_CLONE and the lists of scripts/bend-corpus.sh"]
fn every_accepted_file_parses_and_its_declarations_are_found() {
    let clone =
        PathBuf::from(std::env::var("BEND_CLONE").expect("BEND_CLONE: the path to the bend clone"));
    let out = std::env::var("BEND_CORPUS_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spikes/out"));
    let accepted = read_list(&out, "parse-accepted.txt");
    let tsv = read_list(&out, "parse-decls.tsv");
    let mut expected: BTreeMap<&str, Vec<Entry>> = BTreeMap::new();
    for line in tsv.lines() {
        let col: Vec<&str> = line.split('\t').collect();
        let count = |i: usize| col.get(i).and_then(|c| c.parse().ok());
        expected.entry(col[0]).or_default().push(Entry {
            kind: col[1].to_string(),
            name: col[2].to_string(),
            params: count(3),
            templates: count(4),
        });
    }

    let (mut files, mut found, mut bytes) = (0usize, 0usize, 0usize);
    let mut problems: Vec<String> = Vec::new();
    let mut elapsed = std::time::Duration::ZERO;
    for file in accepted.lines() {
        let text = fs::read_to_string(clone.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let started = Instant::now();
        let tree = parse(&text);
        elapsed += started.elapsed();
        files += 1;
        bytes += text.len();
        let got = entries(&tree.decls);
        found += got.len();
        if let Some(e) = tree.errors.first() {
            problems.push(format!(
                "{file}: {} error(s), the first at {:?}: expected {}, observed {}",
                tree.errors.len(),
                e.span,
                e.expected,
                e.observed
            ));
            continue;
        }
        let want = expected.get(file).map_or(&[][..], Vec::as_slice);
        let agrees = |g: &Entry, w: &Entry| {
            g.kind == w.kind
                && g.params == w.params
                && (g.templates.is_none() || g.templates == w.templates)
                && same_name(&tree, &g.name, &w.name)
        };
        if got.len() != want.len() || !got.iter().zip(want).all(|(g, w)| agrees(g, w)) {
            let at = got
                .iter()
                .zip(want)
                .position(|(g, w)| !agrees(g, w))
                .unwrap_or(got.len().min(want.len()));
            problems.push(format!(
                "{file}: declarations differ at #{at}: we have {:?}, Bend has {:?} ({} against {})",
                got.get(at),
                want.get(at),
                got.len(),
                want.len()
            ));
            continue;
        }
        problems.extend(
            common::invariants(&text, &tree)
                .into_iter()
                .map(|v| format!("{file}: {v}")),
        );
    }
    println!(
        "corpus: {files} files, {bytes} bytes, {found} declarations (types, laws, defs and constructors), parsed in {elapsed:?}"
    );
    assert!(
        problems.is_empty(),
        "{} problem(s); the first 60:\n{}",
        problems.len(),
        problems
            .iter()
            .take(60)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
