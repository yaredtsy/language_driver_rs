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

use std::process::ExitCode;

use analyzer_core::edit::apply;
use analyzer_core::text::{Encoding, LineIndex};
use rust_syntax::{
    Edition, MarkerStyle, header_marker, marker, occurrences, outline, plan_header_marker,
    plan_marker,
};

const USAGE: &str = "usage: rust-cli outline <file> [--label <label>] [--edition <2015|2018|2021|2024>]
       rust-cli marker <file> (--def <qualname> [--occurrence <n>] | --header) --text <text> [--write] [--label <label>] [--edition <2015|2018|2021|2024>]";

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
