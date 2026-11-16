//! `rust-cli`: try the analyzer by hand.
//!
//! - `outline <file> [--label <label>] [--edition <2015|2018|2021|2024>]`: the defs of one file. A qualname is not unique in a file (D15), so a def
//!   that is not the first of its name prints `#<occurrence>` (RS1 §4), and a person who has to name it says which one. Without `--edition` the
//!   file is read as the latest edition: the driver reads it from `Cargo.toml`, and this tool does not look at one.

use std::process::ExitCode;

use analyzer_core::text::{Encoding, LineIndex};
use rust_syntax::{Edition, MarkerStyle, occurrences, outline};

const USAGE: &str =
    "usage: rust-cli outline <file> [--label <label>] [--edition <2015|2018|2021|2024>]";

/// Remove `--name value` from `args` and return the value.
fn take_value(args: &mut Vec<String>, name: &str) -> Option<String> {
    let at = args.iter().position(|a| a == name)?;
    if at + 1 >= args.len() {
        return None;
    }
    let value = args.remove(at + 1);
    args.remove(at);
    Some(value)
}

fn edition(written: &str) -> Option<Edition> {
    match written {
        "2015" => Some(Edition::E2015),
        "2018" => Some(Edition::E2018),
        "2021" => Some(Edition::E2021),
        "2024" => Some(Edition::E2024),
        _ => None,
    }
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let label = take_value(&mut args, "--label").unwrap_or_else(|| "ID".to_owned());
    let written_edition = take_value(&mut args, "--edition");
    let edition = match written_edition.as_deref().map(edition) {
        None => Edition::LATEST,
        Some(Some(edition)) => edition,
        Some(None) => {
            eprintln!("--edition is one of 2015, 2018, 2021, 2024\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["outline", path] => print_outline(path, &label, edition),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn print_outline(path: &str, label: &str, edition: Edition) -> ExitCode {
    if !path.ends_with(".rs") {
        eprintln!("{path}: not a .rs file");
        return ExitCode::from(2);
    }
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::from(1);
        }
    };
    let lines = LineIndex::new(&text);
    // 1-based line:column in UTF-16 units, as an editor shows them
    let at = |offset| {
        lines.position(&text, offset, Encoding::Utf16).map_or_else(
            || "?".to_owned(),
            |p| format!("{}:{}", p.line + 1, p.column + 1),
        )
    };
    // the label is a parameter of the analyzer; the command line gives it (default "ID")
    let result = outline(&text, edition, MarkerStyle::docstring(label));
    for (def, occurrence) in result.defs.iter().zip(occurrences(&result)) {
        let depth = std::iter::successors(def.parent, |&p| result.defs[p].parent).count();
        let twin = if occurrence == 0 {
            String::new()
        } else {
            format!(" #{occurrence}")
        };
        let marker = def
            .marker
            .as_ref()
            .map_or_else(String::new, |m| format!("  {label}: {m}"));
        let flag = if def.syntax_error {
            "  SYNTAX-ERROR"
        } else {
            ""
        };
        println!(
            "{}{:?} {}{twin}  {}-{}{marker}{flag}",
            "  ".repeat(depth),
            def.kind,
            def.qualname,
            at(def.range.start),
            at(def.range.end)
        );
    }
    for e in &result.syntax_errors {
        println!("syntax error {}: {}", at(e.range.start), e.message);
    }
    ExitCode::SUCCESS
}
