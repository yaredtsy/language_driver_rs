//! The wire: the call tree as V-NOC stores it, a view of the graph. `graph → render → Node → Frame`.
//!
//! Written only against `analyzer-core` types, and it names no language and no engine, so when a second language needs it this file moves to
//! `analyzer-core` unchanged. It has no tree rules of its own: recursion, cuts, the budget and the order of siblings are the renderer's.
//!
//! Like the renderer, nothing here recurses once per level (building the frame, merging and dropping use explicit stacks). `Frame`'s derived
//! `Clone`, `Debug`, `PartialEq` and `Serialize` do recurse: do not use them on a tree tens of thousands of levels deep.

use std::collections::HashMap;

use analyzer_core::graph::{ContextId, Graph, StableDef};
use analyzer_core::outline::DefKind;
use analyzer_core::render::{Node, RenderOptions, render};
use analyzer_core::text::LineIndex;
use serde::Serialize;

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
    /// 0 in an unmerged wire. In a merged one, the calls joined into this node, less one.
    pub call_count: u32,
    /// Where the call is. `None` for the root.
    pub call_site: Option<LineSpan>,
    /// Which graph contexts this node shows.
    pub contexts: Vec<ContextId>,
    /// `recursive`, `cut`, `incomplete`, and with the options on, `external` and `unresolved`.
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
    let call_site = (|| {
        let site = node.site?;
        let caller = graph.def(caller?)?;
        let mut at = |relative: u32| positions.at(&caller.file, caller.range.start + relative);
        Some(LineSpan {
            start: at(site.start)?,
            end: at(site.end)?,
        })
    })();
    Frame {
        target_qname: node.qname.clone(),
        target_id: target_id(graph, &node.func, style),
        call_count: 0,
        call_site,
        contexts: node.contexts.clone(),
        marks: node
            .marks
            .iter()
            .map(|m| m.as_str().to_ascii_lowercase())
            .collect(),
        children: Vec::new(),
    }
}

/// The wire of `entry`: the tree `render` draws, as frames, merged when asked.
pub fn wire(graph: &Graph, entry: &ContextId, sources: &dyn Sources, opts: &WireOptions) -> Frame {
    let tree = render(graph, entry, &opts.render);
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

/// Siblings with one `target_qname` become one node: `call_count` is the sum of theirs plus one for each node joined, `contexts` and
/// `marks` are unioned, and their children are concatenated (and merged the same way, one level down).
fn group(kids: Vec<Frame>) -> Vec<Frame> {
    let mut out: Vec<Frame> = Vec::new();
    for mut c in kids {
        match out.iter_mut().find(|m| m.target_qname == c.target_qname) {
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
    use analyzer_core::graph::{Context, DefInfo, Edge, EdgeTo, Site, Stats};
    use analyzer_core::text::Span;

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
        DefInfo::new(id, qname, kind, "m.py", Span::new(start, end))
    }

    fn call(from: &str, at: u32, to: &str) -> Edge {
        Edge {
            from: ContextId::of(marked(from)),
            site: Site::new(at, at + 3),
            to: EdgeTo::Context(ContextId::of(marked(to))),
        }
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
            vec![ContextId::of(marked("f"))],
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
            &ContextId::of(marked("f")),
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
            vec![ContextId::of(marked("main"))],
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
                Edge {
                    from: ContextId::of(marked("main")),
                    site: Site::new(0, 3),
                    to: EdgeTo::Construct {
                        class: marked("Report"),
                        init: None,
                    },
                },
                Edge {
                    from: ContextId::of(marked("main")),
                    site: Site::new(5, 8),
                    to: EdgeTo::Context(ContextId::of(path)),
                },
            ],
            Stats::default(),
        );
        let frame = wire(
            &graph,
            &ContextId::of(marked("main")),
            &files("x\n"),
            &opts(false),
        );
        let ids: Vec<&str> = std::iter::once(&frame)
            .chain(&frame.children)
            .map(|f| f.target_id.as_str())
            .collect();
        assert_eq!(ids, ["F/main", "C/Report", ""]);
        let bare = WireOptions::new(WireStyle::default(), false, RenderOptions::default());
        let frame = wire(&graph, &ContextId::of(marked("main")), &files("x\n"), &bare);
        assert_eq!(
            frame.target_id, "main",
            "no prefix given: the bare marker text"
        );
    }

    fn twice() -> Graph {
        Graph::new(
            vec![ContextId::of(marked("main"))],
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
        let entry = ContextId::of(marked("main"));
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
        let entry = ContextId::of(marked("main"));
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
        let entry = ContextId::of(marked("main"));
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
            vec![ContextId::of(marked("f0"))],
            defs,
            contexts,
            edges,
            Stats::default(),
        );
        let entry = ContextId::of(marked("f0"));
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

    #[test]
    fn a_site_whose_file_cannot_be_read_has_no_position_and_the_node_stays() {
        let graph = twice();
        let none = Files(HashMap::new());
        let frame = wire(&graph, &ContextId::of(marked("main")), &none, &opts(false));
        assert_eq!(frame.children.len(), 2);
        assert!(frame.children.iter().all(|c| c.call_site.is_none()));
    }
}
