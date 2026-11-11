//! The types an outline returns: a flat list of definitions, in source order, parents before children.

use serde::Serialize;

use crate::text::Span;

/// What a definition is. The set is open: a language may add a kind without a core release.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[non_exhaustive]
pub enum DefKind {
    Function,
    Method,
    Class,
    Struct,
    Interface,
    Trait,
    Enum,
    Module,
    Namespace,
    Type,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Def {
    /// The language's own naming rule. It is stored in users' files, so it never changes once released.
    pub qualname: String,
    pub name: String,
    pub kind: DefKind,
    /// Index of the enclosing def in the same list.
    pub parent: Option<usize>,
    /// The whole definition: first decorator / attribute / doc comment up to the last byte.
    pub range: Span,
    /// Just the name, for "reveal in editor".
    pub name_range: Span,
    pub params: Vec<String>,
    /// The marker text found on this def, exactly as written.
    pub marker: Option<String>,
    /// A parse error lies inside this def.
    pub syntax_error: bool,
}

impl Def {
    /// The required fields. The rest start empty; set them by assignment (the struct is `#[non_exhaustive]`, so other
    /// crates cannot write a struct literal, which is what lets core add a field later).
    pub fn new(
        qualname: impl Into<String>,
        name: impl Into<String>,
        kind: DefKind,
        range: Span,
        name_range: Span,
    ) -> Self {
        Def {
            qualname: qualname.into(),
            name: name.into(),
            kind,
            parent: None,
            range,
            name_range,
            params: Vec::new(),
            marker: None,
            syntax_error: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct SyntaxError {
    pub range: Span,
    pub message: String,
}

impl SyntaxError {
    pub fn new(range: Span, message: impl Into<String>) -> Self {
        SyntaxError {
            range,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Outline {
    pub defs: Vec<Def>,
    /// The file's own marker, from its header comment.
    pub file_marker: Option<String>,
    pub syntax_errors: Vec<SyntaxError>,
}

impl Outline {
    /// Children of `parent`, in source order. `None` gives the top-level defs.
    pub fn children(&self, parent: Option<usize>) -> impl Iterator<Item = (usize, &Def)> {
        self.defs
            .iter()
            .enumerate()
            .filter(move |(_, d)| d.parent == parent)
    }

    /// The list contract: every `parent` points at an earlier entry. Language tests call this on every fixture.
    pub fn is_well_formed(&self) -> bool {
        self.defs
            .iter()
            .enumerate()
            .all(|(i, d)| d.parent.is_none_or(|p| p < i))
            && self
                .defs
                .windows(2)
                .all(|w| w[0].range.start <= w[1].range.start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str, parent: Option<usize>, start: u32) -> Def {
        let mut d = Def::new(
            name,
            name,
            DefKind::Function,
            Span::new(start, start + 5),
            Span::new(start, start + 1),
        );
        d.parent = parent;
        d
    }

    #[test]
    fn children_of_a_parent_and_of_the_top() {
        let outline = Outline {
            defs: vec![
                def("A", None, 0),
                def("A.m", Some(0), 1),
                def("B", None, 10),
            ],
            ..Outline::default()
        };
        let top: Vec<_> = outline.children(None).map(|(i, _)| i).collect();
        let under_a: Vec<_> = outline.children(Some(0)).map(|(i, _)| i).collect();
        assert_eq!((top, under_a), (vec![0, 2], vec![1]));
    }

    #[test]
    fn well_formed_needs_parents_first_and_source_order() {
        let good = Outline {
            defs: vec![def("A", None, 0), def("A.m", Some(0), 1)],
            ..Outline::default()
        };
        let child_first = Outline {
            defs: vec![def("A.m", Some(1), 1), def("A", None, 0)],
            ..Outline::default()
        };
        let unsorted = Outline {
            defs: vec![def("B", None, 9), def("A", None, 0)],
            ..Outline::default()
        };
        assert!(good.is_well_formed());
        assert!(!child_first.is_well_formed());
        assert!(!unsorted.is_well_formed());
    }
}
