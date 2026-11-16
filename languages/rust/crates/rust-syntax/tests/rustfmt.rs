//! RS2 §7: `rustfmt` leaves a planned marker alone. Every def of every fixture that takes a marker gets one; the text is formatted with
//! the repo's `rustfmt.toml`, and again with `wrap_comments = true`; each marker line must come out byte-identical (apart from the
//! indentation `rustfmt` chooses) and must still be read from the same def. A test, not a spike, because a `rustfmt` update can change the
//! answer. It needs `rustfmt` on the PATH (the toolchain of `rust-toolchain.toml` has it).

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use analyzer_core::edit::apply;
use rust_syntax::{Edition, MarkerStyle, outline, plan_marker};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: rust_syntax::Placement::Docstring,
};
const CONFIG: &str = "../../../../rustfmt.toml";

/// `text` with a marker `fmt-N` on every def that takes one, and the line each one was written as (without its indentation).
fn mark_everything(text: &str) -> (String, Vec<(String, String)>) {
    let mut text = text.to_owned();
    let mut written = Vec::new();
    loop {
        let marker_text = format!("fmt-{}", written.len());
        let defs = outline(&text, Edition::E2024, STYLE).defs;
        let Some(edit) = defs
            .iter()
            .find_map(|d| plan_marker(&text, Edition::E2024, d, &marker_text, STYLE).ok())
        else {
            return (text, written);
        };
        let line = edit
            .text
            .lines()
            .find(|l| l.contains(&format!("ID: {marker_text}")))
            .expect("the edit holds the marker line")
            .trim()
            .to_owned();
        text = apply(&text, &[edit]).unwrap();
        written.push((marker_text, line));
    }
}

fn rustfmt(text: &str, extra: &[&str]) -> Result<String, String> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--config-path", CONFIG])
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rustfmt is installed");
    let mut stdin = child.stdin.take().unwrap();
    let input = text.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    if out.status.success() {
        Ok(String::from_utf8(out.stdout).unwrap())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// The def (qualname, occurrence) each marker of `fmt-N` is on.
fn homes(text: &str) -> Vec<(String, String)> {
    let o = outline(text, Edition::E2024, STYLE);
    let occurrences = rust_syntax::occurrences(&o);
    let mut homes: Vec<_> = o
        .defs
        .iter()
        .zip(occurrences)
        .filter_map(|(d, n)| {
            let m = d.marker.as_ref()?;
            m.starts_with("fmt-")
                .then(|| (m.clone(), format!("{}#{n}", d.qualname)))
        })
        .collect();
    homes.sort();
    homes
}

#[test]
fn rustfmt_leaves_every_planned_marker_alone() {
    let mut checked = 0;
    let mut files = 0;
    for entry in fs::read_dir("tests/fixtures").unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        // `rustfmt` does not format a file that does not parse
        if !outline(&text, Edition::E2024, STYLE)
            .syntax_errors
            .is_empty()
        {
            continue;
        }
        let (marked, written) = mark_everything(&text);
        if written.is_empty() {
            continue;
        }
        files += 1;
        for (name, extra) in [
            ("repo config", vec![]),
            ("wrap_comments", vec!["--config", "wrap_comments=true"]),
        ] {
            let formatted = rustfmt(&marked, &extra).unwrap_or_else(|e| {
                panic!("{} with {name}: rustfmt refused:\n{e}", path.display())
            });
            for (marker_text, line) in &written {
                assert!(
                    formatted.lines().any(|l| l.trim() == line),
                    "{} with {name}: the line of {marker_text}, `{line}`, was changed",
                    path.display()
                );
            }
            assert_eq!(
                homes(&formatted),
                homes(&marked),
                "{} with {name}: a marker moved to another def",
                path.display()
            );
            checked += written.len();
        }
    }
    assert!(
        files >= 20 && checked >= 300,
        "{files} files, {checked} markers checked"
    );
}

/// The control: `wrap_comments` does change a comment, here a long summary line, so a marker that comes out unchanged was left alone by a
/// `rustfmt` that was looking at comments, not by one that was not.
#[test]
fn wrap_comments_is_in_effect_and_still_leaves_the_marker() {
    let long = format!("/// {}\nfn f() {{}}\n", "word ".repeat(40));
    let (marked, written) = mark_everything(&long);
    let plain = rustfmt(&marked, &[]).unwrap();
    let wrapped = rustfmt(&marked, &["--config", "wrap_comments=true"]).unwrap();
    assert_eq!(
        plain.lines().count(),
        marked.lines().count(),
        "the repo config leaves comments"
    );
    assert!(
        wrapped.lines().count() > plain.lines().count(),
        "the long line was re-wrapped:\n{wrapped}"
    );
    let (_, line) = &written[0];
    assert!(plain.lines().any(|l| l.trim() == line));
    assert!(wrapped.lines().any(|l| l.trim() == line));
}
