//! The file header (03 §1.3): what `header_marker` reads and where `plan_header_marker` writes it. The planned text of every fixture is a reviewed file
//! under `tests/snapshots/planned/` (`ANALYZER_BLESS=1` writes it; read it before committing): the header and a marker on every def, all applied.

use std::fs;
use std::path::Path;

use analyzer_core::edit::apply;
use analyzer_testkit::expect_file;
use bend_syntax::{
    MarkerStyle, Placement, Skip, header_marker, header_slot, outline, plan_header_marker,
    plan_marker,
};

const ID: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Above,
};

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new("tests/fixtures").join(format!("{name}.bend")))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn header_planned(text: &str, marker: &str) -> String {
    let edit = plan_header_marker(text, marker, ID).unwrap_or_else(|e| panic!("{text:?}: {e}"));
    apply(text, &[edit]).expect("the edit applies")
}

#[test]
fn the_header_is_read_from_the_comments_before_the_first_import() {
    for (name, want) in [
        ("file_marker_before_imports", Some("file-1")),
        ("file_marker_after_licence", Some("file-3")),
        // no imports: the comments before the first def's own doc block
        ("file_marker_no_imports", Some("file-2")),
        // a marker after the first import is no header
        ("file_marker_after_first_import", None),
        // a marker directly above the first def is that def's, not the file's
        ("def_marker_not_file_marker", None),
        ("no_file_marker", None),
        ("marked_def", None),
        ("marker_blank_line_away", None),
        ("only_comments", None),
        ("empty", None),
        ("imports_only", None),
    ] {
        let text = fixture(name);
        assert_eq!(header_marker(&text, ID).as_deref(), want, "{name}");
        assert_eq!(
            outline(&text, ID).file_marker.as_deref(),
            want,
            "{name}: Outline::file_marker"
        );
    }
}

#[test]
fn a_def_marker_directly_above_the_first_def_is_the_defs() {
    let text = fixture("def_marker_not_file_marker");
    let o = outline(&text, ID);
    assert_eq!(o.file_marker, None);
    assert_eq!(o.defs[0].marker.as_deref(), Some("first-1"));
}

/// The slot is offset 0 for every file, 127 of the corpus start with code on the first line; past a byte-order mark.
#[test]
fn the_slot_is_the_start_of_the_file() {
    assert_eq!(header_slot(""), 0);
    assert_eq!(header_slot("def a() -> Nat: 0\n"), 0);
    assert_eq!(header_slot("# comment\n"), 0);
    assert_eq!(header_slot("\u{feff}def a() -> Nat: 0\n"), 3);
}

#[test]
fn the_header_goes_at_offset_zero_with_the_files_line_ending() {
    assert_eq!(
        header_planned("import Base\n\ndef a() -> Nat: 0\n", "f"),
        "# ID: f\nimport Base\n\ndef a() -> Nat: 0\n"
    );
    assert_eq!(
        header_planned("# licence\n\nimport Base\n", "f"),
        "# ID: f\n# licence\n\nimport Base\n"
    );
    assert_eq!(
        header_planned("import Base\r\n", "f"),
        "# ID: f\r\nimport Base\r\n"
    );
    assert_eq!(header_planned("", "f"), "# ID: f\n");
    assert_eq!(
        header_planned("# only a comment", "f"),
        "# ID: f\n# only a comment"
    );
    assert_eq!(
        header_planned("\u{feff}import Base\n", "f"),
        "\u{feff}# ID: f\nimport Base\n",
        "the byte-order mark stays first"
    );
}

/// Text at offset 0 that is the first def (its first token or its doc block) is not the file's to be marked directly: a line right above it would be
/// that def's marker. An empty line follows the header then, and the two stay apart.
#[test]
fn a_header_above_the_first_def_is_kept_apart_from_it() {
    let text = "def a() -> Nat: 0\n";
    let out = header_planned(text, "f");
    assert_eq!(out, "# ID: f\n\ndef a() -> Nat: 0\n");
    let o = outline(&out, ID);
    assert_eq!(
        (o.file_marker.as_deref(), o.defs[0].marker.as_deref()),
        (Some("f"), None)
    );
    // the same when the first def has a doc block: the block is the def's, the header is not part of it
    let text = "# the doc\ndef a() -> Nat: 0\n";
    let out = header_planned(text, "f");
    assert_eq!(out, "# ID: f\n\n# the doc\ndef a() -> Nat: 0\n");
    let o = outline(&out, ID);
    assert_eq!(
        (o.file_marker.as_deref(), o.defs[0].marker.as_deref()),
        (Some("f"), None)
    );
    // and the def can still be marked on its own
    let edit = plan_marker(&out, &o.defs[0], "d", ID).expect("the def is unmarked");
    let both = apply(&out, &[edit]).expect("applies");
    let o = outline(&both, ID);
    assert_eq!(
        (o.file_marker.as_deref(), o.defs[0].marker.as_deref()),
        (Some("f"), Some("d"))
    );
    // CRLF
    assert_eq!(
        header_planned("def a() -> Nat: 0\r\n", "f"),
        "# ID: f\r\n\r\ndef a() -> Nat: 0\r\n"
    );
}

#[test]
fn a_marked_file_is_refused_with_its_marker() {
    let text = fixture("file_marker_before_imports");
    assert_eq!(
        plan_header_marker(&text, "other", ID),
        Err(Skip::AlreadyMarked {
            existing: "file-1".to_string()
        })
    );
}

#[test]
fn a_text_that_does_not_read_back_is_refused() {
    for bad in ["", " x", "x ", "a\nb"] {
        assert_eq!(
            plan_header_marker("import Base\n", bad, ID),
            Err(Skip::InvalidText),
            "{bad:?}"
        );
    }
    assert_eq!(
        plan_header_marker("import Base\n", "x", MarkerStyle::docstring("ID")),
        Err(Skip::PlacementNotSupported)
    );
}

/// A file Bend cannot read still takes a header at the top, and reads it back.
#[test]
fn a_broken_file_takes_a_header() {
    let text = "def broken(a: Nat, , b) -> Nat: 0\ndef c() -> Nat: 1\n";
    let out = header_planned(text, "f");
    assert_eq!(header_marker(&out, ID).as_deref(), Some("f"));
}

/// The planned text of every fixture: the header and a marker on every def that takes one, applied together. A reviewed snapshot.
#[test]
fn the_planned_text_of_every_fixture_is_a_reviewed_file() {
    let mut names: Vec<_> = fs::read_dir("tests/fixtures")
        .expect("fixtures")
        .map(|e| e.expect("entry").path())
        .collect();
    names.sort();
    for path in names {
        let text = fs::read_to_string(&path).expect("text");
        let o = outline(&text, ID);
        let mut edits = Vec::new();
        if let Ok(e) = plan_header_marker(&text, "FILE", ID) {
            edits.push(e);
        }
        for (i, d) in o.defs.iter().enumerate() {
            if let Ok(e) = plan_marker(&text, d, &format!("d{i}"), ID) {
                edits.push(e);
            }
        }
        let out = apply(&text, &edits).expect("all the edits apply together");
        let name = path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        // CR and BOM are made visible: a snapshot is read by a person
        let shown = out.replace('\r', "␍").replace('\u{feff}', "␣BOM␣");
        expect_file(&Path::new("tests/snapshots/planned").join(&name), &shown);
        let after = outline(&out, ID);
        if o.file_marker.is_none() && edits.len() > o.defs.len() {
            // the header was planned: it reads back, and no def took it
            assert_eq!(after.file_marker.as_deref(), Some("FILE"), "{name}");
        }
        for (i, d) in after.defs.iter().enumerate() {
            assert!(
                d.marker.as_deref() != Some("FILE"),
                "{name}: def {i} took the file's marker"
            );
        }
        assert_eq!(after.defs.len(), o.defs.len(), "{name}");
    }
}
