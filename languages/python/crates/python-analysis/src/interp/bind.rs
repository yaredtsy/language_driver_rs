// src/interp/bind.rs
use ruff_python_ast::{
    ArgOrKeyword, Arguments, Expr, ExprRef, ParameterWithDefault, StmtFunctionDef,
};
use smallvec::{SmallVec, smallvec};

use crate::interp::Mode;
use crate::interp::env::Env;
use crate::interp::solver::Solver;
use crate::interp::value::{AbstractValue, ActivationId, OriginKey, Path, Root, Values};
use crate::interp::walk::Walk;
use crate::sem::{Definition, Name, SIGNATURE_KEEPING, Shape, Type};

#[derive(Clone, Default)]
pub(crate) struct EvaluatedArgs<'db> {
    pub positional: SmallVec<[Values<'db>; 4]>,
    pub keywords: SmallVec<[(Name, Values<'db>); 2]>,
    pub has_splat: bool,
}

impl<'db> EvaluatedArgs<'db> {
    fn keyword(&self, name: &str) -> Option<&Values<'db>> {
        self.keywords
            .iter()
            .find(|(n, _)| n.as_str() == name)
            .map(|(_, v)| v)
    }
    /// Python never passes "no value"; an empty slot binds as `Unknown`.
    fn with_unknown_for_empty(&self) -> Self {
        let fix = |v: &Values<'db>| {
            if v.is_empty() {
                smallvec![AbstractValue::Unknown]
            } else {
                v.clone()
            }
        };
        EvaluatedArgs {
            positional: self.positional.iter().map(fix).collect(),
            keywords: self
                .keywords
                .iter()
                .map(|(n, v)| (n.clone(), fix(v)))
                .collect(),
            has_splat: self.has_splat,
        }
    }
    pub(crate) fn has_empty(&self) -> bool {
        self.positional.iter().any(|v| v.is_empty())
            || self.keywords.iter().any(|(_, v)| v.is_empty())
    }
}

/// What each parameter holds, one slot per parameter, in `param_names` order.
#[derive(Clone, Default)]
pub(crate) struct Bound<'db> {
    pub slots: SmallVec<[Values<'db>; 4]>,
}

/// Parameter names in slot order: positional, keyword-only, `*args`, `**kwargs`.
pub(crate) fn param_names(def: &StmtFunctionDef) -> impl Iterator<Item = &Name> {
    let p = &def.parameters;
    p.posonlyargs
        .iter()
        .chain(&p.args)
        .chain(&p.kwonlyargs)
        .map(|p| &p.parameter.name.id)
        .chain(p.vararg.iter().map(|p| &p.name.id))
        .chain(p.kwarg.iter().map(|p| &p.name.id))
}

impl<'db> Walk<'db> {
    /// In the caller's frame and env, left to right, each argument once.
    pub(crate) fn eval_arguments(
        &mut self,
        sol: &mut Solver<'db>,
        args: &Arguments,
        env: &mut Env<'db>,
    ) -> EvaluatedArgs<'db> {
        let mut out = EvaluatedArgs::default();
        for arg in args.iter_source_order() {
            match arg {
                ArgOrKeyword::Arg(Expr::Starred(st)) => {
                    self.eval(sol, &st.value, env);
                    out.has_splat = true;
                }
                ArgOrKeyword::Arg(e) => {
                    let v = self.eval(sol, e, env);
                    out.positional.push(v);
                }
                ArgOrKeyword::Keyword(kw) => {
                    let v = self.eval(sol, &kw.value, env);
                    if let Some(name) = &kw.arg {
                        out.keywords.push((name.id.clone(), v));
                    }
                }
            }
        }
        out
    }
    /// One value per parameter, in `param_names` order, defaults filled.
    /// Must run with the callee's frame active.
    pub(crate) fn bind_parameters(
        &mut self,
        sol: &mut Solver<'db>,
        func: Definition<'db>,
        def: &StmtFunctionDef,
        receiver: Option<AbstractValue<'db>>,
        args: &EvaluatedArgs<'db>,
        enclosing: Option<ActivationId>,
    ) -> Bound<'db> {
        let args = &args.with_unknown_for_empty();
        let mut positional = args.positional.iter();
        let params = &def.parameters;
        let unknown = || smallvec![AbstractValue::Unknown];
        let mut slots: SmallVec<[Values<'db>; 4]> = SmallVec::new();

        // `self` is the first parameter, whatever it is called.
        let has_first = params.posonlyargs.first().or(params.args.first()).is_some();
        if let (Some(recv), true) = (receiver, has_first) {
            slots.push(smallvec![recv]); // even when the rest can't be matched
        }
        let skip = slots.len(); // 0 or 1: parameters already bound

        if args.has_splat || self.decorator_changes_signature(sol, def) {
            for _ in param_names(def).skip(skip) {
                slots.push(unknown());
            }
            return Bound { slots };
        }

        for p in params.posonlyargs.iter().chain(&params.args).skip(skip) {
            let name = &p.parameter.name.id;
            let v = args
                .keyword(name)
                .cloned()
                .or_else(|| positional.next().cloned())
                .or_else(|| self.default_value(sol, func, enclosing, p))
                .unwrap_or_else(unknown);
            slots.push(v);
        }
        for p in &params.kwonlyargs {
            let name = &p.parameter.name.id;
            let v = args
                .keyword(name)
                .cloned()
                .or_else(|| self.default_value(sol, func, enclosing, p))
                .unwrap_or_else(unknown);
            slots.push(v);
        }
        if params.vararg.is_some() {
            slots.push(unknown());
        }
        if params.kwarg.is_some() {
            slots.push(unknown());
        }
        if sol.opts.mode == Mode::Inline {
            self.annotation_fallback(sol, def, &mut slots);
        }
        Bound { slots }
    }

    /// A parameter with no value starts from its annotation, lifted: an object of its
    /// own for this run (`Entry`, not `OPAQUE`, so writes to it are tracked here).
    fn annotation_fallback(
        &mut self,
        sol: &mut Solver<'db>,
        def: &StmtFunctionDef,
        slots: &mut SmallVec<[Values<'db>; 4]>,
    ) {
        let params = &def.parameters;
        let positional = params.posonlyargs.iter().chain(&params.args);
        let others = params.kwonlyargs.iter();
        for (i, p) in positional.chain(others).enumerate() {
            if slots.get(i).map(|v| v.as_slice()) != Some(&[AbstractValue::Unknown][..]) {
                continue;
            }
            let key = OriginKey::Entry {
                act: self.frame.act,
                path: Path::root(Root::Param(i as u8)),
            };
            let v = match &p.parameter.annotation {
                Some(ann) => {
                    let t = sol.sem.type_of(&self.frame.model, ExprRef::from(&**ann));
                    self.instances_of(sol, t)
                }
                None => continue,
            };
            if v.is_empty() {
                continue;
            }
            slots[i] = self.reorigin(v, key);
            sol.stats.annotation_fallbacks += 1;
        }
    }

    /// Objects of the class(es) a type expression names. A union splits into its members.
    fn instances_of(&mut self, sol: &mut Solver<'db>, t: Type<'db>) -> Values<'db> {
        let file = self.frame.file;
        match sol.sem.shape(t, file) {
            Shape::Union(members) => members
                .into_iter()
                .flat_map(|m| self.instances_of(sol, m))
                .collect(),
            _ => match sol.sem.instance_of(t, file) {
                Some(inst) => self.lift(sol, inst),
                None => Values::new(),
            },
        }
    }

    /// The callee's starting env, from the bound slots.
    pub(crate) fn env_from_bound(&self, def: &StmtFunctionDef, bound: &Bound<'db>) -> Env<'db> {
        let mut env = Env::default();
        for (name, v) in param_names(def).zip(&bound.slots) {
            env.bind(name, v.clone());
        }
        env
    }

    /// Binding is only trusted for project decorators and the signature-keeping allow-list.
    fn decorator_changes_signature(
        &mut self,
        sol: &mut Solver<'db>,
        def: &StmtFunctionDef,
    ) -> bool {
        for dec in &def.decorator_list {
            // `@retry` and `@functools.lru_cache(maxsize=1)` alike: look at the callee.
            let target = match &dec.expression {
                Expr::Call(c) => &*c.func,
                e => e,
            };
            let ty = sol.sem.type_of(&self.frame.model, ExprRef::from(target));
            let trusted = match sol.sem.shape(ty, self.frame.file) {
                Shape::Function(d) => {
                    sol.sem.is_project(d) || SIGNATURE_KEEPING.contains(&sol.sem.qname(d).as_str())
                }
                Shape::BoundMethod { func, .. } => {
                    SIGNATURE_KEEPING.contains(&sol.sem.qname(func).as_str())
                }
                Shape::Class(d) => SIGNATURE_KEEPING.contains(&sol.sem.qname(d).as_str()),
                _ => false,
            };
            if !trusted {
                return true;
            }
        }
        false
    }

    fn default_value(
        &mut self,
        sol: &mut Solver<'db>,
        func: Definition<'db>,
        enclosing: Option<ActivationId>,
        p: &ParameterWithDefault,
    ) -> Option<Values<'db>> {
        let expr = p.default.as_deref()?;
        if let Some(act) = enclosing {
            // a nested def: the value computed when its `def` ran
            return self
                .defaults
                .get(&(func, act))?
                .iter()
                .find(|(n, _)| *n == p.parameter.name.id)
                .map(|(_, v)| v.clone());
        }
        let ty = sol.sem.type_of(&self.frame.model, ExprRef::from(expr));
        Some(self.lift(sol, ty))
    }
}
