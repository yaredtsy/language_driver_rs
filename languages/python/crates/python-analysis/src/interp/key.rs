use ruff_index::{IndexVec, newtype_index};
use ruff_python_ast::StmtFunctionDef;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::hash::Hash;

use smallvec::smallvec;

use crate::interp::bind::{Bound, param_names};
use crate::interp::env::Env;
use crate::interp::relevant::relevant_params;
use crate::interp::solver::Solver;
use crate::interp::value::{AbstractValue, ActivationId, OriginKey, Path, Root, Values};
use crate::interp::walk::Walk;
use crate::sem::{Definition, Name, Type};
use ty_python_core::ProgramFile;

#[newtype_index]
#[derive(Ord, PartialOrd)] // `fingerprint` sorts FpIds
pub struct FpId;

#[newtype_index]
pub struct ContextId;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Fp<'db> {
    Unknown,
    Data(Type<'db>),
    Function(Definition<'db>),
    Class(Definition<'db>),
    Module(Type<'db>),
    Union(SmallVec<[FpId; 4]>),
    Closure {
        func: Definition<'db>,
        free: SmallVec<[(Name, FpId); 2]>,     // sorted by name
        defaults: SmallVec<[(Name, FpId); 2]>, // the defaults it was made with
    },
    Back(u8), // the closure k levels up
    /// An object that holds state: its class and its attributes, sorted by name.
    Instance {
        class: Definition<'db>,
        attrs: SmallVec<[(Name, FpId); 4]>,
    },
    /// Its class only: no state, or the depth limit.
    Opaque(Definition<'db>),
    /// A bound method's receiver.
    Bound {
        func: Definition<'db>,
        recv: FpId,
    },
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Context<'db> {
    pub func: Definition<'db>,
    pub args: SmallVec<[FpId; 4]>,
    /// The called closure's own free values. Empty for a plain function.
    pub free: SmallVec<[(Name, FpId); 2]>,
    /// ... and its own defaults.
    pub defaults: SmallVec<[(Name, FpId); 2]>,
}

/// The closures being fingerprinted, innermost last, compared by `(func, activation)`.
#[derive(Default)]
pub(crate) struct ClosureStack<'db>(SmallVec<[(Definition<'db>, ActivationId); 2]>);

impl<'db> ClosureStack<'db> {
    pub(crate) fn position(&self, f: Definition<'db>, a: ActivationId) -> Option<u8> {
        self.0
            .iter()
            .rev()
            .position(|&(g, b)| g == f && b == a)
            .map(|k| k as u8)
    }
    pub(crate) fn with<R>(
        &mut self,
        f: Definition<'db>,
        a: ActivationId,
        body: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.0.push((f, a));
        let r = body(self);
        self.0.pop();
        r
    }
}

pub(crate) struct Interner<K, I: ruff_index::Idx> {
    items: IndexVec<I, K>,
    ids: FxHashMap<K, I>,
}

impl<K, I: ruff_index::Idx> Default for Interner<K, I> {
    fn default() -> Self {
        Interner {
            items: IndexVec::new(),
            ids: FxHashMap::default(),
        }
    }
}

impl<K: Clone + Eq + Hash, I: ruff_index::Idx> Interner<K, I> {
    pub(crate) fn intern(&mut self, k: K) -> I {
        if let Some(&id) = self.ids.get(&k) {
            return id;
        }
        let id = self.items.push(k.clone());
        self.ids.insert(k, id);
        id
    }
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }
    #[cfg(test)]
    pub(crate) fn iter(&self) -> impl Iterator<Item = (I, &K)> {
        self.items.iter_enumerated()
    }
}

impl<K, I: ruff_index::Idx> std::ops::Index<I> for Interner<K, I> {
    type Output = K;
    fn index(&self, i: I) -> &K {
        &self.items[i]
    }
}

impl<'db> Solver<'db> {
    /// `depth`: how many closures these values sit inside. 0 for a parameter slot.
    pub(crate) fn fingerprint(
        &mut self,
        w: &Walk<'db>,
        env: &Env<'db>,
        file: ProgramFile<'db>,
        vals: &Values<'db>,
        depth: u8,
        cl: &mut ClosureStack<'db>,
    ) -> FpId {
        let mut ids: SmallVec<[FpId; 4]> = vals
            .iter()
            .map(|v| self.fp_one(w, env, file, *v, depth, cl))
            .collect();
        ids.sort();
        ids.dedup();
        match ids.len() {
            0 => self.fps.intern(Fp::Unknown), // no value reaches: keyed as Unknown
            1 => ids[0],
            _ => self.fps.intern(Fp::Union(ids)),
        }
    }

    fn fp_one(
        &mut self,
        w: &Walk<'db>,
        env: &Env<'db>,
        file: ProgramFile<'db>,
        v: AbstractValue<'db>,
        depth: u8,
        cl: &mut ClosureStack<'db>,
    ) -> FpId {
        let fp = match v {
            AbstractValue::Function(d) => Fp::Function(d),
            AbstractValue::BoundMethod { func, receiver } => Fp::Bound {
                func,
                recv: self.fp_one(w, env, file, w.values[receiver], depth + 1, cl),
            },
            AbstractValue::Class(d) => Fp::Class(d), // a class is a constant, like a function
            // An input object this walk only named (`attr_origin`) and nobody wrote onto keys as `Opaque`, as ty's object did before it had
            // a name: `from_fp` gives the callee the same starting env for `Opaque(C)` and `Inst(C, [])`, so the digest would only differ.
            AbstractValue::Instance { class, origin }
                if !w.holds_state(origin)
                    || depth == self.opts.limits.fp_depth
                    || (w.named.contains(&origin) && env.attrs_of(origin).is_empty()) =>
            {
                Fp::Opaque(class)
            }
            AbstractValue::Instance { class, origin } => Fp::Instance {
                class,
                attrs: env
                    .attrs_of(origin)
                    .into_iter()
                    .map(|(name, vals)| {
                        (name, self.fingerprint(w, env, file, &vals, depth + 1, cl))
                    })
                    .collect(),
            },
            AbstractValue::Module(t) => Fp::Module(t),
            AbstractValue::Ty(t) => {
                let widened = self.sem.widen(t, file);
                // a key holds only a type that has one stable form (step 05.01)
                if self.sem.stable_type(widened, file).is_some() {
                    Fp::Data(widened)
                } else {
                    self.stats.types_dropped += 1;
                    Fp::Unknown
                }
            }
            AbstractValue::Unknown => Fp::Unknown,
            AbstractValue::Closure { func, activation } => match cl.position(func, activation) {
                Some(k) => Fp::Back(k),
                None if depth == self.opts.limits.fp_depth => {
                    self.fp_closure_at_limit(w, func, activation)
                }
                None => cl.with(func, activation, |cl| Fp::Closure {
                    func,
                    free: self.fp_free(w, env, func, activation, cl, depth + 1),
                    defaults: self.fp_defaults(w, env, func, activation, cl, depth + 1),
                }),
            },
            // a replay: this round's value (step 04.01, decision 8)
            AbstractValue::Pending(id) => match &w.now {
                Some(now) => return self.fingerprint(w, env, file, &now[id], depth, cl),
                None => {
                    debug_assert!(
                        false,
                        "a pending value reached a key: reaches_pending missed it"
                    );
                    Fp::Unknown
                }
            },
        };
        self.fps.intern(fp)
    }

    /// At the depth limit a closure keeps its function; every captured value
    /// and default is `Unknown`.
    fn fp_closure_at_limit(
        &mut self,
        w: &Walk<'db>,
        func: Definition<'db>,
        act: ActivationId,
    ) -> Fp<'db> {
        let unknown = self.fps.intern(Fp::Unknown);
        Fp::Closure {
            func,
            free: self
                .sem
                .free_variables(func)
                .into_iter()
                .map(|n| (n, unknown))
                .collect(),
            defaults: w
                .defaults
                .get(&(func, act))
                .into_iter()
                .flatten()
                .map(|(n, _)| (n.clone(), unknown))
                .collect(),
        }
    }

    fn fp_free(
        &mut self,
        w: &Walk<'db>,
        env: &Env<'db>,
        func: Definition<'db>,
        activation: ActivationId,
        cl: &mut ClosureStack<'db>,
        depth: u8,
    ) -> SmallVec<[(Name, FpId); 2]> {
        let file = func.program_file(self.sem.db);
        let mut out = SmallVec::new();
        for n in self.sem.free_variables(func) {
            let vals = w.lookup_in(self, func, activation, &n);
            out.push((n, self.fingerprint(w, env, file, &vals, depth, cl)));
        }
        out
    }

    fn fp_defaults(
        &mut self,
        w: &Walk<'db>,
        env: &Env<'db>,
        func: Definition<'db>,
        activation: ActivationId,
        cl: &mut ClosureStack<'db>,
        depth: u8,
    ) -> SmallVec<[(Name, FpId); 2]> {
        let file = func.program_file(self.sem.db);
        let mut out = SmallVec::new();
        for (n, vals) in w.defaults.get(&(func, activation)).into_iter().flatten() {
            out.push((n.clone(), self.fingerprint(w, env, file, vals, depth, cl)));
        }
        out
    }

    /// `closure`: the activation of the closure being called, when the callee is one.
    pub(crate) fn key(
        &mut self,
        w: &Walk<'db>,
        func: Definition<'db>,
        bound: &Bound<'db>,
        closure: Option<ActivationId>,
        env: &Env<'db>,
    ) -> ContextId {
        let file = func.program_file(self.sem.db);
        // a parameter the walk can't be affected by is Unknown: callers share one summary
        let relevant = relevant_params(self.sem.db, func);
        debug_assert_eq!(
            relevant.len(),
            bound.slots.len(),
            "relevance out of sync with binding"
        );
        let args = bound
            .slots
            .iter()
            .enumerate()
            .map(|(i, v)| {
                if relevant.get(i).copied().unwrap_or(true) {
                    self.fingerprint(w, env, file, v, 0, &mut ClosureStack::default())
                } else {
                    self.fps.intern(Fp::Unknown)
                }
            })
            .collect();
        // The called closure counts as a closure on the stack (a `Back(0)` inside reaches it). Its free values are its inputs, as its
        // parameters are: depth 0. Its defaults stay at depth 1. (A closure passed as a value is `fp_one`'s: free values one below.)
        let (free, defaults) = match closure {
            Some(act) => ClosureStack::default().with(func, act, |cl| {
                (
                    self.fp_free(w, env, func, act, cl, 0),
                    self.fp_defaults(w, env, func, act, cl, 1),
                )
            }),
            None => (SmallVec::new(), SmallVec::new()),
        };
        self.contexts.intern(Context {
            func,
            args,
            free,
            defaults,
        })
    }
}

impl<'db> Walk<'db> {
    // moved from pylspt unchanged: the name says where the value comes from, and renaming touches every caller
    #[allow(clippy::wrong_self_convention)]
    /// A value from a fingerprint, sitting at `at`. `cl`: the closures being rebuilt,
    /// innermost last. Attributes of the objects go into `env`.
    fn from_fp(
        &mut self,
        sol: &Solver<'db>,
        fp: FpId,
        at: Path,
        cl: &mut Vec<(Definition<'db>, ActivationId)>,
        env: &mut Env<'db>,
    ) -> Values<'db> {
        match &sol.fps[fp] {
            Fp::Function(d) => smallvec![AbstractValue::Function(*d)],
            Fp::Class(d) => smallvec![AbstractValue::Class(*d)],
            Fp::Module(t) => smallvec![AbstractValue::Module(*t)],
            Fp::Data(t) => smallvec![AbstractValue::Ty(*t)],
            Fp::Unknown => smallvec![AbstractValue::Unknown],
            Fp::Instance { class, attrs } => {
                // 05 §6 claim 1: a key names objects at most `fp_depth` steps from a root, so `max_path` (never less) always fits them
                debug_assert!(at.steps.len() <= usize::from(sol.opts.limits.fp_depth));
                let origin = self.origins.intern(OriginKey::Entry {
                    act: self.frame.act,
                    path: at.clone(),
                });
                for (name, a) in attrs {
                    let v = self.from_fp(sol, *a, at.attr(name), cl, env);
                    env.join_attr(origin, name, v); // a union shares this origin
                }
                smallvec![AbstractValue::Instance {
                    class: *class,
                    origin
                }]
            }
            Fp::Opaque(class) => {
                debug_assert!(at.steps.len() <= usize::from(sol.opts.limits.fp_depth));
                let origin = self.origins.intern(OriginKey::Entry {
                    act: self.frame.act,
                    path: at,
                });
                smallvec![AbstractValue::Instance {
                    class: *class,
                    origin
                }]
            }
            Fp::Bound { func, recv } => {
                let r = self.from_fp(sol, *recv, at.recv(), cl, env);
                r.into_iter()
                    .map(|r| AbstractValue::BoundMethod {
                        func: *func,
                        receiver: self.values.intern(r),
                    })
                    .collect()
            }
            Fp::Union(ids) => {
                let mut out = Values::new();
                for &i in ids {
                    out.extend(self.from_fp(sol, i, at.clone(), cl, env));
                }
                out
            }
            Fp::Back(k) => {
                let (func, activation) = cl[cl.len() - 1 - *k as usize];
                smallvec![AbstractValue::Closure { func, activation }]
            }
            Fp::Closure {
                func,
                free,
                defaults,
            } => {
                let activation = self.activations.push(Env::flat());
                cl.push((*func, activation));
                for (n, (name, fp)) in free.iter().enumerate() {
                    let v = self.from_fp(sol, *fp, at.free(n as u8), cl, env);
                    self.activations[activation].bind(name, v);
                }
                let d = defaults
                    .iter()
                    .enumerate()
                    .map(|(n, (name, fp))| {
                        (
                            name.clone(),
                            self.from_fp(sol, *fp, at.default(n as u8), cl, env),
                        )
                    })
                    .collect();
                self.defaults.insert((*func, activation), d);
                cl.pop();
                smallvec![AbstractValue::Closure {
                    func: *func,
                    activation
                }]
            }
        }
    }

    /// The starting env of a context's walk. Reads the key and the code. Nothing else.
    pub(crate) fn materialize(
        &mut self,
        sol: &Solver<'db>,
        ctx: ContextId,
        node: &StmtFunctionDef,
    ) -> Env<'db> {
        let key = &sol.contexts[ctx];
        let mut env = Env {
            scope: Some(sol.sem.body_scope(key.func, node)),
            ..Env::default()
        };
        if !key.free.is_empty() || !key.defaults.is_empty() {
            // the called closure's own activation, so a Back(0) inside reaches it
            let activation = self.activations.push(Env::flat());
            let mut cl = vec![(key.func, activation)];
            for (i, (name, fp)) in key.free.iter().enumerate() {
                let v = self.from_fp(sol, *fp, Path::root(Root::Free(i as u8)), &mut cl, &mut env);
                self.activations[activation].bind(name, v);
            }
            let d = key
                .defaults
                .iter()
                .enumerate()
                .map(|(i, (n, fp))| {
                    (
                        n.clone(),
                        self.from_fp(
                            sol,
                            *fp,
                            Path::root(Root::Default(i as u8)),
                            &mut cl,
                            &mut env,
                        ),
                    )
                })
                .collect();
            self.defaults.insert((key.func, activation), d);
            env.enclosing = Some(activation);
        }
        for (i, (param, &fp)) in param_names(node).zip(&key.args).enumerate() {
            let v = self.from_fp(
                sol,
                fp,
                Path::root(Root::Param(i as u8)),
                &mut Vec::new(),
                &mut env,
            );
            env.bind(param, v);
        }
        env
    }
}
