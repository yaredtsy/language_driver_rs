// src/interp/entry.rs
//! Inline mode is the fixture suite's cross-check and is not reachable from the public API.
#![allow(dead_code)]
use crate::interp::bind::EvaluatedArgs;
use crate::interp::env::Env;
use crate::interp::solver::Solver;
use crate::interp::value::{AbstractValue, ActivationId, OriginKey};
use crate::interp::walk::Walk;
use crate::sem::{Definition, EntryKind, Scope};

/// What the entry's walk begins with.
#[derive(Default)]
pub(crate) struct Start<'db> {
    pub receiver: Option<AbstractValue<'db>>,
    pub enclosing: Option<ActivationId>,
}

impl<'db> Walk<'db> {
    /// Walk down from the module to `entry`, running what Python would run to get
    /// there. Every node it makes is dropped; its effects stay (in `env`, and in the
    /// runs the entry's free variables are read from).
    pub(crate) fn prologue(
        &mut self,
        sol: &mut Solver<'db>,
        entry: Definition<'db>,
        env: &mut Env<'db>,
    ) -> Start<'db> {
        let none = EvaluatedArgs::default();
        let mut enclosing = None; // the run of the nearest enclosing *function*
        let mut class = None; // the class just stepped into, if the next step is its method
        for scope in sol.sem.scope_chain(entry) {
            match scope {
                Scope::Class(c) => class = Some(c), // a class body is not a run
                Scope::Function(f) => {
                    let receiver = match class.take() {
                        Some(c) => self.receiver_for(sol, c, f, enclosing, env),
                        None => None, // a plain function or a closure
                    };
                    if f == entry {
                        return Start {
                            receiver,
                            enclosing,
                        };
                    }
                    let run = self.walk_inline(sol, f, enclosing, receiver, &none, env); // hidden
                    sol.stats.prologue_walks += 1;
                    enclosing = run.act;
                }
            }
        }
        unreachable!("the chain ends with the entry")
    }

    /// `func` is defined in `class`: what it is called on.
    fn receiver_for(
        &mut self,
        sol: &mut Solver<'db>,
        class: Definition<'db>,
        func: Definition<'db>,
        enclosing: Option<ActivationId>,
        env: &mut Env<'db>,
    ) -> Option<AbstractValue<'db>> {
        match sol.sem.entry_kind(class, func) {
            EntryKind::Method => Some(self.build_object(sol, class, enclosing, env)),
            EntryKind::ClassMethod => Some(AbstractValue::Class(class)),
            EntryKind::StaticMethod => None,
        }
    }

    /// A new `class` object with `__init__` walked on it, hidden.
    fn build_object(
        &mut self,
        sol: &mut Solver<'db>,
        class: Definition<'db>,
        enclosing: Option<ActivationId>,
        env: &mut Env<'db>,
    ) -> AbstractValue<'db> {
        let site = sol.sem.def_range(class); // the class statement: one object
        let origin = self.origins.intern(OriginKey::New {
            site,
            act: self.frame.act,
        });
        let obj = AbstractValue::Instance { class, origin };
        if let Some(init) = sol.sem.init_of(class).filter(|&i| sol.sem.is_project(i)) {
            // an inherited `__init__` lives in another scope chain: it gets no run of ours
            let defined_here = matches!(
                sol.sem.scope_chain(init).iter().rev().nth(1),
                Some(Scope::Class(c)) if *c == class
            );
            let run = if defined_here { enclosing } else { None };
            self.walk_inline(sol, init, run, Some(obj), &EvaluatedArgs::default(), env);
            sol.stats.prologue_walks += 1;
        }
        obj
    }
}
