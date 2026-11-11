//! `typescript-cli`: try the analyzer by hand.
//!
//! - `outline <file> [--label <label>]`: the defs of one file.
//! - `files <dir> <file>`: which config owns `file` under `dir`, and what the loaded program holds. The tool for "why is this call unresolved":
//!   the answer is often that the file is not in the program the config describes.
//! - `calls <dir> <file> --entry <qualname> [--id <marker>] [--graph] [--timing]`: the call tree under a def, then the breakdown of the graph (edges by kind,
//!   unresolved edges by reason). A qualname is not unique in a file (D15): when it matches several defs they are listed and the exit code is 2, never
//!   the first one picked; `--id` names the one that carries that marker.

use std::path::Path;
use std::process::ExitCode;

use analyzer_core::text::{Encoding, LineIndex};
use typescript_analysis::{
    Cancel, Entry, Options, Project, RenderOptions, breakdown, call_graph, render, to_text,
};
use typescript_syntax::{Dialect, MarkerStyle, outline};

const USAGE: &str = "usage: typescript-cli outline <file> [--label <label>]
       typescript-cli files <dir> <file>
       typescript-cli calls <dir> <file> --entry <qualname> [--id <marker>] [--graph] [--timing] [--label <label>]";

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

/// Remove `--name` from `args`; whether it was there.
fn take_flag(args: &mut Vec<String>, name: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != name);
    args.len() != before
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let label = take_value(&mut args, "--label").unwrap_or_else(|| "ID".to_owned());
    let entry = take_value(&mut args, "--entry");
    let id = take_value(&mut args, "--id");
    let graph = take_flag(&mut args, "--graph");
    let timing = take_flag(&mut args, "--timing");
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["outline", path] => print_outline(path, &label),
        ["files", dir, file] => print_files(Path::new(dir), Path::new(file)),
        ["calls", dir, file] => match entry {
            Some(entry) => print_calls(
                Path::new(dir),
                file,
                &Calls {
                    entry: &entry,
                    id: id.as_deref(),
                    label,
                    graph,
                    timing,
                },
            ),
            None => {
                eprintln!("calls needs --entry <qualname>\n{USAGE}");
                ExitCode::from(2)
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// What `calls` was asked.
struct Calls<'a> {
    entry: &'a str,
    id: Option<&'a str>,
    label: String,
    graph: bool,
    timing: bool,
}

fn print_calls(dir: &Path, file: &str, ask: &Calls<'_>) -> ExitCode {
    let started = std::time::Instant::now();
    let project = match Project::open(dir, Path::new(file)) {
        Ok(project) => project,
        Err(e) => {
            eprintln!("{file}: {e}");
            return ExitCode::from(1);
        }
    };
    let opened = started.elapsed();
    // The label lives for the whole run of a one-shot program, which is what `Options` asks of a marker style.
    let label: &'static str = Box::leak(ask.label.clone().into_boxed_str());
    let style = MarkerStyle::docstring(label);
    let Some(matches) = project.defs_named(file, ask.entry, style) else {
        eprintln!("{file}: not a TypeScript or JavaScript file the project holds");
        return ExitCode::from(1);
    };
    let chosen = match (ask.id, matches.as_slice()) {
        (_, []) => {
            eprintln!("{file}: no def named `{}`", ask.entry);
            return ExitCode::from(1);
        }
        (Some(id), _) => match matches.iter().find(|m| m.marker.as_deref() == Some(id)) {
            Some(found) => found,
            None => {
                eprintln!("{file}: no def `{}` carries {label}: {id}", ask.entry);
                return ExitCode::from(1);
            }
        },
        (None, [only]) => only,
        (None, _) => {
            // D15: a qualname is not unique; the person says which, by the marker, and the first is never picked
            let text = std::fs::read_to_string(dir.join(file)).unwrap_or_default();
            let lines = LineIndex::new(&text);
            eprintln!(
                "{file}: `{}` matches {} defs; pick one with --id <marker>:",
                ask.entry,
                matches.len()
            );
            for m in &matches {
                let at = lines
                    .position(&text, m.range.start, Encoding::Utf16)
                    .map_or_else(
                        || "?".to_owned(),
                        |p| format!("{}:{}", p.line + 1, p.column + 1),
                    );
                let marker = m
                    .marker
                    .as_ref()
                    .map_or_else(|| "no marker".to_owned(), |m| format!("{label}: {m}"));
                eprintln!("  #{}  {:?}  {at}  {marker}", m.occurrence, m.kind);
            }
            return ExitCode::from(2);
        }
    };
    let entry = Entry {
        qualname: ask.entry,
        occurrence: chosen.occurrence,
    };
    let solving = std::time::Instant::now();
    let Some(graph) = call_graph(&project, file, &entry, &Options::new(style), &Cancel::new())
    else {
        eprintln!("{file}: no def `{}`", ask.entry);
        return ExitCode::from(1);
    };
    if ask.timing {
        eprintln!(
            "open {} ms, graph {} ms",
            opened.as_millis(),
            solving.elapsed().as_millis()
        );
    }
    let summary = breakdown(&graph);
    if ask.graph {
        // the graph on stdout and nothing else, so it can be read as it is; the breakdown goes to stderr
        println!("{graph:#?}");
        eprint!("{summary}");
    } else {
        print!(
            "{}",
            to_text(&render(
                &graph,
                &graph.entries[0],
                &RenderOptions::default()
            ))
        );
        println!();
        print!("{summary}");
    }
    ExitCode::SUCCESS
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
