//! `typescript-cli calls … --precise`: the context layer on. On these scenarios the layer changes no edge, so the tree, the
//! wire, the merged wire and stderr are byte for byte what the run without the flag prints.
//!
//! The layer does count what it read, in `stats.language` (for example `member_call_on_trusted`). So `--graph` is the same text
//! once the `language: { … }` block of `stats` is taken out of both, and the precise block must hold every entry of the plain
//! one with the same number: the layer adds counters of its own, it never changes or drops one the default resolver says.

use std::path::PathBuf;
use std::process::{Command, Output};

fn scenario(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../scenarios")
        .join(name)
        .join("typescript")
}

/// `typescript-cli calls <scenario>/typescript main.ts --entry main <flags>`.
fn calls(scenario_name: &str, flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_typescript-cli"))
        .arg("calls")
        .arg(scenario(scenario_name))
        .args(["main.ts", "--entry", "main"])
        .args(flags)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

/// Splits the `Debug` text of a graph at the `language: { … }` block of `stats`: the text with the block cut out, and the
/// block's entries as `(name, number)` lines. An empty block prints as `language: {},` on one line; a full one prints one
/// `"name": n,` line per entry between `language: {` and `},`. Any other shape is a failure of this test, not a guess.
fn split_language(text: &str) -> (String, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let starts: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].trim_start().starts_with("language: {"))
        .collect();
    assert_eq!(starts.len(), 1, "expected one `language: {{` line");
    let start = starts[0];
    let first = lines[start].trim();
    let (end, entries) = if first == "language: {}," {
        (start, Vec::new())
    } else {
        assert_eq!(first, "language: {", "unknown shape of the language block");
        let close = (start + 1..lines.len())
            .find(|&i| lines[i].trim() == "},")
            .expect("the language block has no closing `},`");
        let entries = lines[start + 1..close]
            .iter()
            .map(|l| l.trim().to_string())
            .collect();
        (close, entries)
    };
    let mut rest = lines[..start].to_vec();
    rest.extend_from_slice(&lines[end + 1..]);
    (rest.join("\n"), entries)
}

/// Every entry of `plain` is in `precise`, with the same number.
fn assert_keeps_entries(plain: &[String], precise: &[String], at: &str) {
    for entry in plain {
        assert!(
            precise.contains(entry),
            "language entry {entry} of the plain run is missing or changed with --precise: {at}\nprecise has {precise:?}"
        );
    }
}

#[test]
fn precise_prints_what_no_flag_prints() {
    let scenarios = [
        "construct-marked-class",
        "external-call",
        "method-on-class",
        "recursion-direct",
        "straight-line",
        "two-callers",
    ];
    let outputs: [&[&str]; 4] = [&[], &["--wire"], &["--wire", "--merged"], &["--graph"]];
    for name in scenarios {
        for flags in outputs {
            let plain = calls(name, flags);
            let mut with = flags.to_vec();
            with.push("--precise");
            let precise = calls(name, &with);
            let at = format!("{name} {flags:?}");
            if flags == ["--graph"] {
                let (plain_rest, plain_language) = split_language(&stdout(&plain));
                let (precise_rest, precise_language) = split_language(&stdout(&precise));
                assert_eq!(plain_rest, precise_rest, "stdout outside language: {at}");
                assert_keeps_entries(&plain_language, &precise_language, &at);
            } else {
                assert_eq!(stdout(&plain), stdout(&precise), "stdout: {at}");
            }
            assert_eq!(stderr(&plain), stderr(&precise), "stderr: {at}");
        }
    }
}

#[test]
fn the_usage_names_precise() {
    let out = Command::new(env!("CARGO_BIN_EXE_typescript-cli"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--precise"), "{}", stderr(&out));
}
