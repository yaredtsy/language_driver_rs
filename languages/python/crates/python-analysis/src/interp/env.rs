// src/interp/env.rs
use crate::interp::value::{AbstractValue, Values, dedup};
use crate::interp::value::{ActivationId, OriginId};
use crate::sem::{BodyScope, Name};

/// What each name holds on the path being walked. Cloning is a pointer copy.
#[derive(Clone, Default)]
pub(crate) struct Env<'db> {
    pub locals: rpds::HashTrieMap<Name, Values<'db>>,
    /// What each object holds on this path: (which object, attribute name).
    pub attrs: rpds::HashTrieMap<(OriginId, Name), Values<'db>>,
    /// Which function body these locals belong to. Fixed for a run.
    pub scope: Option<BodyScope<'db>>,
    /// The run where this function's `def` ran. NEVER the caller.
    pub enclosing: Option<ActivationId>,
    /// Holds a closure's free values by name, whatever scope binds them.
    pub flat: bool,
}

impl<'db> Env<'db> {
    pub(crate) fn flat() -> Self {
        Env {
            flat: true,
            ..Env::default()
        }
    }
    pub(crate) fn get(&self, name: &Name) -> Option<&Values<'db>> {
        self.locals.get(name)
    }
    pub(crate) fn bind(&mut self, name: &Name, v: Values<'db>) {
        self.locals.insert_mut(name.clone(), v);
    }
    /// `strong`: the receiver is exactly one object, so the write replaces.
    /// Otherwise it is added to what the attribute may already hold, and when this path holds nothing for it, to `Unknown`: the object
    /// may keep a value this path does not know, and the read must not look as if the write were the only one.
    pub(crate) fn bind_attr(
        &mut self,
        origin: OriginId,
        name: &Name,
        v: Values<'db>,
        strong: bool,
    ) {
        let key = (origin, name.clone());
        let v = match (strong, self.attrs.get(&key)) {
            (false, Some(old)) => {
                let mut m = old.clone();
                m.extend(v);
                dedup(&mut m);
                m
            }
            (false, None) => {
                let mut m = v;
                m.push(AbstractValue::Unknown);
                dedup(&mut m);
                m
            }
            _ => v,
        };
        self.attrs.insert_mut(key, v);
    }
    /// Rebuilding an object from a fingerprint: what the attribute is, added to what an earlier member of a union already put there. Not a
    /// write: nothing here may have been missed, so unlike a weak `bind_attr` it adds no `Unknown`.
    pub(crate) fn join_attr(&mut self, origin: OriginId, name: &Name, v: Values<'db>) {
        let key = (origin, name.clone());
        let v = match self.attrs.get(&key) {
            Some(old) => {
                let mut m = old.clone();
                m.extend(v);
                dedup(&mut m);
                m
            }
            None => v,
        };
        self.attrs.insert_mut(key, v);
    }
    /// One object's attributes, sorted by name. Scans the flat map.
    pub(crate) fn attrs_of(&self, origin: OriginId) -> Vec<(Name, Values<'db>)> {
        let mut out: Vec<(Name, Values<'db>)> = self
            .attrs
            .iter()
            .filter(|((o, _), _)| *o == origin)
            .map(|((_, n), v)| (n.clone(), v.clone()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
    /// Union per name. A name missing from an arm was not assigned on that path.
    pub(crate) fn join(arms: Vec<Option<Env<'db>>>) -> Option<Env<'db>> {
        let mut arms = arms.into_iter().flatten();
        let mut out = arms.next()?;
        for arm in arms {
            union_into(&mut out.locals, &arm.locals);
            union_into(&mut out.attrs, &arm.attrs);
        }
        Some(out)
    }
}

fn union_into<'db, K: Clone + Eq + std::hash::Hash>(
    out: &mut rpds::HashTrieMap<K, Values<'db>>,
    arm: &rpds::HashTrieMap<K, Values<'db>>,
) {
    for (key, vals) in arm.iter() {
        let merged = match out.get(key) {
            Some(old) if old == vals => continue,
            Some(old) => {
                let mut m = old.clone();
                m.extend(vals.iter().copied());
                dedup(&mut m);
                m
            }
            None => vals.clone(),
        };
        out.insert_mut(key.clone(), merged);
    }
}
