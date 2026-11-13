// src/interp/env.rs
use crate::interp::value::{AbstractValue, Values, dedup};
use crate::interp::value::{ActivationId, OriginId};
use crate::sem::{BodyScope, Name};

/// How sure a walk is that it wrote an attribute: on every path that reaches here (`Must`), or on some of them (`May`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Write {
    Must,
    May,
}

/// What each name holds on the path being walked. Cloning is a pointer copy.
#[derive(Clone, Default)]
pub(crate) struct Env<'db> {
    pub locals: rpds::HashTrieMap<Name, Values<'db>>,
    /// What each object holds on this path: (which object, attribute name).
    pub attrs: rpds::HashTrieMap<(OriginId, Name), Values<'db>>,
    /// Every attribute this walk wrote, on any object, and how sure it is. The record is of the write, not of a changed value: `p.h = p.h`
    /// is in it. Only `bind_attr` adds to it.
    pub written: rpds::HashTrieMap<(OriginId, Name), Write>,
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
        // a weak write on top of a strong one leaves it strong: the attribute was written on every path anyway
        if strong || self.written.get(&key).is_none() {
            let how = if strong { Write::Must } else { Write::May };
            self.written.insert_mut(key.clone(), how);
        }
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
    /// A strong write whose values are all `Unknown`, as an effect says it: the object no longer holds what it held, so the attribute is
    /// removed and a read of it asks the engine's declared type. It is a write: `Must` is recorded.
    pub(crate) fn forget_attr(&mut self, origin: OriginId, name: &Name) {
        let key = (origin, name.clone());
        self.attrs.remove_mut(&key);
        self.written.insert_mut(key, Write::Must);
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
    /// Union per name. A name missing from an arm was not assigned on that path. A write stays `Must` only where every arm has it `Must`.
    pub(crate) fn join(arms: Vec<Option<Env<'db>>>) -> Option<Env<'db>> {
        let mut arms = arms.into_iter().flatten();
        let mut out = arms.next()?;
        for arm in arms {
            union_into(&mut out.locals, &arm.locals);
            union_into(&mut out.attrs, &arm.attrs);
            join_written(&mut out.written, &arm.written);
        }
        Some(out)
    }
}

fn join_written(
    out: &mut rpds::HashTrieMap<(OriginId, Name), Write>,
    arm: &rpds::HashTrieMap<(OriginId, Name), Write>,
) {
    // collected first: `out` cannot be changed while it is iterated
    let weakened: Vec<(OriginId, Name)> = out
        .iter()
        .filter(|(key, how)| **how == Write::Must && arm.get(*key) != Some(&Write::Must))
        .map(|(key, _)| key.clone())
        .collect();
    for key in weakened {
        out.insert_mut(key, Write::May);
    }
    for key in arm.keys() {
        if out.get(key).is_none() {
            out.insert_mut(key.clone(), Write::May);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    fn obj() -> OriginId {
        OriginId::from_u32(1)
    }

    fn attr() -> Name {
        Name::new("handler")
    }

    fn unknown<'db>() -> Values<'db> {
        smallvec![AbstractValue::Unknown]
    }

    fn written_of(env: &Env<'_>) -> Option<Write> {
        env.written.get(&(obj(), attr())).copied()
    }

    #[test]
    fn a_strong_write_is_must() {
        let mut env = Env::default();
        env.bind_attr(obj(), &attr(), unknown(), true);
        assert_eq!(written_of(&env), Some(Write::Must));
    }

    #[test]
    fn a_weak_write_is_may() {
        let mut env = Env::default();
        env.bind_attr(obj(), &attr(), unknown(), false);
        assert_eq!(written_of(&env), Some(Write::May));
    }

    #[test]
    fn a_weak_write_over_a_strong_one_stays_must() {
        let mut env = Env::default();
        env.bind_attr(obj(), &attr(), unknown(), true);
        env.bind_attr(obj(), &attr(), unknown(), false);
        assert_eq!(written_of(&env), Some(Write::Must));
    }

    #[test]
    fn a_join_of_a_must_and_nothing_is_may() {
        let mut written = Env::default();
        written.bind_attr(obj(), &attr(), unknown(), true);
        let joined = Env::join(vec![Some(written), Some(Env::default())]).unwrap();
        assert_eq!(written_of(&joined), Some(Write::May));
        // the other way round: the arm that wrote is not the first one
        let mut written = Env::default();
        written.bind_attr(obj(), &attr(), unknown(), true);
        let joined = Env::join(vec![Some(Env::default()), Some(written)]).unwrap();
        assert_eq!(written_of(&joined), Some(Write::May));
    }

    #[test]
    fn a_join_of_two_musts_is_must() {
        let mut a = Env::default();
        a.bind_attr(obj(), &attr(), unknown(), true);
        let b = a.clone();
        let joined = Env::join(vec![Some(a), Some(b)]).unwrap();
        assert_eq!(written_of(&joined), Some(Write::Must));
    }

    #[test]
    fn rebuilding_an_object_from_a_fingerprint_records_no_write() {
        let mut env = Env::default();
        env.join_attr(obj(), &attr(), unknown());
        assert_eq!(written_of(&env), None);
        assert!(env.written.is_empty());
    }

    #[test]
    fn a_forgotten_attribute_is_absent_and_written_must() {
        let mut env = Env::default();
        env.bind_attr(obj(), &attr(), unknown(), true);
        env.forget_attr(obj(), &attr());
        assert!(env.attrs.get(&(obj(), attr())).is_none());
        assert_eq!(written_of(&env), Some(Write::Must));
        // on an object that never held it, too: it is still a write
        let mut env = Env::default();
        env.forget_attr(obj(), &attr());
        assert_eq!(written_of(&env), Some(Write::Must));
    }
}
