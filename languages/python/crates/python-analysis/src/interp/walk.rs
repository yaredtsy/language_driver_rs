use ruff_python_ast::name::Name;
use rustc_hash::FxHashMap;
// src/interp/walk.rs
use crate::interp::defer::Deferred;
use crate::interp::env::Env;
use crate::interp::key::Interner;
use crate::interp::solver::Solver;
use crate::interp::summary::CallEdge;
use crate::interp::tree::Node;
use crate::interp::value::{
    AbstractValue, ActivationId, DeferId, OriginId, OriginKey, ValueArena, Values, dedup,
};
use crate::sem::{Definition, SemanticModel};
use ruff_index::IndexVec;
use ruff_python_ast::visitor::{Visitor, walk_expr, walk_pattern, walk_stmt};
use ruff_python_ast::{Expr, ExprRef, Pattern, Stmt};
use ruff_text_size::TextSize;
use smallvec::smallvec;
use ty_python_core::ProgramFile;

pub(crate) struct Walk<'db> {
    pub frame: Frame<'db>,
    pub activations: IndexVec<ActivationId, Env<'db>>,
    /// Which object is which: where it was built, and in which run.
    pub origins: Interner<OriginKey, OriginId>,
    /// Receivers held by `BoundMethod`s.
    pub values: ValueArena<'db>,
    /// A nested def's defaults, as computed when its `def` ran. Per run.
    pub defaults: FxHashMap<(Definition<'db>, ActivationId), Vec<(Name, Values<'db>)>>,
    /// Operations that waited on a pending value, in the order the walk met them.
    pub deferred: IndexVec<DeferId, Deferred<'db>>,
    /// What each deferred operation resolved to in the latest round.
    pub resolved: IndexVec<DeferId, Values<'db>>,
    /// Set while the solver repeats the deferred operations.
    pub replaying: bool,
    /// During a replay: the values of the pending operations in this round.
    pub now: Option<IndexVec<DeferId, Values<'db>>>,
    /// The body yields: its call returns a generator, whatever it returns.
    pub generator: bool,
}

/// The body being walked now.
pub(crate) struct Frame<'db> {
    pub func: Definition<'db>,
    pub file: ProgramFile<'db>,
    pub model: SemanticModel<'db>,
    pub children: Vec<Node>,
    pub returns: Values<'db>,
    /// The env at each `return` and at the end of the body.
    pub exits: Vec<Env<'db>>,
    pub calls: Vec<CallEdge<'db>>, // one per call site with a project callee
    pub def_start: TextSize,       // where this `def` starts: sites are relative to it
    pub act: ActivationId,
}

fn frame_for<'db>(sol: &Solver<'db>, func: Definition<'db>, act: ActivationId) -> Frame<'db> {
    let file = func.program_file(sol.sem.db);
    Frame {
        func,
        file,
        model: sol.sem.model(file),
        children: Vec::new(),
        returns: Values::new(),
        exits: Vec::new(),
        calls: Vec::new(),
        def_start: sol.sem.def_position(func).1,
        act,
    }
}

impl<'db> Walk<'db> {
    pub(crate) fn new(sol: &Solver<'db>, func: Definition<'db>) -> Self {
        let mut activations = IndexVec::new();
        let act = activations.push(Env::default());
        let mut origins = Interner::default();
        let opaque = origins.intern(OriginKey::Opaque);
        debug_assert_eq!(opaque, OriginId::OPAQUE); // interned first, in every walk
        Walk {
            frame: frame_for(sol, func, act),
            activations,
            origins,
            values: Interner::default(),
            defaults: FxHashMap::default(),
            deferred: IndexVec::new(),
            resolved: IndexVec::new(),
            replaying: false,
            now: None,
            generator: false,
        }
    }
    pub(crate) fn new_frame(&mut self, sol: &Solver<'db>, func: Definition<'db>) -> Frame<'db> {
        let act = self.activations.push(Env::default());
        frame_for(sol, func, act)
    }

    pub(crate) fn walk_stmts(
        &mut self,
        sol: &mut Solver<'db>,
        body: &[Stmt],
        mut env: Env<'db>,
    ) -> Option<Env<'db>> {
        for stmt in body {
            self.activations[self.frame.act] = env.clone(); // what closures read
            env = self.walk_stmt(sol, stmt, env)?;
        }
        Some(env)
    }
    fn walk_stmt(
        &mut self,
        sol: &mut Solver<'db>,
        stmt: &Stmt,
        mut env: Env<'db>,
    ) -> Option<Env<'db>> {
        match stmt {
            Stmt::Return(s) => {
                let v = match &s.value {
                    Some(e) => self.eval(sol, e, &mut env),
                    None => self.none_value(sol),
                };
                self.frame.returns.extend(v);
                self.frame.exits.push(env); // the state at this `return`
                return None;
            }
            Stmt::Raise(s) => {
                if let Some(e) = &s.exc {
                    self.eval(sol, e, &mut env);
                }
                if let Some(c) = &s.cause {
                    self.eval(sol, c, &mut env);
                }
                return None;
            }
            Stmt::FunctionDef(d) => {
                for dec in &d.decorator_list {
                    self.eval(sol, &dec.expression, &mut env); // runs now, in this function
                }
                let mut defaults = Vec::new();
                for p in d.parameters.iter_non_variadic_params() {
                    if let Some(default) = p.default.as_deref() {
                        let v = self.eval(sol, default, &mut env); // runs now: the `def` statement's calls
                        defaults.push((p.parameter.name.id.clone(), v)); // and its value is kept
                    }
                }
                let func = sol.sem.definition_of(&self.frame.model, d);
                self.defaults.insert((func, self.frame.act), defaults);
                env.bind(
                    &d.name.id,
                    smallvec![AbstractValue::Closure {
                        func,
                        activation: self.frame.act
                    }],
                );
            }
            Stmt::Expr(s) => {
                self.eval(sol, &s.value, &mut env);
            }
            Stmt::While(s) => {
                self.eval(sol, &s.test, &mut env);
                let before = env;
                let body = self.walk_stmts(sol, &s.body, before.clone());
                let after = Env::join(vec![Some(before), body])?;
                env = self.walk_stmts(sol, &s.orelse, after)?;
            }
            Stmt::If(s) => {
                let mut cond = env;
                self.eval(sol, &s.test, &mut cond); // evaluated once, value ignored
                let mut arms = vec![self.walk_stmts(sol, &s.body, cond.clone())];
                for clause in &s.elif_else_clauses {
                    if let Some(t) = &clause.test {
                        self.eval(sol, t, &mut cond); // runs only if earlier tests failed
                    }
                    arms.push(self.walk_stmts(sol, &clause.body, cond.clone()));
                }
                if !s.elif_else_clauses.iter().any(|c| c.test.is_none()) {
                    arms.push(Some(cond)); // no `else`: skipping is an arm
                }
                env = Env::join(arms)?;
            }
            Stmt::For(s) => {
                self.eval(sol, &s.iter, &mut env);
                let before = env;
                let mut body_env = before.clone();
                self.bind_from_ty(sol, &s.target, &mut body_env); // the loop variable: ty's element type
                let body = self.walk_stmts(sol, &s.body, body_env);
                let after = Env::join(vec![Some(before), body])?; // ran zero times, or ran
                env = self.walk_stmts(sol, &s.orelse, after)?;
            }
            Stmt::Try(s) => {
                let before = env;
                let body = self.walk_stmts(sol, &s.body, before.clone());
                let start = Env::join(vec![Some(before), body.clone()])?;
                let mut arms = vec![match body {
                    Some(b) => self.walk_stmts(sol, &s.orelse, b),
                    None => None,
                }];
                for h in &s.handlers {
                    let h = h.as_except_handler().unwrap();
                    arms.push(self.walk_stmts(sol, &h.body, start.clone()));
                }
                let joined = Env::join(arms)?;
                env = self.walk_stmts(sol, &s.finalbody, joined)?;
            }
            Stmt::Match(s) => {
                self.eval(sol, &s.subject, &mut env);
                let before = env;
                let mut arms = Vec::new();
                for case in &s.cases {
                    let mut arm = before.clone();
                    for name in pattern_names(&case.pattern) {
                        arm.bind(name, smallvec![AbstractValue::Unknown]);
                    }
                    if let Some(guard) = &case.guard {
                        self.eval(sol, guard, &mut arm);
                    }
                    arms.push(self.walk_stmts(sol, &case.body, arm));
                }
                arms.push(Some(before)); // no case matched
                env = Env::join(arms)?;
            }
            Stmt::With(s) => {
                for item in &s.items {
                    self.eval(sol, &item.context_expr, &mut env);
                    if let Some(target) = &item.optional_vars {
                        self.bind_from_ty(sol, target, &mut env);
                    }
                }
                env = self.walk_stmts(sol, &s.body, env)?;
            }
            Stmt::Import(s) => {
                for alias in &s.names {
                    let name = match &alias.asname {
                        Some(as_name) => as_name.id.clone(),
                        None => Name::new(alias.name.id.split('.').next().unwrap()),
                    };
                    let ty = sol.sem.type_of_import(&self.frame.model, alias);
                    let v = self.lift(sol, ty);
                    env.bind(&name, v);
                }
            }
            Stmt::ImportFrom(s) => {
                for alias in &s.names {
                    let name = alias.asname.as_ref().unwrap_or(&alias.name).id.clone();
                    let ty = sol.sem.type_of_import(&self.frame.model, alias);
                    let v = self.lift(sol, ty);
                    env.bind(&name, v);
                }
            }
            Stmt::Assign(s) => {
                let v = self.eval(sol, &s.value, &mut env);
                for target in &s.targets {
                    self.bind_target(sol, target, v.clone(), &mut env);
                }
            }
            Stmt::AnnAssign(s) => {
                if let Some(value) = &s.value {
                    let v = self.eval(sol, value, &mut env);
                    self.bind_target(sol, &s.target, v, &mut env);
                }
            }
            other => {
                for e in simple_stmt_exprs(other) {
                    self.eval(sol, e, &mut env);
                }
            }
        }
        Some(env)
    }
    /// A local of this run, else a free variable read from the run where the
    /// `def` ran (never the caller's), else `None`: ty answers.
    pub(crate) fn lookup(
        &self,
        sol: &Solver<'db>,
        env: &Env<'db>,
        name: &Name,
    ) -> Option<Values<'db>> {
        if let Some(v) = env.get(name) {
            return Some(v.clone());
        }
        let bound_in = sol.sem.free_variable_scope(env.scope?, name.as_str())?;
        let mut cur = env.enclosing;
        while let Some(act) = cur {
            let outer = &self.activations[act];
            if outer.flat {
                return outer.get(name).cloned();
            }
            if outer.scope == Some(bound_in) {
                return outer.get(name).cloned();
            }
            cur = outer.enclosing;
        }
        None
    }
    /// A free variable of the closure `(func, act)`, read exactly as its own
    /// body would: `lookup` from an empty env of `func`'s scope whose
    /// `enclosing` is `act`. Empty if no run has bound it.
    pub(crate) fn lookup_in(
        &self,
        sol: &Solver<'db>,
        func: Definition<'db>,
        act: ActivationId,
        name: &Name,
    ) -> Values<'db> {
        let parsed = sol.sem.parsed(func);
        let Some(node) = sol.sem.function_node(func, &parsed) else {
            return Values::new();
        };
        let probe = Env {
            scope: Some(sol.sem.body_scope(func, node)),
            enclosing: Some(act),
            ..Env::default()
        };
        self.lookup(sol, &probe, name).unwrap_or_default()
    }
    pub(crate) fn none_value(&mut self, sol: &mut Solver<'db>) -> Values<'db> {
        let none = sol.sem.none_type(self.frame.file);
        self.lift(sol, none)
    }
    pub(crate) fn eval(
        &mut self,
        sol: &mut Solver<'db>,
        expr: &Expr,
        env: &mut Env<'db>,
    ) -> Values<'db> {
        match expr {
            Expr::Name(n) => match self.lookup(sol, env, &n.id) {
                Some(v) => v,
                None => {
                    // a name bound by a decorated `def` is that def, whatever the decorator returns
                    if let Some(defs) = sol.sem.decorated_defs(&self.frame.model, n) {
                        return defs.into_iter().map(AbstractValue::Function).collect();
                    }
                    let ty = sol.sem.type_of(&self.frame.model, ExprRef::from(expr));
                    self.lift(sol, ty)
                }
            },

            Expr::Named(n) => {
                let v = self.eval(sol, &n.value, env);
                self.bind_target(sol, &n.target, v.clone(), env);
                v
            }
            Expr::Call(call) => self.eval_call(sol, call, env),
            Expr::Await(a) => self.eval(sol, &a.value, env),
            Expr::Lambda(_) => smallvec![AbstractValue::Unknown],
            Expr::Attribute(a) => self.eval_attribute(sol, a, env),
            Expr::If(e) => {
                self.eval(sol, &e.test, env); // evaluated, ignored
                let mut v = self.eval(sol, &e.body, env);
                v.extend(self.eval(sol, &e.orelse, env)); // both possible
                dedup(&mut v);
                v
            }
            Expr::BoolOp(b) => {
                let mut v = Values::new();
                for e in &b.values {
                    v.extend(self.eval(sol, e, env)); // `a or b`: either operand
                }
                dedup(&mut v);
                v
            }
            other => {
                for child in direct_children(other) {
                    self.eval(sol, child, env);
                }
                let ty = sol.sem.type_of(&self.frame.model, ExprRef::from(other));
                self.lift(sol, ty)
            }
        }
    }
    fn bind_target(
        &mut self,
        sol: &mut Solver<'db>,
        target: &Expr,
        v: Values<'db>,
        env: &mut Env<'db>,
    ) {
        match target {
            Expr::Name(n) => env.bind(&n.id, v),
            Expr::Attribute(a) => self.bind_attribute(sol, a, v, env),
            other => self.bind_from_ty(sol, other, env), // decision 5
        }
    }
    fn bind_from_ty(&mut self, sol: &mut Solver<'db>, target: &Expr, env: &mut Env<'db>) {
        match target {
            Expr::Name(n) => {
                let ty = sol.sem.type_of(&self.frame.model, ExprRef::from(target));
                let v = self.lift(sol, ty);
                env.bind(&n.id, v);
            }
            Expr::Tuple(t) => {
                for e in &t.elts {
                    self.bind_from_ty(sol, e, env)
                }
            }
            Expr::List(l) => {
                for e in &l.elts {
                    self.bind_from_ty(sol, e, env)
                }
            }
            Expr::Starred(st) => self.bind_from_ty(sol, &st.value, env),
            other => {
                self.eval(sol, other, env);
            } // `d[k]`, `a.b`: their calls
        }
    }
}

fn direct_children(expr: &Expr) -> Vec<&Expr> {
    struct Kids<'a>(Vec<&'a Expr>);
    impl<'a> Visitor<'a> for Kids<'a> {
        fn visit_expr(&mut self, e: &'a Expr) {
            self.0.push(e); // no recursion: `eval` recurses
        }
    }
    let mut kids = Kids(Vec::new());
    walk_expr(&mut kids, expr);
    kids.0
}

fn simple_stmt_exprs(stmt: &Stmt) -> Vec<&Expr> {
    struct Exprs<'a>(Vec<&'a Expr>);
    impl<'a> Visitor<'a> for Exprs<'a> {
        fn visit_expr(&mut self, e: &'a Expr) {
            self.0.push(e);
        }
        fn visit_stmt(&mut self, _: &'a Stmt) {} // simple statements have no nested ones
    }
    let mut v = Exprs(Vec::new());
    walk_stmt(&mut v, stmt);
    v.0
}

/// Every name a `case` pattern binds, at any depth:
/// `case [Point(x=a), *rest] as whole:` binds `a`, `rest` and `whole`.
fn pattern_names(pattern: &Pattern) -> Vec<&Name> {
    struct Names<'a>(Vec<&'a Name>);
    impl<'a> Visitor<'a> for Names<'a> {
        fn visit_pattern(&mut self, p: &'a Pattern) {
            match p {
                Pattern::MatchAs(m) => {
                    if let Some(n) = &m.name {
                        self.0.push(&n.id)
                    }
                }
                Pattern::MatchStar(m) => {
                    if let Some(n) = &m.name {
                        self.0.push(&n.id)
                    }
                }
                Pattern::MatchMapping(m) => {
                    if let Some(n) = &m.rest {
                        self.0.push(&n.id)
                    }
                }
                _ => {}
            }
            walk_pattern(self, p); // nested patterns
        }
        fn visit_expr(&mut self, _: &'a Expr) {} // `case Color.RED:` binds nothing
    }
    let mut names = Names(Vec::new());
    names.visit_pattern(pattern);
    names.0
}

/// Does `body` yield? Nested `def`, `class` and `lambda` are their own scopes.
pub(crate) fn contains_yield(body: &[Stmt]) -> bool {
    struct Finder(bool);
    impl<'a> Visitor<'a> for Finder {
        fn visit_stmt(&mut self, s: &'a Stmt) {
            if !matches!(s, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
                walk_stmt(self, s);
            }
        }
        fn visit_expr(&mut self, e: &'a Expr) {
            match e {
                Expr::Yield(_) | Expr::YieldFrom(_) => self.0 = true,
                Expr::Lambda(_) => {}
                _ => walk_expr(self, e),
            }
        }
    }
    let mut f = Finder(false);
    for s in body {
        f.visit_stmt(s);
    }
    f.0
}
