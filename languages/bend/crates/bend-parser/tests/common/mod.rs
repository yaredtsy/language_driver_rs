//! Shared by the test files of this crate. Not the testkit: `bend-parser` depends on `analyzer-core` and nothing else, dev-dependencies included.
#![allow(dead_code, unreachable_pub)]

use std::fs;
use std::path::{Path, PathBuf};

use analyzer_core::text::Span;
use bend_parser::{Decl, DefBody, Tree};

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

/// Every `.bend` under `tests/fixtures`, sorted by name.
pub fn fixtures() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = fs::read_dir(fixtures_dir())
        .expect("tests/fixtures exists")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "bend" {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_string();
            Some((
                name,
                fs::read_to_string(&path).expect("a fixture is UTF-8 text"),
            ))
        })
        .collect();
    out.sort();
    out
}

/// The testkit's `expect_file`, written out: `ANALYZER_BLESS=1` writes the expected file, for a person to review with `git diff`.
pub fn expect_file(path: &Path, actual: &str) {
    if std::env::var_os("ANALYZER_BLESS").is_some_and(|v| v == "1") {
        fs::create_dir_all(path.parent().expect("a snapshot has a folder"))
            .expect("create the snapshot folder");
        fs::write(path, actual).expect("write the snapshot");
        return;
    }
    let expected = fs::read_to_string(path).unwrap_or_else(|_| {
        panic!("no expected file {}\n  run with ANALYZER_BLESS=1 to write it, then review it.\n--- actual ---\n{actual}", path.display())
    });
    assert!(
        expected == actual,
        "{} differs\n  run with ANALYZER_BLESS=1 to rewrite it, then review the diff.\n--- expected ---\n{expected}--- actual ---\n{actual}",
        path.display()
    );
}

/// The tree's `{:#?}` print with each `Span { start, end }` folded to `start..end "the text it covers"`, so that a snapshot can be reviewed against
/// the fixture without counting bytes.
pub fn debug_print(tree: &Tree, text: &str) -> String {
    let raw = format!("{tree:#?}");
    let lines: Vec<&str> = raw.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let number = |s: Option<&&str>, key: &str| -> Option<u32> {
            s?.trim().strip_prefix(key)?.strip_suffix(',')?.parse().ok()
        };
        if let (Some(head), Some(start), Some(end), Some(close)) = (
            line.strip_suffix("Span {"),
            number(lines.get(i + 1), "start: "),
            number(lines.get(i + 2), "end: "),
            lines.get(i + 3),
        ) {
            let comma = if close.trim() == "}," { "," } else { "" };
            out.push_str(&format!(
                "{head}{start}..{end} {}{comma}\n",
                quote(Span::new(start, end).slice(text))
            ));
            i += 4;
        } else {
            out.push_str(line);
            out.push('\n');
            i += 1;
        }
    }
    out
}

fn quote(s: Option<&str>) -> String {
    match s {
        None => "(not a slice)".to_string(),
        Some(s) => {
            let one_line = s.replace('\n', "⏎");
            let shown: String = one_line.chars().take(48).collect();
            let more = if one_line.chars().count() > 48 {
                "…"
            } else {
                ""
            };
            format!("{shown:?}{more}")
        }
    }
}

/// What must hold of any tree of `text`, whatever the text: spans in bounds and on character boundaries, names that are what their span says,
/// declarations in order and apart, comments that are comments. Returns the violations.
pub fn invariants(text: &str, tree: &Tree) -> Vec<String> {
    let mut bad = Vec::new();
    let slice = |what: &str, span: Span, bad: &mut Vec<String>| -> Option<String> {
        let s = span.slice(text).map(str::to_string);
        if s.is_none() {
            bad.push(format!("{what}: span {span:?} is not a slice of the text"));
        }
        s
    };
    let mut previous_comment = None;
    for &c in &tree.comments {
        if let Some(s) = slice("comment", c, &mut bad)
            && (!s.starts_with('#') || s.contains('\n'))
        {
            bad.push(format!("comment {c:?} is {s:?}"));
        }
        if previous_comment.is_some_and(|p: Span| p.end > c.start) {
            bad.push(format!("comment {c:?} overlaps or precedes the one before"));
        }
        previous_comment = Some(c);
    }
    for import in &tree.imports {
        if let Some(s) = slice("import", import.span, &mut bad)
            && !s.starts_with("import")
        {
            bad.push(format!("import {:?} is {s:?}", import.span));
        }
        if slice("import path", import.path_span, &mut bad).is_some_and(|s| s != import.path) {
            bad.push(format!(
                "import path {:?} is not {:?}",
                import.path_span, import.path
            ));
        }
        if let Some(alias) = &import.alias
            && slice("alias", alias.span, &mut bad).is_some_and(|s| s != alias.text)
        {
            bad.push(format!("alias {alias:?} is not its span"));
        }
    }
    let mut previous_end = 0;
    for decl in &tree.decls {
        let span = decl.span();
        let Some(s) = slice("declaration", span, &mut bad) else {
            continue;
        };
        if !(s.starts_with('@') || ["def", "type", "law"].iter().any(|k| s.starts_with(k))) {
            bad.push(format!(
                "declaration {span:?} starts with {:?}",
                s.chars().take(10).collect::<String>()
            ));
        }
        if span.start < previous_end {
            bad.push(format!(
                "declaration {span:?} starts before the previous one ends ({previous_end})"
            ));
        }
        previous_end = span.end;
        let name = decl.name();
        if slice("name", name.span, &mut bad).is_some_and(|s| s != name.text) {
            bad.push(format!("name {name:?} is not its span"));
        }
        if name.span.start < span.start || name.span.end > span.end {
            bad.push(format!("name {name:?} is outside its declaration {span:?}"));
        }
        if let Some(doc) = decl.doc()
            && (slice("doc", doc, &mut bad).is_some_and(|s| !s.starts_with('#'))
                || doc.end > span.start)
        {
            bad.push(format!(
                "doc {doc:?} of {name:?} is not a comment block above it"
            ));
        }
        let binds: Vec<_> = match decl {
            Decl::Def(d) => {
                if let DefBody::Source(b) = &d.body {
                    slice("body", *b, &mut bad);
                }
                d.params.iter().collect()
            }
            Decl::Type(d) => d
                .params
                .iter()
                .chain(d.ctrs.iter().flat_map(|c| c.fields.iter()))
                .collect(),
            Decl::Law(d) => {
                d.clauses.iter().for_each(|c| {
                    slice("clause name", c.name.span, &mut bad);
                });
                Vec::new()
            }
            _ => Vec::new(),
        };
        for b in binds {
            if slice("binder name", b.name.span, &mut bad).is_some_and(|s| s != b.name.text) {
                bad.push(format!("binder {:?} is not its span", b.name));
            }
            if let Some(ty) = b.ty {
                slice("binder type", ty, &mut bad);
            }
        }
    }
    for e in &tree.errors {
        slice("error", e.span, &mut bad);
    }
    bad
}
