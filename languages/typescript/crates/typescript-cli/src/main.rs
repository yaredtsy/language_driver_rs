//! `typescript-cli`: try the analyzer by hand.
//!
//! - `outline <file> [--label <label>]`: the defs of one file.
//! - `files <dir> <file>`: which config owns `file` under `dir`, and what the loaded program holds. The tool for "why is this call unresolved":
//!   the answer is often that the file is not in the program the config describes.
//! - `calls <dir> <file> [--entry <qualname> [--id <marker>]] [--graph | --wire [--merged] [--function-prefix P] [--class-prefix P]] [--timing]`: the call
//!   tree under a def, then the breakdown of the graph (edges by kind, unresolved edges by reason). A qualname is not unique in a file (D15): when it
//!   matches several defs they are listed and the exit code is 2, never the first one picked; `--id` names the one that carries that marker. With no
//!   `--entry` the entry is the file's own code (the module, D32), and `--id` is a usage error: it picks among defs, and a file is one. `--wire` prints
//!   the wire (D17) instead of the tree, as JSON alone on stdout, with the prefixes the caller gives for the ids of functions and classes.

use std::path::Path;
use std::process::ExitCode;

use analyzer_core::text::{Encoding, LineIndex};
use typescript_analysis::{
    Cancel, Entry, Options, Project, RenderOptions, WireOptions, WireStyle, breakdown, call_graph,
    render, to_text, wire,
};
use typescript_syntax::{Dialect, MarkerStyle, outline};

const USAGE: &str = "usage: typescript-cli outline <file> [--label <label>]
       typescript-cli files <dir> <file>
       typescript-cli calls <dir> <file> [--entry <qualname> [--id <marker>]] [--graph | --wire [--merged] [--function-prefix <p>] [--class-prefix <p>]] [--timing] [--label <label>]";

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
    let wire = take_flag(&mut args, "--wire");
    let merged = take_flag(&mut args, "--merged");
    let function_prefix = take_value(&mut args, "--function-prefix");
    let class_prefix = take_value(&mut args, "--class-prefix");
    let timing = take_flag(&mut args, "--timing");
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["outline", path] => print_outline(path, &label),
        ["files", dir, file] => print_files(Path::new(dir), Path::new(file)),
        ["calls", dir, file]
            if (graph && wire)
                || (!wire && (merged || function_prefix.is_some() || class_prefix.is_some())) =>
        {
            eprintln!(
                "--graph and --wire are two outputs, and --merged and the prefixes belong to --wire\n{USAGE}"
            );
            ExitCode::from(2)
        }
        ["calls", _, _] if entry.is_none() && id.is_some() => {
            eprintln!(
                "--id picks among the defs that --entry names; without --entry the entry is the file\n{USAGE}"
            );
            ExitCode::from(2)
        }
        ["calls", dir, file] => print_calls(
            Path::new(dir),
            file,
            &Calls {
                entry: entry.as_deref(),
                id: id.as_deref(),
                label,
                graph,
                wire: wire.then(|| WireStyle {
                    function_prefix: function_prefix.unwrap_or_default(),
                    class_prefix: class_prefix.unwrap_or_default(),
                }),
                merged,
                timing,
            },
        ),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// What `calls` was asked.
struct Calls<'a> {
    /// The qualname of a def; `None` is the file's own code.
    entry: Option<&'a str>,
    id: Option<&'a str>,
    label: String,
    graph: bool,
    /// `Some`: print the wire, with these prefixes.
    wire: Option<WireStyle>,
    merged: bool,
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
    let entry = match ask.entry {
        Some(qualname) => match choose_def(&project, dir, file, qualname, ask.id, style) {
            Ok(entry) => entry,
            Err(code) => return code,
        },
        None => Entry::File,
    };
    let solving = std::time::Instant::now();
    let Some(graph) = call_graph(&project, file, &entry, &Options::new(style), &Cancel::new())
    else {
        match ask.entry {
            Some(qualname) => eprintln!("{file}: no def `{qualname}`"),
            None => eprintln!("{file}: {NOT_PROJECT_CODE}"),
        }
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
    if let Some(style) = &ask.wire {
        // the wire alone on stdout: JSON, so it can be read as it is; the breakdown goes to stderr
        let options = WireOptions::new(style.clone(), ask.merged, RenderOptions::default());
        let frame = wire(&graph, &graph.entries[0], &project, &options);
        match serde_json::to_string_pretty(&frame) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("{file}: the wire cannot be written: {e}");
                return ExitCode::from(1);
            }
        }
        eprint!("{summary}");
    } else if ask.graph {
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

/// `line:column`, 1-based, in UTF-16 units, as an editor shows them.
fn line_and_column(text: &str, lines: &LineIndex, offset: u32) -> String {
    lines.position(text, offset, Encoding::Utf16).map_or_else(
        || "?".to_owned(),
        |p| format!("{}:{}", p.line + 1, p.column + 1),
    )
}

/// The def of `file` that owns the marker `id` (D34 rule 1: the first in source order that carries the text), with where it is: from the outline alone,
/// which reports the marker on every def that carries it. `None` when the file cannot be read as code or no def carries it.
fn owner_of(
    dir: &Path,
    file: &str,
    id: &str,
    style: MarkerStyle<'static>,
) -> Option<(String, String)> {
    let text = std::fs::read_to_string(dir.join(file)).ok()?;
    let found = outline(&text, Dialect::from_path(file)?, style);
    let owner = found
        .defs
        .iter()
        .filter(|def| def.marker.as_deref() == Some(id))
        .min_by_key(|def| def.range.start)?;
    let lines = LineIndex::new(&text);
    Some((
        owner.qualname.clone(),
        line_and_column(&text, &lines, owner.range.start),
    ))
}

const NOT_PROJECT_CODE: &str = "not project code (in node_modules, a declaration file, outside the root) or not a TypeScript or JavaScript file the project holds";

/// The def `qualname` names in `file`, as an entry: the one `id` picks, or the only one. A message and the exit code when there is none, or several (D15).
fn choose_def<'a>(
    project: &Project,
    dir: &Path,
    file: &str,
    qualname: &'a str,
    id: Option<&str>,
    style: MarkerStyle<'static>,
) -> Result<Entry<'a>, ExitCode> {
    let label = style.label;
    let Some(matches) = project.defs_named(file, qualname, style) else {
        eprintln!("{file}: {NOT_PROJECT_CODE}");
        return Err(ExitCode::from(1));
    };
    let chosen = match (id, matches.as_slice()) {
        (_, []) => {
            eprintln!("{file}: no def named `{qualname}`");
            return Err(ExitCode::from(1));
        }
        // the def that owns the marker (D34): a def that carries the same text without owning it has another id, so it is not what `--id` names
        (Some(id), _) => match matches
            .iter()
            .find(|m| m.owns_marker && m.marker.as_deref() == Some(id))
        {
            Some(found) => found,
            None if matches.iter().any(|m| m.marker.as_deref() == Some(id)) => {
                eprintln!("{file}: no def `{qualname}` owns {label}: {id}");
                match owner_of(dir, file, id, style) {
                    Some((owner, at)) => eprintln!(
                        "  `{owner}` at {at} owns it, and `{qualname}` is named by where it is"
                    ),
                    None => eprintln!("  another def of the file owns it"),
                }
                return Err(ExitCode::from(1));
            }
            None => {
                eprintln!("{file}: no def `{qualname}` carries {label}: {id}");
                return Err(ExitCode::from(1));
            }
        },
        (None, [only]) => only,
        (None, _) => {
            // D15: a qualname is not unique; the person says which, by the marker, and the first is never picked
            let text = std::fs::read_to_string(dir.join(file)).unwrap_or_default();
            let lines = LineIndex::new(&text);
            eprintln!(
                "{file}: `{qualname}` matches {} defs; pick one with --id <marker>:",
                matches.len()
            );
            for m in &matches {
                let at = line_and_column(&text, &lines, m.range.start);
                let marker = m
                    .marker
                    .as_ref()
                    .map_or_else(|| "no marker".to_owned(), |m| format!("{label}: {m}"));
                eprintln!("  #{}  {:?}  {at}  {marker}", m.occurrence, m.kind);
            }
            return Err(ExitCode::from(2));
        }
    };
    Ok(Entry::Def {
        qualname,
        occurrence: chosen.occurrence,
    })
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
