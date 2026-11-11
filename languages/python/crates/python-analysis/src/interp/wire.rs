// src/interp/wire.rs
use crate::interp::canon::StableContextId;
use crate::interp::graph::Span;
use crate::interp::tree::{Mark, Node};

/// The old renderer's tree: one node per call site, one function, one path. Kept until the core renderer is proven against it (PY6).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FrameJson {
    pub target_qname: String,
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
