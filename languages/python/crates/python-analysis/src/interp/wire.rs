// src/interp/wire.rs
use crate::interp::canon::StableContextId;
use crate::interp::graph::Span;
use crate::interp::tree::{Mark, Node};

/// The old renderer's tree: one node per call site, one function, one path. Kept until the core renderer is proven against it (PY6).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FrameJson {
    pub target_qname: String,
    /// The style's prefix and the docstring ID; empty only when the function has no ID.
    pub target_id: String,
    pub call_count: u32, // always 0: merging siblings is the consumer's job
    pub call_site: Option<Span>,
    pub contexts: Vec<StableContextId>, // which graph contexts this node shows
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<Mark>,
    pub children: Vec<FrameJson>,
}

impl PartialEq for Span {
    fn eq(&self, o: &Self) -> bool {
        (self.start, self.end) == (o.start, o.end)
    }
}
impl Eq for Span {}

impl FrameJson {
    pub fn count(&self) -> u32 {
        1 + self.children.iter().map(FrameJson::count).sum::<u32>()
    }

    /// The same tree for `to_text` and the harness.
    pub fn to_node(&self) -> Node {
        Node {
            qname: self.target_qname.clone(),
            site: None,
            marks: self.marks.clone(),
            children: self.children.iter().map(FrameJson::to_node).collect(),
        }
    }
}

/// The merge, as an option: siblings with one `target_qname` become one node with `call_count = Σ call_count + k − 1`, and their
/// children are merged the same way. The analyzer's own default is one node per call site; a consumer that wants the merged tree asks
/// for it.
pub fn merge_for_output(root: &FrameJson) -> FrameJson {
    let mut out = FrameJson {
        children: Vec::new(),
        ..root.clone()
    };
    for c in &root.children {
        match out
            .children
            .iter_mut()
            .find(|m| m.target_qname == c.target_qname)
        {
            Some(m) => {
                m.call_count += c.call_count + 1;
                for id in &c.contexts {
                    if !m.contexts.contains(id) {
                        m.contexts.push(id.clone());
                    }
                }
                for mark in &c.marks {
                    if !m.marks.contains(mark) {
                        m.marks.push(*mark);
                    }
                }
                m.children.extend(c.children.iter().cloned());
            }
            None => out.children.push(c.clone()),
        }
    }
    out.children = out.children.iter().map(merge_for_output).collect();
    out.contexts.sort();
    out
}
