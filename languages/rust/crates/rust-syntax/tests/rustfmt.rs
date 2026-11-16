//! RS2 §7: `rustfmt` leaves a planned marker alone. Every def of every fixture that takes a marker gets one; the text is formatted with
//! the repo's `rustfmt.toml`, and again with `wrap_comments = true`; each marker line must come out byte-identical (apart from the
//! indentation `rustfmt` chooses) and must still be read from the same def. A test, not a spike, because a `rustfmt` update can change the
//! answer. It needs `rustfmt` on the PATH (the toolchain of `rust-toolchain.toml` has it).

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use analyzer_core::edit::apply;
use rust_syntax::{Edition, MarkerStyle, header_marker, outline, plan_header_marker, plan_marker};

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
            // the header last, once every def has its marker
            if let Ok(edit) = plan_header_marker(&text, "fmt-header", STYLE) {
                text = apply(&text, &[edit]).unwrap();
            }
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

/// Is the header of `marked` directly under a block of `//` lines in which one is wider than `rustfmt`'s `comment_width` (80)? With
/// `wrap_comments` the block is refilled as one paragraph, and the header line, which is part of it, is joined to the text above
/// (`// end ID: fmt-header`): the one shape a header is not safe in (see `a_header_under_a_wide_comment_is_joined_by_wrap_comments`).
fn header_is_under_a_wide_comment(marked: &str) -> bool {
    let lines: Vec<&str> = marked.lines().collect();
    let Some(at) = lines.iter().position(|l| l.trim() == "// ID: fmt-header") else {
        return false;
    };
    lines[..at]
        .iter()
        .rev()
        .map(|l| l.trim_start())
        .take_while(|l| l.starts_with("//") && !l.starts_with("///") && !l.starts_with("//!"))
        .any(|l| l.len() > 80)
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
            // the header is read from the formatted text too, except in the one shape that is known not to survive `wrap_comments`
            let at_risk = name == "wrap_comments" && header_is_under_a_wide_comment(&marked);
            if marked.contains("ID: fmt-header") && !at_risk {
                assert!(
                    formatted.lines().any(|l| l.trim() == "// ID: fmt-header"),
                    "{} with {name}: the header line was changed",
                    path.display()
                );
                assert_eq!(
                    header_marker(&formatted, STYLE).as_deref(),
                    Some("fmt-header"),
                    "{} with {name}: the header is no longer read",
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

/// A known gap, pinned so that it is seen and is the first thing to fail when it is fixed (RS2 §6, found in step 4): a header put as the last
/// line of a `//` block in which one line is wider than `comment_width` (80) is joined to the text above by `wrap_comments`, which refills the
/// block as one paragraph, and then reads as nothing. A second plan would write another header. The repo's own config leaves it alone, and
/// so does a header under short lines.
#[test]
fn a_header_under_a_wide_comment_is_joined_by_wrap_comments() {
    // 90 columns and a short tail, so that the refill leaves the header on the tail's line
    let wide = format!("// {}end\nfn f() {{}}\n", "word ".repeat(18));
    let edit = plan_header_marker(&wide, "fmt-header", STYLE).unwrap();
    let marked = apply(&wide, &[edit]).unwrap();
    assert!(header_is_under_a_wide_comment(&marked));
    assert_eq!(
        header_marker(&rustfmt(&marked, &[]).unwrap(), STYLE).as_deref(),
        Some("fmt-header"),
        "the repo's own config leaves it alone"
    );
    let wrapped = rustfmt(&marked, &["--config", "wrap_comments=true"]).unwrap();
    assert!(wrapped.contains("end ID: fmt-header"), "{wrapped}");
    assert_eq!(
        header_marker(&wrapped, STYLE),
        None,
        "the gap is closed: delete this test and the `at_risk` exception"
    );
    // short lines above are safe
    let short = "// short licence\n// another short line\nfn f() {}\n";
    let edit = plan_header_marker(short, "fmt-header", STYLE).unwrap();
    let marked = apply(short, &[edit]).unwrap();
    assert!(!header_is_under_a_wide_comment(&marked));
    let wrapped = rustfmt(&marked, &["--config", "wrap_comments=true"]).unwrap();
    assert_eq!(
        header_marker(&wrapped, STYLE).as_deref(),
        Some("fmt-header")
    );
}
