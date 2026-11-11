// src/interp/relevant.rs
use ruff_db::parsed::parsed_module;
use ruff_python_ast::visitor::{Visitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt};
use rustc_hash::{FxHashMap, FxHashSet};
use ty_python_semantic::Db;

use crate::interp::bind::param_names;
use crate::sem::{Definition, DefinitionKind, Name};

/// Which parameters of `func` can affect its walk. One flag per parameter,
/// in the order `bind_parameters` fills `Bound.slots`.
///
/// Reads only the function's own body (rule 3: precision depends only on the
/// key and the code), so salsa can cache it until the file changes.
///
/// Conservative: a parameter is relevant if it, or a local that may hold its
/// value, reaches a call, an attribute receiver, a return/yield/await, a
/// store through an attribute or subscript, a `with`, or a nested scope.
/// A parameter used only in a condition is not relevant.
#[salsa::tracked(returns(ref))]
pub fn relevant_params<'db>(db: &'db dyn Db, func: Definition<'db>) -> Vec<bool> {
    let parsed = parsed_module(db, func.python_file(db)).load(db);
    let DefinitionKind::Function(f) = func.kind(db) else {
        return Vec::new();
    };
    let def = f.node(&parsed);
    let params: Vec<&Name> = param_names(def).collect(); // slot order, step 02.01
    if params.is_empty() {
        return Vec::new();
    }

    // holds[name] = the parameters a local may hold. Seeded by the parameters themselves.
    let mut holds: FxHashMap<Name, FxHashSet<usize>> = params
        .iter()
        .enumerate()
        .map(|(i, n)| ((*n).clone(), FxHashSet::from_iter([i])))
        .collect();
    let mut relevant = vec![false; params.len()];

    loop {
        let mut pass = Taint {
            holds: &mut holds,
            relevant: &mut relevant,
            escapes: FxHashSet::default(),
            changed: false,
        };
        pass.visit_body(&def.body);
        let changed = pass.changed;
        if !changed || relevant.iter().all(|&r| r) {
            break; // nothing left to find
        }
    }
    relevant
}

struct Taint<'a> {
    holds: &'a mut FxHashMap<Name, FxHashSet<usize>>,
    relevant: &'a mut Vec<bool>,
    /// Names declared `global` or `nonlocal`: a write to one leaves the function.
    escapes: FxHashSet<Name>,
    changed: bool,
}

/// Every name read or written anywhere inside the node.
#[derive(Default)]
struct Names(Vec<Name>);

impl<'a> Visitor<'a> for Names {
    fn visit_expr(&mut self, e: &'a Expr) {
        if let Expr::Name(n) = e {
            self.0.push(n.id.clone());
        }
        walk_expr(self, e);
    }
}

impl Taint<'_> {
    /// Every parameter any name in `e` may hold.
    fn params_in(&self, e: &Expr) -> FxHashSet<usize> {
        let mut names = Names::default();
        names.visit_expr(e);
        self.params_of(names)
    }

    fn params_of(&self, names: Names) -> FxHashSet<usize> {
        let mut out = FxHashSet::default();
        for n in names.0 {
            if let Some(set) = self.holds.get(&n) {
                out.extend(set);
            }
        }
        out
    }

    fn mark(&mut self, ps: FxHashSet<usize>) {
        for i in ps {
            if !self.relevant[i] {
                self.relevant[i] = true;
                self.changed = true;
            }
        }
    }

    fn sink(&mut self, e: &Expr) {
        let ps = self.params_in(e);
        self.mark(ps);
    }

    /// A nested `def` or `class` captures: every name it reads, decorators and defaults included.
    fn sink_names_in(&mut self, stmt: &Stmt) {
        let mut names = Names::default();
        names.visit_stmt(stmt);
        let ps = self.params_of(names);
        self.mark(ps);
    }

    /// `target = from`: every name the target binds may now hold what `from` holds.
    /// An attribute or subscript target is a store: a sink for both sides.
    fn flow(&mut self, target: &Expr, from: &Expr) {
        match target {
            Expr::Name(n) if self.escapes.contains(&n.id) => self.sink(from),
            Expr::Name(n) => {
                let ps = self.params_in(from);
                let set = self.holds.entry(n.id.clone()).or_default();
                let before = set.len();
                set.extend(&ps);
                if set.len() != before {
                    self.changed = true;
                }
            }
            Expr::Tuple(t) => t.elts.iter().for_each(|e| self.flow(e, from)),
            Expr::List(l) => l.elts.iter().for_each(|e| self.flow(e, from)),
            Expr::Starred(s) => self.flow(&s.value, from),
            other => {
                self.sink(from);
                self.sink(other);
            }
        }
    }
}

impl<'a> Visitor<'a> for Taint<'_> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::Assign(a) => a.targets.iter().for_each(|t| self.flow(t, &a.value)),
            Stmt::AnnAssign(a) => {
                if let Some(v) = &a.value {
                    self.flow(&a.target, v)
                }
            }
            Stmt::AugAssign(a) => self.flow(&a.target, &a.value),
            Stmt::For(f) => self.flow(&f.target, &f.iter),
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    self.sink(v)
                }
            }
            Stmt::With(w) => {
                for item in &w.items {
                    self.sink(&item.context_expr);
                    if let Some(t) = &item.optional_vars {
                        self.flow(t, &item.context_expr)
                    }
                }
            }
            Stmt::Match(m) => self.sink(&m.subject), // its patterns bind names: be conservative
            Stmt::Global(g) => self.escapes.extend(g.names.iter().map(|i| i.id.clone())),
            Stmt::Nonlocal(n) => self.escapes.extend(n.names.iter().map(|i| i.id.clone())),
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => self.sink_names_in(stmt),
            _ => {} // `if` and `while` tests are not sinks; their bodies are visited below
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, e: &'a Expr) {
        match e {
            Expr::Call(c) => {
                self.sink(&c.func);
                for arg in c.arguments.iter_source_order() {
                    self.sink(arg.value())
                }
            }
            Expr::Attribute(a) => self.sink(&a.value),
            Expr::Yield(_) | Expr::YieldFrom(_) | Expr::Await(_) => self.sink(e),
            Expr::Named(n) => self.flow(&n.target, &n.value),
            Expr::Lambda(_)
            | Expr::ListComp(_)
            | Expr::SetComp(_)
            | Expr::DictComp(_)
            | Expr::Generator(_) => self.sink(e),
            _ => {}
        }
        walk_expr(self, e);
    }
}
