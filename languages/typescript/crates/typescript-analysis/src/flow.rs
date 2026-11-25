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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "kept for TC7 (anonymous functions: a call inside one becomes a context of its own); only the table's tests read it today"
        )
    )]
    pub(crate) anons: Box<[AnonFlow]>,
    /// The hand-overs, sorted by `at` (start, then end), ties in the order made: every def `Sem::handed_over` finds, then the names of a hand-over position it
    /// found no def for, through `expr_of`.
    pub(crate) handed: Box<[HandedFlow]>,
    /// How many nodes reading the table met of a kind a view's `expect` did not handle (`view::counted`), kept with it as `Body` keeps its own.
    pub(crate) unhandled: u64,
    /// The variables the def declares, one row each, sorted by `at` (`07-tc3` §2). `eval::solve` gives each its value.
    pub(crate) vars: Box<[Var]>,
    /// Writes of a capable variable that none of `07-tc3` §2's rows covers (each also made its variable `open`).
    pub(crate) unmodelled: u64,
    /// The def this one is nested in; `None` at the top of its file (and for the file's own code). A method's is its class, as the outline says.
    pub(crate) around: Option<DefId>,
}

impl Flow {
    /// The row of the variable whose name starts at `at` (`vars` is sorted by `at`, so a binary search).
    pub(crate) fn var_index(&self, at: u32) -> Option<usize> {
        self.vars.binary_search_by_key(&at, |var| var.at).ok()
    }

    /// The row of the def's own parameter `index`.
    pub(crate) fn param_var(&self, index: u32) -> Option<usize> {
        self.vars.iter().position(|var| var.param == Some(index))
    }
}

/// A variable as a number: the def that declares it (a def of the outline, or the file's own code: `DefId` with no index) and where its name starts in the
/// file (the start of the name's own span). Good while the program is loaded, as `DefId` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct VarId {
    pub(crate) def: DefId,
    pub(crate) at: u32,
}

/// One variable the def declares (`07-tc3` §2).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Var {
    /// Where its name starts in the file: with the table's own def, its `VarId`.
    pub(crate) at: u32,
    /// `Some(i)`: the def's own parameter `i` (a place in `Flow::params`).
    pub(crate) param: Option<u32>,
    /// Its type can hold a function, an object or an instance (`holds_only_data` is false), as `Param::capable`.
    pub(crate) capable: bool,
    /// Every expression assigned to it, in source order, the initializer first. Read only when `capable`; empty otherwise.
    pub(crate) assigned: Box<[Expr]>,
    /// Assigned somewhere in a way the table does not model. Said only when `capable`; `false` otherwise.
    pub(crate) open: bool,
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "kept for the table's tests, which print a site by its place; no phase reads it"
        )
    )]
    pub(crate) span: Span,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "kept for the table's tests and for TC4 and TC5, which treat a `new`, a JSX element or an accessor differently from a call; no phase reads it yet"
        )
    )]
    pub(crate) kind: FlowKind,
    pub(crate) callee: Expr,
    /// `Some` for a call or a `new` (its arguments) and for JSX (one `Expr::Object`: the attributes, TC4). `None` for the other kinds: **not read**, which a
    /// later phase takes as unknown, never as "no argument".
    pub(crate) args: Option<Box<[Arg]>>,
    /// The innermost anonymous function whose span holds the site.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "kept for TC7, which gives the calls of an anonymous function their own context; only the table's tests read it today"
        )
    )]
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

/// What an expression is, as far as the code alone says. `Member`, `Object`, `Omit` and `Default` are TC4's (`08-tc4` §2): the table reads them, and no reader
/// gives them a value before step 2. Later phases add the others (`New`, `This`, `Call`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Expr {
    /// Anything not below.
    Unknown,
    /// A literal and the other forms of `03-design.md` §3's table: nothing callable.
    Data,
    /// Nothing: `void …` (always `undefined`), or the global `undefined`. A value that is **not passed**, as far as a default is concerned, which
    /// `Data` is not (the review before TC3, §1.1).
    Undefined,
    /// A name or a function the checker resolves to a project def with a body.
    Def(DefId),
    /// The i-th parameter of this def's own declaration (the place in `Flow::params`).
    Param(u32),
    /// The i-th anonymous function of this body.
    Anon(u32),
    /// A name of a variable: of this def, or of a def around it (the file's code included).
    Var(VarId),
    /// One of these: `c ? a : b`, `a || b`, `a ?? b`, `a && b`, a name with several functions. Never nested, never a member twice, always two or more.
    OneOf(Box<[Expr]>),
    /// `x.k`, `x?.k`, `x["k"]` (a string literal): the member `k` of what `x` is. Never of `Unknown` (that is `Unknown`).
    Member(Box<Expr>, Box<str>),
    /// An object literal, or the attributes of a JSX element: its parts in source order (`08-tc4` §2).
    Object(Box<[Part]>),
    /// `...rest` in an object pattern: what `x` is without the members named before it in the pattern. Never of `Unknown`.
    Omit(Box<Expr>, Box<[Box<str>]>),
    /// A pattern element's `= d`: `x`, or `d` where `x` is nothing (`undefined`).
    Default(Box<Expr>, Box<Expr>),
}

/// One part of an `Expr::Object`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// `k: e`, a shorthand `k`, a method `k() {}` (its def), a JSX attribute.
    Named(Box<str>, Expr),
    /// `...e` in a literal, `{...e}` in JSX.
    Spread(Expr),
    /// A member the table cannot name: a computed key, a getter, a setter. The object is then not complete.
    Unnamed,
}
