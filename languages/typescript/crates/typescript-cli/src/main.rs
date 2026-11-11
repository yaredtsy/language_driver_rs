//! `typescript-cli outline <file>`: print the defs of one file, to try the analyzer by hand.

use std::process::ExitCode;

use analyzer_core::text::{Encoding, LineIndex};
use typescript_syntax::{Dialect, outline};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [command, path] = args.as_slice() else {
        eprintln!("usage: typescript-cli outline <file>");
        return ExitCode::from(2);
    };
    if command != "outline" {
        eprintln!("unknown command {command:?}; usage: typescript-cli outline <file>");
        return ExitCode::from(2);
    }
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
    let result = outline(&text, dialect);
    for def in &result.defs {
        let depth = std::iter::successors(def.parent, |&p| result.defs[p].parent).count();
        let flag = if def.syntax_error {
            "  SYNTAX-ERROR"
        } else {
            ""
        };
        println!(
            "{}{:?} {}  {}-{}{flag}",
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
