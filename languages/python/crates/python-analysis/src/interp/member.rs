use ruff_python_ast::{Expr, ExprAttribute, ExprRef};
use ruff_text_size::{Ranged, TextRange, TextSize};
use smallvec::smallvec;

use crate::interp::{
    Mode, Walk,
    bind::EvaluatedArgs,
    env::Env,
    solver::Solver,
    value::{AbstractValue, OriginId, OriginKey, Values, dedup},
};
use crate::sem::{BoundTo, Name, Shape};

// src/interp/member.rs
impl<'db> Walk<'db> {
    pub(crate) fn eval_attribute(
        &mut self,
        sol: &mut Solver<'db>,
        attr: &ExprAttribute,
        env: &mut Env<'db>,
    ) -> Values<'db> {
        if is_zero_arg_super(&attr.value)
            && let Some(v) = self.super_member(sol, attr, env)
        {
            return v;
        }
        let recvs = self.eval(sol, &attr.value, env);
        let mut out = Values::new();
        let mut asked_ty = false;
        for recv in recvs {
            match recv {
                AbstractValue::Instance { .. }
                | AbstractValue::Class(_)
                | AbstractValue::Module(_) => {
                    out.extend(self.member(sol, recv, &attr.attr.id, attr.range(), env));
                }
                _ if !asked_ty => {
                    asked_ty = true;
                    let ty = sol.sem.type_of(&self.frame.model, ExprRef::from(attr));
                    out.extend(self.lift(sol, ty));
                }
                _ => {}
            }
        }
        dedup(&mut out);
        if out.len() > sol.opts.limits.max_chain_fanout {
            // Which ones survive must not depend on the order they were joined in.
            out.sort_by_key(|v| cut_order(sol, v));
            out.truncate(sol.opts.limits.max_chain_fanout);
            sol.stats.chain_fanout_hits += 1;
        }
        out
    }

    /// `super().name`: ty says which parent method; the receiver is this method's own
    /// `self` (or its class, for a classmethod). `None`: not a method ty bound.
    fn super_member(
        &mut self,
        sol: &mut Solver<'db>,
        attr: &ExprAttribute,
        env: &Env<'db>,
    ) -> Option<Values<'db>> {
        let t = sol.sem.type_of(&self.frame.model, ExprRef::from(attr));
        let Shape::BoundMethod { func, .. } = sol.sem.shape(t, self.frame.file) else {
            return None;
        };
        let me = self.first_parameter_value(sol, env);
        let recvs: Values<'db> = match sol.sem.bound_to(t) {
            BoundTo::Object => me, // a plain method: the same object
            BoundTo::Class => me
                .iter()
                .map(|r| match *r {
                    AbstractValue::Instance { class, .. } => AbstractValue::Class(class),
                    other => other, // already a Class: `cls`
                })
                .collect(),
            BoundTo::Other => return Some(self.lift(sol, t)),
        };
        Some(
            recvs
                .into_iter()
                .map(|r| AbstractValue::BoundMethod {
                    func,
                    receiver: self.values.intern(r),
                })
                .collect(),
        )
    }

    /// The value of this method's first parameter, from the env.
    fn first_parameter_value(&self, sol: &Solver<'db>, env: &Env<'db>) -> Values<'db> {
        let parsed = sol.sem.parsed(self.frame.func);
        let Some(node) = sol.sem.function_node(self.frame.func, &parsed) else {
            return Values::new();
        };
        let Some(first) = node.parameters.iter_non_variadic_params().next() else {
            return Values::new();
        };
        env.get(&first.parameter.name.id)
            .cloned()
            .unwrap_or_default()
    }

    /// Give every `OPAQUE` instance in `vals` the origin named by `key`.
    pub(crate) fn reorigin(&mut self, vals: Values<'db>, key: OriginKey) -> Values<'db> {
        let origin = self.origins.intern(key);
        vals.into_iter()
            .map(|v| match v {
                AbstractValue::Instance { class, origin: o } if o == OriginId::OPAQUE => {
                    AbstractValue::Instance { class, origin }
                }
                other => other,
            })
            .collect()
    }

    /// What `.name` is on one receiver. ty finds it; the receiver bound is ours.
    /// Its own state comes first, then the class (through ty).
    fn member(
        &mut self,
        sol: &mut Solver<'db>,
        recv: AbstractValue<'db>,
        name: &str,
        site: TextRange,
        env: &mut Env<'db>,
    ) -> Values<'db> {
        let file = self.frame.file;
        match recv {
            AbstractValue::Instance { class, origin } => {
                if origin.holds_state()
                    && let Some(v) = env.attrs.get(&(origin, Name::from(name)))
                {
                    return v.clone(); // 1. this object, on this path
                }
                // 2. a property: its getter runs here, as a call at the attribute expression
                if let Some(getter) = sol.sem.property_parts(class, name).and_then(|p| p.getter) {
                    let args = EvaluatedArgs::default();
                    return match sol.opts.mode {
                        Mode::Inline => {
                            self.call_function(sol, getter, None, Some(recv), site, &args, env)
                        }
                        Mode::Summary => self.call_accessor(sol, getter, recv, args, site, env),
                    };
                }
                // 3. the class side, through ty
                let t = sol.sem.member(sol.sem.instance_type(class), name, file);
                match t.map(|t| (t, sol.sem.shape(t, file))) {
                    Some((t, Shape::BoundMethod { func, .. })) => {
                        let receiver = match sol.sem.bound_to(t) {
                            BoundTo::Object => recv,                       // a plain method: OUR object
                            BoundTo::Class => AbstractValue::Class(class), // a classmethod: the object's class
                            BoundTo::Other => return self.lift(sol, t),
                        };
                        let receiver = self.values.intern(receiver);
                        smallvec![AbstractValue::BoundMethod { func, receiver }]
                    }
                    Some((t, _)) => self.lift(sol, t), // a class variable, a declared attribute
                    None => smallvec![AbstractValue::Unknown],
                }
            }
            AbstractValue::Class(c) => match sol.sem.member(sol.sem.class_type(c), name, file) {
                Some(t) => self.lift(sol, t), // a method stays an unbound Function
                None => smallvec![AbstractValue::Unknown],
            },
            AbstractValue::Module(m) => match sol.sem.member(m, name, file) {
                Some(t) => self.lift(sol, t),
                None => smallvec![AbstractValue::Unknown],
            },
            _ => unreachable!("member is only called for Instance, Class and Module"),
        }
    }

    /// `a.b = v`: the value goes to every object `a` may be.
    pub(crate) fn bind_attribute(
        &mut self,
        sol: &mut Solver<'db>,
        attr: &ExprAttribute,
        v: Values<'db>,
        env: &mut Env<'db>,
    ) {
        let recvs = self.eval(sol, &attr.value, env);
        // Counted before the filter: `{a, OPAQUE}` or `{a, Unknown}` may be the
        // other one, so `a` only may have changed.
        let strong = recvs.len() == 1; // exactly one receiver: replace
        for r in recvs.iter().copied() {
            let AbstractValue::Instance { class, origin } = r else {
                continue;
            };
            match sol.sem.property_parts(class, &attr.attr.id) {
                // a property: its setter runs, and nothing is stored under its name
                Some(parts) => {
                    if let Some(setter) = parts.setter {
                        let args = EvaluatedArgs {
                            positional: smallvec![v.clone()],
                            ..Default::default()
                        };
                        match sol.opts.mode {
                            Mode::Inline => {
                                self.call_function(
                                    sol,
                                    setter,
                                    None,
                                    Some(r),
                                    attr.range(),
                                    &args,
                                    env,
                                );
                            }
                            Mode::Summary => {
                                self.call_accessor(sol, setter, r, args, attr.range(), env);
                            }
                        }
                    }
                }
                None if origin.holds_state() => {
                    env.bind_attr(origin, &attr.attr.id, v.clone(), strong)
                }
                None => {}
            }
        }
    }
}

/// `super()` with no arguments: its meaning is lexical, so it is read from syntax.
pub(crate) fn is_zero_arg_super(e: &Expr) -> bool {
    matches!(e, Expr::Call(c)
        if c.arguments.is_empty()
        && matches!(&*c.func, Expr::Name(n) if n.id.as_str() == "super"))
}

/// The order a fan-out cut keeps: where the value's class or function is
/// defined, then which object. Values with neither go last, in their order.
fn cut_order<'db>(
    sol: &Solver<'db>,
    v: &AbstractValue<'db>,
) -> (bool, Option<(String, TextSize)>, Option<OriginId>) {
    let def = match *v {
        AbstractValue::Instance { class, .. } | AbstractValue::Class(class) => Some(class),
        AbstractValue::Function(d)
        | AbstractValue::Closure { func: d, .. }
        | AbstractValue::BoundMethod { func: d, .. } => Some(d),
        _ => None,
    };
    let origin = match *v {
        AbstractValue::Instance { origin, .. } => Some(origin),
        _ => None,
    };
    // `false` sorts first: values with a definition before those without
    (def.is_none(), def.map(|d| sol.sem.def_position(d)), origin)
}
