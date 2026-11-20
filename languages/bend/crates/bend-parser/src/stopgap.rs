//! STOPGAP for BD1 step 3, deleted in step 4, when `term` reads terms and builds them.
//!
//! A parameter's type, a return type, a type's kind and a law clause's type are terms, and `tele` has to get past each of them to the next binder. Step 3 has
//! no term parser yet, so this reads **where a term ends** without building it: the shape of `bend.ts` 1810 `parse_term` / 1818 `parse_term_base` /
//! 2101 `parse_term_ops` (what continues a term, what ends it), with every bracketed group passed over as a balanced run instead of read. It is the reader
//! of step 4 with the tree taken out, so a mistake in it shows in the corpus test as a declaration that could not be read.
//!
//! What it does not read, it refuses (`None`): a `do` block and a `%` rewrite as a header term. Neither occurs in a header in the corpus.

use crate::parse::{Parser, is_head, is_keyword, is_name};

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

impl Parser<'_> {
    /// `bend.ts` 1810 `parse_term`, without the tree.
    pub(crate) fn skip_term(&mut self, lvl: i32) -> Option<()> {
        self.skip();
        self.skip_term_base()?;
        self.skip_term_ops(lvl)
    }

    /// `bend.ts` 1818 `parse_term_base`.
    fn skip_term_base(&mut self) -> Option<()> {
        let beg = self.pos;
        let c = self.peek();
        if is_head(c) {
            let k = self.lexeme()?;
            match k.text.as_str() {
                "Type" | "Data" | "Quant" => return Some(()),
                "Kind" => {
                    self.eat("(")?;
                    self.skip_term(0)?;
                    return self.eat(")");
                }
                "do" => return self.fail("a term (a do block is read in step 4)", beg, self.pos),
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
                _ => {}
            }
            if self.take("{") {
                return self.skip_balanced(b'}');
            }
            return Some(());
        }
        if c.is_ascii_digit() {
            return self.skip_number();
        }
        match c {
            b'@' => self.skip_all(false),
            b'&' => {
                if matches!(self.byte(self.pos + 1), b'0'..=b'2') {
                    self.pos += 2;
                    Some(())
                } else {
                    self.skip_all(true)
                }
            }
            b'+' => {
                self.pos += 1;
                self.skip_term(12)
            }
            b'\\' => {
                self.pos += 1;
                self.eat("{")?;
                self.skip_balanced(b'}')
            }
            b'%' => self.fail("a term (a rewrite is read in step 4)", beg, self.pos + 1),
            b'{' => {
                self.pos += 1;
                self.skip_balanced(b'}')
            }
            b'(' => {
                self.pos += 1;
                self.skip_balanced(b')')
            }
            b'[' => {
                self.pos += 1;
                self.skip_balanced(b']')
            }
            b'\'' | b'"' => match self.quoted_end(self.pos) {
                Some(end) => {
                    self.pos = end;
                    Some(())
                }
                None => self.fail(format!("a closing {}", char::from(c)), self.pos, self.pos),
            },
            b'?' => {
                self.pos += 1;
                self.name().map(|_| ())
            }
            _ => self.fail("a term", self.pos, self.pos),
        }
    }

    /// `bend.ts` 2241 `parse_term_all`: `@ Quant Name : A -> B` and `& Name : A -> B`.
    fn skip_all(&mut self, exi: bool) -> Option<()> {
        self.pos += 1;
        if !exi {
            self.quant();
        }
        self.name()?;
        self.eat(":")?;
        self.skip_term(1)?;
        self.eat("->")?;
        self.skip_term(0)
    }

    /// `bend.ts` 2278 `parse_term_num`: `(\d+)(n|\.\d+([eE][+-]?\d+)?)?`, and the name characters that may not follow.
    fn skip_number(&mut self) -> Option<()> {
        while self.peek().is_ascii_digit() {
            self.pos += 1;
        }
        match self.peek() {
            b'n' => {
                self.pos += 1;
                if self.take("+") {
                    return self.skip_term(0);
                }
                if is_name(self.peek()) {
                    return self.fail("a nat literal (NUMBER n)", self.pos, self.pos);
                }
                Some(())
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
                Some(())
            }
            _ if is_name(self.peek()) => self.fail(
                "a numeric literal (NUMBER is U32, NUMBER n is Nat)",
                self.pos,
                self.pos,
            ),
            _ => Some(()),
        }
    }

    /// `bend.ts` 2088 `parse_nl`: only whitespace between the start of the line and here.
    fn nl_before(&self) -> bool {
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
    fn skip_term_ops(&mut self, lvl: i32) -> Option<()> {
        loop {
            self.skip();
            if self.nl_before() && (self.at("(") || self.at("[")) {
                return Some(());
            }
            if self.at("(") || self.at("!(") {
                if self.take("!") {
                    continue;
                }
                self.pos += 1;
                self.skip_balanced(b')')?;
                continue;
            }
            if self.at("[") {
                self.pos += 1;
                self.skip_balanced(b']')?;
                self.skip();
                if !self.nl_before() && self.take("<-") {
                    self.skip_term(2)?;
                }
                continue;
            }
            if lvl == 0 && self.take("=>") {
                self.skip_term(0)?;
                continue;
            }
            let Some(op) = self.infix_at() else {
                return Some(());
            };
            let (prc, right) = infix(op);
            let nx = self.byte(self.pos + op.len());
            let glued = self.pos > 0 && !self.byte(self.pos - 1).is_ascii_whitespace();
            let ends = (prc < lvl && !(op == "<" && glued))
                || ((op == "-" || op == "+") && (nx == b'>' || is_head(nx)))
                || (op.starts_with('>') && glued)
                || (op == "%" && !nx.is_ascii_whitespace());
            if ends {
                return Some(());
            }
            self.pos += op.len();
            self.skip_term(if right { prc } else { prc + 1 })?;
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
                self.take(",");
                self.skip_args(b'>')?;
            }
        }
    }

    /// `bend.ts` 2227 `parse_term_args`.
    fn skip_args(&mut self, close: u8) -> Option<()> {
        let close_text = char::from(close).to_string();
        loop {
            self.skip();
            if self.take(&close_text) {
                return Some(());
            }
            self.skip_term(0)?;
            self.skip();
            self.take(",");
        }
    }

    /// Passes a bracketed group whose opener was just taken, to its `close`: nested brackets, strings, chars and comments are read as tokens, the rest is
    /// not looked at (what is inside is a body in `bend.ts`, 2260 `parse_term_tup`).
    fn skip_balanced(&mut self, close: u8) -> Option<()> {
        let mut open = vec![close];
        while let Some(&want) = open.last() {
            let b = self.byte(self.pos);
            if self.pos >= self.bytes.len() {
                return self.fail(format!("'{}'", char::from(want)), self.pos, self.pos);
            }
            match b {
                b'"' | b'\'' => match self.quoted_end(self.pos) {
                    Some(end) => self.pos = end,
                    None => {
                        return self.fail(
                            format!("a closing {}", char::from(b)),
                            self.pos,
                            self.pos,
                        );
                    }
                },
                b'#' => self.comment(),
                b'(' | b'[' | b'{' => {
                    open.push(match b {
                        b'(' => b')',
                        b'[' => b']',
                        _ => b'}',
                    });
                    self.pos += 1;
                }
                b')' | b']' | b'}' => {
                    if b != want {
                        return self.fail(
                            format!("'{}'", char::from(want)),
                            self.pos,
                            self.pos + 1,
                        );
                    }
                    open.pop();
                    self.pos += 1;
                }
                _ => self.pos += 1,
            }
        }
        Some(())
    }
}
