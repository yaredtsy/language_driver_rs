//! The reader. It mirrors `bend2/bend.ts` function for function (`parse_skip` → `skip`, `parse_name` → `name`, `parse_tele` → `tele`, `parse_book` →
//! `book`), and each function names its `bend.ts` line in its doc comment: a diff of `bend.ts` between two pins says which functions here to touch.
//!
//! `bend` is scanner-less recursive descent that throws at its first error. Here a step that can fail returns `Option` (rule 8) and records what went wrong
//! in `Parser::failure` (`fail`): `None` is the throw. The only catch is in `book`, which resumes at the next declaration (BD-D10).

use std::collections::{BTreeMap, BTreeSet};

use analyzer_core::text::Span;

use crate::tree::{
    Bind, Clause, CtrDecl, Decl, DefBody, DefDecl, Import, LawDecl, Name, ParseError, Quant,
    StrLit, Tree, TypeDecl,
};

/// `bend.ts` 1546 `KEYWORDS`: every word the parser dispatches on is reserved and names nothing (`as` and `unsafe` stay free).
const KEYWORDS: [&str; 16] = [
    "def", "type", "law", "match", "case", "do", "return", "for", "exs", "where", "is", "import",
    "Type", "Data", "Kind", "Quant",
];

/// `bend.ts` 498 `char_is_head`: ASCII letters and `_`.
pub(crate) fn is_head(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// `bend.ts` 503 `char_is_name`: a head, an ASCII digit or `.`. Not `-`.
pub(crate) fn is_name(b: u8) -> bool {
    is_head(b) || b.is_ascii_digit() || b == b'.'
}

pub(crate) fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

/// What a `None` was about: the parser's `parse_fail` (1565) without the throw.
struct Failure {
    beg: usize,
    expected: String,
    observed: String,
}

pub(crate) struct Parser<'a> {
    text: &'a str,
    pub(crate) bytes: &'a [u8],
    pub(crate) pos: usize,
    /// Where the text really starts: past a byte-order mark. `bend.ts` trims one off an import line but does not skip one before a declaration, so a
    /// file that starts with one would have no outline at all; here it is whitespace at the start of the file.
    bom: usize,
    /// The end of the last token before the latest `skip` that moved: where a term or a declaration really ended, before the whitespace and comments
    /// that `skip` passed on the way to the next token.
    pub(crate) tail: usize,
    skip_end: usize,
    /// Every comment, start → end. A map so that reading the same text twice (a rewind) records it once.
    comments: BTreeMap<usize, usize>,
    /// Positions where a typed let was tried and did not hold (`bend.ts` `p.ann`): without it, nested annotations are read twice at every level.
    pub(crate) ann: BTreeSet<usize>,
    failure: Option<Failure>,
}

/// What `scan_lines` found: where it stopped (a line start, or the end of the text), and the end of the last code it passed.
/// A place in the text that `Parser::reset` can go back to.
#[derive(Clone, Copy)]
pub(crate) struct Mark {
    pub(crate) pos: usize,
    tail: usize,
    skip_end: usize,
}

pub(crate) struct Scan {
    pub(crate) stop: usize,
    pub(crate) last: Option<usize>,
}

/// Never fails, never panics (BD1 §2.1).
pub(crate) fn parse(text: &str) -> Tree {
    let mut p = Parser::new(text);
    let mut errors = Vec::new();
    if u32::try_from(text.len()).is_err() {
        errors.push(ParseError {
            span: Span::new(0, 0),
            expected: "a text of at most 4 GiB (spans are 32-bit)".to_string(),
            observed: format!("{} bytes", text.len()),
        });
        return Tree {
            imports: Vec::new(),
            decls: Vec::new(),
            comments: Vec::new(),
            errors,
        };
    }
    let imports = p.imports(&mut errors);
    let decls = p.book(&mut errors);
    let comments = p
        .comments
        .iter()
        .map(|(&beg, &end)| p.span(beg, end))
        .collect();
    Tree {
        imports,
        decls,
        comments,
        errors,
    }
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        let bom = if text.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };
        Parser {
            text,
            bytes: text.as_bytes(),
            pos: bom,
            tail: bom,
            bom,
            skip_end: usize::MAX,
            comments: BTreeMap::new(),
            ann: BTreeSet::new(),
            failure: None,
        }
    }

    /// Byte offsets are `u32` in a `Span`. `parse` refuses a text that does not fit, so the clamp here is never reached.
    pub(crate) fn span(&self, beg: usize, end: usize) -> Span {
        let to_u32 = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Span::new(to_u32(beg), to_u32(end))
    }

    pub(crate) fn slice(&self, beg: usize, end: usize) -> &'a str {
        self.text.get(beg..end).unwrap_or("")
    }

    /// `bend.ts` 1565 `parse_fail`. Records the failure and returns `None`, which is how every caller up to `book` gives up.
    pub(crate) fn fail<T>(
        &mut self,
        expected: impl Into<String>,
        beg: usize,
        end: usize,
    ) -> Option<T> {
        let observed = if beg < end {
            format!("'{}'", self.slice(beg, end))
        } else if let Some(c) = self
            .text
            .get(self.pos..)
            .and_then(|rest| rest.chars().next())
        {
            format!("'{c}'")
        } else {
            "end of input".to_string()
        };
        self.failure = Some(Failure {
            beg,
            expected: expected.into(),
            observed,
        });
        None
    }

    /// `bend.ts` 1570 `parse_peek`; a NUL stands for the end of the text, since no token starts with one.
    pub(crate) fn peek(&self) -> u8 {
        self.bytes.get(self.pos).copied().unwrap_or(0)
    }

    pub(crate) fn byte(&self, at: usize) -> u8 {
        self.bytes.get(at).copied().unwrap_or(0)
    }

    /// `bend.ts` 1580 `parse_at`.
    pub(crate) fn at(&self, s: &str) -> bool {
        self.bytes
            .get(self.pos..)
            .is_some_and(|rest| rest.starts_with(s.as_bytes()))
    }

    /// `bend.ts` 1587 `parse_take`.
    pub(crate) fn take(&mut self, s: &str) -> bool {
        let found = self.at(s);
        if found {
            self.pos += s.len();
        }
        found
    }

    /// `bend.ts` 1613 `parse_eat`.
    pub(crate) fn eat(&mut self, s: &str) -> Option<()> {
        self.skip();
        if self.take(s) {
            Some(())
        } else {
            self.fail(format!("'{s}'"), self.pos, self.pos)
        }
    }

    /// `bend.ts` 1595 `parse_skip`: whitespace and `#` comments. Also records every comment's span, which `bend` throws away.
    pub(crate) fn skip(&mut self) {
        if self.pos != self.skip_end {
            self.tail = self.pos;
        }
        while let Some(&b) = self.bytes.get(self.pos) {
            match b {
                b' ' | b'\n' | b'\r' | b'\t' => self.pos += 1,
                b'#' => self.comment(),
                _ => break,
            }
        }
        self.skip_end = self.pos;
    }

    /// The comment that starts at `pos`, read to the end of its line.
    pub(crate) fn comment(&mut self) {
        let beg = self.pos;
        while self.bytes.get(self.pos).is_some_and(|&b| b != b'\n') {
            self.pos += 1;
        }
        self.record_comment(beg, self.pos);
    }

    /// A comment's recorded span does not include the `\r` of a `\r\n`.
    pub(crate) fn record_comment(&mut self, beg: usize, end: usize) {
        let end = if end > beg + 1 && self.bytes.get(end - 1) == Some(&b'\r') {
            end - 1
        } else {
            end
        };
        self.comments.insert(beg, end);
    }

    /// `bend.ts` 1620 `parse_at_word`.
    pub(crate) fn at_word(&mut self, w: &str) -> bool {
        self.skip();
        self.at(w) && !is_name(self.byte(self.pos + w.len()))
    }

    /// `bend.ts` 1628 `parse_word`.
    pub(crate) fn word(&mut self, w: &str) -> bool {
        let found = self.at_word(w);
        if found {
            self.pos += w.len();
        }
        found
    }

    /// `bend.ts` 1636 `parse_lexeme`: a name's characters. A dot must be followed by a letter or `_` (`a.1` and `a.` are no names).
    pub(crate) fn lexeme(&mut self) -> Option<Name> {
        self.skip();
        if !is_head(self.peek()) {
            return self.fail("a name", self.pos, self.pos);
        }
        let beg = self.pos;
        while is_name(self.peek()) {
            self.pos += 1;
        }
        let text = self.slice(beg, self.pos);
        let bad_dot = text
            .bytes()
            .enumerate()
            .any(|(i, b)| b == b'.' && !is_head(text.as_bytes().get(i + 1).copied().unwrap_or(0)));
        if bad_dot {
            return self.fail(
                format!("a name (words joined by dots, got '{text}')"),
                beg,
                self.pos,
            );
        }
        Some(Name {
            text: text.to_string(),
            span: self.span(beg, self.pos),
        })
    }

    /// `bend.ts` 1652 `parse_name`: a lexeme that is not a keyword.
    pub(crate) fn name(&mut self) -> Option<Name> {
        let k = self.lexeme()?;
        if is_keyword(&k.text) {
            return self.fail(
                format!("a name (got the keyword '{}')", k.text),
                self.pos - k.text.len(),
                self.pos,
            );
        }
        Some(k)
    }

    /// `bend.ts` 1750 `parse_quant`.
    pub(crate) fn quant(&mut self) -> Quant {
        self.skip();
        if self.take("-") {
            Quant::Erased
        } else if self.take("+") {
            Quant::Reusable
        } else {
            Quant::Plain
        }
    }

    /// `bend.ts` 1557 `parse_col`: the 1-based column of `at`, in bytes.
    pub(crate) fn col(&self, at: usize) -> usize {
        at.saturating_sub(self.line_start(at)) + 1
    }

    /// The end of what has been read: where the last token ended. Right after a `skip` that is `tail` (the whitespace and comments it passed are not
    /// part of the term before them); after a token taken without one it is `pos`.
    pub(crate) fn end(&self) -> usize {
        if self.pos == self.skip_end {
            self.tail
        } else {
            self.pos
        }
    }

    /// `bend.ts` 1561 `parse_span`: from `beg` to the end of what has been read.
    pub(crate) fn span_from(&self, beg: usize) -> Span {
        self.span(beg, self.end())
    }

    pub(crate) fn text_at(&self, at: usize) -> Option<&'a str> {
        self.text.get(at..)
    }

    /// The reader's place, for a rewind (`bend.ts` `[p.pos, …] = [at, …]`, 2411).
    pub(crate) fn mark(&self) -> Mark {
        Mark {
            pos: self.pos,
            tail: self.tail,
            skip_end: self.skip_end,
        }
    }

    pub(crate) fn reset(&mut self, m: Mark) {
        (self.pos, self.tail, self.skip_end) = (m.pos, m.tail, m.skip_end);
    }

    fn line_start(&self, at: usize) -> usize {
        self.bytes
            .get(..at)
            .and_then(|before| before.iter().rposition(|&b| b == b'\n'))
            .map_or(self.bom, |i| i + 1)
    }

    /// `bend.ts` 2487 `parse_tele`: a parameter list, up to and including its `close`. `out` keeps the binders read so far if it gives up.
    pub(crate) fn tele(&mut self, close: u8, out: &mut Vec<Bind>) -> Option<()> {
        let close_text = char::from(close).to_string();
        let first = out.len();
        let mut templates = 0usize;
        loop {
            self.skip();
            if self.take(&close_text) {
                return Some(());
            }
            if close == b')' && self.at("~") && templates < out.len() - first {
                return self.fail(
                    "a plain binder (only leading binders take ~)",
                    self.pos,
                    self.pos,
                );
            }
            let beg = self.pos;
            let template = close == b')' && self.take("~");
            let quant = if template { Quant::Plain } else { self.quant() };
            let name = self.name()?;
            if close == b'}' && out[first..].iter().any(|b| b.name.text == name.text) {
                return self.fail(
                    format!("a fresh field name (duplicate declaration: {})", name.text),
                    name.span.start as usize,
                    self.pos,
                );
            }
            self.skip();
            let bare = quant == Quant::Plain && !template && !self.at(":");
            let ty = if bare {
                None
            } else {
                self.eat(":")?;
                Some(self.term(0)?)
            };
            let end = if bare {
                name.span.end as usize
            } else {
                self.end()
            };
            out.push(Bind {
                quant,
                template,
                name,
                ty,
                span: self.span(beg, end),
            });
            templates += usize::from(template);
            self.skip();
            self.take(",");
        }
    }

    /// The imports at the head of the file: `bend.ts` 973–1000, inside `book_load`. It reads lines with a regex, not with `parse_*`, and so does this: a
    /// blank or `#` line, or an `import` line, until the first line that is none of them. Leaves `pos` at the start of that line.
    fn imports(&mut self, errors: &mut Vec<ParseError>) -> Vec<Import> {
        let mut out = Vec::new();
        let mut ls = self.bom;
        while ls < self.bytes.len() {
            let le = self
                .bytes
                .get(ls..)
                .and_then(|rest| rest.iter().position(|&b| b == b'\n'))
                .map_or(self.bytes.len(), |i| ls + i);
            let line = self.slice(ls, le);
            let trimmed = line.trim();
            let off = ls + (line.len() - line.trim_start().len());
            if trimmed.is_empty() {
                ls = le + 1;
                continue;
            }
            if trimmed.starts_with('#') {
                self.record_comment(off, off + trimmed.len());
                ls = le + 1;
                continue;
            }
            let is_import = trimmed
                .strip_prefix("import")
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace));
            if !is_import {
                break;
            }
            match self.import_line(trimmed, off) {
                Some(import) => out.push(import),
                None => errors.push(ParseError {
                    span: self.span(off, off + trimmed.len()),
                    expected: "an import ('import Base', or 'import <path> as <Name>')".to_string(),
                    observed: format!("'{trimmed}'"),
                }),
            }
            ls = le + 1;
        }
        self.pos = ls.min(self.bytes.len());
        out
    }

    /// The regex of `book_load`: `^import\s+(\S+)(?:\s+as\s+([A-Za-z_]\w*))?\s*(?:#.*)?$` on the trimmed line, and its rule that a path with no alias
    /// is only `Base`. `off` is where the trimmed line starts in the text. A trailing comment is recorded.
    fn import_line(&mut self, trimmed: &str, off: usize) -> Option<Import> {
        let after_kw = trimmed.strip_prefix("import")?;
        if !after_kw.starts_with(char::is_whitespace) {
            return None;
        }
        let rest = after_kw.trim_start();
        let path_len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let (path, tail) = rest.split_at(path_len);
        if path.is_empty() {
            return None;
        }
        let at = |s: &str| off + (trimmed.len() - s.len());
        let path_span = self.span(at(rest), at(rest) + path.len());
        let after_path = tail.trim_start();
        let mut alias = None;
        let mut end = at(rest) + path.len();
        let mut remaining = after_path;
        if let Some(after_as) = after_path
            .strip_prefix("as")
            .filter(|s| tail.starts_with(char::is_whitespace) && s.starts_with(char::is_whitespace))
        {
            let name_part = after_as.trim_start();
            let len = name_part
                .bytes()
                .take_while(|&b| b.is_ascii_alphanumeric() || b == b'_')
                .count();
            let (a, after_alias) = name_part.split_at(len);
            if !a.bytes().next().is_some_and(is_head) {
                return None;
            }
            let a_beg = at(name_part);
            alias = Some(Name {
                text: a.to_string(),
                span: self.span(a_beg, a_beg + a.len()),
            });
            end = a_beg + a.len();
            remaining = after_alias.trim_start();
        }
        if remaining.starts_with('#') {
            let c = at(remaining);
            self.record_comment(c, c + remaining.len());
        } else if !remaining.is_empty() {
            return None;
        }
        if alias.is_none() && path != "Base" {
            return None;
        }
        Some(Import {
            path: path.to_string(),
            path_span,
            alias,
            span: self.span(off, end),
        })
    }

    /// `bend.ts` 2593 `parse_book`: the declarations. Where `bend` throws, this records a `ParseError` and resumes at the next line that starts at column 0
    /// with `def`, `type`, `law`, `@unsafe` or `import` (BD-D10); the broken declaration keeps its name, its range and a `syntax_error` flag.
    fn book(&mut self, errors: &mut Vec<ParseError>) -> Vec<Decl> {
        let mut decls = Vec::new();
        loop {
            self.skip();
            if self.pos >= self.bytes.len() {
                return decls;
            }
            let start = self.pos;
            let decl = self.decl(start);
            let Some(f) = self.failure.take() else {
                decls.extend(decl);
                continue;
            };
            let floor = self
                .tail
                .max(decl.as_ref().map_or(start, |d| d.span().end as usize));
            let scan = self.scan_lines(f.beg.max(start), start);
            let last = scan.last.unwrap_or(0).max(floor);
            if let Some(mut d) = decl {
                d.set_span(self.span(start, last));
                decls.push(d);
            }
            errors.push(ParseError {
                span: self.span(f.beg, scan.last.map_or(f.beg, |l| l.max(f.beg))),
                expected: f.expected,
                observed: f.observed,
            });
            self.pos = scan.stop;
        }
    }

    /// One declaration, from its first token: the dispatch of `parse_book`'s loop. `None` with a failure recorded when not even its name was read; a
    /// `Some` with a failure recorded is a broken declaration (`syntax_error` set).
    fn decl(&mut self, start: usize) -> Option<Decl> {
        if self.take("@") {
            if !self.word("unsafe") {
                return self.fail("'unsafe' (the one decorator)", self.pos, self.pos);
            }
            if !self.word("def") {
                return self.fail("'def' (@unsafe marks the def below it)", self.pos, self.pos);
            }
            return self.def(start, true);
        }
        if self.word("def") {
            return self.def(start, false);
        }
        if self.word("type") {
            return self.type_decl(start);
        }
        if self.word("law") {
            return self.law(start);
        }
        self.fail("'def', 'type' or 'law'", self.pos, self.pos)
    }

    /// `bend.ts` 2535 `parse_def`.
    fn def(&mut self, start: usize, unsafe_: bool) -> Option<Decl> {
        let name = self.name()?;
        let mut d = DefDecl {
            span: self.span(start, name.span.end as usize),
            name,
            unsafe_,
            question: false,
            params: Vec::new(),
            ret: None,
            body: None,
            doc: self.doc_above(start),
            syntax_error: false,
        };
        d.syntax_error = self.def_rest(&mut d, start).is_none();
        Some(Decl::Def(d))
    }

    fn def_rest(&mut self, d: &mut DefDecl, start: usize) -> Option<()> {
        d.question = self.take("?");
        d.unsafe_ |= d.question;
        self.eat("(")?;
        self.tele(b')', &mut d.params)?;
        self.skip();
        if self.take("->") {
            d.ret = Some(self.term(0)?);
        }
        self.eat(":")?;
        let colon_end = self.pos;
        d.span = self.span(start, colon_end);
        if self.at_word("import") {
            if d.params.iter().any(|b| b.template) {
                return self.fail("a body (a template is not foreign)", self.pos, self.pos);
            }
            let mut lits = Vec::new();
            while self.word("import") {
                self.eat("\"")?;
                let beg = self.pos - 1;
                while self.peek() != b'"' && self.pos < self.bytes.len() {
                    self.pos += 1;
                }
                let value = self.slice(beg + 1, self.pos);
                if self.pos >= self.bytes.len() {
                    // the error starts at the opening quote: that is where reading resumes from
                    return self.fail("a closing \"", beg, beg + 1);
                }
                self.eat("\"")?;
                if !(value.ends_with(".c") || value.ends_with(".js")) {
                    return self.fail("a .c or .js path", beg, self.pos);
                }
                lits.push(StrLit {
                    value: value.to_string(),
                    span: self.span(beg, self.pos),
                });
                d.span = self.span(start, self.pos);
            }
            d.body = Some(DefBody::Foreign(lits));
            return Some(());
        }
        let body = self.body(0)?;
        d.span = self.span(start, body.span().end as usize);
        d.body = Some(DefBody::Code(body));
        Some(())
    }

    /// The `type` arm of `parse_book` (2603): name, `<params>`, `is`, the kind, `:`, then constructors until a character that cannot start one.
    fn type_decl(&mut self, start: usize) -> Option<Decl> {
        let name = self.name()?;
        let mut d = TypeDecl {
            span: self.span(start, name.span.end as usize),
            name,
            params: Vec::new(),
            kind: None,
            ctrs: Vec::new(),
            doc: self.doc_above(start),
            syntax_error: false,
        };
        d.syntax_error = self.type_rest(&mut d, start).is_none();
        Some(Decl::Type(d))
    }

    fn type_rest(&mut self, d: &mut TypeDecl, start: usize) -> Option<()> {
        self.skip();
        if self.take("<") {
            self.tele(b'>', &mut d.params)?;
        }
        if !self.word("is") {
            return self.fail("'is'", self.pos, self.pos);
        }
        d.kind = Some(self.term(0)?);
        self.eat(":")?;
        d.span = self.span(start, self.pos);
        loop {
            self.skip();
            if !is_head(self.peek()) || ["def", "type", "law"].iter().any(|w| self.at_word(w)) {
                break;
            }
            let name = self.name()?;
            let beg = name.span.start as usize;
            self.eat("{")?;
            let mut fields = Vec::new();
            let closed = self.tele(b'}', &mut fields);
            d.ctrs.push(CtrDecl {
                name,
                fields,
                span: self.span(beg, self.pos),
            });
            closed?;
            d.span = self.span(start, self.pos);
        }
        Some(())
    }

    /// The `law` arm of `parse_book` (2631): name, `:`, the clauses, then the claim, a body.
    fn law(&mut self, start: usize) -> Option<Decl> {
        let name = self.name()?;
        let mut d = LawDecl {
            span: self.span(start, name.span.end as usize),
            name,
            clauses: Vec::new(),
            claim: None,
            doc: self.doc_above(start),
            syntax_error: false,
        };
        d.syntax_error = self.law_rest(&mut d, start).is_none();
        Some(Decl::Law(d))
    }

    fn law_rest(&mut self, d: &mut LawDecl, start: usize) -> Option<()> {
        self.eat(":")?;
        d.span = self.span(start, self.pos);
        let mut templates = 0usize;
        while self.at_word("for") || self.at_word("exs") {
            let beg = self.pos;
            let all = self.word("for");
            if !all {
                self.word("exs");
            }
            self.skip();
            if all && self.at("~") && templates < d.clauses.len() {
                return self.fail(
                    "a plain clause (only leading clauses take ~)",
                    self.pos,
                    self.pos,
                );
            }
            let template = all && self.take("~");
            let quant = if template || !all {
                Quant::Plain
            } else {
                self.quant()
            };
            let name = self.name()?;
            self.eat(":")?;
            let ty = self.term(0)?;
            let where_ = if self.word("where") {
                Some(self.term(0)?)
            } else {
                None
            };
            templates += usize::from(template);
            d.clauses.push(Clause {
                exists: !all,
                quant,
                template,
                name,
                ty,
                where_,
                span: self.span(beg, self.end()),
            });
            d.span = self.span(start, self.end());
        }
        let claim = self.block()?;
        d.span = self.span(start, claim.span().end as usize);
        d.claim = Some(claim);
        Some(())
    }

    /// The recovery scan (BD-D10): passes lines from `from` (which may be in the middle of a line) up to the first line that starts at column 0 with `def`,
    /// `type`, `law`, `@unsafe` or `import`, or the end of the text. Blank and comment-only lines pass. Strings and chars are read as one token on their line, so a `#` inside one is
    /// not a comment. Every comment on the way is recorded.
    pub(crate) fn scan_lines(&mut self, from: usize, decl_start: usize) -> Scan {
        let n = self.bytes.len();
        let mut i = from;
        let mut last = None;
        if i > decl_start && self.is_resync_line(i) {
            return Scan { stop: i, last };
        }
        while i < n {
            match self.byte(i) {
                b'\n' => {
                    i += 1;
                    if self.is_resync_line(i) {
                        return Scan { stop: i, last };
                    }
                }
                quote @ (b'"' | b'\'') => {
                    // a string or a char runs to its close or to the end of its line: a body is read for real, so a string that spans lines
                    // is not looked for here, and one that never closes cannot swallow the declarations below it
                    let mut j = i + 1;
                    while j < n && self.byte(j) != b'\n' {
                        match self.byte(j) {
                            b'\\' if self.byte(j + 1) != b'\n' => j += 2,
                            c if c == quote => {
                                j += 1;
                                break;
                            }
                            _ => j += 1,
                        }
                    }
                    let end = j.min(n);
                    last = Some(end);
                    i = end;
                }
                b'#' => {
                    self.pos = i;
                    self.comment();
                    i = self.pos;
                }
                b' ' | b'\t' | b'\r' => i += 1,
                _ => {
                    last = Some(i + 1);
                    i += 1;
                }
            }
        }
        Scan { stop: n, last }
    }

    /// Whether `at` starts a line that begins at column 0 with `def`, `type`, `law`, `@unsafe` or `import`.
    fn is_resync_line(&self, at: usize) -> bool {
        if at != 0 && at != self.bom && self.bytes.get(at - 1) != Some(&b'\n') {
            return false;
        }
        let rest = self.bytes.get(at..).unwrap_or(&[]);
        let word_at = |w: &str| {
            rest.starts_with(w.as_bytes()) && !is_name(rest.get(w.len()).copied().unwrap_or(0))
        };
        ["def", "type", "law", "import", "@unsafe"]
            .iter()
            .any(|w| word_at(w))
    }

    /// The comment block directly above the declaration that starts at `start`: comment-only lines with no blank line between them and the declaration,
    /// from the first `#` of the topmost to the end of the last. `None` when the declaration is not the first thing on its line.
    fn doc_above(&self, start: usize) -> Option<Span> {
        let ls = self.line_start(start);
        if self
            .bytes
            .get(ls..start)?
            .iter()
            .any(|b| !matches!(b, b' ' | b'\t'))
        {
            return None;
        }
        let (mut beg, mut end) = (None, None);
        let mut line = ls;
        while line > self.bom {
            let prev_end = line - 1;
            let prev_start = self.line_start(prev_end);
            let text = self.bytes.get(prev_start..prev_end)?;
            let text_end = if text.last() == Some(&b'\r') {
                prev_end - 1
            } else {
                prev_end
            };
            let indent = text
                .iter()
                .take_while(|b| matches!(b, b' ' | b'\t'))
                .count();
            if text.get(indent) != Some(&b'#') {
                break;
            }
            beg = Some(prev_start + indent);
            end.get_or_insert(text_end);
            line = prev_start;
        }
        Some(self.span(beg?, end?))
    }
}
