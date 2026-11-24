//! What a context holds besides the function: its key, interned (`docs/03-typescript/12-context-layer/03-design.md` §6). A key is a number into
//! the request's `Keys`; what it holds is a `KeyData` in its normal form. TypeScript is not context sensitive yet: `KeyData` has no field, so the
//! only key is the open one, `KeyId::OPEN`, with digest zeros, and one context is one function (`docs/02-tree-rules/03-the-seam.md` §3).

use indexmap::IndexSet;

/// A key: its number in the request's `Keys`. Two keys of one request are equal exactly when their numbers are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct KeyId(u32);

impl KeyId {
    /// Everything unknown: the key of a request's entry always, and of every context until the layer knows a value. Its digest is zeros, so a
    /// context under it has the id it has without the layer.
    pub(crate) const OPEN: KeyId = KeyId(0);
}

/// The key the solver is keyed by (`Lang::Key`).
pub(crate) type Key = KeyId;

/// What a key holds, in its normal form. No field yet: TC1 step 4 adds the receiver, the parameters and the captured variables, so until then
/// every `KeyData` is the open key's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct KeyData {}

impl KeyData {
    /// The one normal form (`03-design.md` §6), made by `Keys::intern` and nowhere else. The identity while `KeyData` has no field.
    fn normal(self) -> KeyData {
        self
    }
}

/// Every key of one request, each once: its number is its place in `table`, and `digests` holds its digest at the same place, made once, when
/// the key is first met. Numbers follow the order keys are first met, which is the solver's order, so they are the same on every run (rule 9).
pub(crate) struct Keys {
    table: IndexSet<KeyData>,
    digests: Vec<[u8; 16]>,
}

impl Keys {
    /// The table of a new request: the open key alone, number 0.
    pub(crate) fn new() -> Keys {
        let mut keys = Keys {
            table: IndexSet::new(),
            digests: Vec::new(),
        };
        let open = keys.intern(KeyData::default());
        debug_assert_eq!(open, KeyId::OPEN, "the open key is the first one");
        keys
    }

    /// The number of `data`, in its normal form: the one it already has, or the next one.
    pub(crate) fn intern(&mut self, data: KeyData) -> KeyId {
        let (index, new) = self.table.insert_full(data.normal());
        if new {
            self.digests.push(digest_of(&self.table[index]));
        }
        KeyId(u32::try_from(index).expect(
            "a request has fewer than u32::MAX keys: its contexts are bounded by its budget",
        ))
    }

    /// The digest of `key`, a key of this table.
    pub(crate) fn digest(&self, key: KeyId) -> [u8; 16] {
        self.digests[key.0 as usize]
    }
}

/// The digest of a key in its normal form: zeros for the open key, the only key there is. TC2 step 1 replaces this with core's function (D57),
/// with the test that pins its bytes.
fn digest_of(_data: &KeyData) -> [u8; 16] {
    [0; 16]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_table_holds_the_open_key_alone_with_digest_zeros() {
        let keys = Keys::new();
        assert_eq!(keys.digest(KeyId::OPEN), [0; 16]);
        assert_eq!(keys.table.len(), 1);
        assert_eq!(keys.digests.len(), 1);
    }

    #[test]
    fn interning_the_open_key_again_gives_its_number_and_adds_nothing() {
        let mut keys = Keys::new();
        assert_eq!(keys.intern(KeyData::default()), KeyId::OPEN);
        assert_eq!(keys.intern(KeyData::default()), KeyId::OPEN);
        assert_eq!(keys.table.len(), 1);
        assert_eq!(keys.digests.len(), 1);
    }
}
