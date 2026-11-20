//! Shared by the test files of this crate. Not the testkit: `bend-parser` depends on `analyzer-core` and nothing else, dev-dependencies included.
#![allow(dead_code, unreachable_pub)]

use std::fs;
use std::path::{Path, PathBuf};

use analyzer_core::text::Span;
use bend_parser::{ArraySize, Body, Decl, DefBody, DoKind, Term, TermKind, Tree};

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

/// Every `.bend` under `tests/fixtures`, sorted by name.
pub fn fixtures() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = fs::read_dir(fixtures_dir())
        .expect("tests/fixtures exists")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "bend" {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_string();
            Some((
                name,
                fs::read_to_string(&path).expect("a fixture is UTF-8 text"),
            ))
        })
        .collect();
    out.sort();
    out
}

/// The testkit's `expect_file`, written out: `ANALYZER_BLESS=1` writes the expected file, for a person to review with `git diff`.
pub fn expect_file(path: &Path, actual: &str) {
    if std::env::var_os("ANALYZER_BLESS").is_some_and(|v| v == "1") {
        fs::create_dir_all(path.parent().expect("a snapshot has a folder"))
            .expect("create the snapshot folder");
        fs::write(path, actual).expect("write the snapshot");
        return;
    }
    let expected = fs::read_to_string(path).unwrap_or_else(|_| {
        panic!("no expected file {}\n  run with ANALYZER_BLESS=1 to write it, then review it.\n--- actual ---\n{actual}", path.display())
    });
    assert!(
        expected == actual,
        "{} differs\n  run with ANALYZER_BLESS=1 to rewrite it, then review the diff.\n--- expected ---\n{expected}--- actual ---\n{actual}",
        path.display()
    );
}

// ---- the walk -------------------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub enum Node<'a> {
    Term(&'a Term),
    Body(&'a Body),
}

impl Node<'_> {
    pub fn span(self) -> Span {
        match self {
            Node::Term(t) => t.span,
            Node::Body(b) => b.span(),
        }
    }
}

/// The terms and bodies directly inside a node, in source order. (An array-write let lists its values and its rest only: its pattern is the array
/// of the first value, read twice.)
pub fn children(n: Node<'_>) -> Vec<Node<'_>> {
    let mut out: Vec<Node> = Vec::new();
    match n {
        Node::Body(Body::Term(t)) => out.push(Node::Term(t)),
        Node::Body(Body::Match(m)) => {
            out.extend(m.scrutinees.iter().map(Node::Term));
            for c in &m.cases {
                out.extend(c.patterns.iter().map(Node::Term));
                out.push(Node::Body(&c.body));
            }
        }
        Node::Body(Body::Let(l)) => {
            if !l.array_write {
                out.extend(l.pats.iter().map(Node::Term));
            }
            out.extend(l.ty.iter().map(Node::Term));
            out.extend(l.values.iter().map(Node::Term));
            out.push(Node::Body(&l.rest));
        }
        Node::Body(_) => {}
        Node::Term(t) => match &t.kind {
            TermKind::Kind(a) | TermKind::Plus(a) => out.push(Node::Term(a)),
            TermKind::NatPlus { rest, .. } => out.push(Node::Term(rest)),
            TermKind::List(xs) => out.extend(xs.iter().map(Node::Term)),
            TermKind::ArrayNew { elem, ty, size } => {
                out.push(Node::Term(elem));
                out.push(Node::Term(ty));
                if let ArraySize::Count(n) | ArraySize::Depth(n) = size {
                    out.push(Node::Term(n));
                }
            }
            TermKind::Ctr { args, .. } | TermKind::TypeApp { args, .. } => {
                out.extend(args.iter().map(Node::Term))
            }
            TermKind::Call { callee, args, .. } => {
                out.push(Node::Term(callee));
                out.extend(args.iter().map(|a| Node::Term(&a.term)));
            }
            TermKind::Index {
                array,
                index,
                write,
            } => {
                out.push(Node::Term(array));
                out.push(Node::Term(index));
                out.extend(write.iter().map(|w| Node::Term(w)));
            }
            TermKind::Op { lhs, rhs, .. } => out.extend([Node::Term(lhs), Node::Term(rhs)]),
            TermKind::Arrow { from, to } => out.extend([Node::Term(from), Node::Term(to)]),
            TermKind::All { domain, body, .. } => {
                out.extend([Node::Term(domain), Node::Term(body)])
            }
            TermKind::Lam { annot, body, .. } => {
                out.extend(annot.iter().map(|t| Node::Term(t)));
                out.push(Node::Body(body));
            }
            TermKind::LamMatch { arms, tail } => {
                out.extend(arms.iter().map(|a| Node::Term(&a.body)));
                out.extend(tail.iter().map(|t| Node::Term(t)));
            }
            TermKind::Annot { term, ty } => out.extend([Node::Term(term), Node::Term(ty)]),
            TermKind::Eq { lhs, rhs, ty, .. } => {
                out.extend([Node::Term(lhs), Node::Term(rhs), Node::Term(ty)])
            }
            TermKind::Rewrite {
                eq, motive, rest, ..
            } => out.extend([Node::Term(eq), Node::Term(motive), Node::Body(rest)]),
            TermKind::Group { body, ns } => {
                out.push(Node::Body(body));
                out.extend(ns.iter().map(|t| Node::Term(t)));
            }
            TermKind::Tuple { items, ns } => {
                out.extend(items.iter().map(Node::Body));
                out.extend(ns.iter().map(|t| Node::Term(t)));
            }
            TermKind::Do(d) => {
                out.extend(d.args.iter().map(Node::Term));
                for s in &d.stmts {
                    match &s.kind {
                        DoKind::Let { ty, value, .. }
                        | DoKind::Bind { ty, value, .. }
                        | DoKind::Exec { ty, value } => {
                            out.extend([Node::Term(ty), Node::Term(value)]);
                        }
                        DoKind::Step(t) | DoKind::Return(t) | DoKind::Value(t) => {
                            out.push(Node::Term(t))
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        },
    }
    out
}

/// The roots of a tree: every term and body a declaration holds, in source order.
pub fn roots(tree: &Tree) -> Vec<Node<'_>> {
    let mut out = Vec::new();
    for d in &tree.decls {
        match d {
            Decl::Def(f) => {
                out.extend(
                    f.params
                        .iter()
                        .filter_map(|b| b.ty.as_ref())
                        .map(Node::Term),
                );
                out.extend(f.ret.iter().map(Node::Term));
                if let Some(DefBody::Code(b)) = &f.body {
                    out.push(Node::Body(b));
                }
            }
            Decl::Type(t) => {
                out.extend(
                    t.params
                        .iter()
                        .filter_map(|b| b.ty.as_ref())
                        .map(Node::Term),
                );
                out.extend(t.kind.iter().map(Node::Term));
                for c in &t.ctrs {
                    out.extend(
                        c.fields
                            .iter()
                            .filter_map(|b| b.ty.as_ref())
                            .map(Node::Term),
                    );
                }
            }
            Decl::Law(l) => {
                for c in &l.clauses {
                    out.push(Node::Term(&c.ty));
                    out.extend(c.where_.iter().map(Node::Term));
                }
                out.extend(l.claim.iter().map(Node::Body));
            }
            _ => {}
        }
    }
    out
}

/// The visit of every node, parents before children.
pub fn walk<'a>(n: Node<'a>, f: &mut impl FnMut(Node<'a>)) {
    f(n);
    for c in children(n) {
        walk(c, f);
    }
}

/// A term's kind as a word, for counting.
pub fn kind_name(t: &Term) -> &'static str {
    match &t.kind {
        TermKind::Name(_) => "Name",
        TermKind::Type
        | TermKind::Data
        | TermKind::Quant
        | TermKind::Kind(_)
        | TermKind::Qua(_) => "Type/Kind/Quant",
        TermKind::Hole(_) => "Hole",
        TermKind::Lit(_) | TermKind::NatPlus { .. } => "Lit",
        TermKind::List(_) => "List",
        TermKind::ArrayNew { .. } => "ArrayNew",
        TermKind::Ctr { .. } => "Ctr",
        TermKind::TypeApp { .. } => "TypeApp",
        TermKind::Plus(_) => "Plus",
        TermKind::Call { .. } => "Call",
        TermKind::Index { .. } => "Index",
        TermKind::Op { .. } => "Op",
        TermKind::Arrow { .. } => "Arrow",
        TermKind::All { .. } => "All",
        TermKind::Lam { .. } => "Lam",
        TermKind::LamMatch { .. } => "LamMatch",
        TermKind::Annot { .. } | TermKind::Eq { .. } | TermKind::Refl => "Annot/Eq/Refl",
        TermKind::Rewrite { .. } => "Rewrite",
        TermKind::Group { .. } => "Group",
        TermKind::Tuple { .. } => "Tuple",
        TermKind::Do(_) => "Do",
        _ => "other",
    }
}

// ---- invariants -----------------------------------------------------------------------------------------------------------------------------

/// What must hold of any tree of `text`, whatever the text: spans in bounds and on character boundaries, names that are what their span says,
/// declarations in order and apart, comments that are comments, every term inside its parent and apart from its siblings. Returns the violations.
pub fn invariants(text: &str, tree: &Tree) -> Vec<String> {
    let mut bad = Vec::new();
    let slice = |what: &str, span: Span, bad: &mut Vec<String>| -> Option<String> {
        let s = span.slice(text).map(str::to_string);
        if s.is_none() {
            bad.push(format!("{what}: span {span:?} is not a slice of the text"));
        }
        s
    };
    let mut previous_comment = None;
    for &c in &tree.comments {
        if let Some(s) = slice("comment", c, &mut bad)
            && (!s.starts_with('#') || s.contains('\n'))
        {
            bad.push(format!("comment {c:?} is {s:?}"));
        }
        if previous_comment.is_some_and(|p: Span| p.end > c.start) {
            bad.push(format!("comment {c:?} overlaps or precedes the one before"));
        }
        previous_comment = Some(c);
    }
    for import in &tree.imports {
        if let Some(s) = slice("import", import.span, &mut bad)
            && !s.starts_with("import")
        {
            bad.push(format!("import {:?} is {s:?}", import.span));
        }
        if slice("import path", import.path_span, &mut bad).is_some_and(|s| s != import.path) {
            bad.push(format!(
                "import path {:?} is not {:?}",
                import.path_span, import.path
            ));
        }
        if let Some(alias) = &import.alias
            && slice("alias", alias.span, &mut bad).is_some_and(|s| s != alias.text)
        {
            bad.push(format!("alias {alias:?} is not its span"));
        }
    }
    let mut previous_end = 0;
    for decl in &tree.decls {
        let span = decl.span();
        let Some(s) = slice("declaration", span, &mut bad) else {
            continue;
        };
        if !(s.starts_with('@') || ["def", "type", "law"].iter().any(|k| s.starts_with(k))) {
            bad.push(format!(
                "declaration {span:?} starts with {:?}",
                s.chars().take(10).collect::<String>()
            ));
        }
        if span.start < previous_end {
            bad.push(format!(
                "declaration {span:?} starts before the previous one ends ({previous_end})"
            ));
        }
        previous_end = span.end;
        let name = decl.name();
        if slice("name", name.span, &mut bad).is_some_and(|s| s != name.text) {
            bad.push(format!("name {name:?} is not its span"));
        }
        if name.span.start < span.start || name.span.end > span.end {
            bad.push(format!("name {name:?} is outside its declaration {span:?}"));
        }
        if let Some(doc) = decl.doc()
            && (slice("doc", doc, &mut bad).is_some_and(|s| !s.starts_with('#'))
                || doc.end > span.start)
        {
            bad.push(format!(
                "doc {doc:?} of {name:?} is not a comment block above it"
            ));
        }
        let binds: Vec<_> = match decl {
            Decl::Def(d) => d.params.iter().collect(),
            Decl::Type(d) => d
                .params
                .iter()
                .chain(d.ctrs.iter().flat_map(|c| c.fields.iter()))
                .collect(),
            _ => Vec::new(),
        };
        for b in binds {
            if slice("binder name", b.name.span, &mut bad).is_some_and(|s| s != b.name.text) {
                bad.push(format!("binder {:?} is not its span", b.name));
            }
        }
        if let Decl::Law(l) = decl {
            for c in &l.clauses {
                if slice("clause name", c.name.span, &mut bad).is_some_and(|s| s != c.name.text) {
                    bad.push(format!("clause name {:?} is not its span", c.name));
                }
            }
        }
        // everything a declaration holds lies inside it
        let mut inside = Vec::new();
        match decl {
            Decl::Def(f) => {
                inside.extend(
                    f.params
                        .iter()
                        .filter_map(|b| b.ty.as_ref())
                        .map(Node::Term),
                );
                inside.extend(f.ret.iter().map(Node::Term));
                if let Some(DefBody::Code(b)) = &f.body {
                    inside.push(Node::Body(b));
                }
            }
            Decl::Law(l) => inside.extend(l.claim.iter().map(Node::Body)),
            _ => {}
        }
        for n in inside {
            if n.span().start < span.start || n.span().end > span.end {
                bad.push(format!(
                    "{:?} is outside its declaration {span:?}",
                    n.span().slice(text)
                ));
            }
        }
    }
    for e in &tree.errors {
        slice("error", e.span, &mut bad);
    }
    for root in roots(tree) {
        walk(root, &mut |n| node_invariants(text, n, &mut bad));
    }
    bad
}

fn node_invariants(text: &str, n: Node<'_>, bad: &mut Vec<String>) {
    let span = n.span();
    let Some(s) = span.slice(text) else {
        bad.push(format!("a term's span {span:?} is not a slice of the text"));
        return;
    };
    if let Node::Term(t) = n {
        match &t.kind {
            TermKind::Name(k) if k != s => bad.push(format!("name {k:?} is {s:?} at {span:?}")),
            TermKind::Call { callee, .. }
                if callee.span.start != span.start || !s.ends_with(')') =>
            {
                bad.push(format!(
                    "call {s:?} does not run from its callee to its `)`"
                ));
            }
            TermKind::Op { lhs, rhs, .. }
                if lhs.span.start != span.start || rhs.span.end != span.end =>
            {
                bad.push(format!(
                    "operator {s:?} does not run from its left to its right"
                ));
            }
            TermKind::Op {
                lhs,
                op,
                op_span,
                rhs,
            } if op_span.start < lhs.span.end
                || rhs.span.start < op_span.end
                || text.get(op_span.start as usize..op_span.end as usize) != Some(op.text()) =>
            {
                bad.push(format!(
                    "operator {s:?}: its token {op_span:?} is not `{}` between its sides",
                    op.text()
                ));
            }
            TermKind::Plus(_) if !s.starts_with('+') => {
                bad.push(format!("plus {s:?} does not start with +"))
            }
            _ => {}
        }
    }
    let mut previous_end = span.start;
    for k in children(n) {
        let ks = k.span();
        if ks.start < span.start || ks.end > span.end {
            bad.push(format!(
                "{:?} is outside its parent {s:?} {span:?}",
                ks.slice(text)
            ));
        }
        if ks.start < previous_end {
            bad.push(format!(
                "{:?} overlaps the sibling before it, in {s:?}",
                ks.slice(text)
            ));
        }
        previous_end = previous_end.max(ks.end);
    }
}

// ---- the snapshot ---------------------------------------------------------------------------------------------------------------------------

#[derive(Clone)]
enum S {
    Atom(String),
    List(Vec<S>),
}

fn atom(s: impl Into<String>) -> S {
    S::Atom(s.into())
}

fn list(head: &str, rest: Vec<S>) -> S {
    let mut v = vec![atom(head)];
    v.extend(rest);
    S::List(v)
}

fn quant_mark(q: bend_parser::Quant) -> &'static str {
    match q {
        bend_parser::Quant::Erased => "-",
        bend_parser::Quant::Reusable => "+",
        _ => "",
    }
}

fn s_terms(ts: &[Term]) -> Vec<S> {
    ts.iter().map(s_term).collect()
}

fn s_term(t: &Term) -> S {
    use bend_parser::Lit;
    match &t.kind {
        TermKind::Name(k) => atom(k.clone()),
        TermKind::Type => atom("Type"),
        TermKind::Data => atom("Data"),
        TermKind::Quant => atom("Quant"),
        TermKind::Kind(a) => list("Kind", vec![s_term(a)]),
        TermKind::Qua(q) => atom(format!("&{q}")),
        TermKind::Hole(n) => atom(format!("?{}", n.text)),
        TermKind::Lit(Lit::U32(n)) => list("u32", vec![atom(n.clone())]),
        TermKind::Lit(Lit::Nat(n)) => list("nat", vec![atom(n.clone())]),
        TermKind::Lit(Lit::F32(n)) => list("f32", vec![atom(n.clone())]),
        TermKind::Lit(Lit::Char(c)) => list("char", vec![atom(format!("{c:?}"))]),
        TermKind::Lit(Lit::Str(c)) => list("str", vec![atom(format!("{c:?}"))]),
        TermKind::Lit(_) => atom("lit?"),
        TermKind::NatPlus { n, rest } => list("nat+", vec![atom(n.clone()), s_term(rest)]),
        TermKind::List(xs) => list("list", s_terms(xs)),
        TermKind::ArrayNew { elem, ty, size } => {
            let size = match size {
                ArraySize::Count(n) => list("*", vec![s_term(n)]),
                ArraySize::Depth(n) => list("^", vec![s_term(n)]),
                _ => atom("size?"),
            };
            list("array", vec![s_term(elem), s_term(ty), size])
        }
        TermKind::Ctr { name, args } => list(
            "ctr",
            [vec![atom(name.text.clone())], s_terms(args)].concat(),
        ),
        TermKind::TypeApp { name, args } => list(
            "tapp",
            [vec![atom(name.text.clone())], s_terms(args)].concat(),
        ),
        TermKind::Plus(a) => list("+", vec![s_term(a)]),
        TermKind::Call { callee, gpu, args } => {
            let args = args.iter().map(|a| {
                if a.template {
                    list("~", vec![s_term(&a.term)])
                } else {
                    s_term(&a.term)
                }
            });
            list(
                if *gpu { "call!" } else { "call" },
                std::iter::once(s_term(callee)).chain(args).collect(),
            )
        }
        TermKind::Index {
            array,
            index,
            write,
        } => {
            let mut v = vec![s_term(array), s_term(index)];
            if let Some(w) = write {
                v.push(atom("<-"));
                v.push(s_term(w));
            }
            list("index", v)
        }
        TermKind::Op { lhs, op, rhs, .. } => {
            list("op", vec![atom(op.text()), s_term(lhs), s_term(rhs)])
        }
        TermKind::Arrow { from, to } => list("->", vec![s_term(from), s_term(to)]),
        TermKind::All {
            exists,
            quant,
            name,
            domain,
            body,
        } => list(
            if *exists { "exists" } else { "all" },
            vec![
                atom(format!("{}{}", quant_mark(*quant), name.text)),
                s_term(domain),
                s_term(body),
            ],
        ),
        TermKind::Lam {
            reusable,
            binder,
            annot,
            body,
        } => {
            let mut v = vec![atom(format!(
                "{}{}",
                if *reusable { "+" } else { "" },
                binder.text
            ))];
            if let Some(a) = annot {
                v.push(list("ty", vec![s_term(a)]));
            }
            v.push(s_body(body));
            list("lam", v)
        }
        TermKind::LamMatch { arms, tail } => {
            let mut v: Vec<S> = arms
                .iter()
                .map(|a| list(&a.ctr.text, vec![s_term(&a.body)]))
                .collect();
            if let Some(t) = tail {
                v.push(list("tail", vec![s_term(t)]));
            }
            list("lammatch", v)
        }
        TermKind::Annot { term, ty } => list("annot", vec![s_term(term), s_term(ty)]),
        TermKind::Eq {
            lhs,
            rhs,
            ty,
            negated,
        } => list(
            if *negated { "neq" } else { "eq" },
            vec![s_term(lhs), s_term(rhs), s_term(ty)],
        ),
        TermKind::Refl => atom("refl"),
        TermKind::Rewrite {
            eq,
            named,
            motive,
            rest,
        } => {
            let mut v = vec![s_term(eq)];
            if let Some(n) = named {
                v.push(list("as", vec![atom(n.text.clone())]));
            }
            v.extend([s_term(motive), s_body(rest)]);
            list("rewrite", v)
        }
        TermKind::Group { body, ns } => {
            let mut v = vec![s_body(body)];
            if let Some(ns) = ns {
                v.push(list("ns", vec![s_term(ns)]));
            }
            list("group", v)
        }
        TermKind::Tuple { items, ns } => {
            let mut v: Vec<S> = items.iter().map(s_body).collect();
            if let Some(ns) = ns {
                v.push(list("ns", vec![s_term(ns)]));
            }
            list("tuple", v)
        }
        TermKind::Do(d) => {
            let mut v = vec![atom(d.monad.text.clone()), list("args", s_terms(&d.args))];
            for s in &d.stmts {
                let mark = |reusable: &bool| if *reusable { "+" } else { "" };
                v.push(match &s.kind {
                    DoKind::Let {
                        reusable,
                        name,
                        ty,
                        value,
                    } => list(
                        "let",
                        vec![
                            atom(format!("{}{}", mark(reusable), name.text)),
                            s_term(ty),
                            s_term(value),
                        ],
                    ),
                    DoKind::Bind {
                        reusable,
                        name,
                        ty,
                        value,
                    } => list(
                        "bind",
                        vec![
                            atom(format!("{}{}", mark(reusable), name.text)),
                            s_term(ty),
                            s_term(value),
                        ],
                    ),
                    DoKind::Exec { ty, value } => list("exec", vec![s_term(ty), s_term(value)]),
                    DoKind::Step(t) => list("step", vec![s_term(t)]),
                    DoKind::Return(t) => list("return", vec![s_term(t)]),
                    DoKind::Value(t) => list("value", vec![s_term(t)]),
                    _ => atom("stmt?"),
                });
            }
            list("do", v)
        }
        _ => atom("term?"),
    }
}

fn s_body(b: &Body) -> S {
    match b {
        Body::Term(t) => s_term(t),
        Body::Match(m) => {
            let mut v = vec![list("on", s_terms(&m.scrutinees))];
            for c in &m.cases {
                v.push(list(
                    "case",
                    vec![list("pats", s_terms(&c.patterns)), s_body(&c.body)],
                ));
            }
            list("match", v)
        }
        Body::Let(l) => {
            let mut head = String::from("let");
            if l.erased {
                head.push('-');
            }
            if l.array_write {
                head.push_str("[write]");
            }
            let mut v = vec![list("pats", s_terms(&l.pats))];
            if let Some(t) = &l.ty {
                v.push(list("ty", vec![s_term(t)]));
            }
            v.push(list("values", s_terms(&l.values)));
            v.push(s_body(&l.rest));
            list(&head, v)
        }
        _ => atom("body?"),
    }
}

fn s_bind(b: &bend_parser::Bind) -> S {
    let marks = format!(
        "{}{}",
        if b.template { "~" } else { "" },
        quant_mark(b.quant)
    );
    match &b.ty {
        Some(t) => list(&format!("{marks}{}", b.name.text), vec![s_term(t)]),
        None => atom(format!("{marks}{}", b.name.text)),
    }
}

/// The span with the head and the tail of the text it covers, enough to see where it starts and where it ends.
fn show_span(text: &str, span: Span) -> String {
    let Some(s) = span.slice(text) else {
        return format!("{}..{} (not a slice)", span.start, span.end);
    };
    let one = s.replace('\n', "⏎");
    let chars: Vec<char> = one.chars().collect();
    let shown = if chars.len() <= 26 {
        one
    } else {
        format!(
            "{}…{}",
            chars[..12].iter().collect::<String>(),
            chars[chars.len() - 12..].iter().collect::<String>()
        )
    };
    format!("{}..{} {shown:?}", span.start, span.end)
}

fn s_decl(text: &str, d: &Decl) -> S {
    let mut v = vec![atom(d.name().text.clone()), atom(show_span(text, d.span()))];
    if let Some(doc) = d.doc() {
        v.push(list("doc", vec![atom(show_span(text, doc))]));
    }
    if d.syntax_error() {
        v.push(atom("SYNTAX-ERROR"));
    }
    let binds = |bs: &[bend_parser::Bind]| list("params", bs.iter().map(s_bind).collect());
    let head = match d {
        Decl::Def(f) => {
            if f.unsafe_ {
                v.push(atom(if f.question { "unsafe?" } else { "unsafe" }));
            }
            v.push(binds(&f.params));
            if let Some(r) = &f.ret {
                v.push(list("ret", vec![s_term(r)]));
            }
            match &f.body {
                Some(DefBody::Code(b)) => v.push(list("body", vec![s_body(b)])),
                Some(DefBody::Foreign(ls)) => v.push(list(
                    "foreign",
                    ls.iter().map(|l| atom(format!("{:?}", l.value))).collect(),
                )),
                _ => {}
            }
            "def"
        }
        Decl::Type(t) => {
            v.push(binds(&t.params));
            if let Some(k) = &t.kind {
                v.push(list("kind", vec![s_term(k)]));
            }
            for c in &t.ctrs {
                v.push(list(
                    "ctr",
                    vec![atom(c.name.text.clone()), binds(&c.fields)],
                ));
            }
            "type"
        }
        Decl::Law(l) => {
            for c in &l.clauses {
                let mark = format!(
                    "{}{}",
                    if c.template { "~" } else { "" },
                    quant_mark(c.quant)
                );
                let mut cv = vec![atom(format!("{mark}{}", c.name.text)), s_term(&c.ty)];
                if let Some(w) = &c.where_ {
                    cv.push(list("where", vec![s_term(w)]));
                }
                v.push(list(if c.exists { "exs" } else { "for" }, cv));
            }
            if let Some(c) = &l.claim {
                v.push(list("claim", vec![s_body(c)]));
            }
            "law"
        }
        _ => "decl?",
    };
    list(head, v)
}

fn flat(s: &S) -> String {
    match s {
        S::Atom(a) => a.clone(),
        S::List(xs) => format!("({})", xs.iter().map(flat).collect::<Vec<_>>().join(" ")),
    }
}

fn pretty(s: &S, indent: usize, out: &mut String) {
    let one = flat(s);
    match s {
        S::List(xs) if indent + one.chars().count() > 110 && xs.len() > 1 => {
            out.push('(');
            out.push_str(&flat(&xs[0]));
            for x in &xs[1..] {
                out.push('\n');
                out.push_str(&" ".repeat(indent + 2));
                pretty(x, indent + 2, out);
            }
            out.push(')');
        }
        _ => out.push_str(&one),
    }
}

/// The snapshot: one s-expression per declaration, imports, comments and errors, broken into lines where it is long. Terms carry no spans in it
/// (`invariants` checks them); declarations show where they start and end.
pub fn render(tree: &Tree, text: &str) -> String {
    let mut out = String::new();
    let mut add = |s: S| {
        pretty(&s, 0, &mut out);
        out.push('\n');
    };
    for i in &tree.imports {
        let mut v = vec![atom(format!("{:?}", i.path))];
        if let Some(a) = &i.alias {
            v.push(list("as", vec![atom(a.text.clone())]));
        }
        add(list("import", v));
    }
    for d in &tree.decls {
        add(s_decl(text, d));
    }
    for c in &tree.comments {
        add(list("comment", vec![atom(show_span(text, *c))]));
    }
    for e in &tree.errors {
        add(list(
            "ERROR",
            vec![
                atom(show_span(text, e.span)),
                atom(format!("expected {:?}", e.expected)),
                atom(format!("observed {:?}", e.observed)),
            ],
        ));
    }
    if out.is_empty() {
        out.push_str("(empty)\n");
    }
    out
}
