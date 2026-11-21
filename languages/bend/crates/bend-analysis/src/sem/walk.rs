//! What the body of each declaration says, before any name is looked up (docs/08-bend/03 §3.2): the places a name is called, handed over or built, the
//! sugars that stand for a call, and the terms that are skipped because they are types. A name is not resolved here (that needs the book), but whether it
//! is **bound** is decided here, from the scopes of the body, first as `parse_var` does: a parameter, a `let` or a `case` pattern's name, a lambda's binder,
//! a do's name, a rewrite's named equation, an `All` binder. Nothing is pre-computed in the parser: the scope is a stack the walk pushes and pops.
//!
//! Everything returned is owned data with absolute byte spans into the text.

use analyzer_core::text::Span;
use bend_parser::{Body, Decl, DefBody, DoKind, Op, Term, TermKind};

use super::flow::{Expr, Flow, LamFacts, Pat};

/// One place in a body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) span: Span,
    pub(crate) what: What,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum What {
    /// `f(a)`: the callee is a name that is not bound. The span is the name. `args` is what each argument is, in the order written.
    Call { name: String, args: Vec<ArgShape> },
    /// The callee is a bound name, or no name at all (`f(a)(b)`, `(x => ..)(a)`): the value called is not a def. The span is the callee. `param` is the
    /// position in the def's written parameter list when the callee is one of the def's own parameters (the innermost binder of that name); `args` as for `Call`.
    Opaque {
        param: Option<u32>,
        args: Vec<ArgShape>,
    },
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

/// What one argument of a call is, as far as the text says: owned data, read through parentheses as `passable` does. The argument at position `i` of the written
/// list is the callee's parameter `i` (an erased `-A: Type` is passed explicitly, so there is no mapping to make).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ArgShape {
    /// A bare name that is not bound: a def, a type or nothing, as the book says.
    Name(String),
    /// A bare name bound to the def's own parameter at this position (the innermost binder of the name wins).
    Param(u32),
    /// A bare name bound to anything else: a `let`, a `case` pattern, a lambda's binder, a do's name, an `@` binder, a rewrite's equation.
    Local,
    /// A lambda (`x => ..`, `\{..}`), the `n`-th the walk of this body enters (from 0).
    Lambda(u32),
    /// Anything else: a call, a constructor, a literal, an operator.
    Other,
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
    /// The same body as a tree, whose sites are `items` by index (`flow.rs`). Read by the evaluator (V2); until then only the tests read it.
    #[allow(dead_code)]
    pub(crate) flow: Flow,
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

/// What a name is, in the scopes of the body at the place it is read.
enum Binding {
    Unbound,
    /// The def's own parameter at this position of its written list (its binder number is the position).
    Param(u32),
    /// Any other binder, by number.
    Local(u32),
}

impl Binding {
    /// The binder's number, when a binder binds the name.
    fn number(&self) -> Option<u32> {
        match self {
            Binding::Unbound => None,
            Binding::Param(at) => Some(*at),
            Binding::Local(n) => Some(*n),
        }
    }
}

/// A binder in reach: its name, its number, and the position of the def's parameter it is (`None` for any other binder).
struct Bound {
    name: String,
    number: u32,
    param: Option<u32>,
}

struct Walker {
    items: Vec<Item>,
    /// The binders in reach, innermost last.
    scope: Vec<Bound>,
    /// Lambdas entered so far: the number of the next one.
    lambdas: u32,
    /// What each lambda entered says of itself, by number (`Flow::lambdas`).
    lams: Vec<LamFacts>,
    /// The number the next binder takes: the parameters are `0..params`.
    next_binder: u32,
    /// Every binder read so far, in the order it was read: what a lambda's `reads` are cut from.
    uses: Vec<u32>,
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
    /// A place in the body. Its index is what a node of the `Flow` carries (before the items are sorted by place).
    fn push(&mut self, span: Span, what: What) -> u32 {
        self.items.push(Item { span, what });
        (self.items.len() - 1) as u32
    }

    /// `parse_var`: a name is looked up in the binders first, and a dotted name is never one. The innermost binder of a name decides what it is.
    fn binding(&self, name: &str) -> Binding {
        if name.contains('.') {
            return Binding::Unbound;
        }
        match self.scope.iter().rev().find(|b| b.name == name) {
            None => Binding::Unbound,
            Some(Bound {
                param: Some(at), ..
            }) => Binding::Param(*at),
            Some(Bound { number, .. }) => Binding::Local(*number),
        }
    }

    fn bound(&self, name: &str) -> bool {
        !matches!(self.binding(name), Binding::Unbound)
    }

    /// A binder that is not a parameter of the def: its number.
    fn bind(&mut self, name: &str) -> u32 {
        let number = self.fresh();
        self.scope.push(Bound {
            name: name.to_owned(),
            number,
            param: None,
        });
        number
    }

    /// A number for a binder that has no name in the text (the argument of `\{ .. }`, the `_` of a do statement).
    fn fresh(&mut self) -> u32 {
        self.next_binder += 1;
        self.next_binder - 1
    }

    /// A read of the binder `number`.
    fn var(&mut self, number: u32) -> Expr {
        self.uses.push(number);
        Expr::Var(number)
    }

    fn skip(&mut self, term: &Term) -> Expr {
        self.push(term.span, What::TypePosition);
        Expr::Data
    }

    /// The names a pattern binds: a name, `+x`, the arguments of `K{..}`, the rest of `3n+p`, both sides of `h <> t`, the items of a list or a tuple.
    fn binders(&mut self, pattern: &Term) -> Pat {
        match &pattern.kind {
            TermKind::Name(name) if !name.contains('.') => Pat::Bind(self.bind(name)),
            TermKind::Plus(inner) => self.binders(inner),
            TermKind::Ctr { name, args } => Pat::Ctr {
                name: name.text.clone(),
                fields: args.iter().map(|a| self.binders(a)).collect(),
            },
            TermKind::NatPlus { rest, .. } => self.opaque(|w| {
                w.binders(rest);
            }),
            TermKind::Op { lhs, rhs, .. } => self.opaque(|w| {
                w.binders(lhs);
                w.binders(rhs);
            }),
            TermKind::List(items) => self.opaque(|w| {
                items.iter().for_each(|i| drop(w.binders(i)));
            }),
            TermKind::Group { body, .. } => self.body_binders(body),
            TermKind::Tuple { items, .. } => self.opaque(|w| {
                items.iter().for_each(|i| drop(w.body_binders(i)));
            }),
            _ => Pat::Skip,
        }
    }

    /// A pattern of several parts the layer does not follow: the binders `f` made, which are the numbers it took.
    fn opaque(&mut self, f: impl FnOnce(&mut Self)) -> Pat {
        let first = self.next_binder;
        f(self);
        Pat::Opaque((first..self.next_binder).collect())
    }

    fn body_binders(&mut self, body: &Body) -> Pat {
        match body {
            Body::Term(term) => self.binders(term),
            _ => Pat::Skip,
        }
    }

    fn body(&mut self, body: &Body, ns: &Ns) -> Expr {
        match body {
            Body::Term(term) => self.term(term, ns),
            Body::Match(m) => {
                let on = m
                    .scrutinees
                    .iter()
                    .map(|scrutinee| self.term(scrutinee, &Ns::None))
                    .collect();
                let mut cases = Vec::new();
                for case in &m.cases {
                    let mark = self.scope.len();
                    let pats: Box<[Pat]> = case.patterns.iter().map(|p| self.binders(p)).collect();
                    let body = self.body(&case.body, &Ns::None);
                    self.scope.truncate(mark);
                    cases.push((pats, body));
                }
                Expr::Match {
                    on,
                    cases: cases.into(),
                }
            }
            Body::Let(l) => {
                // the values are read before the names are bound: `x = f(x)` calls with the outer `x`
                let values = l
                    .values
                    .iter()
                    .map(|value| self.term(value, &Ns::None))
                    .collect();
                if let Some(ty) = &l.ty {
                    self.skip(ty);
                }
                let mark = self.scope.len();
                let pats = l.pats.iter().map(|p| self.binders(p)).collect();
                let rest = self.body(&l.rest, ns);
                self.scope.truncate(mark);
                Expr::Let {
                    pats,
                    values,
                    rest: Box::new(rest),
                }
            }
            _ => Expr::Unknown,
        }
    }

    /// A direct argument of a call, or a direct field of a constructor, or the value of a `do` bind, exec or step, or a `return` (a direct argument of the `bind` or `pure` the sugar
    /// stands for): a bare name that is not bound is handed over; anything else is walked.
    fn passable(&mut self, term: &Term) -> Expr {
        let inner = strip_group(term);
        match &inner.kind {
            TermKind::Name(name) => match self.binding(name).number() {
                Some(number) => self.var(number),
                None => {
                    let item = self.push(inner.span, What::Passed { name: name.clone() });
                    Expr::Name {
                        item: Some(item),
                        name: name.clone(),
                    }
                }
            },
            _ => self.term(term, &Ns::None),
        }
    }

    /// A direct argument of a call: what it is (`ArgShape`), and then it is handed over or walked as `passable` does. A lambda's number is the one it gets when
    /// the walk enters it, which is the count of lambdas before the argument is walked.
    fn arg(&mut self, term: &Term) -> (ArgShape, Expr) {
        let inner = strip_group(term);
        let shape = match &inner.kind {
            TermKind::Name(name) => match self.binding(name) {
                Binding::Unbound => ArgShape::Name(name.clone()),
                Binding::Param(at) => ArgShape::Param(at),
                Binding::Local(_) => ArgShape::Local,
            },
            TermKind::Lam { .. } | TermKind::LamMatch { .. } => ArgShape::Lambda(self.lambdas),
            _ => ArgShape::Other,
        };
        (shape, self.passable(term))
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

    /// A lambda chain `w => b => body`: one node with every binder of the chain, and one number and one set of facts for each lambda of it (the walk numbers every `=>`).
    fn lam(&mut self, term: &Term) -> Expr {
        let (first, mark, uses_at) = (self.lambdas, self.scope.len(), self.uses.len());
        let mut chain: Vec<u32> = Vec::new();
        let mut at = term;
        let body = loop {
            let TermKind::Lam {
                binder,
                annot,
                body,
                ..
            } = &at.kind
            else {
                unreachable!("`lam` is given a lambda");
            };
            self.lambdas += 1;
            self.lams.push(LamFacts {
                binders: 0,
                reads: Box::new([]),
            });
            if let Some(annot) = annot {
                self.skip(annot);
            }
            chain.push(self.bind(&binder.text));
            match &**body {
                Body::Term(next) if matches!(strip_group(next).kind, TermKind::Lam { .. }) => {
                    at = strip_group(next);
                }
                other => break self.body(other, &Ns::None),
            }
        };
        self.scope.truncate(mark);
        self.facts(first, &chain, uses_at);
        Expr::Lam {
            nth: first,
            binders: chain.into(),
            body: Box::new(body),
        }
    }

    /// The facts of the lambdas `first..` of a chain whose binders are `chain`: the one at `k` has the binders from `k` on, and reads what is below its first binder.
    fn facts(&mut self, first: u32, chain: &[u32], uses_at: usize) {
        for (k, below) in chain.iter().enumerate() {
            let mut reads: Vec<u32> = self.uses[uses_at..]
                .iter()
                .copied()
                .filter(|used| used < below)
                .collect();
            reads.sort_unstable();
            reads.dedup();
            self.lams[first as usize + k] = LamFacts {
                binders: (chain.len() - k) as u32,
                reads: reads.into(),
            };
        }
    }

    fn term(&mut self, term: &Term, ns: &Ns) -> Expr {
        match &term.kind {
            TermKind::Name(name) => match self.binding(name).number() {
                Some(number) => self.var(number),
                None => {
                    let item = self.push(term.span, What::Deep { name: name.clone() });
                    Expr::Name {
                        item: Some(item),
                        name: name.clone(),
                    }
                }
            },
            TermKind::Type
            | TermKind::Data
            | TermKind::Quant
            | TermKind::Qua(_)
            | TermKind::Lit(_)
            | TermKind::Refl => Expr::Data,
            TermKind::Hole(_) => Expr::Unknown,
            TermKind::Kind(inner) => self.skip(inner),
            TermKind::NatPlus { rest, .. } => {
                self.push(term.span, What::Data);
                let rest = self.term(rest, &Ns::None);
                Expr::Seq(Box::new([rest, Expr::Data]))
            }
            TermKind::List(items) => {
                let mut parts: Vec<Expr> = items.iter().map(|i| self.term(i, &Ns::None)).collect();
                parts.push(Expr::Unknown);
                Expr::Seq(parts.into())
            }
            TermKind::ArrayNew { elem, ty, size } => {
                let item = self.push(term.span, What::Sugar(Sugar::Own("Array.new".to_owned())));
                let mut operands = vec![self.term(elem, &Ns::None)];
                self.skip(ty);
                match size {
                    bend_parser::ArraySize::Count(n) | bend_parser::ArraySize::Depth(n) => {
                        operands.push(self.term(n, &Ns::None));
                    }
                    _ => {}
                }
                Expr::Sugar {
                    item,
                    operands: operands.into(),
                }
            }
            TermKind::Ctr { name, args } => {
                let item = self.push(
                    name.span,
                    What::Ctr {
                        name: name.text.clone(),
                    },
                );
                Expr::Ctr {
                    item,
                    name: name.text.clone(),
                    fields: args.iter().map(|a| self.passable(a)).collect(),
                }
            }
            TermKind::TypeApp { args, .. } => {
                args.iter().for_each(|a| drop(self.skip(a)));
                Expr::Data
            }
            TermKind::Plus(inner) => self.term(inner, &Ns::None),
            TermKind::Call { callee, args, .. } => {
                let head = strip_group(callee);
                // the callee's binding is read before anything is walked, and the item's slot is taken now, so that two places at one span (`a[i](1)`: the call
                // and the `Array.get` it stands on) keep the order layer 1 had, the outer one first; the arguments are filled in once they are walked
                let callee_is = match &head.kind {
                    TermKind::Name(name) => match self.binding(name) {
                        Binding::Unbound => Ok(name.clone()),
                        Binding::Param(at) => Err(Some(at)),
                        Binding::Local(_) => Err(None),
                    },
                    _ => Err(None),
                };
                let slot = self.push(
                    head.span,
                    What::Opaque {
                        param: None,
                        args: Vec::new(),
                    },
                );
                let head_expr = match &head.kind {
                    // a callee that is a name is the item's own: an unbound one has no place of its own, a bound one is a read of the binder
                    TermKind::Name(name) => match self.binding(name).number() {
                        Some(number) => self.var(number),
                        None => Expr::Name {
                            item: None,
                            name: name.clone(),
                        },
                    },
                    _ => self.term(callee, &Ns::None),
                };
                let (shapes, exprs): (Vec<ArgShape>, Vec<Expr>) =
                    args.iter().map(|a| self.arg(&a.term)).unzip();
                self.items[slot as usize].what = match callee_is {
                    Ok(name) => What::Call { name, args: shapes },
                    Err(param) => What::Opaque {
                        param,
                        args: shapes,
                    },
                };
                Expr::Call {
                    item: slot,
                    head: Box::new(head_expr),
                    args: exprs.into(),
                }
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
                let item = self.push(term.span, What::Sugar(Sugar::Own(name.to_owned())));
                let mut operands = vec![self.term(array, &Ns::None)];
                // `parse_term_ns(p, ix, U32)`: the operators of the index are U32's
                let u32 = Ns::Name {
                    name: "U32".to_owned(),
                    bound: false,
                };
                operands.push(self.term(index, &u32));
                if let Some(value) = write {
                    operands.push(self.term(value, &Ns::None));
                }
                Expr::Sugar {
                    item,
                    operands: operands.into(),
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
                let (through, sugar_item) = match op {
                    Op::And | Op::Or | Op::Append => {
                        self.push(*at, What::BaseFixed);
                        (ns.clone(), None)
                    }
                    Op::Pair | Op::Either => {
                        self.push(term.span, What::BaseFixed);
                        (Ns::None, None)
                    }
                    Op::Cons => {
                        self.push(*at, What::BaseFixed);
                        (Ns::None, None)
                    }
                    Op::Min => (Ns::None, None),
                    other => {
                        let sugar = match verb(*other) {
                            Some(verb) => Self::sugar_of(ns, verb),
                            None => Sugar::Unknown,
                        };
                        (ns.clone(), Some(self.push(*at, What::Sugar(sugar))))
                    }
                };
                let operands = [self.term(lhs, &through), self.term(rhs, &through)];
                match sugar_item {
                    Some(item) => Expr::Sugar {
                        item,
                        operands: Box::new(operands),
                    },
                    // Base's own: what it builds is not ours to read
                    None => {
                        let [lhs, rhs] = operands;
                        Expr::Seq(Box::new([lhs, rhs, Expr::Unknown]))
                    }
                }
            }
            TermKind::Arrow { from, to } => {
                let from = self.term(from, &Ns::None);
                let to = self.term(to, &Ns::None);
                Expr::Seq(Box::new([from, to, Expr::Data]))
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
                self.bind(&name.text);
                let body = self.term(body, &Ns::None);
                self.scope.pop();
                Expr::Seq(Box::new([body, Expr::Data]))
            }
            TermKind::Lam { .. } => self.lam(term),
            TermKind::LamMatch { arms, tail } => {
                let (nth, uses_at) = (self.lambdas, self.uses.len());
                self.lambdas += 1;
                self.lams.push(LamFacts {
                    binders: 0,
                    reads: Box::new([]),
                });
                // the argument the arms match on has no name in the text
                let binder = self.fresh();
                let mut parts: Vec<Expr> = arms
                    .iter()
                    .map(|arm| self.term(&arm.body, &Ns::None))
                    .collect();
                if let Some(tail) = tail {
                    parts.push(self.term(tail, &Ns::None));
                }
                // which arm answers is not decided here
                parts.push(Expr::Unknown);
                self.facts(nth, &[binder], uses_at);
                Expr::Lam {
                    nth,
                    binders: Box::new([binder]),
                    body: Box::new(Expr::Seq(parts.into())),
                }
            }
            TermKind::Annot { term: inner, ty } => {
                self.skip(ty);
                self.term(inner, &Ns::None)
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
                let lhs = self.term(lhs, &Ns::None);
                let rhs = self.term(rhs, &Ns::None);
                Expr::Seq(Box::new([lhs, rhs, Expr::Data]))
            }
            TermKind::Rewrite {
                eq,
                named,
                motive,
                rest,
            } => {
                let eq = self.term(eq, &Ns::None);
                self.skip(motive);
                let mark = self.scope.len();
                if let Some(named) = named {
                    self.bind(&named.text);
                }
                let rest = self.body(rest, &Ns::None);
                self.scope.truncate(mark);
                Expr::Seq(Box::new([eq, rest]))
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
                self.body(body, &ns)
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
                let mut parts: Vec<Expr> = items
                    .iter()
                    .enumerate()
                    .map(|(at, item)| {
                        let ns = if at + 1 == items.len() {
                            &last
                        } else {
                            &Ns::None
                        };
                        self.body(item, ns)
                    })
                    .collect();
                parts.push(Expr::Unknown);
                Expr::Seq(parts.into())
            }
            TermKind::Do(d) => self.do_block(d),
            _ => Expr::Unknown,
        }
    }

    /// `do M<T.., B>:` and its statements: each is a node that holds the statements after it, as `bend.ts` nests them (`M.bind(.., x => rest)`).
    fn do_block(&mut self, d: &bend_parser::Do) -> Expr {
        /// A statement as the walk reads it, before the ones after it are known.
        enum Stmt {
            Let {
                binder: u32,
                value: Expr,
            },
            Bind {
                item: u32,
                binder: Option<u32>,
                value: Expr,
                pure: bool,
            },
            Last(Expr),
        }
        let bound = self.bound(&d.monad.text);
        let sugar = |verb: &str| {
            if bound {
                Sugar::Bound
            } else {
                Sugar::Named(format!("{}.{verb}", d.monad.text))
            }
        };
        d.args.iter().for_each(|a| drop(self.skip(a)));
        let mark = self.scope.len();
        let mut stmts = Vec::new();
        for stmt in &d.stmts {
            match &stmt.kind {
                DoKind::Let {
                    name, ty, value, ..
                } => {
                    self.skip(ty);
                    let value = self.term(value, &Ns::None);
                    let binder = self.bind(&name.text);
                    stmts.push(Stmt::Let { binder, value });
                }
                DoKind::Bind {
                    name, ty, value, ..
                } => {
                    let item = self.push(stmt.span, What::Sugar(sugar("bind")));
                    self.skip(ty);
                    let value = self.passable(value);
                    let binder = Some(self.bind(&name.text));
                    stmts.push(Stmt::Bind {
                        item,
                        binder,
                        value,
                        pure: false,
                    });
                }
                DoKind::Exec { ty, value } => {
                    let item = self.push(stmt.span, What::Sugar(sugar("bind")));
                    self.skip(ty);
                    let value = self.passable(value);
                    let binder = Some(self.fresh());
                    stmts.push(Stmt::Bind {
                        item,
                        binder,
                        value,
                        pure: false,
                    });
                }
                DoKind::Step(step) => {
                    let item = self.push(stmt.span, What::Sugar(sugar("bind")));
                    let value = self.passable(step);
                    let binder = Some(self.fresh());
                    stmts.push(Stmt::Bind {
                        item,
                        binder,
                        value,
                        pure: false,
                    });
                }
                DoKind::Return(value) => {
                    let item = self.push(stmt.span, What::Sugar(sugar("pure")));
                    let value = self.passable(value);
                    stmts.push(Stmt::Bind {
                        item,
                        binder: None,
                        value,
                        pure: true,
                    });
                }
                DoKind::Value(value) => {
                    // `bend` annotates the final value with the monad: a type, not a call
                    self.push(value.span, What::Data);
                    stmts.push(Stmt::Last(self.term(value, &Ns::None)));
                }
                _ => {}
            }
        }
        self.scope.truncate(mark);
        // the type terms are types, so `Data`: `T.., A, B` for a bind and `T.., B` for a pure, in `bend.ts`'s order
        let types = |pure: bool| -> Box<[Expr]> {
            vec![Expr::Data; d.args.len() + usize::from(!pure)].into()
        };
        let mut rest = Expr::Unknown;
        for stmt in stmts.into_iter().rev() {
            rest = match stmt {
                Stmt::Last(value) => value,
                Stmt::Let { binder, value } => Expr::Let {
                    pats: Box::new([Pat::Bind(binder)]),
                    values: Box::new([value]),
                    rest: Box::new(rest),
                },
                Stmt::Bind {
                    item,
                    binder,
                    value,
                    pure,
                } => Expr::Bind {
                    item,
                    monad: d.monad.text.clone(),
                    types: types(pure),
                    value: Box::new(value),
                    binder,
                    rest: Box::new(rest),
                    pure,
                },
            };
        }
        rest
    }
}

fn walk_decl(decl: &Decl) -> Option<DefWalk> {
    let params = match decl {
        Decl::Def(def) => def.params.len() as u32,
        _ => 0,
    };
    let mut walker = Walker {
        items: Vec::new(),
        scope: Vec::new(),
        lambdas: 0,
        lams: Vec::new(),
        next_binder: params,
        uses: Vec::new(),
    };
    let mut body = Expr::Unknown;
    match decl {
        Decl::Def(def) => {
            for (at, bind) in def.params.iter().enumerate() {
                if let Some(ty) = &bind.ty {
                    walker.skip(ty);
                }
                walker.scope.push(Bound {
                    name: bind.name.text.clone(),
                    number: at as u32,
                    param: Some(at as u32),
                });
            }
            if let Some(ret) = &def.ret {
                walker.skip(ret);
            }
            if let Some(DefBody::Code(code)) = &def.body {
                body = walker.body(code, &Ns::None);
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
    // the items go in source order, equal places in the order they were taken (the sort is stable), and the nodes of the flow are renumbered to match
    let mut order: Vec<usize> = (0..walker.items.len()).collect();
    order.sort_by_key(|&at| (walker.items[at].span.start, walker.items[at].span.end));
    let mut place = vec![0u32; order.len()];
    for (new, &old) in order.iter().enumerate() {
        place[old] = new as u32;
    }
    let items = order.iter().map(|&at| walker.items[at].clone()).collect();
    body.map_items(&|old| place[old as usize]);
    Some(DefWalk {
        name_span: decl.name().span,
        items,
        flow: Flow {
            params,
            lambdas: walker.lams.into(),
            body,
        },
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
                    What::Call { name, .. } => format!("call {name}"),
                    What::Opaque { .. } => "opaque".to_owned(),
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

    fn shape(arg: &ArgShape) -> String {
        match arg {
            ArgShape::Name(name) => format!("name {name}"),
            ArgShape::Param(at) => format!("param {at}"),
            ArgShape::Local => "local".to_owned(),
            ArgShape::Lambda(n) => format!("lambda {n}"),
            ArgShape::Other => "other".to_owned(),
        }
    }

    /// Each call or opaque callee of the one def of `text`: the text at the span, the parameter it is (opaque only), and the shapes of its arguments.
    fn calls(text: &str) -> Vec<(String, Option<u32>, Vec<String>)> {
        let walks = walks(text);
        assert_eq!(walks.len(), 1, "one declaration: {text}");
        walks[0]
            .items
            .iter()
            .filter_map(|i| {
                let (param, args) = match &i.what {
                    What::Call { args, .. } => (None, args),
                    What::Opaque { param, args } => (*param, args),
                    _ => return None,
                };
                Some((
                    text[i.span.start as usize..i.span.end as usize].to_owned(),
                    param,
                    args.iter().map(shape).collect(),
                ))
            })
            .collect()
    }

    fn call(callee: &str, param: Option<u32>, args: &[&str]) -> (String, Option<u32>, Vec<String>) {
        (
            callee.to_owned(),
            param,
            args.iter().map(|a| (*a).to_owned()).collect(),
        )
    }

    #[test]
    fn a_callee_that_is_a_parameter_says_which_by_its_place_in_the_written_list() {
        // templates first, then the rest, as written: `h` is 0, `x` 1, `f` 2
        assert_eq!(
            calls("def g(~h: U32 -> U32, x: U32, f: U32 -> U32) -> U32:\n  h(f(x))\n"),
            [
                call("h", Some(0), &["other"]),
                call("f", Some(2), &["param 1"]),
            ]
        );
        // a def's own name is no parameter, and neither is a name nobody binds
        assert_eq!(
            calls("def g(x: U32) -> U32:\n  k(x)\n"),
            [call("k", None, &["param 0"])]
        );
    }

    #[test]
    fn the_innermost_binder_of_a_name_decides_what_it_is() {
        // a `let` over a parameter: the call after it is on a local, and the name as an argument is a local
        assert_eq!(
            calls("def g(f: U32 -> U32) -> U32:\n  a = h(f)\n  f = a\n  f(f)\n"),
            [call("h", None, &["param 0"]), call("f", None, &["local"]),]
        );
        // a lambda's binder over a parameter, and the parameter again once the lambda is closed
        assert_eq!(
            calls("def g(f: U32 -> U32) -> U32:\n  k(x => (f => f(x))(1), f)\n"),
            [
                call("k", None, &["lambda 0", "param 0"]),
                call("f => f(x)", None, &["other"]),
                call("f", None, &["local"]),
            ]
        );
    }

    #[test]
    fn two_places_at_one_span_keep_the_outer_one_first() {
        // the call of an element and the `Array.get` it stands on are both at `a[i]`; layer 1 listed the call first
        for (text, span) in [
            ("def g(a: Array, i: U32) -> U32:\n  a[i](1)\n", "a[i]"),
            ("def g(a: Bool, b: Bool) -> U32:\n  (a & b)(1)\n", "a & b"),
        ] {
            let walks = walks(text);
            let at: Vec<&What> = walks[0]
                .items
                .iter()
                .filter(|i| &text[i.span.start as usize..i.span.end as usize] == span)
                .map(|i| &i.what)
                .collect();
            assert!(
                matches!(
                    at[..],
                    [What::Opaque { .. }, What::Sugar(_) | What::BaseFixed]
                ),
                "{text}: {at:?}"
            );
        }
    }

    #[test]
    fn a_case_variable_a_let_and_a_do_name_are_local() {
        assert_eq!(
            calls(
                "def g(xs: List) -> U32:\n  match xs:\n    case Cons{h, t}:\n      k(h, t)\n    case Nil{}:\n      k(1, xs)\n"
            ),
            [
                call("k", None, &["local", "local"]),
                call("k", None, &["other", "param 0"]),
            ]
        );
        // a do's name, bound after its value
        assert_eq!(
            calls("def g() -> IO(U32):\n  do IO<U32>:\n    x : U32 <- m()\n    k(x)\n"),
            [call("m", None, &[]), call("k", None, &["local"])]
        );
    }

    #[test]
    fn an_argument_is_read_through_parentheses() {
        assert_eq!(
            calls("def g(f: U32) -> U32:\n  k((f), (a), ((x => x)), (1), ~(y => y))\n"),
            [call(
                "k",
                None,
                &["param 0", "name a", "lambda 0", "other", "lambda 1"]
            )]
        );
    }

    #[test]
    fn a_lambda_is_numbered_as_the_walk_enters_it_and_a_match_lambda_is_one() {
        // an outer lambda is entered before the ones inside it
        assert_eq!(
            calls("def g() -> U32:\n  k(x => m(y => y), z => z, \\{ Nil: 1 })\n"),
            [
                call("k", None, &["lambda 0", "lambda 2", "lambda 3"]),
                call("m", None, &["lambda 1"]),
            ]
        );
        // a lambda written in a type is not walked and not numbered
        assert_eq!(
            calls("def g(x: List<(a => a)>) -> U32:\n  k(z => z)\n"),
            [call("k", None, &["lambda 0"])]
        );
    }

    #[test]
    fn the_inner_call_of_a_call_of_a_call_carries_its_own_arguments() {
        // `f(a)(k)`: the inner call is `f` with `[a]`; the outer callee is a call's result, which no parameter is
        assert_eq!(
            calls("def g(k: U32 -> U32) -> U32:\n  f(a)(k)\n"),
            [
                call("f", None, &["name a"]),
                call("f(a)", None, &["param 0"]),
            ]
        );
    }
}
