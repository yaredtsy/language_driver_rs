//! The body of a def as plain data (`docs/03-typescript/12-context-layer/03-design.md` §4): what `sem` understood of it, read once per program and kept.
//! It holds numbers and owned values, never an engine pointer and never a `Func<'p>`, so it can be kept inside the `Sem` and handed out as an `Arc`.

use typescript_syntax::Span;

/// A def as a number: the program's number, the file's number in it, and the outline index + 1 (0 is the file's own code). Good while that program is
/// loaded; `Sem::func` gives `None` for a number of another program. The derived order (program, file, def) is the order of a set of them (`value::Value`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct DefId {
    program: u32,
    file: u32,
    def: u32,
}

impl DefId {
    pub(crate) fn new(program: u32, file: u32, index: Option<u32>) -> DefId {
        DefId {
            program,
            file,
            def: index.map_or(0, |index| index + 1),
        }
    }

    pub(crate) fn program(self) -> u32 {
        self.program
    }

    pub(crate) fn file(self) -> u32 {
        self.file
    }

    /// The outline index; `None` for the file's own code.
    pub(crate) fn index(self) -> Option<u32> {
        self.def.checked_sub(1)
    }
}

/// What `sem` read of one def's body.
#[derive(Debug)]
pub(crate) struct Flow {
    /// The parameters of the def's own declaration, in order, without a `this` parameter. Empty for the file's code, a class, and any def that is not function-like.
    pub(crate) params: Box<[Param]>,
    /// Index for index with `Sem::call_sites`.
    pub(crate) sites: Box<[SiteFlow]>,
    /// The anonymous functions written in the body, in source order (`Expr::Anon` and `SiteFlow::anon` are places in it).
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    pub(crate) anons: Box<[AnonFlow]>,
    /// The hand-overs, sorted by `at` (start, then end), ties in the order made: every def `Sem::handed_over` finds, then the names of a hand-over position it
    /// found no def for, through `expr_of`.
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    pub(crate) handed: Box<[HandedFlow]>,
    /// How many nodes reading the table met of a kind a view's `expect` did not handle (`view::counted`), kept with it as `Body` keeps its own.
    pub(crate) unhandled: u64,
}

/// A place where the body hands a function over without calling it (D22): a name in a hand-over position, or a def written there.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HandedFlow {
    /// The span of the name (or of the written def's name, or of a literal member's key), in the file.
    pub(crate) at: Option<Span>,
    pub(crate) value: Expr,
    /// Written inside an expression (a named function expression, a member of a literal), not named.
    pub(crate) written: bool,
}

/// One parameter of a declaration (`06-tc2` §2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Param {
    /// `...rest`.
    pub(crate) rest: bool,
    /// Declared by a plain name (`cb`), not a binding pattern (`{ onClose }`): only such a parameter is ever an `Expr::Param`.
    pub(crate) named: bool,
    /// The default's expression, read in this def's own table (so a default naming an earlier parameter is `Param(j)`).
    pub(crate) default: Option<Expr>,
    /// The body writes the parameter (`cb = cb || noop`), in a nested def or an anonymous function too. Looked for only when the parameter is named and
    /// capable; `false` on the others (they are never in a key).
    pub(crate) assigned: bool,
    /// Its declared type can hold a function, an object or an instance (`holds_only_data` is false). A binding pattern is not capable until TC4.
    pub(crate) capable: bool,
}

/// A site of the body, read.
#[derive(Debug)]
pub(crate) struct SiteFlow {
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    pub(crate) span: Span,
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    pub(crate) kind: FlowKind,
    pub(crate) callee: Expr,
    /// `Some` for a call or a `new` (its arguments) and for JSX (one `Unknown`: the attributes, TC4). `None` for the other kinds: **not read**, which a
    /// later phase takes as unknown, never as "no argument".
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    pub(crate) args: Option<Box<[Arg]>>,
    /// The innermost anonymous function whose span holds the site.
    #[cfg_attr(not(test), expect(dead_code, reason = "read from TC2 on"))]
    pub(crate) anon: Option<u32>,
}

/// What a site is, as far as the layer tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlowKind {
    Call,
    New,
    Jsx,
    Tagged,
    Decorator,
    Accessor,
}

/// One argument of a call.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Arg {
    pub(crate) value: Expr,
    /// `...xs`: `value` is what is spread, and it is not one argument.
    pub(crate) spread: bool,
}

/// An anonymous function written in a body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AnonFlow {
    pub(crate) span: Span,
}

/// What an expression is, as far as the code alone says. Later phases add variants (`Var`, `Member`, `Object`, `New`, `This`, `Call`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Expr {
    /// Anything not below.
    Unknown,
    /// A literal and the other forms of `03-design.md` §3's table: nothing callable.
    Data,
    /// A name or a function the checker resolves to a project def with a body.
    Def(DefId),
    /// The i-th parameter of this def's own declaration (the place in `Flow::params`).
    Param(u32),
    /// The i-th anonymous function of this body.
    Anon(u32),
    /// One of these: `c ? a : b`, `a || b`, `a ?? b`, `a && b`, a name with several functions. Never nested, never a member twice, always two or more.
    OneOf(Box<[Expr]>),
}
