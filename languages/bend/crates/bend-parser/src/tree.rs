//! The tree: what the user wrote, at the surface level (docs/08-bend/02-phase-1-parsing.md §2.2). Sugar is kept, nothing is desugared, every node has a
//! [`Span`] (byte offsets into the text that was parsed), a dotted name is one [`Name`].
//!
//! Terms and bodies are the surface forms of `bend.ts`: `f(a, b)` is one [`TermKind::Call`] with two arguments, an operator keeps the operator that was
//! written, a list stays a list. Nothing is resolved: a [`TermKind::Name`] is a name, whether it is a binder, a def, Base's or a constructor is for the
//! analysis (BD4); `+x` is a [`TermKind::Plus`] that the walk decides; a `~` is accepted on any call argument. Binder scopes are not computed.

use analyzer_core::text::Span;

/// The answer of [`crate::parse`]: always a tree, never a failure. What could not be read is in `errors`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Tree {
    /// The `import` lines at the head of the file, in order. Nothing was read from them: the paths come back as written.
    pub imports: Vec<Import>,
    /// The declarations in source order, including broken ones (flagged `syntax_error`, see [`DefDecl::syntax_error`]).
    pub decls: Vec<Decl>,
    /// Every `#` comment, from the `#` to the end of the line (a `\r` before the `\n` is not part of it), in source order. `bend` throws them away;
    /// markers and docs need them.
    pub comments: Vec<Span>,
    /// What could not be read, in source order. Recovery is at declaration boundaries only (BD-D10): see [`ParseError`].
    pub errors: Vec<ParseError>,
}

/// One `import` line: `import Base` or `import <path> as <Alias>` (`bend.ts` `book_load`, a regex on the trimmed line, not `parse_*`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Import {
    /// The path as written: `Base`, `./m.bend`, `../lib/x.bend`. Not checked beyond the line's shape; resolving it is `bend-analysis`'s.
    pub path: String,
    pub path_span: Span,
    pub alias: Option<Name>,
    /// From `import` to the end of the alias (or of the path): a trailing comment is not part of it.
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decl {
    Type(TypeDecl),
    Def(DefDecl),
    Law(LawDecl),
}

impl Decl {
    pub fn name(&self) -> &Name {
        match self {
            Decl::Type(d) => &d.name,
            Decl::Def(d) => &d.name,
            Decl::Law(d) => &d.name,
        }
    }

    /// From the first token of the declaration (`@`, `def`, `type`, `law`) to the last byte of what was read of it. The comment block above is not in it:
    /// it is `doc`.
    pub fn span(&self) -> Span {
        match self {
            Decl::Type(d) => d.span,
            Decl::Def(d) => d.span,
            Decl::Law(d) => d.span,
        }
    }

    pub(crate) fn set_span(&mut self, span: Span) {
        match self {
            Decl::Type(d) => d.span = span,
            Decl::Def(d) => d.span = span,
            Decl::Law(d) => d.span = span,
        }
    }

    pub fn doc(&self) -> Option<Span> {
        match self {
            Decl::Type(d) => d.doc,
            Decl::Def(d) => d.doc,
            Decl::Law(d) => d.doc,
        }
    }

    pub fn syntax_error(&self) -> bool {
        match self {
            Decl::Type(d) => d.syntax_error,
            Decl::Def(d) => d.syntax_error,
            Decl::Law(d) => d.syntax_error,
        }
    }
}

/// `("@unsafe")? "def" Name "?"? "(" [Bind ","?] ")" ("->" Term)? ":" (Body | ["import" STRING]+)`
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DefDecl {
    pub name: Name,
    /// Written `@unsafe` or with the `?` sugar: the flag is about proofs, not about what the def calls.
    pub unsafe_: bool,
    /// Written `def f?(…)`. `unsafe_` is also true then.
    pub question: bool,
    pub params: Vec<Bind>,
    /// The `-> T` of the header. `None` is the form that fills a law (`def f(x):`, its parameters bare names): the parser cannot tell whether such a law
    /// exists, that is a book's question.
    pub ret: Option<Term>,
    /// `None` only when the declaration is broken before its body (`syntax_error`).
    pub body: Option<DefBody>,
    pub span: Span,
    /// The `#` comment block directly above the declaration (no blank line between), from its first `#` to the end of its last line.
    pub doc: Option<Span>,
    /// The declaration could not be read to its end. The name and the parameters read so far are kept, `span` runs to the last code before the
    /// next declaration, and an entry of [`Tree::errors`] says why.
    pub syntax_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DefBody {
    /// The body: lets, then a reply; or a match.
    Code(Body),
    /// `import "./e.c"` lines: a foreign effect, code we cannot read.
    Foreign(Vec<StrLit>),
}

/// A `"…"` read raw to the next `"` (no escapes): a foreign import's path.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StrLit {
    /// Without the quotes.
    pub value: String,
    /// With the quotes.
    pub span: Span,
}

/// `"type" Name ("<" [Bind ","?] ">")? "is" Term ":" [Ctr]`
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TypeDecl {
    pub name: Name,
    pub params: Vec<Bind>,
    /// The term after `is`: `Data`, `Kind(a)`, …
    pub kind: Option<Term>,
    pub ctrs: Vec<CtrDecl>,
    pub span: Span,
    pub doc: Option<Span>,
    pub syntax_error: bool,
}

/// `Name "{" [Bind ","?] "}"`: a constructor. Not a def (BD-D3).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CtrDecl {
    pub name: Name,
    pub fields: Vec<Bind>,
    /// From the name to the closing `}`.
    pub span: Span,
}

/// `"law" Name ":" [Clause] Body`
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LawDecl {
    pub name: Name,
    pub clauses: Vec<Clause>,
    /// The claim after the clauses.
    pub claim: Option<Body>,
    pub span: Span,
    pub doc: Option<Span>,
    pub syntax_error: bool,
}

/// `("for" (Quant | "~") | "exs") Name ":" Term ("where" Term)?`
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Clause {
    /// `exs` (an existential) rather than `for`.
    pub exists: bool,
    pub quant: Quant,
    /// `for ~k: T`.
    pub template: bool,
    pub name: Name,
    pub ty: Term,
    pub where_: Option<Term>,
    /// From `for` / `exs` to the end of the clause.
    pub span: Span,
}

/// A parameter, a type parameter or a constructor field: `+x: A`, `~f: A -> B`, `-A: Kind(a)`, or a bare `a`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Bind {
    /// As written. A template (`~`) has none written: `Plain`.
    pub quant: Quant,
    pub template: bool,
    pub name: Name,
    /// `None` for a bare name. (To Bend a bare name is `-name: Quant`; the surface form keeps it as written.)
    pub ty: Option<Term>,
    /// From the first mark (`~`, `-`, `+`) or the name to the end of the type.
    pub span: Span,
}

/// A binder's quantity mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Quant {
    /// `-x`: erased.
    Erased,
    /// `x`: used once. Also what a name with no mark is.
    Plain,
    /// `+x`: reusable.
    Reusable,
}

/// A name as `bend` reads it: letters, digits, `_` and `.`, ASCII only. The dot means nothing: `Nat.add` is one name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Name {
    pub text: String,
    pub span: Span,
}

/// Something that was not what the grammar expects. `bend` stops at its first error; we go on at the next declaration (BD-D10).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParseError {
    /// From where the reader stopped to the last code before the point where reading resumed: the next line at column 0 that starts with `def`, `type`,
    /// `law`, `@unsafe` or `import`, or the end of the text. Empty when it resumed at once.
    pub span: Span,
    /// What was expected, in `bend`'s words where there are some.
    pub expected: String,
    /// What was there: `'x'`, or `end of input`.
    pub observed: String,
}

/// A body: `Local ::= (Quant Name | Term)+ "=" Term+ ";"? Body`, `Match`, or a reply term (`bend.ts` `parse_body`, 2376).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Body {
    Match(Match),
    Let(Let),
    /// The reply.
    Term(Term),
}

impl Body {
    pub fn span(&self) -> Span {
        match self {
            Body::Match(m) => m.span,
            Body::Let(l) => l.span,
            Body::Term(t) => t.span,
        }
    }
}

/// `match a b: case p q: body …`: one pattern per scrutinee in every case.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Match {
    pub scrutinees: Vec<Term>,
    pub cases: Vec<Case>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Case {
    /// Terms read as patterns: a binder is a [`TermKind::Name`], a constructor pattern a [`TermKind::Ctr`], a literal a [`TermKind::Lit`]. Whether a
    /// name is a binder or a constructor is for the analysis.
    pub patterns: Vec<Term>,
    pub body: Body,
    /// From `case` to the end of the body.
    pub span: Span,
}

/// `pat = value` and the body after it. `x y z = a b c` is one `Let` with three patterns and three values.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Let {
    /// `-x = v`: the binder is erased (written with a `-`).
    pub erased: bool,
    /// The left-hand terms, read as patterns: `x`, `+x`, `K{a, b}`, `3n`, … A name stays a name.
    pub pats: Vec<Term>,
    /// `x : T = v`: the written type.
    pub ty: Option<Term>,
    pub values: Vec<Term>,
    /// The statement form `a[i] <- v` followed by another statement: Bend reads it as the let `a = a[i] <- v`. `pats` is `[a]`, `values` is the index
    /// term with its write.
    pub array_write: bool,
    pub rest: Box<Body>,
    pub span: Span,
}

/// A term. `span` runs from its first token to its last.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Term {
    pub kind: TermKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TermKind {
    /// A name, dots included. Not resolved: a binder, a def, Base's, a constructor or a type is the analysis's question.
    Name(String),
    Type,
    Data,
    /// The word `Quant`.
    Quant,
    /// `Kind(t)`.
    Kind(Box<Term>),
    /// `&0`, `&1`, `&2`: the quantity as its digit.
    Qua(u8),
    /// `?Name`.
    Hole(Name),
    Lit(Lit),
    /// `3n+t`: a Nat literal and the term after the glued `+`.
    NatPlus {
        n: String,
        rest: Box<Term>,
    },
    /// `[a, b]` (commas optional).
    List(Vec<Term>),
    /// `[v : T * n]` and `[v : T ^ d]`.
    ArrayNew {
        elem: Box<Term>,
        ty: Box<Term>,
        size: ArraySize,
    },
    /// `K{a, b}`, with the `{` glued to the name.
    Ctr {
        name: Name,
        args: Vec<Term>,
    },
    /// `D<A, B>`: a name followed by type arguments.
    TypeApp {
        name: Name,
        args: Vec<Term>,
    },
    /// `+t`: a quantified datatype (`+D<A>`) or a reusable variable (`+x`). Which one is for the analysis.
    Plus(Box<Term>),
    /// `f(a, b)`: one call with two arguments. `f(a)(b)` is a call whose callee is a call. `f!(a)` has `gpu` set.
    Call {
        callee: Box<Term>,
        gpu: bool,
        args: Vec<Arg>,
    },
    /// `x[i]` and `x[i] <- v`.
    Index {
        array: Box<Term>,
        index: Box<Term>,
        write: Option<Box<Term>>,
    },
    /// An infix operator as written. Inside parens that carry a `: T`, `T` names the operator's namespace ([`TermKind::Group`], [`TermKind::Tuple`]).
    Op {
        lhs: Box<Term>,
        op: Op,
        rhs: Box<Term>,
    },
    /// `A -> B`.
    Arrow {
        from: Box<Term>,
        to: Box<Term>,
    },
    /// `@ Quant name : A -> B`, and with `exists` the prefix `& name : A -> B`.
    All {
        exists: bool,
        quant: Quant,
        name: Name,
        domain: Box<Term>,
        body: Box<Term>,
    },
    /// `x => body`, `+x => body`: one binder. The body is a body (lets and matches allowed). A binder in parens, `(x : T) => body`, keeps its `: T` in
    /// `annot`: `bend.ts` reads a group as the term inside it, so the parens are no more than that.
    Lam {
        reusable: bool,
        binder: Name,
        annot: Option<Box<Term>>,
        body: Box<Body>,
    },
    /// `\{ K: a; L: b; tail }`: arms, then an optional tail term; `\{}` has neither.
    LamMatch {
        arms: Vec<MatchArm>,
        tail: Option<Box<Term>>,
    },
    /// `{e : T}`.
    Annot {
        term: Box<Term>,
        ty: Box<Term>,
    },
    /// `{a == b : T}`, and with `negated` `{a != b : T}`.
    Eq {
        lhs: Box<Term>,
        rhs: Box<Term>,
        ty: Box<Term>,
        negated: bool,
    },
    /// `{==}`.
    Refl,
    /// `%e : P; body` and `%e@E : P; body` (`named` is the name before the `@`).
    Rewrite {
        eq: Box<Term>,
        named: Option<Name>,
        motive: Box<Term>,
        rest: Box<Body>,
    },
    /// `( body )`, with the `: T` that may follow the body (`(a + b : T)`).
    Group {
        body: Box<Body>,
        ns: Option<Box<Term>>,
    },
    /// `(a, b, c)`: two or more bodies; `ns` is the `: T` after the last.
    Tuple {
        items: Vec<Body>,
        ns: Option<Box<Term>>,
    },
    Do(Do),
}

/// `Name ":" Term` in `\{ … }`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MatchArm {
    pub ctr: Name,
    pub body: Term,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Arg {
    /// Written `~t`.
    pub template: bool,
    pub term: Term,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArraySize {
    /// `* n`.
    Count(Box<Term>),
    /// `^ d`.
    Depth(Box<Term>),
}

/// A literal as written: the digits (without the `n`), the whole float, or the text between the quotes (escapes not decoded).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Lit {
    U32(String),
    Nat(String),
    F32(String),
    Char(String),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Op {
    /// `&`.
    Pair,
    /// `|`.
    Either,
    /// `||`.
    Or,
    /// `&&`.
    And,
    Lt,
    Le,
    Gt,
    Ge,
    /// `<>`.
    Cons,
    /// `++`.
    Append,
    /// `<&>`.
    Min,
    /// `.|.`.
    BitOr,
    /// `.^.`.
    BitXor,
    /// `.&.`.
    BitAnd,
    Shl,
    Shr,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

impl Op {
    /// The operator as written.
    pub fn text(self) -> &'static str {
        match self {
            Op::Pair => "&",
            Op::Either => "|",
            Op::Or => "||",
            Op::And => "&&",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Cons => "<>",
            Op::Append => "++",
            Op::Min => "<&>",
            Op::BitOr => ".|.",
            Op::BitXor => ".^.",
            Op::BitAnd => ".&.",
            Op::Shl => "<<",
            Op::Shr => ">>",
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "/",
            Op::Mod => "%",
        }
    }
}

/// `do M<ts>:` and its statements, in order. The last is a [`DoKind::Return`] or a [`DoKind::Value`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Do {
    pub monad: Name,
    /// The terms between `<` and `>`.
    pub args: Vec<Term>,
    pub stmts: Vec<DoStmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DoStmt {
    pub kind: DoKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DoKind {
    /// `x : T = v`.
    Let {
        reusable: bool,
        name: Name,
        ty: Term,
        value: Term,
    },
    /// `x : A <- v`.
    Bind {
        reusable: bool,
        name: Name,
        ty: Term,
        value: Term,
    },
    /// `t <- v`: `t` is the type, the binder is `_`.
    Exec { ty: Term, value: Term },
    /// A term followed by `;` or by a line at the block's column: a step of type `Unit`.
    Step(Term),
    /// `return t`: the last statement.
    Return(Term),
    /// A final value: the last statement.
    Value(Term),
}
