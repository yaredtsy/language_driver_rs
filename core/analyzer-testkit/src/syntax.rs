//! The three checks every language's syntax crate gets for free.

use std::fs;
use std::path::{Path, PathBuf};

use analyzer_core::edit::{Edit, apply};
use analyzer_core::outline::{Def, Outline};

use crate::expect_file;

/// A language's syntax crate, as three plain function pointers and two paths. No trait: nothing here carries state.
pub struct Subject {
    pub outline: fn(&str) -> Outline,
    /// The edit that puts `marker` on `def`, or `None` when the language refuses (already marked, not on its own line).
    pub plan_marker: fn(&str, &Def, &str) -> Option<Edit>,
    /// One file per case.
    pub fixtures: &'static str,
    /// One reviewed snapshot per fixture, named after it.
    pub snapshots: &'static str,
}

/// The outline as stable text: one line per def, indented by depth. This is what snapshots hold, so it is fixed.
pub fn format_outline(outline: &Outline) -> String {
    let mut out = String::new();
    if let Some(m) = &outline.file_marker {
        out.push_str(&format!("file marker: {m}\n"));
    }
    for (i, d) in outline.defs.iter().enumerate() {
        let depth =
            std::iter::successors(d.parent, |&p| outline.defs.get(p).and_then(|d| d.parent))
                .count();
        out.push_str(&"  ".repeat(depth));
        out.push_str(&format!(
            "{:?} {} [{}..{}] name [{}..{}]",
            d.kind, d.qualname, d.range.start, d.range.end, d.name_range.start, d.name_range.end
        ));
        if !d.params.is_empty() {
            out.push_str(&format!(" ({})", d.params.join(", ")));
        }
        if let Some(m) = &d.marker {
            out.push_str(&format!(" marker={m:?}"));
        }
        if d.syntax_error {
            out.push_str(" SYNTAX-ERROR");
        }
        if d.parent.is_some_and(|p| p >= i) {
            out.push_str(" BAD-PARENT");
        }
        out.push('\n');
    }
    for e in &outline.syntax_errors {
        out.push_str(&format!(
            "syntax error [{}..{}]: {}\n",
            e.range.start, e.range.end, e.message
        ));
    }
    out
}

fn fixtures(dir: &str) -> Vec<(PathBuf, String)> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read fixtures {dir}: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures in {dir}");
    paths
        .into_iter()
        .map(|p| {
            let text = fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()));
            (p, text)
        })
        .collect()
}

fn name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// The outline of every fixture equals its reviewed snapshot, and is a well-formed list.
pub fn snapshot_every_fixture(subject: &Subject) {
    for (path, text) in fixtures(subject.fixtures) {
        let outline = (subject.outline)(&text);
        assert!(
            outline.is_well_formed(),
            "{}: parents must come before children, in source order",
            name(&path)
        );
        let snapshot = Path::new(subject.snapshots).join(format!("{}.outline", name(&path)));
        expect_file(&snapshot, &format_outline(&outline));
    }
}

/// Plan a marker on every def of every fixture, apply it, and outline again. The def carries the marker, no other def
/// changed, nothing was lost (the old text is a subsequence of the new), and planning again is refused. "Only the
/// marker was added" is stricter and is the language's own test: what counts as the marker, a line or a rewritten
/// docstring, differs per language.
pub fn marker_round_trip(subject: &Subject) {
    let mut planned = 0;
    for (path, text) in fixtures(subject.fixtures) {
        let file = name(&path);
        let before = (subject.outline)(&text);
        for (i, def) in before.defs.iter().enumerate() {
            let marker = format!("round-trip-{i}");
            let Some(edit) = (subject.plan_marker)(&text, def, &marker) else {
                continue;
            };
            planned += 1;
            let at = &def.qualname;
            let new = apply(&text, std::slice::from_ref(&edit))
                .unwrap_or_else(|| panic!("{file}: the edit for {at} does not apply"));

            assert!(
                is_subsequence(&text, &new),
                "{file}: marking {at} damaged the text: something that was there is gone"
            );

            let after = (subject.outline)(&new);
            assert_eq!(
                after.defs.len(),
                before.defs.len(),
                "{file}: marking {at} changed the number of defs"
            );
            assert_eq!(
                after.syntax_errors.len(),
                before.syntax_errors.len(),
                "{file}: marking {at} changed the syntax errors"
            );
            for (j, (b, a)) in before.defs.iter().zip(&after.defs).enumerate() {
                assert_eq!(a.qualname, b.qualname, "{file}: marking {at} renamed a def");
                let want = if i == j {
                    Some(marker.as_str())
                } else {
                    b.marker.as_deref()
                };
                assert_eq!(
                    a.marker.as_deref(),
                    want,
                    "{file}: after marking {at}, the marker of {}",
                    b.qualname
                );
            }
            assert!(
                (subject.plan_marker)(&new, &after.defs[i], "second").is_none(),
                "{file}: {at} was marked twice"
            );
        }
    }
    assert!(
        planned > 0,
        "no fixture def accepted a marker: the round trip proved nothing"
    );
}

/// Every byte of `old` appears in `new`, in order. An edit that only adds text passes; one that deletes or rewrites fails.
fn is_subsequence(old: &str, new: &str) -> bool {
    let mut rest = new.bytes();
    old.bytes().all(|b| rest.any(|n| n == b))
}

/// The outline of every fixture, cut at every 50th byte (on a character boundary), returns and is well formed.
pub fn cut_every_50_bytes(subject: &Subject) {
    for (path, text) in fixtures(subject.fixtures) {
        let mut cut = 0;
        loop {
            let mut at = cut.min(text.len());
            while !text.is_char_boundary(at) {
                at -= 1;
            }
            let part = &text[..at];
            let outline = (subject.outline)(part);
            assert!(
                outline.is_well_formed(),
                "{} cut at {at}: not a well-formed list",
                name(&path)
            );
            let in_bounds = |end: u32| end as usize <= part.len();
            assert!(
                outline
                    .defs
                    .iter()
                    .all(|d| in_bounds(d.range.end) && in_bounds(d.name_range.end)),
                "{} cut at {at}: a def reaches past the text",
                name(&path)
            );
            if at == text.len() {
                break;
            }
            cut += 50;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toy;

    #[test]
    fn toy_language_passes_all_three() {
        snapshot_every_fixture(&toy::SYNTAX);
        marker_round_trip(&toy::SYNTAX);
        cut_every_50_bytes(&toy::SYNTAX);
    }

    #[test]
    #[should_panic(expected = "damaged the text")]
    fn a_planner_that_destroys_text_is_caught() {
        // replaces the `fn` line instead of inserting above it
        fn bad(text: &str, def: &Def, marker: &str) -> Option<Edit> {
            let end = text[def.range.start as usize..].find('\n')? as u32 + def.range.start;
            Some(Edit::replace(
                analyzer_core::text::Span::new(def.range.start, end),
                format!("// ID: {marker}"),
            ))
        }
        marker_round_trip(&Subject {
            plan_marker: bad,
            ..toy::SYNTAX
        });
    }

    #[test]
    fn subsequence_means_added_only() {
        assert!(is_subsequence("ab", "xaxbx"));
        assert!(is_subsequence("", "x"));
        assert!(!is_subsequence("ab", "ba"));
        assert!(!is_subsequence("abc", "ab"));
    }

    #[test]
    #[should_panic(expected = "changed the number of defs")]
    fn a_planner_that_hides_a_def_is_caught() {
        // keeps every old byte but turns the `fn` line into a comment, so the def disappears
        fn hide(text: &str, def: &Def, marker: &str) -> Option<Edit> {
            let _ = (text, marker);
            Some(Edit::insert(def.range.start, "// "))
        }
        marker_round_trip(&Subject {
            plan_marker: hide,
            ..toy::SYNTAX
        });
    }

    #[test]
    #[should_panic(expected = "proved nothing")]
    fn a_planner_that_refuses_everything_proves_nothing() {
        marker_round_trip(&Subject {
            plan_marker: |_, _, _| None,
            ..toy::SYNTAX
        });
    }

    #[test]
    #[should_panic(expected = "reaches past the text")]
    fn an_outline_that_ignores_the_cut_is_caught() {
        cut_every_50_bytes(&Subject {
            outline: |_| toy::outline(&"fn a\n".repeat(30)),
            ..toy::SYNTAX
        });
    }

    #[test]
    fn outline_text_is_stable() {
        let o = toy::outline("// ID: m1\nfn a\n");
        assert_eq!(
            format_outline(&o),
            "Function a [0..14] name [13..14] marker=\"m1\"\n"
        );
    }
}
