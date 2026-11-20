//! `bend-cli`: try the analyzer by hand.
//!
//! - `outline <file> [--label <label>]`: the defs of one file, with their markers.
//! - `calls <root> <file> (--entry <qualname> | --all) [--max-contexts N] [--max-depth N] [--graph | --wire [--merged] [--function-prefix P]
//!   [--class-prefix P]] [--label <label>] [--timing]`: the call tree under a def of `file` (relative to `root`, the folder that was asked for), then a summary of
//!   the graph (edges by kind, unresolved edges by reason, every counter). `--max-contexts` and `--max-depth` are the solver's limits: a cut is drawn and
//!   counted. When a file has two defs of one name (a law and its proof) the one with code is the entry. `--wire` prints the wire (D17) alone on stdout as
//!   JSON, with the prefixes the caller gives for the ids of functions and classes; `--merged` joins siblings with one target. `--all` is the sweep: it enters
//!   **every def of every file of the book once**, at depth 1 (a def's own body, so each site and each counter is seen once), and prints one table instead of a tree.
//!
//! Errors go to stderr with exit code 1 (a file or entry that is not there) or 2 (bad usage); nothing panics on input.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use analyzer_core::text::{Encoding, LineIndex};
use bend_analysis::{
    Book, Cancel, ContextId, DefKind, EdgeTo, Graph, Limits, MarkerStyle, Options, RenderOptions,
    WireOptions, WireStyle, call_graph, render, to_text, wire,
};
use bend_syntax::{occurrences, outline};

const USAGE: &str = "usage: bend-cli outline <file> [--label <label>]
       bend-cli calls <root> <file> (--entry <qualname> | --all) [--max-contexts <n>] [--max-depth <n>] [--graph | --wire [--merged] [--function-prefix <p>] [--class-prefix <p>]] [--label <label>] [--timing]
  <root> is the folder asked for; <file> is the entry file of the book, relative to it. --max-contexts and --max-depth are the solver's limits (a cut is drawn and counted).
  --all enters every def of the book once at depth 1 and prints one table: the sweep.";

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

/// Remove `--name N` from `args` and return `N`: `Ok(None)` when the flag is not there, an `Err` (what to tell the person) when it has no whole number.
fn take_count(args: &mut Vec<String>, name: &str) -> Result<Option<u32>, String> {
    if !args.iter().any(|a| a == name) {
        return Ok(None);
    }
    take_value(args, name)
        .and_then(|v| v.parse::<u32>().ok())
        .map(Some)
        .ok_or_else(|| format!("{name} needs a whole number, 0 or more"))
}

fn take_flag(args: &mut Vec<String>, name: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != name);
    args.len() != before
}

fn usage(why: &str) -> ExitCode {
    eprintln!("{why}\n{USAGE}");
    ExitCode::from(2)
}

fn fail(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::from(1)
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let label = take_value(&mut args, "--label").unwrap_or_else(|| "ID".to_owned());
    let entry = take_value(&mut args, "--entry");
    let (all, graph, as_wire, merged, timing) = (
        take_flag(&mut args, "--all"),
        take_flag(&mut args, "--graph"),
        take_flag(&mut args, "--wire"),
        take_flag(&mut args, "--merged"),
        take_flag(&mut args, "--timing"),
    );
    let (function_prefix, class_prefix) = (
        take_value(&mut args, "--function-prefix"),
        take_value(&mut args, "--class-prefix"),
    );
    let (max_contexts, max_depth) = match (
        take_count(&mut args, "--max-contexts"),
        take_count(&mut args, "--max-depth"),
    ) {
        (Ok(contexts), Ok(depth)) => (contexts, depth),
        (Err(message), _) | (_, Err(message)) => return usage(&message),
    };
    // The label lives for the whole run of a one-shot program, which is what `Options` asks of a marker style.
    let label: &'static str = Box::leak(label.into_boxed_str());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["outline", path] => print_outline(path, label),
        ["calls", root, file] => {
            if graph && as_wire
                || !as_wire && (merged || function_prefix.is_some() || class_prefix.is_some())
            {
                return usage(
                    "--graph and --wire are two outputs, and --merged and the prefixes belong to --wire",
                );
            }
            if all == entry.is_some() {
                return usage("calls needs one of --entry <qualname> and --all");
            }
            if all && (graph || as_wire || max_depth.is_some() || max_contexts.is_some()) {
                return usage(
                    "--all prints its own table at depth 1: no --graph, --wire or limits",
                );
            }
            let mut limits = Limits::default();
            if let Some(n) = max_contexts {
                limits.max_contexts = n;
            }
            if let Some(n) = max_depth {
                limits.max_depth = Some(n);
            }
            let options = Options::new(MarkerStyle::above(label));
            let book = match Book::open(Path::new(root), Path::new(file)) {
                Some(book) => book,
                None => return fail(&format!("{root}: {file} is not a file under that folder")),
            };
            if all {
                return sweep(&book, &options);
            }
            let style = WireStyle {
                function_prefix: function_prefix.unwrap_or_default(),
                class_prefix: class_prefix.unwrap_or_default(),
            };
            let ask = Ask {
                file,
                entry: entry.as_deref().unwrap_or_default(),
                limits,
                graph,
                wire: as_wire.then_some((style, merged)),
                timing,
            };
            print_calls(&book, &options, &ask)
        }
        _ => usage("not a command"),
    }
}

fn print_outline(path: &str, label: &'static str) -> ExitCode {
    let text = match std::fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => return fail(&format!("{path}: {e}")),
    };
    let lines = LineIndex::new(&text);
    // 1-based line:column in UTF-16 units, as an editor shows them
    let at = |offset| {
        lines.position(&text, offset, Encoding::Utf16).map_or_else(
            || "?".to_owned(),
            |p| format!("{}:{}", p.line + 1, p.column + 1),
        )
    };
    let result = outline(&text, MarkerStyle::above(label));
    if let Some(marker) = &result.file_marker {
        println!("file {label}: {marker}");
    }
    for (def, occurrence) in result.defs.iter().zip(occurrences(&result)) {
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
            "{:?} {}{twin}  {}-{}{marker}{flag}",
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

struct Ask<'a> {
    file: &'a str,
    entry: &'a str,
    limits: Limits,
    graph: bool,
    wire: Option<(WireStyle, bool)>,
    timing: bool,
}

/// What a graph is made of: edges by kind, the unresolved ones by reason, then every counter of `Stats.language`.
fn summary(graph: &Graph) -> String {
    let (mut calls, mut passed, mut constructs, mut externals) = (0, 0, 0, 0);
    let mut reasons: BTreeMap<&str, u64> = BTreeMap::new();
    for edge in &graph.edges {
        match &edge.to {
            EdgeTo::Context(..) if edge.passed => passed += 1,
            EdgeTo::Context(..) => calls += 1,
            EdgeTo::Construct { .. } => constructs += 1,
            EdgeTo::External(..) => externals += 1,
            EdgeTo::Unresolved(reason) => *reasons.entry(reason.as_str()).or_default() += 1,
            _ => {}
        }
    }
    let unresolved: u64 = reasons.values().sum();
    let mut out = format!(
        "edges: {calls} call, {constructs} construct, {externals} external, {unresolved} unresolved, {passed} passed\n"
    );
    for (reason, n) in &reasons {
        out.push_str(&format!("  unresolved {reason}: {n}\n"));
    }
    let counters: Vec<String> = graph
        .stats
        .language
        .iter()
        .map(|(k, n)| format!("{k}={n}"))
        .collect();
    if !counters.is_empty() {
        out.push_str(&format!("counters: {}\n", counters.join(", ")));
    }
    out.push_str(&format!(
        "contexts {}, bodies walked {}, cuts {:?}\n",
        graph.stats.contexts, graph.stats.walks, graph.stats.cuts
    ));
    out
}

fn print_calls(book: &Book, options: &Options, ask: &Ask<'_>) -> ExitCode {
    let started = Instant::now();
    let Some(graph) = call_graph(
        book,
        Path::new(ask.file),
        ask.entry,
        options,
        &ask.limits,
        &Cancel::new(),
    ) else {
        return fail(&format!(
            "{}: there is no def `{}` in it",
            ask.file, ask.entry
        ));
    };
    if ask.timing {
        eprintln!("graph {} ms", started.elapsed().as_millis());
    }
    let render_options = RenderOptions::default();
    if let Some((style, merged)) = &ask.wire {
        let frame = wire(
            &graph,
            &graph.entries[0],
            book,
            &WireOptions::new(style.clone(), *merged, render_options),
        );
        match serde_json::to_string_pretty(&frame) {
            Ok(json) => println!("{json}"),
            Err(e) => return fail(&format!("the wire cannot be written: {e}")),
        }
        eprint!("{}", summary(&graph));
    } else if ask.graph {
        println!("{graph:#?}");
        eprint!("{}", summary(&graph));
    } else {
        print!(
            "{}",
            to_text(&render(&graph, &graph.entries[0], &render_options))
        );
        println!();
        print!("{}", summary(&graph));
    }
    ExitCode::SUCCESS
}

/// The sweep: every def of every file of the book entered once at depth 1, and one table of what that came to. A def that `call_graph` would not pick for its
/// qualname (a law whose proof has the name, a second def of one name) is counted and skipped. The book's counters are said once, not once per request.
fn sweep(book: &Book, options: &Options) -> ExitCode {
    let started = Instant::now();
    let limits = Limits::new(Limits::default().max_contexts, Some(1));
    let (mut entered, mut skipped, mut none) = (0u64, 0u64, 0u64);
    let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut reasons: BTreeMap<String, u64> = BTreeMap::new();
    let mut counters: BTreeMap<String, u64> = BTreeMap::new();
    let mut contexts: BTreeSet<ContextId> = BTreeSet::new();
    let mut slowest = (std::time::Duration::ZERO, String::new());
    for file in book.files() {
        for (at, def) in file.outline.defs.iter().enumerate() {
            let is_law =
                |d: &bend_analysis::Def| matches!(&d.kind, DefKind::Other(k) if k == "law");
            let same: Vec<usize> = (0..file.outline.defs.len())
                .filter(|&i| file.outline.defs[i].qualname == def.qualname)
                .collect();
            let chosen = same
                .iter()
                .copied()
                .find(|&i| !is_law(&file.outline.defs[i]))
                .or(same.first().copied());
            if chosen != Some(at) {
                skipped += 1;
                continue;
            }
            let began = Instant::now();
            let graph = call_graph(
                book,
                Path::new(&file.relative),
                &def.qualname,
                options,
                &limits,
                &Cancel::new(),
            );
            let took = began.elapsed();
            let Some(graph) = graph else {
                none += 1;
                continue;
            };
            if took > slowest.0 {
                slowest = (took, format!("{} {}", file.relative, def.qualname));
            }
            entered += 1;
            for edge in &graph.edges {
                let kind = match &edge.to {
                    EdgeTo::Context(..) if edge.passed => "passed",
                    EdgeTo::Context(..) => "call",
                    EdgeTo::Construct { .. } => "construct",
                    EdgeTo::External(..) => "external",
                    EdgeTo::Unresolved(reason) => {
                        *reasons.entry(reason.as_str().to_owned()).or_default() += 1;
                        "unresolved"
                    }
                    _ => "other",
                };
                *kinds.entry(kind.to_owned()).or_default() += 1;
            }
            for (key, n) in &graph.stats.language {
                // the book's counters are in every graph: they are added once, below
                if !book.stats().language.contains_key(key) {
                    *counters.entry(key.clone()).or_default() += n;
                }
            }
            contexts.extend(graph.contexts.iter().map(|c| c.id.clone()));
        }
    }
    for (key, n) in &book.stats().language {
        *counters.entry(key.clone()).or_default() += n;
    }
    let list = |map: &BTreeMap<String, u64>| {
        map.iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!("files {}, defs {}", book.files().len(), book.decls().len());
    println!(
        "entered {entered}, skipped {skipped} (a law with its proof, a repeated name), no graph {none}"
    );
    println!(
        "contexts {} (distinct, in the union of the graphs)",
        contexts.len()
    );
    println!("edges: {}", list(&kinds));
    println!("unresolved: {}", list(&reasons));
    println!("counters: {}", list(&counters));
    println!(
        "slowest request: {:.1} ms ({})",
        slowest.0.as_secs_f64() * 1000.0,
        slowest.1
    );
    println!("time: {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
    ExitCode::SUCCESS
}
