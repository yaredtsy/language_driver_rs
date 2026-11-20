//! The one module of this crate that names a `bend_parser` type, and no `bend_parser` type leaves it (rule 4). `read` parses a file and hands back owned
//! data: what the book needs of it beyond the outline, the `import` lines and, for each declaration, whether it fills a law and which constructors it has.
//!
//! BD4 adds the walk over a def's body here, and it will return owned data too. The tree is ours already, so "owned" costs nothing; the boundary is the
//! point, so that a different parser would replace this folder and nothing else.

use analyzer_core::text::Span;

/// One `import` line, as written.
#[derive(Clone, Debug)]
pub(crate) struct ImportLine {
    /// `Base`, `./m.bend`, `../lib/x.bend`, `0x<hash>/main.bend`.
    pub(crate) path: String,
    /// The `as` name. `import Base` has none.
    pub(crate) alias: Option<String>,
}

/// What the book needs to know of one declaration. `Facts::decls` is in source order, one per declaration that is a def of the outline.
#[derive(Clone, Debug)]
pub(crate) struct DeclFacts {
    /// Where the declaration's name is: how a def of the outline finds its facts.
    pub(crate) name_span: Span,
    /// A `def` with no `->`: the form that fills the law of the same name (`def f(x):`), which only the book can tell exists.
    pub(crate) fills_law: bool,
    /// The constructors of a `type`, by name. Empty for the others.
    pub(crate) ctors: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Facts {
    pub(crate) imports: Vec<ImportLine>,
    pub(crate) decls: Vec<DeclFacts>,
}

impl Facts {
    /// The facts of the declaration whose name is at `name_span`. `None` for a def this text does not have (an outline of another text).
    pub(crate) fn of(&self, name_span: Span) -> Option<&DeclFacts> {
        let at = self
            .decls
            .binary_search_by_key(&name_span.start, |d| d.name_span.start)
            .ok()?;
        self.decls.get(at).filter(|d| d.name_span == name_span)
    }
}

fn decl(d: &bend_parser::Decl) -> Option<DeclFacts> {
    let (fills_law, ctors) = match d {
        bend_parser::Decl::Def(f) => (f.ret.is_none(), Vec::new()),
        bend_parser::Decl::Type(t) => (false, t.ctrs.iter().map(|c| c.name.text.clone()).collect()),
        bend_parser::Decl::Law(_) => (false, Vec::new()),
        _ => return None,
    };
    Some(DeclFacts {
        name_span: d.name().span,
        fills_law,
        ctors,
    })
}

/// Reads `text`: never fails, a broken declaration is read as far as the parser got.
pub(crate) fn read(text: &str) -> Facts {
    let tree = bend_parser::parse(text);
    Facts {
        imports: tree
            .imports
            .iter()
            .map(|i| ImportLine {
                path: i.path.clone(),
                alias: i.alias.as_ref().map(|a| a.text.clone()),
            })
            .collect(),
        decls: tree.decls.iter().filter_map(decl).collect(),
    }
}
