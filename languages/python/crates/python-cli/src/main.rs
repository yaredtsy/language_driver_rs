//! `python-cli`: try the analyzer by hand.
//!
//! - `outline <file> [--label <label>]`: the defs of one file.
//! - `calls <dir> <file> --entry <qualname> [--occurrence <n> | --id <marker>] [--graph | --wire [--merged] [--function-prefix <p>]
//!   [--class-prefix <p>]] [--only-marked] [--timing] [--repeat <n>] [--label <label>]`: the call tree under a def, a blank line, then the counters; with
//!   `--graph` the graph, and with `--wire` the wire (D17), as JSON alone on stdout (the counters go to stderr, and the wire has none). A qualname is not unique in a file (D15, D20): when it matches several defs they are
//!   listed and the exit code is 2, never the first one picked; `--occurrence` or `--id` says which.
//!   `--repeat <n>` (n >= 1) asks the same thing `n` times on one kept `Project`, refreshed before each: it is for timing a kept project,
//!   as a driver keeps one. The output is printed once; if a request's output differs from the first one's, nothing is printed on
//!   stdout, stderr says which request, and the exit code is 1. With `--timing`: one `open` line, then a `request <i>` line each.
//!
//! Exit codes: 0 done, 1 the thing asked for is not there, 2 the command line is wrong. stdout is the answer and nothing else;
//! messages go to stderr.

use std::process::ExitCode;

use std::path::Path;
use std::time::{Duration, Instant};

use analyzer_core::text::{Encoding, LineIndex};
use python_analysis::{
    Entry, Options, Project, Stats, WireStyle, call_graph, call_wire, render, to_text,
};
use python_syntax::{MarkerStyle, outline};

const USAGE: &str = "usage: python-cli outline <file> [--label <label>]
       python-cli calls <dir> <file> --entry <qualname> [--occurrence <n> | --id <marker>]
                 [--graph | --wire [--merged] [--function-prefix <p>] [--class-prefix <p>]] [--only-marked] [--timing] [--repeat <n>] [--label <label>]";

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
    let occurrence = take_value(&mut args, "--occurrence");
    let id = take_value(&mut args, "--id");
    let graph = take_flag(&mut args, "--graph");
    let wire = take_flag(&mut args, "--wire");
    let merged = take_flag(&mut args, "--merged");
    let function_prefix = take_value(&mut args, "--function-prefix");
    let class_prefix = take_value(&mut args, "--class-prefix");
    let only_marked = take_flag(&mut args, "--only-marked");
    let repeat = take_value(&mut args, "--repeat");
    let timing = take_flag(&mut args, "--timing");
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["outline", path] => print_outline(path, &label),
        ["calls", ..]
            if (graph && wire)
                || (!wire && (merged || function_prefix.is_some() || class_prefix.is_some())) =>
        {
            eprintln!(
                "--graph and --wire are two outputs, and --merged and the prefixes belong to --wire\n{USAGE}"
            );
            ExitCode::from(2)
        }
        ["calls", dir, file] => {
            let Some(entry) = entry else {
                eprintln!("calls needs --entry <qualname>\n{USAGE}");
                return ExitCode::from(2);
            };
            if occurrence.is_some() && id.is_some() {
                eprintln!("--occurrence and --id each name one def: give one\n{USAGE}");
                return ExitCode::from(2);
            }
            let occurrence = match occurrence.as_deref().map(str::parse::<u16>) {
                None => None,
                Some(Ok(n)) => Some(n),
                Some(Err(_)) => {
                    eprintln!(
                        "--occurrence is a number, 0 for the first def of that name\n{USAGE}"
                    );
                    return ExitCode::from(2);
                }
            };
            // a count of one or more; `--repeat` with no value is left in `args` and is the usage error below
            let repeat = match repeat.as_deref().map(str::parse::<u32>) {
                None => None,
                Some(Ok(n)) if n >= 1 => Some(n),
                Some(_) => {
                    eprintln!("--repeat is a number of requests, 1 or more\n{USAGE}");
                    return ExitCode::from(2);
                }
            };
            print_calls(
                Path::new(dir),
                file,
                &Calls {
                    entry: &entry,
                    occurrence,
                    id: id.as_deref(),
                    label,
                    graph,
                    wire: wire.then(|| WireStyle {
                        function_prefix: function_prefix.unwrap_or_default(),
                        class_prefix: class_prefix.unwrap_or_default(),
                    }),
                    merged,
                    only_marked,
                    timing,
                    repeat,
                },
            )
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// 1-based `line:column` of byte `offset` in `text`, in UTF-16 units, as an editor shows them.
fn position(text: &str, lines: &LineIndex, offset: u32) -> String {
    lines.position(text, offset, Encoding::Utf16).map_or_else(
        || "?".to_owned(),
        |p| format!("{}:{}", p.line + 1, p.column + 1),
    )
}

/// What `calls` was asked.
struct Calls<'a> {
    entry: &'a str,
    occurrence: Option<u16>,
    id: Option<&'a str>,
    label: String,
    graph: bool,
    /// `Some`: print the wire, with these prefixes for the ids.
    wire: Option<WireStyle>,
    merged: bool,
    only_marked: bool,
    timing: bool,
    /// `Some(n)`: the request `n` times on one project, refreshed before each. `None`: once, with no refresh.
    repeat: Option<u32>,
}

/// What one request prints: the text for stdout and the text for stderr (the counters of `--graph`). Two requests are equal when both are.
#[derive(PartialEq)]
struct Reply {
    out: String,
    err: String,
}

/// The graph's counters as text: one line of the core counts, then a line per cut and a line per language counter. Both are `BTreeMap`s, so
/// the order is fixed (rule 9).
fn counters(stats: &Stats) -> String {
    let mut out = format!(
        "contexts {}, walks {}, reuse hits {}, unresolved {}\n",
        stats.contexts, stats.walks, stats.reuse_hits, stats.unresolved
    );
    for (cut, n) in &stats.cuts {
        out.push_str(&format!("cut {}: {n}\n", cut.as_str()));
    }
    for (key, n) in &stats.language {
        out.push_str(&format!("{key}: {n}\n"));
    }
    out
}

fn print_calls(dir: &Path, file: &str, ask: &Calls<'_>) -> ExitCode {
    let started = Instant::now();
    let mut project = match Project::open(&dir.to_string_lossy()) {
        Ok(project) => project,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let opened = started.elapsed();
    // The label lives for the whole run of a one-shot program, which is what `Options` asks of a marker style.
    let label: &'static str = Box::leak(ask.label.clone().into_boxed_str());
    let style = MarkerStyle::docstring(label);
    // `file` is relative to `dir`, or absolute (`join` with an absolute path gives that path)
    let text = match std::fs::read_to_string(dir.join(file)) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{file}: {e}");
            return ExitCode::from(1);
        }
    };
    // D20: the library takes a def as (qualname, occurrence), counted in the outline's order. The outline is public, so no function is
    // added to the library for it.
    let outline = outline(&text, style);
    let matches: Vec<_> = outline
        .defs
        .iter()
        .filter(|d| d.qualname == ask.entry)
        .enumerate()
        .collect();
    let chosen = match (ask.occurrence, ask.id, matches.as_slice()) {
        (_, _, []) => {
            eprintln!("{file}: no def named `{}`", ask.entry);
            return ExitCode::from(1);
        }
        (Some(n), _, _) => match matches.get(usize::from(n)) {
            Some(found) => found.0,
            None => {
                eprintln!(
                    "{file}: `{}` has {} def(s), so there is no occurrence {n}",
                    ask.entry,
                    matches.len()
                );
                return ExitCode::from(1);
            }
        },
        (None, Some(id), _) => match matches
            .iter()
            .find(|(_, d)| d.marker.as_deref() == Some(id))
        {
            Some(found) => found.0,
            None => {
                eprintln!("{file}: no def `{}` carries {label}: {id}", ask.entry);
                return ExitCode::from(1);
            }
        },
        (None, None, [only]) => only.0,
        (None, None, _) => {
            // D15: a qualname is not unique; the person says which, and the first is never picked
            let lines = LineIndex::new(&text);
            eprintln!(
                "{file}: `{}` matches {} defs; pick one with --occurrence <n> or --id <marker>:",
                ask.entry,
                matches.len()
            );
            for (n, def) in &matches {
                let marker = def
                    .marker
                    .as_ref()
                    .map_or_else(|| "no marker".to_owned(), |m| format!("{label}: {m}"));
                eprintln!(
                    "  #{n}  {:?}  {}  {marker}",
                    def.kind,
                    position(&text, &lines, def.range.start)
                );
            }
            return ExitCode::from(2);
        }
    };
    let Ok(occurrence) = u16::try_from(chosen) else {
        eprintln!(
            "{file}: `{}` has more defs than an occurrence counts",
            ask.entry
        );
        return ExitCode::from(1);
    };
    let entry = Entry {
        qualname: ask.entry,
        occurrence,
    };
    let options = Options {
        marker: style,
        only_marked: ask.only_marked,
        wire: ask.wire.clone().unwrap_or_default(),
        ..Options::default()
    };
    let requests = ask.repeat.unwrap_or(1);
    if ask.timing && ask.repeat.is_some() {
        eprintln!("open {} ms", opened.as_millis());
    }
    let mut first: Option<Reply> = None;
    for i in 1..=requests {
        let refreshing = Instant::now();
        if ask.repeat.is_some() {
            project.refresh();
        }
        let refreshed = refreshing.elapsed();
        let (reply, solved) = match request(&project, file, &entry, &options, ask) {
            Ok(done) => done,
            Err(code) => return code,
        };
        if ask.timing {
            if ask.repeat.is_some() {
                eprintln!(
                    "request {i}: refresh {:.2} ms, solve {:.2} ms",
                    refreshed.as_secs_f64() * 1000.0,
                    solved.as_secs_f64() * 1000.0
                );
            } else {
                eprintln!(
                    "open {} ms, solve {} ms",
                    opened.as_millis(),
                    solved.as_millis()
                );
            }
        }
        // A kept project must give the same answer each time. There is no test for the other case: it needs a file to change while the
        // run is going, and a test cannot do that to a process it has started.
        match &first {
            None => first = Some(reply),
            Some(first) if *first != reply => {
                eprintln!("request {i} gave another answer than request 1");
                return ExitCode::from(1);
            }
            Some(_) => {}
        }
    }
    let Some(reply) = first else {
        return ExitCode::from(1);
    };
    print!("{}", reply.out);
    eprint!("{}", reply.err);
    ExitCode::SUCCESS
}

/// One request on a kept project: what it prints, and how long the solve took (the text is made after the clock stops). A request
/// that has no answer says so on stderr and gives the exit code.
fn request(
    project: &Project,
    file: &str,
    entry: &Entry<'_>,
    options: &Options,
    ask: &Calls<'_>,
) -> Result<(Reply, Duration), ExitCode> {
    let solving = Instant::now();
    let no_def = || {
        eprintln!("{file}: no def `{}`", ask.entry);
        ExitCode::from(1)
    };
    if ask.wire.is_some() {
        // the wire alone on stdout: JSON, so it can be read as it is. `call_wire` returns the frame only, so there are no counters
        let frame = call_wire(project, file, entry, options, ask.merged).ok_or_else(no_def)?;
        let solved = solving.elapsed();
        let out = json_text(file, serde_json::to_string_pretty(&frame))?;
        return Ok((
            Reply {
                out,
                err: String::new(),
            },
            solved,
        ));
    }
    let graph = call_graph(project, file, entry, options)
        .filter(|g| !g.entries.is_empty())
        .ok_or_else(no_def)?;
    let solved = solving.elapsed();
    if ask.graph {
        // the graph alone on stdout, so it can be read as it is; the counters go to stderr
        let out = json_text(file, serde_json::to_string_pretty(&graph))?;
        return Ok((
            Reply {
                out,
                err: counters(&graph.stats),
            },
            solved,
        ));
    }
    let tree = to_text(&render(
        &graph,
        &graph.entries[0],
        &options.render_options(),
    ));
    let out = format!("{tree}\n{}", counters(&graph.stats));
    Ok((
        Reply {
            out,
            err: String::new(),
        },
        solved,
    ))
}

/// The answer as pretty JSON, with the newline `println!` would add. The graph's and the frame's maps are `BTreeMap`s and their lists are
/// sorted, so the bytes are the same for the same input (rule 9).
fn json_text(file: &str, json: serde_json::Result<String>) -> Result<String, ExitCode> {
    json.map(|json| format!("{json}\n")).map_err(|e| {
        eprintln!("{file}: the answer cannot be written: {e}");
        ExitCode::from(1)
    })
}

fn print_outline(path: &str, label: &str) -> ExitCode {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::from(1);
        }
    };
    let lines = LineIndex::new(&text);
    let at = |offset| position(&text, &lines, offset);
    // the label is a parameter of the analyzer; the command line gives it (default "ID")
    let result = outline(&text, MarkerStyle::docstring(label));
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
