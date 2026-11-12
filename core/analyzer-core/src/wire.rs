//! The wire: the call tree as V-NOC stores it, a view of the graph. `graph → render → Node → Frame`.
//!
//! Written only against core types, and it names no language and no engine. It lives in `analyzer-core` because every language has the wire (D17):
//! Python wrote it first and TypeScript is its second user (rule of two), so it moved here unchanged. It has no tree rules of its own: recursion,
//! cuts, the budget and the order of siblings are the renderer's.
//!
//! Like the renderer, nothing here recurses once per level (building the frame, merging and dropping use explicit stacks). `Frame`'s derived
//! `Clone`, `Debug`, `PartialEq` and `Serialize` do recurse: do not use them on a tree tens of thousands of levels deep.

use std::collections::HashMap;

use crate::graph::{ContextId, Graph, Root, StableDef};
use crate::outline::DefKind;
use crate::render::{Mark, Node, RenderOptions, render};
use crate::text::LineIndex;
use serde::Serialize;

/// The version of the wire's format: a plain number, **bumped when a field is added that changes what a node means**, so a reader that predates it
/// would draw a wrong tree and not fail. A reader may ignore a field it does not know; for such a field that is not enough, which is what this is
/// for. A driver reports it when it starts (the driver side, `docs/03-typescript/07-ts5-driver-needs.md` §8); nothing here checks it.
///
/// - **1**: the wire through E3 (D17's fields; E2's `target_*` and `call_site_file` and the merge key by the target's identity, which say more about a
///   node without changing what it is).
/// - **2**: `passed` (D22): a node can be a def the parent hands over, not calls. A reader older than this draws it as a call.
pub const WIRE_FORMAT_VERSION: u32 = 2;

/// How an id is written on the wire: the prefix of its kind, then the marker text. The prefixes are the caller's, never ours (the way the
/// marker label is), so no product word is in this crate. Empty by default: the wire then carries the bare marker text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WireStyle {
    pub function_prefix: String,
    pub class_prefix: String,
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct WireOptions {
    pub style: WireStyle,
    /// `false`: one node per call site, `call_count` 0. `true`: siblings with one target become one node, `call_count` summed.
    pub merged: bool,
    /// The tree's own options: budget, depth, `only_marked`, externals.
    pub render: RenderOptions,
}

impl WireOptions {
    pub fn new(style: WireStyle, merged: bool, render: RenderOptions) -> Self {
        WireOptions {
            style,
            merged,
            render,
        }
    }
}

/// The text of a project file, by its path relative to the project root. The wire needs it for one thing: a call site in the graph is bytes
/// relative to its function, and the wire wants a line and a column.
pub trait Sources {
    fn text(&self, file: &str) -> Option<String>;
}

/// A range in a file, as `[line, column]` pairs, both **1-based**; the column counts characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct LineSpan {
    pub start: (u32, u32),
    pub end: (u32, u32),
}

/// One node of the wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Frame {
    pub target_qname: String,
    /// The style's prefix of the node's kind and its marker text. Empty when the function has no marker, or none is written yet: the
    /// node is still here, named by `target_qname`.
    pub target_id: String,
    /// The target's file, relative to the project root, with `/`. `None` when the graph has no row for it (external, unresolved).
    pub target_file: Option<String>,
    /// The outline's qualname of the target in `target_file`. Empty when `target_file` is `None`.
    pub target_qualname: String,
    /// Which of the defs of `target_file` with this qualname the target is, 0-based (D18).
    pub target_occurrence: u16,
    /// What the target is, as the outline says. `None` when the graph has no row for it.
    pub target_kind: Option<DefKind>,
    /// 0 in an unmerged wire. In a merged one, the calls joined into this node, less one.
    pub call_count: u32,
    /// Where the call is, or where the def is handed over when `passed`. `None` for the root.
    pub call_site: Option<LineSpan>,
    /// The file `call_site` is in, relative to the project root. `None` for the root. For a call made in an inherited constructor it is the
    /// constructor's file, not the class's.
    pub call_site_file: Option<String>,
    /// `true`: the parent hands this def over at `call_site` and does not call it (D22). `call_count` then counts the hand-overs joined into this node,
    /// and `children` are what the def calls when it is walked. Absent when `false`, so a wire with no passed node is what it was.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub passed: bool,
    /// Which graph contexts this node shows.
    pub contexts: Vec<ContextId>,
    /// `recursive`, `cut`, `incomplete`, `likely` (D28), and with the options on, `external` and `unresolved`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<String>,
    pub children: Vec<Frame>,
}

impl Drop for Frame {
    /// The derived drop would recurse once per level and overflow the stack on a deep tree.
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.children);
        while let Some(mut frame) = pending.pop() {
            pending.append(&mut frame.children);
        }
    }
}

/// Lines of the files the sites are in, read once each.
struct Positions<'s> {
    sources: &'s dyn Sources,
    files: HashMap<String, Option<(String, LineIndex)>>,
}

impl<'s> Positions<'s> {
    fn new(sources: &'s dyn Sources) -> Self {
        Positions {
            sources,
            files: HashMap::new(),
        }
    }

    /// 1-based line and character column of byte `offset` of `file`.
    fn at(&mut self, file: &str, offset: u32) -> Option<(u32, u32)> {
        let entry = self.files.entry(file.to_owned()).or_insert_with(|| {
            let text = self.sources.text(file)?;
            let index = LineIndex::new(&text);
            Some((text, index))
        });
        let (text, index) = entry.as_ref()?;
        let line = index.line_of(offset)?;
        let start = index.line_start(line)?;
        let column = text.get(start as usize..offset as usize)?.chars().count() as u32;
        Some((line + 1, column + 1))
    }
}

fn target_id(graph: &Graph, func: &StableDef, style: &WireStyle) -> String {
    let StableDef::Marked(text) = func else {
        return String::new();
    };
    let prefix = match graph.def(func).map(|d| &d.kind) {
        Some(DefKind::Class) => &style.class_prefix,
        _ => &style.function_prefix,
    };
    format!("{prefix}{text}")
}

/// A node, without its children, as a frame. `caller` is the function the call was made in; `None` for the root.
fn head(
    graph: &Graph,
    node: &Node,
    caller: Option<&StableDef>,
    style: &WireStyle,
    positions: &mut Positions<'_>,
) -> Frame {
    // The row of the function the call was made in: the file of the call site and the base of its offsets are the same row, so the file and the
    // position cannot disagree.
    let caller = caller.and_then(|c| graph.def(c));
    let call_site = (|| {
        let site = node.site?;
        let caller = caller?;
        let mut at = |relative: u32| positions.at(&caller.file, caller.range.start + relative);
        Some(LineSpan {
            start: at(site.start)?,
            end: at(site.end)?,
        })
    })();
    // The target's row: none for an external or an unresolved leaf, which the graph names by a display name only.
    let target = graph.def(&node.func);
    Frame {
        target_qname: node.qname.clone(),
        target_id: target_id(graph, &node.func, style),
        target_file: target.map(|d| d.file.clone()),
        target_qualname: target.map(|d| d.qualname.clone()).unwrap_or_default(),
        target_occurrence: target.map_or(0, |d| d.occurrence),
        target_kind: target.map(|d| d.kind.clone()),
        call_count: 0,
        call_site,
        call_site_file: node.site.and(caller).map(|c| c.file.clone()),
        passed: node.passed,
        contexts: node.contexts.clone(),
        marks: node
            .marks
            .iter()
            .map(|m| m.as_str().to_ascii_lowercase())
            .collect(),
        children: Vec::new(),
    }
}

/// The wire of `root`: the tree `render` draws, as frames, merged when asked.
pub fn wire(graph: &Graph, root: &Root, sources: &dyn Sources, opts: &WireOptions) -> Frame {
    let tree = render(graph, root, &opts.render);
    let mut positions = Positions::new(sources);

    struct Open<'n> {
        frame: Frame,
        func: &'n StableDef,
        kids: std::slice::Iter<'n, Node>,
    }
    fn open<'n>(
        graph: &Graph,
        node: &'n Node,
        caller: Option<&StableDef>,
        style: &WireStyle,
        positions: &mut Positions<'_>,
    ) -> Open<'n> {
        Open {
            frame: head(graph, node, caller, style, positions),
            // The calls under this node were made in the function its contexts are of: for a call that is the node's own function, for a
            // construction it is the constructor, not the class.
            func: node.contexts.first().map_or(&node.func, |c| &c.func),
            kids: node.children.iter(),
        }
    }
    let mut stack = vec![open(graph, &tree, None, &opts.style, &mut positions)];
    let frame = loop {
        let top = stack.last_mut().expect("the root is popped last");
        match top.kids.next() {
            Some(child) => {
                let caller = top.func;
                let next = open(graph, child, Some(caller), &opts.style, &mut positions);
                stack.push(next);
            }
            None => {
                let done = stack.pop().expect("just looked at it").frame;
                match stack.last_mut() {
                    Some(parent) => parent.frame.children.push(done),
                    None => break done,
                }
            }
        }
    };
    if opts.merged { merge(frame) } else { frame }
}

/// Whether a frame's edge was `Basis::Likely` (D28): the wire has it as the mark, not as a field of its own.
fn is_likely(f: &Frame) -> bool {
    f.marks
        .iter()
        .any(|m| m.eq_ignore_ascii_case(Mark::Likely.as_str()))
}

/// Whether two sibling frames are one target, reached the same way. Project code is named by (file, outline qualname, occurrence), so two defs with
/// one display name stay two nodes (B6); an external or an unresolved leaf has no row, and its display name decides, as it always did. **Called or
/// passed is part of the key (D22)**: a def called at one site and handed over at another is two nodes, so a hand-over is never counted as a call.
/// **So is likely (D28)**: a call the engine found past something it could not see is never counted with a certain call of the same def.
fn same_target(a: &Frame, b: &Frame) -> bool {
    (
        &a.target_file,
        &a.target_qualname,
        a.target_occurrence,
        &a.target_qname,
        a.passed,
        is_likely(a),
    ) == (
        &b.target_file,
        &b.target_qualname,
        b.target_occurrence,
        &b.target_qname,
        b.passed,
        is_likely(b),
    )
}

/// Siblings with one target (`same_target`) become one node: `call_count` is the sum of theirs plus one for each node joined, `contexts` and
/// `marks` are unioned, and their children are concatenated (and merged the same way, one level down).
fn group(kids: Vec<Frame>) -> Vec<Frame> {
    let mut out: Vec<Frame> = Vec::new();
    for mut c in kids {
        match out.iter_mut().find(|m| same_target(m, &c)) {
            Some(m) => {
                m.call_count += c.call_count + 1;
                for id in std::mem::take(&mut c.contexts) {
                    if !m.contexts.contains(&id) {
                        m.contexts.push(id);
                    }
                }
                for mark in std::mem::take(&mut c.marks) {
                    if !m.marks.contains(&mark) {
                        m.marks.push(mark);
                    }
                }
                m.children.append(&mut c.children);
            }
            None => out.push(c),
        }
    }
    out
}

/// The merged form of a wire: V-NOC's merge, as an option of the view. Merging a merged wire changes nothing. It takes the frame by value
/// because a clone would recurse once per level; a caller that still needs the unmerged wire clones it first, if it is not deep.
pub fn merge(root: Frame) -> Frame {
    struct Open {
        head: Frame,
        todo: std::vec::IntoIter<Frame>,
        done: Vec<Frame>,
    }
    fn open(mut frame: Frame) -> Open {
        let kids = std::mem::take(&mut frame.children);
        Open {
            head: frame,
            todo: group(kids).into_iter(),
            done: Vec::new(),
        }
    }
    let mut stack = vec![open(root)];
    loop {
        let top = stack.last_mut().expect("the root is popped last");
        match top.todo.next() {
            Some(group) => stack.push(open(group)),
            None => {
                let mut finished = stack.pop().expect("just looked at it");
                finished.head.children = finished.done;
                finished.head.contexts.sort();
                match stack.last_mut() {
                    Some(parent) => parent.done.push(finished.head),
                    None => return finished.head,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Basis, Context, DefInfo, Edge, EdgeTo, Site, Stats};
    use crate::text::Span;

    struct Files(HashMap<String, String>);

    impl Sources for Files {
        fn text(&self, file: &str) -> Option<String> {
            self.0.get(file).cloned()
        }
    }

    fn marked(name: &str) -> StableDef {
        StableDef::Marked(name.to_owned())
    }

    fn row(id: StableDef, qname: &str, kind: DefKind, start: u32, end: u32) -> DefInfo {
        DefInfo::new(id, qname, qname, 0, kind, "m.py", Span::new(start, end))
    }

    fn call(from: &str, at: u32, to: &str) -> Edge {
        Edge::new(
            ContextId::of(marked(from)),
            Site::new(at, at + 3),
            EdgeTo::Context(ContextId::of(marked(to)), Basis::Type),
        )
    }

    fn opts(merged: bool) -> WireOptions {
        WireOptions::new(
            WireStyle {
                function_prefix: "F/".into(),
                class_prefix: "C/".into(),
            },
            merged,
            RenderOptions::default(),
        )
    }

    fn files(text: &str) -> Files {
        Files(HashMap::from([("m.py".to_owned(), text.to_owned())]))
    }

    #[test]
    fn a_call_site_is_a_one_based_line_and_a_column_counted_in_characters() {
        // `é` is two bytes and one character: the second call is at character column 8 and byte column 9
        let text = "def f():\n    g()\n    é, g()\n";
        let first = text.find("g()").unwrap() as u32;
        let second = text.rfind("g()").unwrap() as u32;
        let graph = Graph::new(
            vec![Root::Context(ContextId::of(marked("f")))],
            vec![
                row(marked("f"), "m.f", DefKind::Function, 0, text.len() as u32),
                row(marked("g"), "m.g", DefKind::Function, 0, 0),
            ],
            vec![
                Context::new(ContextId::of(marked("f"))),
                Context::new(ContextId::of(marked("g"))),
            ],
            vec![call("f", first, "g"), call("f", second, "g")],
            Stats::default(),
        );
        let frame = wire(
            &graph,
            &Root::Context(ContextId::of(marked("f"))),
            &files(text),
            &opts(false),
        );
        assert_eq!(frame.call_site, None, "the root is not a call");
        let sites: Vec<_> = frame
            .children
            .iter()
            .map(|c| c.call_site.unwrap())
            .collect();
        assert_eq!(
            sites[0],
            LineSpan {
                start: (2, 5),
                end: (2, 8)
            }
        );
        assert_eq!(
            sites[1],
            LineSpan {
                start: (3, 8),
                end: (3, 11)
            }
        );
    }

    #[test]
    fn an_id_is_the_prefix_of_its_kind_and_empty_without_a_marker() {
        let path = StableDef::Path {
            module: "m".into(),
            qname: "plain".into(),
            nth: 0,
        };
        let graph = Graph::new(
            vec![Root::Context(ContextId::of(marked("main")))],
            vec![
                row(marked("main"), "m.main", DefKind::Function, 0, 40),
                row(marked("Report"), "m.Report", DefKind::Class, 50, 90),
                row(path.clone(), "m.plain", DefKind::Function, 100, 120),
            ],
            vec![
                Context::new(ContextId::of(marked("main"))),
                Context::new(ContextId::of(path.clone())),
            ],
            vec![
                Edge::new(
                    ContextId::of(marked("main")),
                    Site::new(0, 3),
                    EdgeTo::Construct {
                        class: marked("Report"),
                        init: None,
                        basis: Basis::Type,
                    },
                ),
                Edge::new(
                    ContextId::of(marked("main")),
                    Site::new(5, 8),
                    EdgeTo::Context(ContextId::of(path), Basis::Type),
                ),
            ],
            Stats::default(),
        );
        let frame = wire(
            &graph,
            &Root::Context(ContextId::of(marked("main"))),
            &files("x\n"),
            &opts(false),
        );
        let ids: Vec<&str> = std::iter::once(&frame)
            .chain(&frame.children)
            .map(|f| f.target_id.as_str())
            .collect();
        assert_eq!(ids, ["F/main", "C/Report", ""]);
        let bare = WireOptions::new(WireStyle::default(), false, RenderOptions::default());
        let frame = wire(
            &graph,
            &Root::Context(ContextId::of(marked("main"))),
            &files("x\n"),
            &bare,
        );
        assert_eq!(
            frame.target_id, "main",
            "no prefix given: the bare marker text"
        );
    }

    fn twice() -> Graph {
        Graph::new(
            vec![Root::Context(ContextId::of(marked("main")))],
            vec![
                row(marked("main"), "m.main", DefKind::Function, 0, 60),
                row(marked("run"), "m.run", DefKind::Function, 70, 90),
                row(marked("leaf"), "m.leaf", DefKind::Function, 100, 110),
            ],
            vec![
                Context::new(ContextId::of(marked("main"))),
                Context::new(ContextId::of(marked("run"))),
                Context::new(ContextId::of(marked("leaf"))),
            ],
            vec![
                call("main", 0, "run"),
                call("main", 10, "run"),
                call("run", 0, "leaf"),
            ],
            Stats::default(),
        )
    }

    #[test]
    fn merging_joins_siblings_counts_them_and_keeps_the_id() {
        let entry = Root::Context(ContextId::of(marked("main")));
        let plain = wire(&twice(), &entry, &files("x\n"), &opts(false));
        let counts = |f: &Frame| f.children.iter().map(|c| c.call_count).collect::<Vec<_>>();
        assert_eq!(counts(&plain), [0, 0]);

        let merged = wire(&twice(), &entry, &files("x\n"), &opts(true));
        assert_eq!(merged.children.len(), 1);
        let run = &merged.children[0];
        assert_eq!((run.target_qname.as_str(), run.call_count), ("m.run", 1));
        assert_eq!(run.target_id, "F/run");
        assert_eq!(
            run.children.len(),
            1,
            "their children are joined and merged too"
        );
        assert_eq!(run.children[0].call_count, 1);
        // the two calls to `run` were one context: it is listed once
        assert_eq!(run.contexts.len(), 1);
    }

    #[test]
    fn a_merged_node_keeps_the_call_site_of_its_first_sibling() {
        // two calls to `run`, at bytes 0 and 10: the merged node is where the first is, and has its site
        let text = "aaaaaaaaaa bbbbbbbbbb\n";
        let entry = Root::Context(ContextId::of(marked("main")));
        let merged = wire(&twice(), &entry, &files(text), &opts(true));
        let site = merged.children[0].call_site.unwrap();
        assert_eq!(
            site,
            LineSpan {
                start: (1, 1),
                end: (1, 4)
            }
        );
    }

    #[test]
    fn merging_twice_changes_nothing() {
        let entry = Root::Context(ContextId::of(marked("main")));
        let once = wire(&twice(), &entry, &files("x\n"), &opts(true));
        assert_eq!(merge(once.clone()), once);
    }

    #[test]
    fn a_very_deep_wire_is_built_merged_and_dropped_without_recursing() {
        let n = 60_000u32;
        let name = |i: u32| format!("f{i}");
        let (mut defs, mut contexts, mut edges) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..n {
            defs.push(row(marked(&name(i)), &name(i), DefKind::Function, 0, 10));
            contexts.push(Context::new(ContextId::of(marked(&name(i)))));
            if i + 1 < n {
                edges.push(call(&name(i), 0, &name(i + 1)));
            }
        }
        let graph = Graph::new(
            vec![Root::Context(ContextId::of(marked("f0")))],
            defs,
            contexts,
            edges,
            Stats::default(),
        );
        let entry = Root::Context(ContextId::of(marked("f0")));
        let deep = wire(&graph, &entry, &files("x\n"), &opts(false));
        let depth = |root: &Frame| {
            let (mut n, mut f) = (1, root);
            while let Some(c) = f.children.first() {
                n += 1;
                f = c;
            }
            n
        };
        assert_eq!(depth(&deep), n as usize, "every level is there");
        let merged = merge(deep);
        assert_eq!(depth(&merged), n as usize);
        drop(merged);
    }

    /// A row with a file, a qualname and an occurrence of its own.
    fn row_in(
        id: StableDef,
        qname: &str,
        (qualname, occurrence): (&str, u16),
        kind: DefKind,
        file: &str,
        (start, end): (u32, u32),
    ) -> DefInfo {
        DefInfo::new(
            id,
            qname,
            qualname,
            occurrence,
            kind,
            file,
            Span::new(start, end),
        )
    }

    /// `main` calls `f` twice, at two sites, and the two `f`s are two defs of one file with one name (`if C: def f() ... else: def f() ...`).
    fn twins() -> Graph {
        Graph::new(
            vec![Root::Context(ContextId::of(marked("main")))],
            vec![
                row_in(
                    marked("main"),
                    "m.main",
                    ("main", 0),
                    DefKind::Function,
                    "m.py",
                    (0, 60),
                ),
                row_in(
                    marked("f0"),
                    "m.f",
                    ("f", 0),
                    DefKind::Function,
                    "m.py",
                    (70, 90),
                ),
                row_in(
                    marked("f1"),
                    "m.f",
                    ("f", 1),
                    DefKind::Function,
                    "m.py",
                    (100, 120),
                ),
            ],
            vec![
                Context::new(ContextId::of(marked("main"))),
                Context::new(ContextId::of(marked("f0"))),
                Context::new(ContextId::of(marked("f1"))),
            ],
            vec![call("main", 0, "f0"), call("main", 10, "f1")],
            Stats::default(),
        )
    }

    #[test]
    fn two_defs_with_one_name_are_two_nodes_in_a_merged_wire() {
        // B6: the merge key was the display name, so the twins became one node with their children mixed
        let entry = Root::Context(ContextId::of(marked("main")));
        let merged = wire(&twins(), &entry, &files("x\n"), &opts(true));
        let kids: Vec<_> = merged
            .children
            .iter()
            .map(|c| (c.target_occurrence, c.call_count))
            .collect();
        assert_eq!(kids, [(0, 0), (1, 0)]);
        // the same def called twice is still one node (`merging_joins_siblings_counts_them_and_keeps_the_id`): the call count is 1
        let once = wire(&twice(), &entry, &files("x\n"), &opts(true));
        assert_eq!(once.children.len(), 1);
        assert_eq!(once.children[0].call_count, 1);
    }

    #[test]
    fn a_frame_names_its_target_by_file_qualname_occurrence_and_kind() {
        let entry = Root::Context(ContextId::of(marked("main")));
        let frame = wire(&twins(), &entry, &files("x\n"), &opts(false));
        // the root: named, and not a call
        assert_eq!(
            (
                frame.target_file.as_deref(),
                frame.target_qualname.as_str(),
                frame.target_occurrence,
                frame.target_kind.clone(),
                frame.call_site_file.clone()
            ),
            (Some("m.py"), "main", 0, Some(DefKind::Function), None)
        );
        let second = &frame.children[1];
        assert_eq!(
            (second.target_qualname.as_str(), second.target_occurrence),
            ("f", 1)
        );
        assert_eq!(second.call_site_file.as_deref(), Some("m.py"));
    }

    #[test]
    fn a_call_in_an_inherited_constructor_is_in_the_constructors_file_not_the_classes() {
        // B1: `class Child(Base): pass` in a.py, `Base.__init__` in b.py calling `helper`
        let text_b = "def __init__(self):\n    helper()\n";
        let site = text_b.find("helper").unwrap() as u32;
        let child = marked("Child");
        let init = ContextId::of(marked("Base.__init__"));
        let graph = Graph::new(
            vec![Root::Context(ContextId::of(marked("make")))],
            vec![
                row_in(
                    marked("make"),
                    "a.make",
                    ("make", 0),
                    DefKind::Function,
                    "a.py",
                    (0, 30),
                ),
                row_in(
                    child.clone(),
                    "a.Child",
                    ("Child", 0),
                    DefKind::Class,
                    "a.py",
                    (40, 60),
                ),
                row_in(
                    marked("Base.__init__"),
                    "b.Base.__init__",
                    ("Base.__init__", 0),
                    DefKind::Method,
                    "b.py",
                    (0, 40),
                ),
                row_in(
                    marked("helper"),
                    "b.helper",
                    ("helper", 0),
                    DefKind::Function,
                    "b.py",
                    (50, 70),
                ),
            ],
            vec![
                Context::new(ContextId::of(marked("make"))),
                Context::new(init.clone()),
                Context::new(ContextId::of(marked("helper"))),
            ],
            vec![
                Edge::new(
                    ContextId::of(marked("make")),
                    Site::new(0, 7),
                    EdgeTo::Construct {
                        class: child,
                        init: Some(init),
                        basis: Basis::Name,
                    },
                ),
                Edge::new(
                    ContextId::of(marked("Base.__init__")),
                    Site::new(site, site + 8),
                    EdgeTo::Context(ContextId::of(marked("helper")), Basis::Name),
                ),
            ],
            Stats::default(),
        );
        let sources = Files(HashMap::from([
            ("a.py".to_owned(), "def make():\n    Child()\n".to_owned()),
            ("b.py".to_owned(), text_b.to_owned()),
        ]));
        let frame = wire(
            &graph,
            &Root::Context(ContextId::of(marked("make"))),
            &sources,
            &opts(false),
        );
        let class = &frame.children[0];
        assert_eq!(
            (
                class.target_file.as_deref(),
                class.target_kind.clone(),
                class.call_site_file.as_deref()
            ),
            (Some("a.py"), Some(DefKind::Class), Some("a.py")),
            "the construction is called from `make`, in a.py"
        );
        let helper = &class.children[0];
        assert_eq!(helper.target_file.as_deref(), Some("b.py"));
        assert_eq!(
            helper.call_site_file.as_deref(),
            Some("b.py"),
            "the call to `helper` is in the constructor's file"
        );
        assert_eq!(
            helper.call_site.unwrap().start,
            (2, 5),
            "and its line is in that file too"
        );
    }

    #[test]
    fn an_external_leaf_has_no_file_no_qualname_and_no_kind() {
        let graph = Graph::new(
            vec![Root::Context(ContextId::of(marked("main")))],
            vec![row_in(
                marked("main"),
                "m.main",
                ("main", 0),
                DefKind::Function,
                "m.py",
                (0, 40),
            )],
            vec![Context::new(ContextId::of(marked("main")))],
            vec![Edge::new(
                ContextId::of(marked("main")),
                Site::new(0, 3),
                EdgeTo::External(
                    crate::graph::External {
                        package: "os".into(),
                        name: "getcwd".into(),
                    },
                    Basis::Name,
                ),
            )],
            Stats::default(),
        );
        let render = RenderOptions {
            show_external: true,
            ..RenderOptions::default()
        };
        let shown = WireOptions::new(WireStyle::default(), false, render);
        let frame = wire(
            &graph,
            &Root::Context(ContextId::of(marked("main"))),
            &files("x\n"),
            &shown,
        );
        let leaf = &frame.children[0];
        assert_eq!(leaf.target_qname, "os.getcwd");
        assert_eq!(
            (
                leaf.target_file.clone(),
                leaf.target_qualname.as_str(),
                leaf.target_occurrence,
                leaf.target_kind.clone()
            ),
            (None, "", 0, None)
        );
    }

    /// The names of the fields and how a `DefKind` is written are the contract with a driver: pinned as text, so they cannot drift.
    #[test]
    fn a_frame_as_json() {
        let entry = Root::Context(ContextId::of(marked("main")));
        let frame = wire(&twice(), &entry, &files("aaaa bbbb\n"), &opts(false));
        let leaf = &frame.children[0];
        let text = serde_json::to_string(leaf).unwrap();
        assert_eq!(
            text,
            concat!(
                r#"{"target_qname":"m.run","target_id":"F/run","target_file":"m.py","target_qualname":"m.run","#,
                r#""target_occurrence":0,"target_kind":"Function","call_count":0,"#,
                r#""call_site":{"start":[1,1],"end":[1,4]},"call_site_file":"m.py","#,
                r#""contexts":[{"func":{"Marked":"run"},"digest":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}],"children":[{"#,
                r#""target_qname":"m.leaf","target_id":"F/leaf","target_file":"m.py","target_qualname":"m.leaf","target_occurrence":0,"#,
                r#""target_kind":"Function","call_count":0,"call_site":null,"call_site_file":"m.py","#,
                r#""contexts":[{"func":{"Marked":"leaf"},"digest":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}],"children":[]}]}"#
            )
        );
        // a class is written as a plain string too
        assert_eq!(
            serde_json::to_string(&DefKind::Class).unwrap(),
            r#""Class""#
        );
    }

    #[test]
    fn a_site_whose_file_cannot_be_read_has_no_position_and_the_node_stays() {
        let graph = twice();
        let none = Files(HashMap::new());
        let frame = wire(
            &graph,
            &Root::Context(ContextId::of(marked("main"))),
            &none,
            &opts(false),
        );
        assert_eq!(frame.children.len(), 2);
        assert!(frame.children.iter().all(|c| c.call_site.is_none()));
    }

    #[test]
    fn under_a_construct_root_a_call_is_in_the_constructors_file() {
        // the same program as above, asked for `Child` itself: the root is in a.py, the call made by `Base.__init__` is in b.py
        let text_b = "def __init__(self):\n    helper()\n";
        let site = text_b.find("helper").unwrap() as u32;
        let init = ContextId::of(marked("Base.__init__"));
        let graph = Graph::new(
            vec![Root::Construct {
                class: marked("Child"),
                init: Some(init.clone()),
            }],
            vec![
                row_in(
                    marked("Child"),
                    "a.Child",
                    ("Child", 0),
                    DefKind::Class,
                    "a.py",
                    (40, 60),
                ),
                row_in(
                    marked("Base.__init__"),
                    "b.Base.__init__",
                    ("Base.__init__", 0),
                    DefKind::Method,
                    "b.py",
                    (0, 40),
                ),
                row_in(
                    marked("helper"),
                    "b.helper",
                    ("helper", 0),
                    DefKind::Function,
                    "b.py",
                    (50, 70),
                ),
            ],
            vec![
                Context::new(init.clone()),
                Context::new(ContextId::of(marked("helper"))),
            ],
            vec![Edge::new(
                init,
                Site::new(site, site + 8),
                EdgeTo::Context(ContextId::of(marked("helper")), Basis::Name),
            )],
            Stats::default(),
        );
        let sources = Files(HashMap::from([
            (
                "a.py".to_owned(),
                "class Child(Base):\n    pass\n".to_owned(),
            ),
            ("b.py".to_owned(), text_b.to_owned()),
        ]));
        let frame = wire(&graph, &graph.entries[0], &sources, &opts(false));
        assert_eq!(
            (
                frame.target_file.as_deref(),
                frame.target_kind.clone(),
                frame.call_site_file.as_deref()
            ),
            (Some("a.py"), Some(DefKind::Class), None),
            "the root is the class, in a.py, and is not a call"
        );
        let helper = &frame.children[0];
        assert_eq!(helper.call_site_file.as_deref(), Some("b.py"));
        assert_eq!(helper.call_site.unwrap().start, (2, 5));
    }

    // D22

    fn toy_wire(
        program: &[(&'static str, &[&'static str])],
        entry: &'static str,
        merged: bool,
    ) -> Frame {
        let g = crate::solve::tests::graph_of(program, entry);
        let text = "x".repeat(2000);
        wire(
            &g,
            &g.entries[0],
            &Files(HashMap::from([("toy.src".to_owned(), text)])),
            &opts(merged),
        )
    }

    /// `(name, passed, call_count)` of the children.
    fn kids(frame: &Frame) -> Vec<(String, bool, u32)> {
        frame
            .children
            .iter()
            .map(|c| (c.target_qname.clone(), c.passed, c.call_count))
            .collect()
    }

    #[test]
    fn d22_the_unmerged_wire_marks_a_passed_frame_and_keeps_its_own_site() {
        let program: &[(&'static str, &[&'static str])] = &[
            ("main", &["a", ">a", ">b"]),
            ("a", &[]),
            ("b", &["c"]),
            ("c", &[]),
        ];
        let frame = toy_wire(program, "main", false);
        assert_eq!(
            kids(&frame),
            [
                ("a".to_owned(), false, 0),
                ("a".to_owned(), true, 0),
                ("b".to_owned(), true, 0)
            ]
        );
        // each has its own call site (the call and the hand-over of `a` are at different places), and what `b` calls is under it
        let sites: Vec<_> = frame.children.iter().map(|c| c.call_site).collect();
        assert!(sites.iter().all(Option::is_some));
        assert_ne!(sites[0], sites[1]);
        assert_eq!(frame.children[2].children.len(), 1);
    }

    #[test]
    fn d22_the_merged_wire_never_folds_a_passed_frame_into_a_call_frame() {
        // `a` called once and handed over twice: a call node, and one passed node with `call_count` 1 (two hand-overs, less one)
        let program: &[(&'static str, &[&'static str])] =
            &[("main", &["a", ">a", ">a"]), ("a", &[])];
        let frame = toy_wire(program, "main", true);
        assert_eq!(
            kids(&frame),
            [("a".to_owned(), false, 0), ("a".to_owned(), true, 1)]
        );
    }

    #[test]
    fn d22_passed_is_in_the_json_only_when_true() {
        let program: &[(&'static str, &[&'static str])] = &[("main", &["a", ">a"]), ("a", &[])];
        let frame = toy_wire(program, "main", false);
        let called = serde_json::to_value(&frame.children[0]).unwrap();
        let passed = serde_json::to_value(&frame.children[1]).unwrap();
        assert!(called.get("passed").is_none(), "{called}");
        assert_eq!(passed["passed"], serde_json::Value::Bool(true));
        // a wire with no passed node has no `passed` key anywhere
        let plain = toy_wire(&[("main", &["a"]), ("a", &[])], "main", false);
        assert!(!serde_json::to_string(&plain).unwrap().contains("passed"));
    }

    #[test]
    fn d22_the_wire_format_version_is_a_number_changed_on_purpose() {
        // 1: through E3. 2: `passed`. A change to this constant is a change to what a node means: it comes with a line in its doc and in
        // `docs/03-typescript/07-ts5-driver-needs.md` §8.
        assert_eq!(WIRE_FORMAT_VERSION, 2);
    }

    // D28

    fn call_with(from: &str, at: u32, to: &str, basis: Basis) -> Edge {
        Edge::new(
            ContextId::of(marked(from)),
            Site::new(at, at + 3),
            EdgeTo::Context(ContextId::of(marked(to)), basis),
        )
    }

    /// `main` calls `run` at 0 and at 10, with the given bases; `run` calls `leaf`.
    fn run_twice(first: Basis, second: Basis) -> Graph {
        let g = twice();
        Graph::new(
            g.entries.clone(),
            g.defs.clone(),
            g.contexts.clone(),
            vec![
                call_with("main", 0, "run", first),
                call_with("main", 10, "run", second),
                call("run", 0, "leaf"),
            ],
            Stats::default(),
        )
    }

    fn wire_of(g: &Graph, merged: bool) -> Frame {
        let entry = Root::Context(ContextId::of(marked("main")));
        wire(g, &entry, &files("x\n"), &opts(merged))
    }

    #[test]
    fn d28_a_likely_frame_has_the_mark_and_what_is_under_it_does_not() {
        let g = run_twice(Basis::Likely, Basis::Type);
        let frame = wire_of(&g, false);
        // marks go on the wire lowercased, as `recursive` and `cut` do
        assert_eq!(frame.children[0].marks, ["likely"]);
        assert!(frame.children[1].marks.is_empty());
        assert!(frame.children[0].children[0].marks.is_empty(), "leaf");
        let json = serde_json::to_value(&frame.children[0]).unwrap();
        assert_eq!(json["marks"], serde_json::json!(["likely"]));
    }

    #[test]
    fn d28_a_likely_call_and_a_certain_call_of_one_def_are_two_merged_frames() {
        let g = run_twice(Basis::Likely, Basis::Type);
        let merged = wire_of(&g, true);
        let shape: Vec<_> = merged
            .children
            .iter()
            .map(|c| (c.marks.clone(), c.call_count))
            .collect();
        assert_eq!(shape, [(vec!["likely".to_owned()], 0), (vec![], 0)]);
        // the other order too: the key does not depend on which is first
        let merged = wire_of(&run_twice(Basis::Type, Basis::Likely), true);
        assert_eq!(merged.children.len(), 2);
    }

    #[test]
    fn d28_two_likely_calls_of_one_def_are_one_merged_frame_and_keep_the_mark() {
        let g = run_twice(Basis::Likely, Basis::Likely);
        let merged = wire_of(&g, true);
        assert_eq!(merged.children.len(), 1);
        assert_eq!(merged.children[0].call_count, 1);
        assert_eq!(merged.children[0].marks, ["likely"]);
        assert_eq!(
            merge(merged.clone()),
            merged,
            "merging twice changes nothing"
        );
    }
}
