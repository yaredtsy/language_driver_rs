//! Terms and bodies: `bend.ts` 1810–2480 (`parse_term` … `parse_terms`), function for function, each with its line. The tree is the surface form: where
//! `bend.ts` builds `App(App(Ref f, a), b)` or desugars a list into `Con{…}`, this keeps `f(a, b)` and `[a, b]` as written. Four things `bend.ts` decides by
//! looking at the book's tables are left open (a name's role, `+Name`, `~` arguments, a pattern's head), and no binder scope is computed.

use crate::parse::{Parser, is_head, is_keyword, is_name};
use crate::tree::{
    Arg, ArraySize, Body, Case, Do, DoKind, DoStmt, Let, Lit, Match, MatchArm, Name, Op, Quant,
    Term, TermKind,
};

/// `bend.ts` 2049 `INFIX`: (precedence, right-associative).
fn infix(op: &str) -> (i32, bool) {
    match op {
        "<-" => (-1, false),
        "&" | "|" => (1, true),
        "||" => (2, false),
        "&&" => (3, false),
        "<" | "<=" | ">" | ">=" => (4, false),
        "<>" | "++" | "<&>" => (5, true),
        ".|." => (6, false),
        ".^." => (7, false),
        ".&." => (8, false),
        "<<" | ">>" => (9, false),
        "+" | "-" => (10, false),
        "*" | "/" | "%" => (11, false),
        _ => (0, true),
    }
}

fn operator(op: &str) -> Option<Op> {
    Some(match op {
        "&" => Op::Pair,
        "|" => Op::Either,
        "||" => Op::Or,
        "&&" => Op::And,
        "<" => Op::Lt,
        "<=" => Op::Le,
        ">" => Op::Gt,
        ">=" => Op::Ge,
        "<>" => Op::Cons,
        "++" => Op::Append,
        "<&>" => Op::Min,
        ".|." => Op::BitOr,
        ".^." => Op::BitXor,
        ".&." => Op::BitAnd,
        "<<" => Op::Shl,
        ">>" => Op::Shr,
        "+" => Op::Add,
        "-" => Op::Sub,
        "*" => Op::Mul,
        "/" => Op::Div,
        "%" => Op::Mod,
        _ => return None,
    })
}

/// A term that is one binder: `x`, `+x`, or either in one pair of parens with a `: T` (`(x : T)`). `bend.ts` 1764 `parse_bind` and the `typed` test of
/// `parse_term_do_stmt` accept a `Var`, and `parse_term_tup` hands back the term inside a group, so a group around a name is the name. A `Var` is a name
/// with no dot (a dotted name is a `Ref`), or `+x` where `x` is no datatype (which the parser cannot know).
fn as_binder(t: &Term) -> Option<(bool, Name, Option<Box<Term>>)> {
    match &t.kind {
        TermKind::Name(k) if !k.contains('.') => Some((
            false,
            Name {
                text: k.clone(),
                span: t.span,
            },
            None,
        )),
        TermKind::Plus(inner) => match &inner.kind {
            TermKind::Name(k) if !k.contains('.') => Some((
                true,
                Name {
                    text: k.clone(),
                    span: inner.span,
                },
                None,
            )),
            _ => None,
        },
        TermKind::Group { body, ns } => match &**body {
            Body::Term(inner) => {
                as_binder(inner).map(|(reusable, name, _)| (reusable, name, ns.clone()))
            }
            _ => None,
        },
        _ => None,
    }
}

impl Parser<'_> {
    /// `bend.ts` 1810 `parse_term`. `lvl` is the precedence floor: an operator below it ends the term.
    pub(crate) fn term(&mut self, lvl: i32) -> Option<Term> {
        self.skip();
        let beg = self.pos;
        let base = self.term_base(beg)?;
        self.term_ops(base, beg, lvl)
    }

    fn mk(&self, kind: TermKind, beg: usize) -> Term {
        Term {
            kind,
            span: self.span_from(beg),
        }
    }

    /// `bend.ts` 1818 `parse_term_base`.
    fn term_base(&mut self, beg: usize) -> Option<Term> {
        let c = self.peek();
        if is_head(c) {
            let k = self.lexeme()?;
            let kind = match k.text.as_str() {
                "Type" => TermKind::Type,
                "Data" => TermKind::Data,
                "Quant" => TermKind::Quant,
                "Kind" => {
                    self.eat("(")?;
                    let g = self.term(0)?;
                    self.eat(")")?;
                    TermKind::Kind(Box::new(g))
                }
                "do" => return self.term_do(beg),
                "match" => {
                    return self.fail(
                        "a term (a match heads a def body, not a term)",
                        beg,
                        self.pos,
                    );
                }
                "case" => {
                    return self.fail(
                        "a match heading this case (this case is orphaned)",
                        beg,
                        self.pos,
                    );
                }
                "return" => return self.fail("a do-block heading this return", beg, self.pos),
                w if is_keyword(w) => {
                    return self.fail(
                        format!("a term (the keyword '{w}' cannot head one)"),
                        beg,
                        self.pos,
                    );
                }
                _ => {
                    if self.take("{") {
                        let args = self.term_args("}")?;
                        TermKind::Ctr { name: k, args }
                    } else {
                        TermKind::Name(k.text)
                    }
                }
            };
            return Some(self.mk(kind, beg));
        }
        if c.is_ascii_digit() {
            return self.term_num(beg);
        }
        let kind = match c {
            b'@' => return self.term_all(false),
            b'&' => {
                if let q @ b'0'..=b'2' = self.byte(self.pos + 1) {
                    self.pos += 2;
                    TermKind::Qua(q - b'0')
                } else {
                    return self.term_all(true);
                }
            }
            b'+' => {
                self.pos += 1;
                let t = self.term(12)?;
                TermKind::Plus(Box::new(t))
            }
            b'\\' => {
                self.pos += 1;
                return self.term_lam_match(beg);
            }
            b'%' => {
                self.pos += 1;
                return self.term_rewrite(beg);
            }
            b'{' => {
                self.pos += 1;
                return self.term_braces(beg);
            }
            b'(' => {
                self.pos += 1;
                return self.term_tup(beg);
            }
            b'[' => {
                self.pos += 1;
                return self.term_bracket(beg);
            }
            b'\'' => {
                self.pos += 1;
                let inner = self.pos;
                self.parse_char()?;
                let raw = self.slice(inner, self.pos).to_string();
                if !self.take("'") {
                    return self.fail("a closing '", self.pos, self.pos);
                }
                TermKind::Lit(Lit::Char(raw))
            }
            b'"' => {
                self.pos += 1;
                let inner = self.pos;
                while !self.take("\"") {
                    if self.pos >= self.bytes.len() {
                        // the error starts at the opening quote: that is where reading resumes from
                        return self.fail("a closing \"", beg, beg + 1);
                    }
                    self.parse_char()?;
                }
                TermKind::Lit(Lit::Str(self.slice(inner, self.pos - 1).to_string()))
            }
            b'?' => {
                self.pos += 1;
                TermKind::Hole(self.name()?)
            }
            _ => return self.fail("a term", self.pos, self.pos),
        };
        Some(self.mk(kind, beg))
    }

    /// `bend.ts` 1660 `parse_char`: one character, or an escape.
    fn parse_char(&mut self) -> Option<()> {
        if self.take("\\") {
            if self.at("u{") {
                let digits = self.bytes.get(self.pos + 2..).map_or(0, |r| {
                    r.iter().take_while(|b| b.is_ascii_hexdigit()).count()
                });
                if digits > 0 && self.byte(self.pos + 2 + digits) == b'}' {
                    self.pos += digits + 3;
                    return Some(());
                }
            }
            let b = self.byte(self.pos);
            self.pos += 1;
            if !matches!(b, b'n' | b't' | b'r' | b'0' | b'\\' | b'\'' | b'"') {
                return self.fail(
                    "an escape (\\n \\t \\r \\0 \\\\ \\' \\\" \\u{1F600})",
                    self.pos - 1,
                    self.pos,
                );
            }
            return Some(());
        }
        match self.text_at(self.pos).and_then(|s| s.chars().next()) {
            Some(c) => {
                self.pos += c.len_utf8();
                Some(())
            }
            None => self.fail("a character", self.pos, self.pos),
        }
    }

    /// `\{ … }`: `bend.ts` 1858.
    fn term_lam_match(&mut self, beg: usize) -> Option<Term> {
        self.eat("{")?;
        let mut arms = Vec::new();
        let mut tail = None;
        loop {
            self.skip();
            if self.take("}") {
                break;
            }
            let t = self.term(0)?;
            self.skip();
            if let TermKind::Name(k) = &t.kind
                && self.take(":")
            {
                let ctr = Name {
                    text: k.clone(),
                    span: t.span,
                };
                let body = self.term(0)?;
                arms.push(MatchArm { ctr, body });
                self.skip();
                self.take(";");
                continue;
            }
            tail = Some(Box::new(t));
            self.skip();
            self.take(";");
            self.eat("}")?;
            break;
        }
        Some(self.mk(TermKind::LamMatch { arms, tail }, beg))
    }

    /// `%e : P; body` and `%e@E : P; body`: `bend.ts` 1895.
    fn term_rewrite(&mut self, beg: usize) -> Option<Term> {
        let e0 = self.term(0)?;
        self.skip();
        let (mut named, mut eq) = (None, e0);
        if self.take("@") {
            let Some((_, name, _)) = as_binder(&eq).filter(|(reusable, ..)| !reusable) else {
                return self.fail(
                    "a name before @ (a rewrite binder is one name: %e@E : P)",
                    self.pos,
                    self.pos,
                );
            };
            named = Some(name);
            eq = self.term(0)?;
        }
        self.eat(":")?;
        let motive = self.term(0)?;
        self.skip();
        self.take(";");
        let rest = self.block()?;
        Some(self.mk(
            TermKind::Rewrite {
                eq: Box::new(eq),
                named,
                motive: Box::new(motive),
                rest: Box::new(rest),
            },
            beg,
        ))
    }

    /// `{e : T}`, `{a == b : T}`, `{a != b : T}`, `{==}`: `bend.ts` 1923.
    fn term_braces(&mut self, beg: usize) -> Option<Term> {
        self.skip();
        if self.take("==") {
            self.eat("}")?;
            return Some(self.mk(TermKind::Refl, beg));
        }
        let a = self.term(0)?;
        self.skip();
        let negated = self.take("!=");
        let b = if negated || self.take("==") {
            Some(self.term(0)?)
        } else {
            None
        };
        self.eat(":")?;
        let ty = self.term(0)?;
        self.eat("}")?;
        let kind = match b {
            None => TermKind::Annot {
                term: Box::new(a),
                ty: Box::new(ty),
            },
            Some(b) => TermKind::Eq {
                lhs: Box::new(a),
                rhs: Box::new(b),
                ty: Box::new(ty),
                negated,
            },
        };
        Some(self.mk(kind, beg))
    }

    /// `[a, b]` and `[v : T * n]`, `[v : T ^ d]`: `bend.ts` 1951.
    fn term_bracket(&mut self, beg: usize) -> Option<Term> {
        self.skip();
        let mut xs = if self.at("]") {
            Vec::new()
        } else {
            vec![self.term(0)?]
        };
        self.skip();
        if !xs.is_empty() && self.take(":") {
            let ty = self.term(12)?;
            self.skip();
            let count = self.take("*");
            if !count {
                self.eat("^")?;
            }
            let n = Box::new(self.term(0)?);
            self.eat("]")?;
            let size = if count {
                ArraySize::Count(n)
            } else {
                ArraySize::Depth(n)
            };
            let elem = Box::new(xs.remove(0));
            return Some(self.mk(
                TermKind::ArrayNew {
                    elem,
                    ty: Box::new(ty),
                    size,
                },
                beg,
            ));
        }
        self.take(",");
        xs.extend(self.term_args("]")?);
        Some(self.mk(TermKind::List(xs), beg))
    }

    /// `do M<ts>:` and its statements: `bend.ts` 1836 and 2329.
    fn term_do(&mut self, beg: usize) -> Option<Term> {
        let monad = self.name()?;
        self.skip();
        if !self.take("<") {
            return self.fail(
                format!("'<' (a do block is written do {}<...>:)", monad.text),
                self.pos,
                self.pos,
            );
        }
        let args = self.term_args(">")?;
        self.eat(":")?;
        self.skip();
        let col = self.col(self.pos);
        let mut stmts = Vec::new();
        loop {
            let (stmt, last) = self.term_do_stmt(col)?;
            stmts.push(stmt);
            if last {
                break;
            }
        }
        Some(self.mk(TermKind::Do(Do { monad, args, stmts }), beg))
    }

    /// `bend.ts` 2329 `parse_term_do_stmt`: one statement, and whether it is the last.
    fn term_do_stmt(&mut self, col: usize) -> Option<(DoStmt, bool)> {
        self.skip();
        let beg = self.pos;
        if self.word("return") {
            let e = self.term(0)?;
            self.do_end()?;
            return Some((
                DoStmt {
                    kind: DoKind::Return(e),
                    span: self.span_from(beg),
                },
                true,
            ));
        }
        let t = self.term(0)?;
        self.skip();
        let binder = as_binder(&t);
        let typed = binder.is_some() && self.take(":");
        let step = !typed && self.more(col);
        let ty = if typed { Some(self.term(1)?) } else { None };
        self.skip();
        let assign = typed && !self.at("==") && self.take("=");
        if typed && !assign {
            self.eat("<-")?;
        } else if !typed && !step && !self.take("<-") {
            self.do_end()?;
            return Some((
                DoStmt {
                    span: t.span,
                    kind: DoKind::Value(t),
                },
                true,
            ));
        }
        let value = if step { None } else { Some(self.term(0)?) };
        let span = self.span_from(beg);
        self.skip();
        self.take(";");
        let kind = match (binder, ty, value) {
            (Some((reusable, name, _)), Some(ty), Some(value)) if assign => DoKind::Let {
                reusable,
                name,
                ty,
                value,
            },
            (Some((reusable, name, _)), Some(ty), Some(value)) => DoKind::Bind {
                reusable,
                name,
                ty,
                value,
            },
            (_, _, None) => DoKind::Step(t),
            (_, _, Some(value)) => DoKind::Exec { ty: t, value },
        };
        Some((DoStmt { kind, span }, false))
    }

    /// `bend.ts` 2366 `parse_do_end`.
    fn do_end(&mut self) -> Option<()> {
        self.skip();
        if self.at("=") && !self.at("==") && !self.at("=>") {
            return self.fail(
                "a do-block let with its type (write \"x : T = ...\")",
                self.pos,
                self.pos,
            );
        }
        Some(())
    }

    /// `bend.ts` 2088 `parse_nl`: only whitespace between the start of the line and here.
    pub(crate) fn nl_before(&self) -> bool {
        let mut j = self.pos;
        while j > 0 {
            j -= 1;
            match self.byte(j) {
                b'\n' => return true,
                b' ' | b'\r' | b'\t' => {}
                _ => return false,
            }
        }
        true
    }

    /// `bend.ts` 2075 `INFIX_BY_HEAD`: the operator at `pos`, longest listed first.
    fn infix_at(&self) -> Option<&'static str> {
        let candidates: &[&'static str] = match self.peek() {
            b'<' => &["<&>", "<-", "<=", "<>", "<<", "<"],
            b'-' => &["->", "-"],
            b'&' => &["&&", "&"],
            b'|' => &["||", "|"],
            b'>' => &[">=", ">>", ">"],
            b'.' => &[".|.", ".^.", ".&."],
            b'+' => &["++", "+"],
            b'*' => &["*"],
            b'/' => &["/"],
            b'%' => &["%"],
            _ => return None,
        };
        candidates.iter().copied().find(|op| self.at(op))
    }

    /// `bend.ts` 2101 `parse_term_ops`: what follows a term and continues it: a call, an index, `=>`, an operator.
    fn term_ops(&mut self, mut out: Term, beg: usize, lvl: i32) -> Option<Term> {
        loop {
            self.skip();
            if self.nl_before() && (self.at("(") || self.at("[")) {
                return Some(out);
            }
            if self.at("(") || self.at("!(") {
                let gpu = self.take("!");
                self.pos += 1;
                let args = self.call_args()?;
                out = self.mk(
                    TermKind::Call {
                        callee: Box::new(out),
                        gpu,
                        args,
                    },
                    beg,
                );
                continue;
            }
            if self.at("[") {
                self.pos += 1;
                let index = self.term(0)?;
                self.eat("]")?;
                let closed = self.pos;
                self.skip();
                let write = if !self.nl_before() && self.take("<-") {
                    Some(Box::new(self.term(2)?))
                } else {
                    None
                };
                let span = if write.is_some() {
                    self.span_from(beg)
                } else {
                    self.span(beg, closed)
                };
                out = Term {
                    kind: TermKind::Index {
                        array: Box::new(out),
                        index: Box::new(index),
                        write,
                    },
                    span,
                };
                continue;
            }
            if lvl == 0 && self.take("=>") {
                let Some((reusable, binder, annot)) = as_binder(&out) else {
                    return self.fail("a lambda binder (one name: k => body)", self.pos, self.pos);
                };
                let body = self.block()?;
                out = self.mk(
                    TermKind::Lam {
                        reusable,
                        binder,
                        annot,
                        body: Box::new(body),
                    },
                    beg,
                );
                continue;
            }
            let Some(op) = self.infix_at() else {
                return Some(out);
            };
            let (prc, right) = infix(op);
            let nx = self.byte(self.pos + op.len());
            let glued = self.pos > 0 && !self.byte(self.pos - 1).is_ascii_whitespace();
            let ends = (prc < lvl && !(op == "<" && glued))
                || ((op == "-" || op == "+") && (nx == b'>' || is_head(nx)))
                || (op.starts_with('>') && glued)
                || (op == "%" && !nx.is_ascii_whitespace());
            if ends {
                return Some(out);
            }
            self.pos += op.len();
            let rhs = self.term(if right { prc } else { prc + 1 })?;
            let span = self.span_from(beg);
            self.skip();
            let next = (self.byte(self.pos), self.byte(self.pos + 1));
            let compound = next == (b'-', b'>')
                || (matches!(next.0, b'&' | b'|') && !matches!(next.1, b'&' | b'|'));
            if op == "<" && glued && compound {
                return self.fail(
                    "'>' or ',' (a compound type argument takes parens: F<(A & B)>)",
                    self.pos,
                    self.pos,
                );
            }
            if op == "<" && (self.at(">") || self.at(",")) {
                let TermKind::Name(k) = &out.kind else {
                    return self.fail(
                        "a family name before <..> (a comparison here needs parens)",
                        self.pos,
                        self.pos,
                    );
                };
                let name = Name {
                    text: k.clone(),
                    span: out.span,
                };
                self.take(",");
                let mut args = vec![rhs];
                args.extend(self.term_args(">")?);
                out = self.mk(TermKind::TypeApp { name, args }, beg);
            } else if op == "->" {
                out = Term {
                    kind: TermKind::Arrow {
                        from: Box::new(out),
                        to: Box::new(rhs),
                    },
                    span,
                };
            } else if let Some(op) = operator(op) {
                out = Term {
                    kind: TermKind::Op {
                        lhs: Box::new(out),
                        op,
                        rhs: Box::new(rhs),
                    },
                    span,
                };
            } else {
                // `<-` ends the term before it is reached (its precedence is below every `lvl`)
                return self.fail("a term", self.pos, self.pos);
            }
        }
    }

    /// The arguments of a call, after its `(`: `~` on any of them is accepted (`bend.ts` reads it only on the leading ones of a template, 2121).
    fn call_args(&mut self) -> Option<Vec<Arg>> {
        let mut args = Vec::new();
        loop {
            self.skip();
            if self.take(")") {
                return Some(args);
            }
            let template = self.take("~");
            let term = self.term(0)?;
            args.push(Arg { template, term });
            self.skip();
            self.take(",");
        }
    }

    /// `bend.ts` 2227 `parse_term_args`.
    fn term_args(&mut self, close: &str) -> Option<Vec<Term>> {
        let mut xs = Vec::new();
        loop {
            self.skip();
            if self.take(close) {
                return Some(xs);
            }
            xs.push(self.term(0)?);
            self.skip();
            self.take(",");
        }
    }

    /// `bend.ts` 2241 `parse_term_all`: `@ Quant Name : A -> B` and `& Name : A -> B`.
    fn term_all(&mut self, exists: bool) -> Option<Term> {
        let beg = self.pos;
        self.pos += 1;
        let quant = if exists { Quant::Plain } else { self.quant() };
        let name = self.name()?;
        self.eat(":")?;
        let domain = self.term(1)?;
        self.eat("->")?;
        let body = self.term(0)?;
        Some(self.mk(
            TermKind::All {
                exists,
                quant,
                name,
                domain: Box::new(domain),
                body: Box::new(body),
            },
            beg,
        ))
    }

    /// `bend.ts` 2260 `parse_term_tup`: `( body )`, with its `: T`, and the tuple `(a, b)`, whose elements are bodies.
    fn term_tup(&mut self, beg: usize) -> Option<Term> {
        let mut items = Vec::new();
        loop {
            self.skip();
            let col = self.col(self.pos) - 1;
            let b = self.body(col)?;
            self.skip();
            let comma = !matches!(b, Body::Match(_) | Body::Let(_)) && self.take(",");
            items.push(b);
            if !comma {
                break;
            }
        }
        let ns = if self.take(":") {
            Some(Box::new(self.term(0)?))
        } else {
            None
        };
        self.eat(")")?;
        let kind = match (items.len(), items.pop()) {
            (1, Some(body)) => TermKind::Group {
                body: Box::new(body),
                ns,
            },
            (_, Some(last)) => {
                items.push(last);
                TermKind::Tuple { items, ns }
            }
            (_, None) => return self.fail("a term", self.pos, self.pos),
        };
        Some(self.mk(kind, beg))
    }

    /// `bend.ts` 2278 `parse_term_num`: `(\d+)(n|\.\d+([eE][+-]?\d+)?)?`, and the name characters that may not follow.
    fn term_num(&mut self, beg: usize) -> Option<Term> {
        while self.peek().is_ascii_digit() {
            self.pos += 1;
        }
        let digits = self.slice(beg, self.pos).to_string();
        match self.peek() {
            b'n' => {
                self.pos += 1;
                if self.take("+") {
                    let rest = self.term(0)?;
                    return Some(self.mk(
                        TermKind::NatPlus {
                            n: digits,
                            rest: Box::new(rest),
                        },
                        beg,
                    ));
                }
                if is_name(self.peek()) {
                    return self.fail("a nat literal (NUMBER n)", self.pos, self.pos);
                }
                Some(self.mk(TermKind::Lit(Lit::Nat(digits)), beg))
            }
            b'.' if self.byte(self.pos + 1).is_ascii_digit() => {
                self.pos += 1;
                while self.peek().is_ascii_digit() {
                    self.pos += 1;
                }
                if matches!(self.peek(), b'e' | b'E') {
                    let before_exponent = self.pos;
                    self.pos += 1;
                    if matches!(self.peek(), b'+' | b'-') {
                        self.pos += 1;
                    }
                    if self.peek().is_ascii_digit() {
                        while self.peek().is_ascii_digit() {
                            self.pos += 1;
                        }
                    } else {
                        self.pos = before_exponent;
                    }
                }
                let text = self.slice(beg, self.pos).to_string();
                Some(self.mk(TermKind::Lit(Lit::F32(text)), beg))
            }
            _ if is_name(self.peek()) => self.fail(
                "a numeric literal (NUMBER is U32, NUMBER n is Nat)",
                self.pos,
                self.pos,
            ),
            _ => Some(self.mk(TermKind::Lit(Lit::U32(digits)), beg)),
        }
    }

    /// `bend.ts` 2462 `parse_more`: a `;`, or the next token on the column `col`.
    fn more(&self, col: usize) -> bool {
        self.at(";") || (self.pos < self.bytes.len() && self.col(self.pos) == col)
    }

    /// `bend.ts` 2466 `parse_block`: a body whose statements may sit at any column deeper than the one before it.
    pub(crate) fn block(&mut self) -> Option<Body> {
        self.skip();
        let col = self.col(self.pos) - 1;
        self.body(col)
    }

    /// `bend.ts` 2472 `parse_terms`: terms up to a `:`.
    fn terms(&mut self) -> Option<Vec<Term>> {
        let mut xs = Vec::new();
        loop {
            xs.push(self.term(0)?);
            self.skip();
            if self.take(":") {
                return Some(xs);
            }
            self.take(",");
        }
    }

    /// `bend.ts` 2376 `parse_body`: a match, or lets then a reply. `col` is the column the statements of the body are measured against.
    pub(crate) fn body(&mut self, col: usize) -> Option<Body> {
        self.skip();
        let beg = self.pos;
        if self.word("match") {
            return self.body_match(beg, col);
        }
        let erased = self.take("-");
        let first = if erased {
            let name = self.name()?;
            Term {
                kind: TermKind::Name(name.text),
                span: name.span,
            }
        } else {
            self.term(0)?
        };
        let mut ts = vec![first];
        self.skip();
        while !self.nl_before()
            && (is_head(self.peek()) || (!erased && self.at("+")))
            && !self.keyword_here()
        {
            ts.push(self.term(0)?);
            self.skip();
        }
        let mark = self.mark();
        let mut ty = None;
        if ts.len() == 1 && self.at(":") && !self.ann.contains(&mark.pos) {
            self.take(":");
            ty = Some(self.term(0)?);
        }
        self.skip();
        if ty.is_some() && !self.at("=") {
            self.ann.insert(mark.pos);
            self.reset(mark);
            ty = None;
        }
        let mut values = Vec::new();
        let mut array_write = false;
        if ty.is_none() && !erased && ts.len() == 1 && !(self.at("=") && !self.at("==")) {
            let Some(array) = self.array_write_target(&ts[0], beg) else {
                return Some(Body::Term(ts.remove(0)));
            };
            if !self.more(self.col(beg)) {
                return Some(Body::Term(ts.remove(0)));
            }
            let write = ts.remove(0);
            values.push(write);
            ts = vec![array];
            array_write = true;
        } else {
            if let Some(g) = ts
                .iter()
                .skip(1)
                .find(|x| self.byte(x.span.start as usize) == b'+')
                && (!self.at("=") || self.at("=="))
            {
                let at = g.span.start as usize;
                return self.fail(
                    "'=' (an operator takes spaces on both sides and a type: (a + b : T))",
                    at,
                    at + 1,
                );
            }
            self.eat("=")?;
        }
        while values.len() < ts.len() {
            values.push(self.term(0)?);
        }
        self.skip();
        self.take(";");
        let rest = self.body(col)?;
        let span = self.span(beg, rest.span().end as usize);
        Some(Body::Let(Let {
            erased,
            pats: ts,
            ty,
            values,
            array_write,
            rest: Box::new(rest),
            span,
        }))
    }

    /// Whether the word that starts at `pos` is a keyword (`bend.ts` 2397: the match of `/^[A-Za-z0-9_.]*/` in `KEYWORDS`).
    fn keyword_here(&self) -> bool {
        let n = self
            .bytes
            .get(self.pos..)
            .map_or(0, |r| r.iter().take_while(|&&b| is_name(b)).count());
        is_keyword(self.slice(self.pos, self.pos + n))
    }

    /// `bend.ts` 2420 `term_write`: the array a statement `a[i] <- v` writes, when `a` is a plain name that starts the statement.
    fn array_write_target(&self, t: &Term, beg: usize) -> Option<Term> {
        match &t.kind {
            TermKind::Index {
                array,
                write: Some(_),
                ..
            } if matches!(array.kind, TermKind::Name(_)) && array.span.start as usize == beg => {
                Some((**array).clone())
            }
            _ => None,
        }
    }

    /// The `match` arm of `parse_body`: `bend.ts` 2379.
    fn body_match(&mut self, beg: usize, col: usize) -> Option<Body> {
        let scrutinees = self.terms()?;
        self.skip();
        let ccol = self.col(self.pos);
        let mut cases = Vec::new();
        while ccol > col && self.at_word("case") && self.col(self.pos) >= ccol {
            let rcol = self.col(self.pos);
            let case_beg = self.pos;
            self.word("case");
            self.skip();
            let qbeg = self.pos;
            let patterns = self.terms()?;
            if patterns.len() != scrutinees.len() {
                let msg = format!("{} patterns (one per scrutinee)", scrutinees.len());
                return self.fail(msg, qbeg, self.pos - 1);
            }
            let body = self.body(rcol)?;
            let span = self.span(case_beg, body.span().end as usize);
            cases.push(Case {
                patterns,
                body,
                span,
            });
        }
        Some(Body::Match(Match {
            scrutinees,
            cases,
            span: self.span_from(beg),
        }))
    }
}
