//! The outline: which nodes are defs, and each one's qualname. The rules are `docs/03-typescript/03-ts1-outline.md` §2–§4 and, once
//! TypeScript is tagged, `languages/typescript/docs/qualnames.md`. **Qualnames are stored in users' files: never change how an
//! existing kind of def is named.**

use analyzer_core::outline::{Def, DefKind, Outline};
use analyzer_core::text::Span;

use crate::Dialect;
use crate::sem::{NodeKind, Parsed, RawNode, with_parsed};

pub(crate) fn outline(text: &str, dialect: Dialect) -> Outline {
    with_parsed(text, dialect, |p| {
        let mut walker = Walker {
            p,
            stack: Vec::new(),
            defs: Vec::new(),
        };
        walker.statements(&p.statements(p.root()));
        let mut outline = Outline::default();
        outline.defs = walker.defs;
        outline
    })
}

/// An enclosing def while the walk is inside it.
struct Frame {
    qualname: String,
    /// Index into `Walker::defs`.
    index: usize,
    /// A function's locals are joined with `.<locals>.`, a container's members with `.`.
    is_function: bool,
}

struct Walker<'p, 'a> {
    p: &'p Parsed<'a>,
    stack: Vec<Frame>,
    defs: Vec<Def>,
}

/// What a def is, as far as naming goes.
struct Entry {
    name: String,
    kind: DefKind,
    is_function: bool,
    range: Span,
    name_range: Span,
    params: Vec<String>,
}

impl Walker<'_, '_> {
    /// Push a def (before its children, so the list is pre-order: parents first, in source order) and open its frame.
    fn enter(&mut self, e: Entry) {
        let qualname = match self.stack.last() {
            None => e.name.clone(),
            Some(parent) if parent.is_function => {
                format!("{}.<locals>.{}", parent.qualname, e.name)
            }
            Some(parent) => format!("{}.{}", parent.qualname, e.name),
        };
        let mut def = Def::new(qualname.clone(), e.name, e.kind, e.range, e.name_range);
        def.parent = self.stack.last().map(|f| f.index);
        def.params = e.params;
        self.defs.push(def);
        self.stack.push(Frame {
            qualname,
            index: self.defs.len() - 1,
            is_function: e.is_function,
        });
    }

    fn leave(&mut self) {
        self.stack.pop();
    }

    /// A list of statements. Overload signatures followed by their implementation are one def, so the list is walked with a window.
    fn statements(&mut self, nodes: &[RawNode]) {
        let mut i = 0;
        while i < nodes.len() {
            let n = nodes[i];
            if self.p.kind(n) == NodeKind::FunctionDeclaration {
                i += self.function_group(&nodes[i..]);
            } else {
                self.visit(n);
                i += 1;
            }
        }
    }

    /// Anything that is not itself a def: look inside it for defs.
    fn visit(&mut self, n: RawNode) {
        match self.p.kind(n) {
            NodeKind::SourceFile | NodeKind::ModuleBlock => self.statements(&self.p.statements(n)),
            NodeKind::FunctionDeclaration => {
                self.function_group(&[n]);
            }
            NodeKind::ClassDeclaration => self.class(n),
            _ if self.p.kind(n) == NodeKind::Other && !self.p.statements(n).is_empty() => {
                // a block, a case clause: the block adds nothing to a name
                self.statements(&self.p.statements(n));
            }
            _ => {
                for child in self.p.children(n) {
                    self.visit(child);
                }
            }
        }
    }

    /// The text of a name token as written: `f`, `#secret`, `"my-key"`, `[Symbol.iterator]`.
    fn name_text(&self, name: RawNode) -> Option<String> {
        self.p.slice(self.p.span(name)).map(str::to_owned)
    }

    /// The name of a declaration and the range to reveal: its name token, or `default` for `export default` without one.
    fn name_of(&self, n: RawNode) -> Option<(String, Span)> {
        if let Some(name) = self.p.name(n) {
            let text = self.name_text(name)?;
            return Some((text, self.p.span(name)));
        }
        if self.p.modifiers(n).default {
            let range = self.p.default_keyword(n).unwrap_or_else(|| self.p.span(n));
            return Some(("default".to_owned(), range));
        }
        None
    }

    /// How many nodes of `group` were taken. A run of same-name function declarations whose last one has a body is one def (the
    /// implementation) and is taken whole; a run with no implementation (a `.d.ts`) is one def per signature.
    fn function_group(&mut self, group: &[RawNode]) -> usize {
        let first = group[0];
        let Some((name, _)) = self.name_of(first) else {
            // an anonymous declaration (a syntax error): no def, but look inside
            self.function_body(first);
            return 1;
        };
        let run = group
            .iter()
            .take_while(|&&n| {
                self.p.kind(n) == NodeKind::FunctionDeclaration
                    && self.name_of(n).is_some_and(|(other, _)| other == name)
            })
            .count();
        let last = group[run - 1];
        let folds = run > 1
            && self.p.body(last).is_some()
            && group[..run - 1].iter().all(|&n| self.p.body(n).is_none());
        let (taken, imp) = if folds { (run, last) } else { (1, first) };
        let (_, name_range) = self.name_of(imp).expect("the name was found above");
        self.enter(Entry {
            name,
            kind: DefKind::Function,
            is_function: true,
            range: Span::new(self.p.span(first).start, self.p.span(imp).end),
            name_range,
            params: self.p.params(imp),
        });
        self.function_body(imp);
        self.leave();
        taken
    }

    /// Look for defs inside a function's body. Parameters are not searched.
    fn function_body(&mut self, f: RawNode) {
        if let Some(body) = self.p.body(f) {
            self.visit(body);
        }
    }

    fn class(&mut self, n: RawNode) {
        let Some((name, name_range)) = self.name_of(n) else {
            return;
        };
        self.enter(Entry {
            name,
            kind: DefKind::Class,
            is_function: false,
            range: self.p.span(n),
            name_range,
            params: Vec::new(),
        });
        self.members(&self.p.members(n));
        self.leave();
    }

    /// The members of a class. Overloaded methods fold the same way overloaded functions do.
    fn members(&mut self, members: &[RawNode]) {
        let mut i = 0;
        while i < members.len() {
            let m = members[i];
            i += match self.p.kind(m) {
                NodeKind::MethodDeclaration => self.method_group(&members[i..]),
                NodeKind::Constructor => {
                    self.constructor(m);
                    1
                }
                NodeKind::GetAccessor | NodeKind::SetAccessor => {
                    self.accessor(m);
                    1
                }
                NodeKind::ClassStaticBlock => {
                    // not a def: what is inside belongs to the class (its body is a child, not `body()`)
                    self.visit(m);
                    1
                }
                _ => 1,
            };
        }
    }

    fn method_group(&mut self, group: &[RawNode]) -> usize {
        let first = group[0];
        let Some((name, _)) = self.name_of(first) else {
            self.function_body(first);
            return 1;
        };
        let run = group
            .iter()
            .take_while(|&&n| {
                self.p.kind(n) == NodeKind::MethodDeclaration
                    && self.name_of(n).is_some_and(|(other, _)| other == name)
            })
            .count();
        let last = group[run - 1];
        let folds = run > 1
            && self.p.body(last).is_some()
            && group[..run - 1].iter().all(|&n| self.p.body(n).is_none());
        let (taken, imp) = if folds { (run, last) } else { (1, first) };
        let (_, name_range) = self.name_of(imp).expect("the name was found above");
        self.enter(Entry {
            name,
            kind: DefKind::Method,
            is_function: true,
            range: Span::new(self.p.span(first).start, self.p.span(imp).end),
            name_range,
            params: self.p.params(imp),
        });
        self.function_body(imp);
        self.leave();
        taken
    }

    fn accessor(&mut self, n: RawNode) {
        let Some((name, name_range)) = self.name_of(n) else {
            return;
        };
        self.enter(Entry {
            name,
            kind: DefKind::Method,
            is_function: true,
            range: self.p.span(n),
            name_range,
            params: self.p.params(n),
        });
        self.function_body(n);
        self.leave();
    }

    /// `constructor` has no name node: its name is the keyword, found in the member's own text (a constructor takes no decorator, and
    /// no modifier contains the word).
    fn constructor(&mut self, n: RawNode) {
        let range = self.p.span(n);
        let text = self.p.slice(range).unwrap_or("");
        let at = text.find("constructor").unwrap_or(0) as u32;
        let name_range = Span::new(
            range.start + at,
            range.start + at + "constructor".len() as u32,
        );
        self.enter(Entry {
            name: "constructor".to_owned(),
            kind: DefKind::Method,
            is_function: true,
            range,
            name_range,
            params: self.p.params(n),
        });
        self.function_body(n);
        self.leave();
    }
}
