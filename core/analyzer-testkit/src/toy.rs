//! A toy language for the testkit's own tests, so the testkit is proved before a real language uses it.
//!
//! Source: a line `fn NAME` is a def; two more spaces of indentation nest it under the def above. A `// ID: x` line
//! directly above is its marker. A scenario program is `main.toy`: lines `NAME: callee callee`, where a callee that
//! is not defined is external and `?` is unresolved.

use std::path::Path;

use analyzer_core::edit::{Edit, MarkerStyle, comment_marker};
use analyzer_core::graph::{DefInfo, External, Graph, Site, StableDef};
use analyzer_core::outline::{Def, DefKind, Outline};
use analyzer_core::solve::{Cancel, Lang, Limits, SiteCalls, Solver, Summary, Target};
use analyzer_core::text::Span;

use crate::syntax::Subject;

const ID: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: analyzer_core::edit::Placement::Docstring,
};

pub fn outline(text: &str) -> Outline {
    let mut out = Outline::default();
    let mut stack: Vec<(usize, usize)> = Vec::new(); // (indent, def index)
    let mut offset = 0usize;
    let mut marker: Option<(String, usize)> = None;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let indent = body.len() - body.trim_start().len();
        if let Some(found) = comment_marker::read(text, offset as u32, "//", ID) {
            marker = Some((found.text, offset));
        } else if let Some(name) = body.trim_start().strip_prefix("fn ") {
            while stack.last().is_some_and(|&(i, _)| i >= indent) {
                stack.pop();
            }
            let parent = stack.last().map(|&(_, d)| d);
            let qualname = match parent {
                Some(p) => format!("{}.{name}", out.defs[p].qualname),
                None => name.to_owned(),
            };
            let name_at = (offset + indent + 3) as u32;
            let start = marker.as_ref().map_or(offset, |(_, at)| *at) as u32;
            let mut def = Def::new(
                qualname,
                name,
                DefKind::Function,
                Span::new(start, (offset + body.len()) as u32),
                Span::new(name_at, name_at + name.len() as u32),
            );
            def.parent = parent;
            def.marker = marker.take().map(|(m, _)| m);
            stack.push((indent, out.defs.len()));
            out.defs.push(def);
        } else {
            marker = None;
        }
        offset += line.len();
    }
    out
}

pub fn plan_marker(text: &str, def: &Def, marker: &str) -> Option<Edit> {
    if def.marker.is_some() {
        return None;
    }
    comment_marker::plan(text, def.range.start, "//", ID, marker)
}

pub const SYNTAX: Subject = Subject {
    outline,
    plan_marker,
    fixtures: "tests/fixtures",
    snapshots: "tests/snapshots",
};

/// A scenario program as a `Lang`: functions are indexes into `names`.
struct Program {
    names: Vec<String>,
    calls: Vec<Vec<String>>,
}

impl Lang for Program {
    type Func = usize;
    type Key = ();

    fn describe(&self, f: usize) -> DefInfo {
        DefInfo::new(
            StableDef::Marked(self.names[f].clone()),
            &self.names[f],
            DefKind::Function,
            "main.toy",
            Span::new(f as u32 * 100, f as u32 * 100 + 50),
        )
    }

    fn digest(&self, _: &()) -> [u8; 16] {
        [0; 16]
    }

    fn walk(&mut self, f: usize, _: &(), _: &Cancel) -> Summary<Self> {
        let sites = self.calls[f]
            .iter()
            .enumerate()
            .map(|(i, callee)| {
                let target = match (callee.as_str(), self.names.iter().position(|n| n == callee)) {
                    ("?", _) => Target::Unresolved,
                    (_, Some(g)) => Target::Call(g, ()),
                    (name, None) => Target::External(External {
                        package: "std".into(),
                        name: name.into(),
                    }),
                };
                SiteCalls::new(Site::at(i as u32 * 10), vec![target])
            })
            .collect();
        Summary::complete(sites)
    }
}

pub fn scenario_graph(dir: &Path) -> Option<Graph> {
    let source = std::fs::read_to_string(dir.join("main.toy")).ok()?;
    let mut program = Program {
        names: Vec::new(),
        calls: Vec::new(),
    };
    for line in source.lines().filter(|l| !l.trim().is_empty()) {
        let (name, calls) = line.split_once(':')?;
        program.names.push(name.trim().to_owned());
        program
            .calls
            .push(calls.split_whitespace().map(str::to_owned).collect());
    }
    let main = program.names.iter().position(|n| n == "main")?;
    Some(Solver::new(program, Limits::default()).graph(&[(main, ())], &Cancel::new()))
}
