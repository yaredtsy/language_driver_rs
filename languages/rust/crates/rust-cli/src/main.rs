//! `rust-cli`: try the analyzer by hand.
//!
//! - `outline <file> [--label <label>] [--edition <2015|2018|2021|2024>]`: the defs of one file. A qualname is not unique in a file (D15), so a def
//!   that is not the first of its name prints `#<occurrence>` (RS1 §4), and a person who has to name it says which one. Without `--edition` the
//!   file is read as the latest edition: the driver reads it from `Cargo.toml`, and this tool does not look at one.
//! - `marker <file> (--def <qualname> [--occurrence <n>] | --header) --text <text> [--write] [--label <label>] [--edition <…>]`: plan the marker
//!   line for one def, or for the file's header (RS2). Without `--write` it prints the edit and changes nothing; with it, the file is written and
//!   read again, and the marker read back is printed. This tool is the only place a file is written (rule 2) and the only place that is given
//!   the marker text: it never makes one up. Exit 0 for a marker planned (and written), 1 for a file that cannot be read, written or read back,
//!   2 for bad usage (a qualname of several defs is listed and none is picked, D15), 3 for a plan that is a skip, with its reason.
//! - `calls <dir> <file> --entry <qualname> [--id <marker> | --occurrence <n>] [--graph] [--timing] [--label <label>]`: the call tree under a def of
//!   `file` (relative to `dir`), then the breakdown of the graph (the load's warnings first, edges by kind, unresolved edges by reason, what is open). A
//!   qualname is not unique in a file (D15): when it matches several defs they are listed and the exit code is 2, never the first one picked. `--entry` is
//!   required: nothing runs at the top of a Rust file, so there is no file entry. The load's warnings go to stderr before the tree.

use std::path::Path;
use std::process::ExitCode;

use analyzer_core::edit::apply;
use analyzer_core::text::{Encoding, LineIndex};
use rust_analysis::{
    Cancel, Entry, LoadOptions, Options, Project, RenderOptions, breakdown, call_graph, render,
    to_text,
};
use rust_syntax::{
    Edition, MarkerStyle, header_marker, marker, occurrences, outline, plan_header_marker,
    plan_marker,
};

const USAGE: &str = "usage: rust-cli outline <file> [--label <label>] [--edition <2015|2018|2021|2024>]
       rust-cli marker <file> (--def <qualname> [--occurrence <n>] | --header) --text <text> [--write] [--label <label>] [--edition <2015|2018|2021|2024>]
       rust-cli calls <dir> <file> --entry <qualname> [--id <marker> | --occurrence <n>] [--graph] [--timing] [--label <label>]";

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

/// Remove the flag `name` from `args` and say whether it was there.
fn take_flag(args: &mut Vec<String>, name: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != name);
    args.len() != before
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
    let entry = take_value(&mut args, "--entry");
    let id = take_value(&mut args, "--id");
    let (graph, timing) = (
        take_flag(&mut args, "--graph"),
        take_flag(&mut args, "--timing"),
    );
    let (def, occurrence, text) = (
        take_value(&mut args, "--def"),
        take_value(&mut args, "--occurrence"),
        take_value(&mut args, "--text"),
    );
    let (header, write) = (
        take_flag(&mut args, "--header"),
        take_flag(&mut args, "--write"),
    );
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
        ["calls", dir, file] => match (entry.as_deref(), occurrence.as_deref().map(str::parse)) {
            (Some(_), Some(Err(_))) | (None, _) => {
                eprintln!("calls needs --entry <qualname>; --occurrence is a number\n{USAGE}");
                ExitCode::from(2)
            }
            (Some(_), Some(Ok(_))) if id.is_some() => {
                eprintln!("--id and --occurrence both pick a def: give one\n{USAGE}");
                ExitCode::from(2)
            }
            (Some(qualname), picked) => print_calls(
                Path::new(dir),
                file,
                &Calls {
                    qualname,
                    occurrence: picked.and_then(Result::ok),
                    id: id.as_deref(),
                    label,
                    graph,
                    timing,
                },
            ),
        },
        ["marker", path] => {
            let target = match (def.as_deref(), header, occurrence.as_deref()) {
                (Some(qualname), false, occurrence) => match occurrence.map(str::parse) {
                    None => Some(Target::Def(qualname, None)),
                    Some(Ok(n)) => Some(Target::Def(qualname, Some(n))),
                    Some(Err(_)) => None,
                },
                (None, true, None) => Some(Target::Header),
                _ => None,
            };
            match (target, text.as_deref()) {
                (Some(target), Some(text)) => {
                    let marker = Marker {
                        path,
                        label: &label,
                        edition,
                        text,
                        write,
                    };
                    marker.run(target)
                }
                _ => {
                    eprintln!("{USAGE}");
                    ExitCode::from(2)
                }
            }
        }
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

/// What `marker` writes it on.
enum Target<'a> {
    /// A def by qualname, and which of the defs of that name (D15) when the person says.
    Def(&'a str, Option<u16>),
    Header,
}

struct Marker<'a> {
    path: &'a str,
    label: &'a str,
    edition: Edition,
    text: &'a str,
    write: bool,
}

impl Marker<'_> {
    fn run(&self, target: Target<'_>) -> ExitCode {
        let path = self.path;
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
        let style = MarkerStyle::docstring(self.label);
        // the def (qualname and occurrence) that takes the marker, or none for the header
        let (planned, home) = match target {
            Target::Header => (plan_header_marker(&text, self.text, style), None),
            Target::Def(qualname, wanted) => {
                let result = outline(&text, self.edition, style);
                let twins: Vec<(usize, u16)> = occurrences(&result)
                    .into_iter()
                    .enumerate()
                    .filter(|&(i, n)| {
                        result.defs[i].qualname == qualname && wanted.is_none_or(|w| w == n)
                    })
                    .collect();
                let [(index, occurrence)] = twins.as_slice() else {
                    return self.not_one(&text, &result, qualname, &twins);
                };
                let def = &result.defs[*index];
                let plan = plan_marker(&text, self.edition, def, self.text, style);
                (plan, Some((qualname, *occurrence)))
            }
        };
        let what = home.map_or_else(
            || "the header".to_owned(),
            |(qualname, n)| format!("{qualname} #{n}"),
        );
        let edit = match planned {
            Ok(edit) => edit,
            Err(skip) => {
                println!("{what}: no marker planned: {skip}");
                return ExitCode::from(3);
            }
        };
        let lines = LineIndex::new(&text);
        let at = lines
            .position(&text, edit.at.start, Encoding::Utf16)
            .map_or_else(
                || "?".to_owned(),
                |p| format!("{}:{}", p.line + 1, p.column + 1),
            );
        println!("{what}: insert at {at}");
        for line in edit.text.split_inclusive('\n') {
            let line = line.trim_end_matches(['\r', '\n']);
            println!("{}", format!("+ {line}").trim_end());
        }
        if !self.write {
            return ExitCode::SUCCESS;
        }
        let Some(new) = apply(&text, std::slice::from_ref(&edit)) else {
            eprintln!("{path}: the edit does not apply");
            return ExitCode::from(1);
        };
        if let Err(e) = std::fs::write(path, &new) {
            eprintln!("{path}: {e}");
            return ExitCode::from(1);
        }
        // read the file again, as the next run would
        let read_back = match std::fs::read_to_string(path) {
            Ok(text) => self.read(&text, home, style),
            Err(e) => {
                eprintln!("{path}: {e}");
                return ExitCode::from(1);
            }
        };
        if read_back.as_deref() == Some(self.text) {
            println!("written: {}: {}", self.label, self.text);
            ExitCode::SUCCESS
        } else {
            eprintln!("{path}: written, but read back as {read_back:?}");
            ExitCode::from(1)
        }
    }

    /// The marker of `home` (a def's qualname and occurrence, or the header when none), read from `text`.
    fn read(
        &self,
        text: &str,
        home: Option<(&str, u16)>,
        style: MarkerStyle<'_>,
    ) -> Option<String> {
        let Some((qualname, occurrence)) = home else {
            return header_marker(text, style);
        };
        let result = outline(text, self.edition, style);
        let index = occurrences(&result)
            .into_iter()
            .enumerate()
            .find(|&(i, n)| result.defs[i].qualname == qualname && n == occurrence)?
            .0;
        marker(text, self.edition, &result.defs[index], style)
    }

    /// No def, or several: say which, and pick none (D15).
    fn not_one(
        &self,
        text: &str,
        result: &rust_syntax::Outline,
        qualname: &str,
        twins: &[(usize, u16)],
    ) -> ExitCode {
        let path = self.path;
        if twins.is_empty() {
            eprintln!("{path}: no def named `{qualname}`");
        } else {
            let lines = LineIndex::new(text);
            eprintln!(
                "{path}: `{qualname}` matches {} defs; pick one with --occurrence <n>:",
                twins.len()
            );
            for &(i, n) in twins {
                let line = lines
                    .position(text, result.defs[i].range.start, Encoding::Utf16)
                    .map_or(0, |p| p.line + 1);
                eprintln!("  #{n}  line {line}");
            }
        }
        ExitCode::from(2)
    }
}

/// What `calls` was asked.
struct Calls<'a> {
    qualname: &'a str,
    /// Which of the defs of that name, when the person says (`--occurrence`).
    occurrence: Option<u16>,
    /// The marker that picks one (`--id`).
    id: Option<&'a str>,
    label: String,
    graph: bool,
    timing: bool,
}

fn print_calls(dir: &Path, file: &str, ask: &Calls<'_>) -> ExitCode {
    let started = std::time::Instant::now();
    let project = match Project::open(dir, Path::new(file), &LoadOptions::default()) {
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
    let occurrence = match choose_def(&project, dir, file, ask, style) {
        Ok(occurrence) => occurrence,
        Err(code) => return code,
    };
    for warning in project.warnings() {
        eprintln!("warning: {warning}");
    }
    let solving = std::time::Instant::now();
    let entry = Entry::Def {
        qualname: ask.qualname,
        occurrence,
    };
    let Some(graph) = call_graph(&project, file, &entry, &Options::new(style), &Cancel::new())
    else {
        eprintln!(
            "{file}: `{}` cannot start a graph: it is a trait, a module, a type alias or a macro, or the code is turned off by a `cfg`",
            ask.qualname
        );
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

/// The occurrence of the def `qualname` names in `file`: the one `--occurrence` or `--id` picks, or the only one. A message and the exit code when there is
/// none, or several (D15): the first is never picked.
fn choose_def(
    project: &Project,
    dir: &Path,
    file: &str,
    ask: &Calls<'_>,
    style: MarkerStyle<'static>,
) -> Result<u16, ExitCode> {
    let qualname = ask.qualname;
    let Some(matches) = project.defs_named(file, qualname, style) else {
        eprintln!(
            "{file}: not a file of the project (see which files a load holds: a `.rs` that some crate reaches by `mod`)"
        );
        return Err(ExitCode::from(1));
    };
    let chosen = match (ask.occurrence, ask.id, matches.as_slice()) {
        (_, _, []) => {
            eprintln!("{file}: no def named `{qualname}`");
            return Err(ExitCode::from(1));
        }
        (Some(n), _, _) => match matches.iter().find(|m| m.occurrence == n) {
            Some(found) => found,
            None => {
                eprintln!("{file}: `{qualname}` has no occurrence #{n}");
                return Err(ExitCode::from(1));
            }
        },
        (None, Some(id), _) => match matches.iter().find(|m| m.marker.as_deref() == Some(id)) {
            Some(found) => found,
            None => {
                eprintln!("{file}: no def `{qualname}` carries {}: {id}", style.label);
                return Err(ExitCode::from(1));
            }
        },
        (None, None, [only]) => only,
        (None, None, _) => {
            // D15: a qualname is not unique; the person says which, and the first is never picked
            let text = std::fs::read_to_string(dir.join(file)).unwrap_or_default();
            let lines = LineIndex::new(&text);
            eprintln!(
                "{file}: `{qualname}` matches {} defs; pick one with --id <marker> or --occurrence <n>:",
                matches.len()
            );
            for m in &matches {
                let at = lines
                    .position(&text, m.range.start, Encoding::Utf16)
                    .map_or_else(
                        || "?".to_owned(),
                        |p| format!("{}:{}", p.line + 1, p.column + 1),
                    );
                let marker = m.marker.as_ref().map_or_else(
                    || "no marker".to_owned(),
                    |m| format!("{}: {m}", style.label),
                );
                eprintln!("  #{}  {:?}  {at}  {marker}", m.occurrence, m.kind);
            }
            return Err(ExitCode::from(2));
        }
    };
    Ok(chosen.occurrence)
}
