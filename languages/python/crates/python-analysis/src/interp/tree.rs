//! Inline mode is the fixture suite's cross-check and is not reachable from the public API.
#![allow(dead_code)]

use ruff_text_size::TextRange;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    pub qname: String,
    pub site: Option<TextRange>, // the call expression's range. None for the root
    pub marks: Vec<Mark>,
    pub children: Vec<Node>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mark {
    Recursive,
    Cut,
    Incomplete,
}

impl Node {
    pub(crate) fn root(qname: String) -> Self {
        Node {
            qname,
            site: None,
            marks: Vec::new(),
            children: Vec::new(),
        }
    }
    pub(crate) fn call(qname: String, site: TextRange) -> Self {
        Node {
            qname,
            site: Some(site),
            marks: Vec::new(),
            children: Vec::new(),
        }
    }
}

/// One line per node, two spaces per level, `module.` dropped from names.
pub(crate) fn to_text(root: &Node, module: &str) -> String {
    fn go(n: &Node, depth: usize, module: &str, out: &mut String) {
        let short = n
            .qname
            .strip_prefix(module)
            .and_then(|rest| rest.strip_prefix('.'))
            .unwrap_or(&n.qname);
        out.push_str(&"  ".repeat(depth));
        out.push_str(short);
        for m in &n.marks {
            out.push_str(match m {
                Mark::Recursive => " [recursive]",
                Mark::Cut => " [cut]",
                Mark::Incomplete => " [incomplete]",
            });
        }
        out.push('\n');
        for c in &n.children {
            go(c, depth + 1, module, out);
        }
    }
    let mut out = String::new();
    go(root, 0, module, &mut out);
    out
}
