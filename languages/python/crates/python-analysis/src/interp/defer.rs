// src/interp/defer.rs
use ruff_index::IndexVec;
use smallvec::smallvec;

use crate::interp::bind::EvaluatedArgs;
use crate::interp::call::How;
use crate::interp::env::Env;
use crate::interp::key::ContextId;
use crate::interp::solver::{Got, Solver};
use crate::interp::summary::SiteId;
use crate::interp::value::{AbstractValue, DeferId, Values, dedup};
use crate::interp::walk::Walk;

#[derive(Clone)]
pub(crate) struct Deferred<'db> {
    pub op: DeferredOp<'db>,
    pub before: Env<'db>, // the env at that point: an O(1) clone
}

// moved from pylspt unchanged: boxing the large variant changes every construction and match, which is a rewrite
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
pub(crate) enum DeferredOp<'db> {
    /// A call to a context that is still running. Its value is that context's returns.
    Apply { ctx: ContextId },
    /// A call whose callee, or an argument a key would read, is pending.
    /// It can't be keyed now. It is keyed in the fixpoint.
    Call {
        site: SiteId,
        callee: Values<'db>,
        args: EvaluatedArgs<'db>,
        /// ty's answer for the call expression, for a callee that is not project code.
        fallback: Values<'db>,
        /// The `CallEdge` pushed at this point of the walk, refilled by every replay.
        edge: usize,
        /// How the site knows what it calls (`How::Named`: the callee expression names a def or a class).
        how: How,
    },
}

/// Every pending value empty: round 0.
pub(crate) fn no_values<'db>(w: &Walk<'db>) -> IndexVec<DeferId, Values<'db>> {
    IndexVec::from_raw(vec![Values::new(); w.deferred.len()])
}

/// Replace every Pending(id) by what it resolved to.
fn substitute<'db>(vals: &Values<'db>, now: &IndexVec<DeferId, Values<'db>>) -> Values<'db> {
    vals.iter()
        .flat_map(|v| match *v {
            AbstractValue::Pending(id) => now[id].clone(),
            other => smallvec![other],
        })
        .collect()
}

impl<'db> Walk<'db> {
    pub(crate) fn defer(&mut self, op: DeferredOp<'db>, env: &Env<'db>) -> DeferId {
        self.deferred.push(Deferred {
            op,
            before: env.clone(),
        })
    }

    /// Each deferred operation once, in order, starting from the last round's
    /// values. An operation may read the results of earlier ones in this same
    /// round. `capped`: the last round allowed, so every result also gets `Unknown`.
    pub(crate) fn replay(
        &mut self,
        sol: &mut Solver<'db>,
        capped: bool,
    ) -> IndexVec<DeferId, Values<'db>> {
        self.now = Some(self.resolved.clone());
        self.replaying = true;
        let ids: Vec<DeferId> = self.deferred.indices().collect();
        for id in ids {
            let d = self.deferred[id].clone();
            let v = match d.op {
                DeferredOp::Apply { ctx } => match sol.get(ctx) {
                    Got::Ready(res) => self.resolve(&res.returns),
                    Got::Pending => Values::new(), // the solver flagged it: the round is abandoned
                },
                DeferredOp::Call {
                    site,
                    callee,
                    args,
                    fallback,
                    edge,
                    how,
                } => {
                    let now = self.now.as_ref().expect("set above");
                    let callee = substitute(&callee, now);
                    let args = EvaluatedArgs {
                        positional: args.positional.iter().map(|v| substitute(v, now)).collect(),
                        keywords: args
                            .keywords
                            .iter()
                            .map(|(n, v)| (n.clone(), substitute(v, now)))
                            .collect(),
                        has_splat: args.has_splat,
                    };
                    self.call_values(
                        sol,
                        site,
                        &callee,
                        &args,
                        None,
                        &fallback,
                        Some(edge),
                        how,
                        &mut d.before.clone(),
                    )
                }
            };
            let mut v = v;
            if capped {
                v.push(AbstractValue::Unknown);
                dedup(&mut v);
            }
            self.now.as_mut().expect("set above")[id] = v;
        }
        self.replaying = false;
        self.now.take().expect("set above")
    }
}
