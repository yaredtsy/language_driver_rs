//! The outline: which nodes are defs, and each one's qualname. The rules are `docs/03-typescript/03-ts1-outline.md` §2–§4 and, once
//! TypeScript is tagged, `languages/typescript/docs/qualnames.md`. **Qualnames are stored in users' files: never change how an
//! existing kind of def is named.**

use analyzer_core::edit::{MarkerStyle, Placement};
use analyzer_core::outline::{Def, DefKind, Outline, SyntaxError};
use analyzer_core::text::Span;

use crate::Dialect;
use crate::marker;
use crate::sem::{NodeKind, Parsed, RawNode, with_parsed};

pub(crate) fn outline(text: &str, dialect: Dialect, style: MarkerStyle<'_>) -> Outline {
    with_parsed(text, dialect, |p| {
        let mut errors: Vec<(Span, String)> = p.syntax_errors();
        errors.sort_by_key(|(range, _)| *range);
        let mut defs = walk(p);
        for def in &mut defs {
            def.syntax_error = errors.iter().any(|(range, _)| overlaps(*range, def.range));
            def.marker = match style.placement {
                Placement::Docstring => {
                    marker::marker_of(text, &p.above(def.range.start), style.label)
                }
                // a placement this analyzer does not build reads nothing
                _ => None,
            };
        }
        let mut outline = Outline::default();
        outline.defs = defs;
        outline.syntax_errors = errors
            .into_iter()
            .map(|(range, message)| SyntaxError::new(range, message))
            .collect();
        outline
    })
}

/// The defs of a parsed file, in source order, parents first. `syntax_error` and `marker` are not filled in yet.
fn walk(p: &Parsed<'_>) -> Vec<Def> {
    let mut walker = Walker {
        p,
        stack: Vec::new(),
        defs: Vec::new(),
    };
    walker.statements(&p.statements(p.root()));
    walker.defs
}

/// Is `def` a def of this text: one with the same qualname and the same range? A def from the outline of another text (an older version of
/// the file) is not, even when a def of that name is still there: its offsets point somewhere else now.
pub(crate) fn has_def(p: &Parsed<'_>, def: &Def) -> bool {
    walk(p)
        .iter()
        .any(|d| d.range == def.range && d.qualname == def.qualname)
}

/// Does a def start at byte `start`?
pub(crate) fn starts_def(p: &Parsed<'_>, start: u32) -> bool {
    walk(p).iter().any(|d| d.range.start == start)
}

/// Does a parse error touch a def? "When unsure, flag more": an empty error range counts as one byte, so an error at a def's first byte is in it.
fn overlaps(error: Span, def: Span) -> bool {
    error.start < def.end && error.end.max(error.start + 1) > def.start
}

/// An enclosing def while the walk is inside it.
struct Frame {
    qualname: String,
    /// Index into `Walker::defs`; `None` for a name that is not a def (an object literal that holds methods).
    index: Option<usize>,
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
        let qualname = self.qualname_of(&e.name);
        let mut def = Def::new(qualname.clone(), e.name, e.kind, e.range, e.name_range);
        def.parent = self.stack.iter().rev().find_map(|f| f.index);
        def.params = e.params;
        self.defs.push(def);
        self.stack.push(Frame {
            qualname,
            index: Some(self.defs.len() - 1),
            is_function: e.is_function,
        });
    }

    /// Open a name that is not a def, so that what is inside is named under it: `api` in `const api = { load() {} }`.
    fn enter_container(&mut self, name: &str) {
        let qualname = self.qualname_of(name);
        self.stack.push(Frame {
            qualname,
            index: None,
            is_function: false,
        });
    }

    fn qualname_of(&self, name: &str) -> String {
        match self.stack.last() {
            None => name.to_owned(),
            Some(parent) if parent.is_function => format!("{}.<locals>.{name}", parent.qualname),
            Some(parent) => format!("{}.{name}", parent.qualname),
        }
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
            NodeKind::ClassDeclaration | NodeKind::ClassExpression => self.class(n),
            NodeKind::FunctionExpression => self.function_expression(n),
            NodeKind::VariableStatement => self.variable_statement(n),
            NodeKind::ObjectLiteral => self.object_literal(n, None),
            NodeKind::InterfaceDeclaration => self.interface(n),
            NodeKind::EnumDeclaration => self.leaf(n, DefKind::Enum),
            NodeKind::TypeAliasDeclaration => self.leaf(n, DefKind::Type),
            NodeKind::ModuleDeclaration => self.module(n),
            NodeKind::ExportAssignment => match self.p.expression(n) {
                // `export default { load() {} }`: the literal is named like a default function
                Some(e) if self.p.kind(e) == NodeKind::ObjectLiteral => {
                    self.object_literal(e, Some("default".to_owned()));
                }
                Some(e) => self.visit(e),
                None => {}
            },
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

    /// The text of a name token as written: `f`, `#secret`, `"my-key"`, `[Symbol.iterator]`. One exception: a string that is a valid
    /// identifier loses its quotes, because `"load"() {}` and `load() {}` declare the same member. Any other string keeps them, so
    /// `"a.b"` cannot be mistaken for a path.
    fn name_text(&self, name: RawNode) -> Option<String> {
        let written = self.p.slice(self.p.span(name))?;
        if self.p.kind(name) == NodeKind::StringLiteral {
            let inner = written
                .strip_prefix(['"', '\''])
                .and_then(|rest| rest.strip_suffix(['"', '\'']));
            if let Some(inner) = inner.filter(|inner| self.p.is_identifier(inner)) {
                return Some(inner.to_owned());
            }
        }
        Some(written.to_owned())
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
            // an anonymous class expression adds nothing to a name: its methods are named from what is around it
            self.members(&self.p.members(n));
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
                NodeKind::PropertyDeclaration => {
                    self.property(m);
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

    /// A named function expression is a def named by its own name: `run(function step() {})`. An anonymous one is not: its body belongs to
    /// the def around it.
    fn function_expression(&mut self, n: RawNode) {
        let Some((name, name_range)) = self.name_of(n) else {
            self.function_body(n);
            return;
        };
        self.enter(Entry {
            name,
            kind: DefKind::Function,
            is_function: true,
            range: self.p.span(n),
            name_range,
            params: self.p.params(n),
        });
        self.function_body(n);
        self.leave();
    }

    /// An arrow, function expression or class expression with no name of its own (the only kind a variable can name).
    fn is_anonymous_callable(&self, n: RawNode) -> bool {
        matches!(
            self.p.kind(n),
            NodeKind::ArrowFunction | NodeKind::FunctionExpression | NodeKind::ClassExpression
        ) && self.p.name(n).is_none()
    }

    /// Define `init`, an anonymous arrow / function / class expression, under `name`. `range` is the whole def.
    fn named_expression(&mut self, init: RawNode, name: RawNode, is_member: bool, range: Span) {
        let Some(text) = self.name_text(name) else {
            return;
        };
        let name_range = self.p.span(name);
        if self.p.kind(init) == NodeKind::ClassExpression {
            self.enter(Entry {
                name: text,
                kind: DefKind::Class,
                is_function: false,
                range,
                name_range,
                params: Vec::new(),
            });
            self.members(&self.p.members(init));
        } else {
            self.enter(Entry {
                name: text,
                kind: if is_member {
                    DefKind::Method
                } else {
                    DefKind::Function
                },
                is_function: true,
                range,
                name_range,
                params: self.p.params(init),
            });
            self.function_body(init);
        }
        self.leave();
    }

    /// `const f = () => {}`: the def is the whole statement when it declares one variable, otherwise just the declarator. Only `const`
    /// names a function: a `let` can be reassigned, so its name is not stable (the engine's rule too).
    fn variable_statement(&mut self, n: RawNode) {
        let declarations = self.p.declarations(n);
        let single = declarations.len() == 1;
        for d in declarations {
            let range = if single {
                self.p.span(n)
            } else {
                self.p.span(d)
            };
            let name = self
                .p
                .name(d)
                .filter(|&nm| self.p.kind(nm) == NodeKind::Identifier);
            match (self.p.initializer(d), name) {
                (Some(init), Some(nm))
                    if self.p.is_const(d) && self.is_anonymous_callable(init) =>
                {
                    self.named_expression(init, nm, false, range);
                }
                (Some(init), _) if self.p.kind(init) == NodeKind::ObjectLiteral => {
                    let name = name.and_then(|nm| self.name_text(nm));
                    self.object_literal(init, name);
                }
                (Some(init), _) => self.visit(init),
                (None, _) => {}
            }
        }
    }

    /// `handle = () => {}` in a class: a method, named like one. Only a plain identifier names it (the engine's rule: a private or
    /// computed name is not accepted).
    fn property(&mut self, n: RawNode) {
        let Some(init) = self.p.initializer(n) else {
            return;
        };
        let name = self
            .p
            .name(n)
            .filter(|&nm| self.p.kind(nm) == NodeKind::Identifier);
        match name {
            Some(nm) if self.is_anonymous_callable(init) => {
                let range = self.p.span(n);
                self.named_expression(init, nm, true, range);
            }
            _ if self.p.kind(init) == NodeKind::ObjectLiteral => {
                let name = self.p.name(n).and_then(|nm| self.name_text(nm));
                self.object_literal(init, name);
            }
            _ => self.visit(init),
        }
    }

    /// The methods and accessors of an object literal are defs (the engine resolves calls to them). The literal is named by the variable
    /// or property that holds it, so `api.load`; an anonymous one adds nothing. A property holding an arrow or function is not a def, for now.
    fn object_literal(&mut self, n: RawNode, name: Option<String>) {
        if let Some(name) = &name {
            self.enter_container(name);
        }
        for m in self.p.members(n) {
            match self.p.kind(m) {
                NodeKind::MethodDeclaration => {
                    self.method_group(&[m]);
                }
                NodeKind::GetAccessor | NodeKind::SetAccessor => self.accessor(m),
                NodeKind::PropertyAssignment => self.property_assignment(m),
                _ => self.visit(m),
            }
        }
        if name.is_some() {
            self.leave();
        }
    }

    /// `key: value` in an object literal: a nested literal is named by the key; anything else is searched for named expressions.
    fn property_assignment(&mut self, n: RawNode) {
        let Some(init) = self.p.initializer(n) else {
            return;
        };
        if self.p.kind(init) == NodeKind::ObjectLiteral {
            let name = self.p.name(n).and_then(|nm| self.name_text(nm));
            self.object_literal(init, name);
        } else {
            self.visit(init);
        }
    }

    /// A def with nothing to look inside: an enum, a type alias.
    fn leaf(&mut self, n: RawNode, kind: DefKind) {
        let Some((name, name_range)) = self.name_of(n) else {
            return;
        };
        self.enter(Entry {
            name,
            kind,
            is_function: false,
            range: self.p.span(n),
            name_range,
            params: Vec::new(),
        });
        self.leave();
    }

    /// An interface and its method signatures. Overload signatures have no implementation here, so each one is its own def with the same
    /// qualname (the core's `occurrence` tells them apart, as for duplicates in Python). Properties and call signatures are not defs.
    fn interface(&mut self, n: RawNode) {
        let Some((name, name_range)) = self.name_of(n) else {
            return;
        };
        self.enter(Entry {
            name,
            kind: DefKind::Interface,
            is_function: false,
            range: self.p.span(n),
            name_range,
            params: Vec::new(),
        });
        for m in self.p.members(n) {
            if self.p.kind(m) != NodeKind::MethodSignature {
                continue;
            }
            let Some((name, name_range)) = self.name_of(m) else {
                continue;
            };
            self.enter(Entry {
                name,
                kind: DefKind::Method,
                is_function: true,
                range: self.p.span(m),
                name_range,
                params: self.p.params(m),
            });
            self.leave();
        }
        self.leave();
    }

    /// `namespace N {}`, `module M {}`, `declare module "x" {}`, `declare global {}`. `namespace A.B {}` is two levels, `A` and `A.B`.
    /// A string name that is not an identifier keeps its quotes (`name_text`), so `"a/b".f` cannot be mistaken for a path.
    fn module(&mut self, n: RawNode) {
        let Some((name, name_range)) = self.name_of(n) else {
            return;
        };
        self.enter(Entry {
            name,
            kind: DefKind::Namespace,
            is_function: false,
            range: self.p.span(n),
            name_range,
            params: Vec::new(),
        });
        if let Some(body) = self.p.body(n) {
            self.visit(body);
        }
        self.leave();
    }
}
