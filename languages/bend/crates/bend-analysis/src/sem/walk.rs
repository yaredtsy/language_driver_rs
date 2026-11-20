//! What the body of each declaration says, before any name is looked up (docs/08-bend/03 §3.2): the places a name is called, handed over or built, the
//! sugars that stand for a call, and the terms that are skipped because they are types. A name is not resolved here (that needs the book), but whether it
//! is **bound** is decided here, from the scopes of the body, first as `parse_var` does: a parameter, a `let` or a `case` pattern's name, a lambda's binder,
//! a do's name, a rewrite's named equation, an `All` binder. Nothing is pre-computed in the parser: the scope is a stack the walk pushes and pops.
//!
//! Everything returned is owned data with absolute byte spans into the text.

use analyzer_core::text::Span;
use bend_parser::{Body, Decl, DefBody, DoKind, Op, Term, TermKind};

/// One place in a body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) span: Span,
    pub(crate) what: What,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum What {
    /// `f(a)`: the callee is a name that is not bound. The span is the name.
    Call { name: String },
    /// The callee is a bound name, or no name at all (`f(a)(b)`, `(x => ..)(a)`): the value called is not a def. The span is the callee.
    Opaque,
    /// A bare name that is not bound, as a direct argument of a call or a direct field of a constructor: handed over, if it is a def.
    Passed { name: String },
    /// A bare name that is not bound anywhere else (a call's result, a let's value, a list's item): never drawn, counted if it is a project def.
    Deep { name: String },
    /// `K{..}`: a constructor. The span is the name.
    Ctr { name: String },
    /// A sugar that stands for a call. The span is the whole sugar.
    Sugar(Sugar),
    /// `&&`, `||`, `++`, `<>`, `&`, `|`: Base's, fixed, never a site. Counted `sugar_base`.
    BaseFixed,
    /// A term that stands for a Base def or constructor and is neither counted nor a site: `3n+p`, `{a != b : T}`, a do block's closing value, `&x: A -> B`.
    Data,
    /// A term that is a type: skipped. Counted `type_position`.
    TypePosition,
}

/// The def a sugar calls, as far as the text says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Sugar {
    /// `Array.get`: this file's def of that name, else Base's (`parse_own`, no alias).
    Own(String),
    /// `T.add`, `M.bind`: the name as `parse_reso` is given it, aliases included.
    Named(String),
    /// The type (or monad) is a bound name: which def runs is not in the text.
    Bound,
    /// No type after the `:`, or not a name: `bend` refuses the program.
    Unknown,
}

/// The items of one declaration, in source order (by start, then end).
#[derive(Clone, Debug)]
pub(crate) struct DefWalk {
    /// Where the declaration's name is: how a def of the outline finds its walk.
    pub(crate) name_span: Span,
    pub(crate) items: Vec<Item>,
}

/// The namespace of the operators in reach: what the `: T` of the nearest group says.
#[derive(Clone, Debug)]
enum Ns {
    /// A group with no `: T`.
    None,
    Name {
        name: String,
        bound: bool,
    },
    /// A `: T` whose head is not a name.
    Bad,
}

struct Walker {
    items: Vec<Item>,
    scope: Vec<String>,
}

/// The sugar of an infix operator that is a def of the namespace (`bend.ts` `INFIX`: the entries whose name starts with a dot).
fn verb(op: Op) -> Option<&'static str> {
    Some(match op {
        Op::Add => "add",
        Op::Sub => "sub",
        Op::Mul => "mul",
        Op::Div => "div",
        Op::Mod => "mod",
        Op::BitOr => "or",
        Op::BitXor => "xor",
        Op::BitAnd => "and",
        Op::Shl => "shln",
        Op::Shr => "shrn",
        Op::Lt => "is_lt",
        Op::Le => "is_le",
        Op::Gt => "is_gt",
        Op::Ge => "is_ge",
        _ => return None,
    })
}

/// A group `(t)` with no `: T` is the term inside it (`bend.ts` `parse_term_tup` returns the body): look through it.
fn strip_group(term: &Term) -> &Term {
    match &term.kind {
        TermKind::Group { body, ns: None } => match &**body {
            Body::Term(inner) => strip_group(inner),
            _ => term,
        },
        _ => term,
    }
}

impl Walker {
    fn push(&mut self, span: Span, what: What) {
        self.items.push(Item { span, what });
    }

    /// `parse_var`: a name is looked up in the binders first, and a dotted name is never one.
    fn bound(&self, name: &str) -> bool {
        !name.contains('.') && self.scope.iter().any(|bound| bound == name)
    }

    fn skip(&mut self, term: &Term) {
        self.push(term.span, What::TypePosition);
    }

    /// The names a pattern binds: a name, `+x`, the arguments of `K{..}`, the rest of `3n+p`, both sides of `h <> t`, the items of a list or a tuple.
    fn binders(&mut self, pattern: &Term) {
        match &pattern.kind {
            TermKind::Name(name) if !name.contains('.') => self.scope.push(name.clone()),
            TermKind::Plus(inner) => self.binders(inner),
            TermKind::Ctr { args, .. } => args.iter().for_each(|a| self.binders(a)),
            TermKind::NatPlus { rest, .. } => self.binders(rest),
            TermKind::Op { lhs, rhs, .. } => {
                self.binders(lhs);
                self.binders(rhs);
            }
            TermKind::List(items) => items.iter().for_each(|i| self.binders(i)),
            TermKind::Group { body, .. } => self.body_binders(body),
            TermKind::Tuple { items, .. } => items.iter().for_each(|i| self.body_binders(i)),
            _ => {}
        }
    }

    fn body_binders(&mut self, body: &Body) {
        if let Body::Term(term) = body {
            self.binders(term);
        }
    }

    fn body(&mut self, body: &Body, ns: &Ns) {
        match body {
            Body::Term(term) => self.term(term, ns),
            Body::Match(m) => {
                for scrutinee in &m.scrutinees {
                    self.term(scrutinee, &Ns::None);
                }
                for case in &m.cases {
                    let mark = self.scope.len();
                    for pattern in &case.patterns {
                        self.binders(pattern);
                    }
                    self.body(&case.body, &Ns::None);
                    self.scope.truncate(mark);
                }
            }
            Body::Let(l) => {
                // the values are read before the names are bound: `x = f(x)` calls with the outer `x`
                for value in &l.values {
                    self.term(value, &Ns::None);
                }
                if let Some(ty) = &l.ty {
                    self.skip(ty);
                }
                let mark = self.scope.len();
                for pattern in &l.pats {
                    self.binders(pattern);
                }
                self.body(&l.rest, ns);
                self.scope.truncate(mark);
            }
            _ => {}
        }
    }

    /// A direct argument of a call, or a direct field of a constructor, or the value of a `do` bind, exec or step (a direct argument of the `bind` the sugar
    /// stands for): a bare name that is not bound is handed over; anything else is walked.
    fn passable(&mut self, term: &Term) {
        let inner = strip_group(term);
        match &inner.kind {
            TermKind::Name(name) => {
                if !self.bound(name) {
                    self.push(inner.span, What::Passed { name: name.clone() });
                }
            }
            _ => self.term(term, &Ns::None),
        }
    }

    /// The namespace a `: T` gives: the name at the head of `T` (`Vec2`, `M.Vec`, `Vec<A>`, `Vec(A)`).
    fn ns_of(&self, ty: &Term) -> Ns {
        match &strip_group(ty).kind {
            TermKind::Name(name) => Ns::Name {
                bound: self.bound(name),
                name: name.clone(),
            },
            TermKind::TypeApp { name, .. } => Ns::Name {
                bound: self.bound(&name.text),
                name: name.text.clone(),
            },
            TermKind::Call { callee, .. } => self.ns_of(callee),
            _ => Ns::Bad,
        }
    }

    fn sugar_of(ns: &Ns, verb: &str) -> Sugar {
        match ns {
            Ns::Name { name, bound: false } => Sugar::Named(format!("{name}.{verb}")),
            Ns::Name { bound: true, .. } => Sugar::Bound,
            Ns::None | Ns::Bad => Sugar::Unknown,
        }
    }

    fn term(&mut self, term: &Term, ns: &Ns) {
        match &term.kind {
            TermKind::Name(name) => {
                if !self.bound(name) {
                    self.push(term.span, What::Deep { name: name.clone() });
                }
            }
            TermKind::Type
            | TermKind::Data
            | TermKind::Quant
            | TermKind::Qua(_)
            | TermKind::Hole(_)
            | TermKind::Lit(_)
            | TermKind::Refl => {}
            TermKind::Kind(inner) => self.skip(inner),
            TermKind::NatPlus { rest, .. } => {
                self.push(term.span, What::Data);
                self.term(rest, &Ns::None);
            }
            TermKind::List(items) => items.iter().for_each(|i| self.term(i, &Ns::None)),
            TermKind::ArrayNew { elem, ty, size } => {
                self.push(term.span, What::Sugar(Sugar::Own("Array.new".to_owned())));
                self.term(elem, &Ns::None);
                self.skip(ty);
                match size {
                    bend_parser::ArraySize::Count(n) | bend_parser::ArraySize::Depth(n) => {
                        self.term(n, &Ns::None);
                    }
                    _ => {}
                }
            }
            TermKind::Ctr { name, args } => {
                self.push(
                    name.span,
                    What::Ctr {
                        name: name.text.clone(),
                    },
                );
                args.iter().for_each(|a| self.passable(a));
            }
            TermKind::TypeApp { args, .. } => args.iter().for_each(|a| self.skip(a)),
            TermKind::Plus(inner) => self.term(inner, &Ns::None),
            TermKind::Call { callee, args, .. } => {
                let head = strip_group(callee);
                match &head.kind {
                    TermKind::Name(name) if !self.bound(name) => {
                        self.push(head.span, What::Call { name: name.clone() });
                    }
                    TermKind::Name(_) => self.push(head.span, What::Opaque),
                    _ => {
                        self.push(head.span, What::Opaque);
                        self.term(callee, &Ns::None);
                    }
                }
                args.iter().for_each(|a| self.passable(&a.term));
            }
            TermKind::Index {
                array,
                index,
                write,
            } => {
                let name = if write.is_some() {
                    "Array.set"
                } else {
                    "Array.get"
                };
                self.push(term.span, What::Sugar(Sugar::Own(name.to_owned())));
                self.term(array, &Ns::None);
                // `parse_term_ns(p, ix, U32)`: the operators of the index are U32's
                let u32 = Ns::Name {
                    name: "U32".to_owned(),
                    bound: false,
                };
                self.term(index, &u32);
                if let Some(value) = write {
                    self.term(value, &Ns::None);
                }
            }
            TermKind::Op {
                lhs,
                op,
                op_span,
                rhs,
            } => {
                // the operator token, where `bend` puts its own `Ref` (`Ref(f, op === "&" || op === "|" ? s : t)`): the whole term for `&` and `|`
                let at = op_span;
                // the namespace reaches the operands of an operator that takes it, and of `&&`, `||` and `++`, which `parse_term_ns` looks through
                let through = match op {
                    Op::And | Op::Or | Op::Append => {
                        self.push(*at, What::BaseFixed);
                        ns.clone()
                    }
                    Op::Pair | Op::Either => {
                        self.push(term.span, What::BaseFixed);
                        Ns::None
                    }
                    Op::Cons => {
                        self.push(*at, What::BaseFixed);
                        Ns::None
                    }
                    Op::Min => Ns::None,
                    other => {
                        let sugar = match verb(*other) {
                            Some(verb) => Self::sugar_of(ns, verb),
                            None => Sugar::Unknown,
                        };
                        self.push(*at, What::Sugar(sugar));
                        ns.clone()
                    }
                };
                self.term(lhs, &through);
                self.term(rhs, &through);
            }
            TermKind::Arrow { from, to } => {
                self.term(from, &Ns::None);
                self.term(to, &Ns::None);
            }
            TermKind::All {
                exists,
                name,
                domain,
                body,
                ..
            } => {
                if *exists {
                    self.push(term.span, What::Data);
                }
                self.skip(domain);
                self.scope.push(name.text.clone());
                self.term(body, &Ns::None);
                self.scope.pop();
            }
            TermKind::Lam {
                binder,
                annot,
                body,
                ..
            } => {
                if let Some(annot) = annot {
                    self.skip(annot);
                }
                self.scope.push(binder.text.clone());
                self.body(body, &Ns::None);
                self.scope.pop();
            }
            TermKind::LamMatch { arms, tail } => {
                arms.iter().for_each(|arm| self.term(&arm.body, &Ns::None));
                if let Some(tail) = tail {
                    self.term(tail, &Ns::None);
                }
            }
            TermKind::Annot { term: inner, ty } => {
                self.skip(ty);
                self.term(inner, &Ns::None);
            }
            TermKind::Eq {
                lhs,
                rhs,
                ty,
                negated,
            } => {
                self.skip(ty);
                if *negated {
                    self.push(term.span, What::Data);
                }
                self.term(lhs, &Ns::None);
                self.term(rhs, &Ns::None);
            }
            TermKind::Rewrite {
                eq,
                named,
                motive,
                rest,
            } => {
                self.term(eq, &Ns::None);
                self.skip(motive);
                let mark = self.scope.len();
                if let Some(named) = named {
                    self.scope.push(named.text.clone());
                }
                self.body(rest, &Ns::None);
                self.scope.truncate(mark);
            }
            // a group with no `: T` is the term inside it, so the namespace of the group around it reaches through
            TermKind::Group { body, ns: own } => {
                let ns = match own {
                    Some(ty) => {
                        self.skip(ty);
                        self.ns_of(ty)
                    }
                    None => ns.clone(),
                };
                self.body(body, &ns);
            }
            // `(a, b : T)`: the `: T` is read after the last item
            TermKind::Tuple { items, ns: own } => {
                let last = match own {
                    Some(ty) => {
                        self.skip(ty);
                        self.ns_of(ty)
                    }
                    None => Ns::None,
                };
                for (at, item) in items.iter().enumerate() {
                    self.body(
                        item,
                        if at + 1 == items.len() {
                            &last
                        } else {
                            &Ns::None
                        },
                    );
                }
            }
            TermKind::Do(d) => {
                let bound = self.bound(&d.monad.text);
                let sugar = |verb: &str| {
                    if bound {
                        Sugar::Bound
                    } else {
                        Sugar::Named(format!("{}.{verb}", d.monad.text))
                    }
                };
                d.args.iter().for_each(|a| self.skip(a));
                let mark = self.scope.len();
                for stmt in &d.stmts {
                    match &stmt.kind {
                        DoKind::Let {
                            name, ty, value, ..
                        } => {
                            self.skip(ty);
                            self.term(value, &Ns::None);
                            self.scope.push(name.text.clone());
                        }
                        DoKind::Bind {
                            name, ty, value, ..
                        } => {
                            self.push(stmt.span, What::Sugar(sugar("bind")));
                            self.skip(ty);
                            self.passable(value);
                            self.scope.push(name.text.clone());
                        }
                        DoKind::Exec { ty, value } => {
                            self.push(stmt.span, What::Sugar(sugar("bind")));
                            self.skip(ty);
                            self.passable(value);
                        }
                        DoKind::Step(step) => {
                            self.push(stmt.span, What::Sugar(sugar("bind")));
                            self.passable(step);
                        }
                        DoKind::Return(value) => {
                            self.push(stmt.span, What::Sugar(sugar("pure")));
                            self.term(value, &Ns::None);
                        }
                        DoKind::Value(value) => {
                            // `bend` annotates the final value with the monad: a type, not a call
                            self.push(value.span, What::Data);
                            self.term(value, &Ns::None);
                        }
                        _ => {}
                    }
                }
                self.scope.truncate(mark);
            }
            _ => {}
        }
    }
}

fn walk_decl(decl: &Decl) -> Option<DefWalk> {
    let mut walker = Walker {
        items: Vec::new(),
        scope: Vec::new(),
    };
    match decl {
        Decl::Def(def) => {
            for bind in &def.params {
                if let Some(ty) = &bind.ty {
                    walker.skip(ty);
                }
                walker.scope.push(bind.name.text.clone());
            }
            if let Some(ret) = &def.ret {
                walker.skip(ret);
            }
            if let Some(DefBody::Code(body)) = &def.body {
                walker.body(body, &Ns::None);
            }
        }
        // a law is a def with no code: its clauses and its claim are all types
        Decl::Law(law) => {
            for clause in &law.clauses {
                walker.skip(&clause.ty);
                if let Some(guard) = &clause.where_ {
                    walker.skip(guard);
                }
            }
            if let Some(claim) = &law.claim {
                walker.push(claim.span(), What::TypePosition);
            }
        }
        Decl::Type(_) => {}
        _ => return None,
    }
    walker.items.sort_by_key(|i| (i.span.start, i.span.end));
    Some(DefWalk {
        name_span: decl.name().span,
        items: walker.items,
    })
}

/// The walk of every declaration of `text`, in source order. Never fails: a broken declaration is walked as far as the parser read it.
pub(crate) fn walks(text: &str) -> Vec<DefWalk> {
    bend_parser::parse(text)
        .decls
        .iter()
        .filter_map(walk_decl)
        .collect()
}

/// The walk of the declaration whose name is at `name_span`.
pub(crate) fn of(walks: &[DefWalk], name_span: Span) -> Option<&DefWalk> {
    let at = walks
        .binary_search_by_key(&name_span.start, |w| w.name_span.start)
        .ok()?;
    walks.get(at).filter(|w| w.name_span == name_span)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The walk of the one def of `text` as `(text at the span, what)` pairs, in order.
    fn items(text: &str) -> Vec<(String, String)> {
        let walks = walks(text);
        assert_eq!(walks.len(), 1, "one declaration: {text}");
        walks[0]
            .items
            .iter()
            .map(|i| {
                let what = match &i.what {
                    What::Call { name } => format!("call {name}"),
                    What::Opaque => "opaque".to_owned(),
                    What::Passed { name } => format!("passed {name}"),
                    What::Deep { name } => format!("deep {name}"),
                    What::Ctr { name } => format!("ctr {name}"),
                    What::Sugar(s) => format!("sugar {s:?}"),
                    What::BaseFixed => "base-fixed".to_owned(),
                    What::Data => "data".to_owned(),
                    What::TypePosition => "type".to_owned(),
                };
                (
                    text[i.span.start as usize..i.span.end as usize].to_owned(),
                    what,
                )
            })
            .collect()
    }

    fn whats(text: &str) -> Vec<String> {
        items(text).into_iter().map(|(_, w)| w).collect()
    }

    #[test]
    fn a_parameter_is_bound_and_a_def_of_the_same_name_is_not_called() {
        // `f` is a parameter: the call is on a value, whatever defs there are
        assert_eq!(
            whats("def g(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n"),
            ["type", "type", "type", "opaque"]
        );
        assert_eq!(
            whats("def g(x: U32) -> U32:\n  f(x)\n"),
            ["type", "type", "call f"]
        );
    }

    #[test]
    fn a_dotted_name_is_never_bound() {
        assert_eq!(
            whats("def g(M.f: U32) -> U32:\n  M.f(1)\n"),
            ["type", "type", "call M.f"]
        );
    }

    #[test]
    fn a_let_binds_after_its_value_and_a_case_for_its_body() {
        // the value `f(x)` is read before `f` is bound; the next `f(1)` is on the binder
        assert_eq!(
            whats("def g(x: U32) -> U32:\n  f = f(x)\n  f(1)\n"),
            ["type", "type", "call f", "opaque"]
        );
        assert_eq!(
            whats(
                "def g(xs: List) -> U32:\n  match xs:\n    case Cons{h, t}:\n      h(1)\n    case Nil{}:\n      h(2)\n"
            ),
            ["type", "type", "opaque", "call h"]
        );
    }

    #[test]
    fn a_lambda_binder_is_in_scope_in_its_body_only() {
        assert_eq!(
            whats("def g() -> U32:\n  (f => f(1))(f)\n"),
            ["type", "opaque", "opaque", "passed f"]
        );
    }

    #[test]
    fn a_direct_argument_is_handed_over_and_a_deeper_name_is_not() {
        let found = items("def g(x: U32) -> U32:\n  h(a, [b], k(c), (d))\n");
        let found: Vec<(&str, &str)> = found
            .iter()
            .map(|(t, w)| (t.as_str(), w.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                ("U32", "type"),
                ("U32", "type"),
                ("h", "call h"),
                ("a", "passed a"),
                ("b", "deep b"),
                ("k", "call k"),
                ("c", "passed c"),
                ("d", "passed d"),
            ]
        );
    }

    #[test]
    fn the_callee_is_looked_for_through_a_group_and_a_call_is_opaque() {
        assert_eq!(whats("def g() -> U32:\n  (f)(1)\n"), ["type", "call f"]);
        assert_eq!(
            whats("def g() -> U32:\n  adder(2)(1)\n"),
            ["type", "call adder", "opaque"]
        );
    }

    #[test]
    fn a_constructor_hands_over_its_direct_fields() {
        assert_eq!(
            whats("def g() -> U32:\n  App{view, tick(1)}\n"),
            ["type", "ctr App", "passed view", "call tick"]
        );
    }

    #[test]
    fn an_operator_takes_the_type_after_the_colon() {
        assert_eq!(
            whats("def g(a: Vec2, b: Vec2) -> Vec2:\n  (a + b : Vec2)\n"),
            ["type", "type", "type", "sugar Named(\"Vec2.add\")", "type"]
        );
        // inside an operator the namespace is kept, through `&&`; a bare operator has none
        assert_eq!(
            whats("def g(a: U32, b: U32) -> Bool:\n  (a < b && b < a : U32)\n"),
            [
                "type",
                "type",
                "type",
                "sugar Named(\"U32.is_lt\")",
                "base-fixed",
                "sugar Named(\"U32.is_lt\")",
                "type"
            ]
        );
        assert_eq!(
            whats("def g(a: U32, b: U32) -> U32:\n  a + b\n"),
            ["type", "type", "type", "sugar Unknown"]
        );
        // a bound type is not a name to look up
        assert_eq!(
            whats("def g(-T: Data, a: T, b: T) -> T:\n  (a + b : T)\n"),
            ["type", "type", "type", "type", "sugar Bound", "type"]
        );
    }

    #[test]
    fn an_index_is_array_get_and_set_and_its_operators_are_u32s() {
        assert_eq!(
            whats("def g(a: Array, i: U32) -> U32:\n  a[i + 1]\n"),
            [
                "type",
                "type",
                "type",
                "sugar Own(\"Array.get\")",
                "sugar Named(\"U32.add\")"
            ]
        );
    }

    #[test]
    fn a_do_block_is_a_bind_per_statement_and_a_pure_per_return() {
        let found = whats(
            "def g() -> IO(U32):\n  do IO<U32>:\n    x : U32 <- a()\n    b(x)\n    return c(x)\n",
        );
        // a statement's own call starts where the statement does and ends sooner, so it comes first: `b(x)` before its bind
        assert_eq!(
            found,
            [
                "type",
                "type",
                "sugar Named(\"IO.bind\")",
                "type",
                "call a",
                "call b",
                "sugar Named(\"IO.bind\")",
                "sugar Named(\"IO.pure\")",
                "call c"
            ]
        );
    }

    #[test]
    fn types_are_skipped_and_a_laws_claim_is_one() {
        assert_eq!(
            whats("def g(x: List<Nat>) -> Or(A(x), B(x)):\n  y : T = f(x)\n  y\n"),
            ["type", "type", "type", "call f"]
        );
        assert_eq!(
            whats("law l:\n  for x: Nat\n  Eq(x, f(x))\n"),
            ["type", "type"]
        );
    }

    #[test]
    fn a_rewrite_walks_its_equation_and_skips_its_motive() {
        assert_eq!(
            whats("def g(x: Nat):\n  %lemma(x) : P(x)\n  {==}\n"),
            ["type", "call lemma", "type"]
        );
    }
}
