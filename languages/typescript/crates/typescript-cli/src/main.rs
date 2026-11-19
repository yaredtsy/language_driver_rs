//! `typescript-cli`: try the analyzer by hand.
//!
//! - `outline <file> [--label <label>]`: the defs of one file.
//! - `files <dir> <file>`: which config owns `file` under `dir`, and what the loaded program holds. The tool for "why is this call unresolved":
//!   the answer is often that the file is not in the program the config describes.
//! - `calls <dir> <file> [--entry <qualname> [--id <marker>]] [--depth N] [--max-contexts N] [--max-nodes N] [--graph | --wire [--merged] [--function-prefix P] [--class-prefix P]] [--timing]`:
//!   the call tree under a def, then the breakdown of the graph (edges by kind, unresolved edges by reason). `--depth` and `--max-contexts` are a
//!   [`Budget`] (whole numbers, 0 and up): a def past it is listed and not walked, drawn `[Cut]`, and counted in the `frontier` line; `--depth 0` is the
//!   entry alone, on the frontier. Asking again for a cut def as the entry continues from there. `--max-nodes N` (1 and up) is the renderer's limit on the
//!   tree and the `--wire` (default 100 000, and not a budget: the graph is complete); when it cut, a line after the breakdown says so. A qualname is not unique in a file (D15): when it
//!   matches several defs they are listed and the exit code is 2, never the first one picked; `--id` names the one that carries that marker. With no
//!   `--entry` the entry is the file's own code (the module, D32), and `--id` is a usage error: it picks among defs, and a file is one. `--wire` prints
//!   the wire (D17) instead of the tree, as JSON alone on stdout, with the prefixes the caller gives for the ids of functions and classes.

use std::path::Path;
use std::process::ExitCode;

use analyzer_core::text::{Encoding, LineIndex};
use typescript_analysis::{
    Budget, Cancel, Entry, Graph, Mark, Node, Options, Project, RenderOptions, Session, Status,
    WireOptions, WireStyle, breakdown, render, to_text, wire,
};
use typescript_syntax::{Dialect, MarkerStyle, outline};

const USAGE: &str = "usage: typescript-cli outline <file> [--label <label>]
       typescript-cli files <dir> <file>
       typescript-cli calls <dir> <file> [--entry <qualname> [--id <marker>]] [--depth <n>] [--max-contexts <n>] [--max-nodes <n>] [--graph | --wire [--merged] [--function-prefix <p>] [--class-prefix <p>]] [--timing] [--label <label>]
  --depth and --max-contexts are the solver's budget: a def past it is listed and not walked, drawn [Cut] and counted in the frontier line.
  --max-nodes is the renderer's limit on the tree or the wire (default 100000): the graph is complete, and a line says when the drawing was cut.";

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

/// Remove `--name N` from `args` and return `N`, a whole number of at least `min`: `Ok(None)` when the flag is not there, an `Err` (what to tell the
/// person) when it has no value or the value is not one (`-1`, `x`, `1.5`, a number past `u32`, a number under `min`).
fn take_count(args: &mut Vec<String>, name: &str, min: u32) -> Result<Option<u32>, String> {
    if !args.iter().any(|a| a == name) {
        return Ok(None);
    }
    let needs = format!("{name} needs a whole number, {min} or more");
    let Some(value) = take_value(args, name) else {
        return Err(needs);
    };
    match value.parse::<u32>() {
        Ok(n) if n >= min => Ok(Some(n)),
        _ => Err(format!("{needs}, not `{value}`")),
    }
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
    let (depth, max_contexts, max_nodes) = match (
        take_count(&mut args, "--depth", 0),
        take_count(&mut args, "--max-contexts", 0),
        take_count(&mut args, "--max-nodes", 1),
    ) {
        (Ok(depth), Ok(max_contexts), Ok(max_nodes)) => (depth, max_contexts, max_nodes),
        (Err(message), _, _) | (_, Err(message), _) | (_, _, Err(message)) => {
            eprintln!("{message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
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
        ["calls", _, _] if graph && max_nodes.is_some() => {
            eprintln!(
                "--max-nodes limits the tree and the wire; --graph prints the graph, which it does not touch\n{USAGE}"
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
                budget: Budget::new(depth, max_contexts),
                max_nodes,
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
    /// `Budget::default()` (no limit) when neither `--depth` nor `--max-contexts` was given.
    budget: Budget,
    /// The renderer's limit on the tree and the wire; `None` keeps `RenderOptions`' default.
    max_nodes: Option<u32>,
}

fn print_calls(dir: &Path, file: &str, ask: &Calls<'_>) -> ExitCode {
    let started = std::time::Instant::now();
    // The label lives for the whole run of a one-shot program, which is what `Options` asks of a marker style.
    let label: &'static str = Box::leak(ask.label.clone().into_boxed_str());
    let style = MarkerStyle::docstring(label);
    // A session even for one request: it is the call that takes a budget, and with none its graph is `call_graph`'s.
    let session = match Session::open(dir, Path::new(file), Options::new(style)) {
        Ok(session) => session,
        Err(e) => {
            eprintln!("{file}: {e}");
            return ExitCode::from(1);
        }
    };
    let opened = started.elapsed();
    let Some(project) = session.project() else {
        eprintln!("{file}: the session has no program");
        return ExitCode::from(1);
    };
    let entry = match ask.entry {
        Some(qualname) => match choose_def(project, dir, file, qualname, ask.id, style) {
            Ok(entry) => entry,
            Err(code) => return code,
        },
        None => Entry::File,
    };
    let solving = std::time::Instant::now();
    let Some(graph) = session.graph(file, &entry, &ask.budget, &Cancel::new()) else {
        match ask.entry {
            Some(qualname) => eprintln!("{file}: no def `{qualname}`"),
            None => eprintln!("{file}: {NOT_PROJECT_CODE}"),
        }
        return ExitCode::from(1);
    };
    let mut render_options = RenderOptions::default();
    if let Some(max_nodes) = ask.max_nodes {
        render_options.max_nodes = max_nodes;
    }
    // The tree is drawn once here: the text prints it, and in the wire case it is only looked at (the wire draws its own) to see whether the renderer cut.
    let (tree, renderer_cut) = if ask.graph {
        (None, false)
    } else {
        let tree = render(&graph, &graph.entries[0], &render_options);
        let cut = cut_by_renderer(&tree, &graph);
        (ask.wire.is_none().then_some(tree), cut)
    };
    if ask.timing {
        eprintln!(
            "open {} ms, graph {} ms{}{}",
            opened.as_millis(),
            solving.elapsed().as_millis(),
            // each said only when there is one, so a request with no budget and a tree that fits prints what it did
            if graph.stats.frontier > 0 {
                format!(", frontier {}", graph.stats.frontier)
            } else {
                String::new()
            },
            if renderer_cut {
                format!(", tree cut at {} nodes", render_options.max_nodes)
            } else {
                String::new()
            }
        );
    }
    // `Breakdown` has no field for the frontier or the renderer's cut (they would be fields of `typescript-analysis`), so their lines are added here, after
    // the breakdown and only when there is one: a request with no budget and a tree that fits prints what it did.
    let mut summary = breakdown(&graph).to_string();
    if graph.stats.frontier > 0 {
        summary.push_str(&format!(
            "  frontier: {} (contexts the budget stopped at: listed, not walked)\n",
            graph.stats.frontier
        ));
    }
    if renderer_cut {
        let graph_cut = graph.stats.frontier > 0 || !graph.stats.cuts.is_empty();
        summary.push_str(&format!(
            "tree: cut by the renderer at {} nodes (--max-nodes); the graph {}\n",
            render_options.max_nodes,
            if graph_cut {
                "has its own cuts, above"
            } else {
                "itself is complete"
            }
        ));
    }
    if let Some(style) = &ask.wire {
        // the wire alone on stdout: JSON, so it can be read as it is; the breakdown goes to stderr
        let options = WireOptions::new(style.clone(), ask.merged, render_options);
        let frame = wire(&graph, &graph.entries[0], project, &options);
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
            to_text(&tree.expect("the tree was drawn: this is not --wire or --graph"))
        );
        println!();
        print!("{summary}");
    }
    ExitCode::SUCCESS
}

/// Whether the renderer left nodes out because of `max_nodes`: a node marked `Cut` that no `Frontier` context stands behind (that one is the solver's
/// budget, drawn `Cut` too). The renderer returns no count, and the CLI sets no other renderer limit (`max_depth` marks `Cut` the same way), so this is
/// what it left. Walks with a stack: a tree can be 100 000 deep.
fn cut_by_renderer(tree: &Node, graph: &Graph) -> bool {
    let mut pending = vec![tree];
    while let Some(node) = pending.pop() {
        let on_frontier = node.contexts.iter().any(|id| {
            graph
                .context(id)
                .is_some_and(|c| c.status == Status::Frontier)
        });
        if node.marks.contains(&Mark::Cut) && !on_frontier {
            return true;
        }
        pending.extend(&node.children);
    }
    false
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
