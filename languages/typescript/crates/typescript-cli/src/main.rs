//! `typescript-cli`: try the analyzer by hand.
//!
//! - `outline <file> [--label <label>]`: the defs of one file.
//! - `files <dir> <file>`: which config owns `file` under `dir`, and what the loaded program holds. The tool for "why is this call unresolved":
//!   the answer is often that the file is not in the program the config describes.

use std::path::Path;
use std::process::ExitCode;

use analyzer_core::text::{Encoding, LineIndex};
use typescript_analysis::Project;
use typescript_syntax::{Dialect, MarkerStyle, outline};

const USAGE: &str = "usage: typescript-cli outline <file> [--label <label>]\n       typescript-cli files <dir> <file>";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let label = match args.iter().position(|a| a == "--label") {
        Some(i) if i + 1 < args.len() => {
            let label = args.remove(i + 1);
            args.remove(i);
            label
        }
        _ => "ID".to_owned(),
    };
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["outline", path] => print_outline(path, &label),
        ["files", dir, file] => print_files(Path::new(dir), Path::new(file)),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// The config that was chosen, then the program: project files, the repo's declaration files, and what a reload watches.
fn print_files(dir: &Path, file: &Path) -> ExitCode {
    let project = match Project::open(dir, file) {
        Ok(project) => project,
        Err(e) => {
            eprintln!("{}: {e}", file.display());
            return ExitCode::from(1);
        }
    };
    println!(
        "config: {}",
        project.config_file().unwrap_or("(inferred project)")
    );
    for (title, lines) in [
        ("files", project.files()),
        ("declaration files", project.declaration_files()),
        ("environment inputs", project.environment_inputs()),
    ] {
        println!("{title} ({}):", lines.len());
        for line in lines {
            println!("  {line}");
        }
    }
    ExitCode::SUCCESS
}

fn print_outline(path: &str, label: &str) -> ExitCode {
    let Some(dialect) = Dialect::from_path(path) else {
        eprintln!("{path}: not a .ts .tsx .js .jsx .mts .cts .mjs .cjs file");
        return ExitCode::from(2);
    };
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
    let result = outline(&text, dialect, MarkerStyle::docstring(label));
    for def in &result.defs {
        let depth = std::iter::successors(def.parent, |&p| result.defs[p].parent).count();
        let flag = if def.syntax_error {
            "  SYNTAX-ERROR"
        } else {
            ""
        };
        // a qualname is not unique in a file: the marker is what tells two defs of one name apart
        let marker = def
            .marker
            .as_ref()
            .map_or_else(String::new, |m| format!("  {label}: {m}"));
        println!(
            "{}{:?} {}  {}-{}{marker}{flag}",
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
