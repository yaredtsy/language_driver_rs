//! Identities that mean the same thing in another request, another process, or
//! after an edit above the function. Plain data: no `'db`.

pub use analyzer_core::graph::StableDef;

/// A `StableDef` as the bytes of a context digest see it. A digest is a hash of a serialized key, and it is the context's identity outside the
/// process (a cache, a UI selection), so those bytes must not move: a marked identity is still written `Node`, as pylspt wrote it, although
/// core calls it `Marked`. Used by `serialize_with` on every `StableDef` that goes into a key; nothing else is written this way.
pub fn hashed_def<S: serde::Serializer>(def: &StableDef, serializer: S) -> Result<S::Ok, S::Error> {
    use serde::Serialize as _;
    #[derive(serde::Serialize)]
    enum Hashed<'a> {
        Node(&'a str),
        Path {
            module: &'a str,
            qname: &'a str,
            nth: u16,
        },
    }
    match def {
        StableDef::Marked(id) => Hashed::Node(id).serialize(serializer),
        StableDef::Path { module, qname, nth } => Hashed::Path {
            module,
            qname,
            nth: *nth,
        }
        .serialize(serializer),
    }
}

/// A type, by structure. Never a display string.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, serde::Serialize)]
pub enum StableType {
    Instance {
        #[serde(serialize_with = "hashed_def")]
        class: StableDef,
        args: Vec<StableType>,
    },
    Callable {
        params: Option<Vec<StableType>>, // None: `...`
        ret: Box<StableType>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Serialize)]
    struct Hashed {
        #[serde(serialize_with = "hashed_def")]
        def: StableDef,
    }

    /// The bytes a context digest hashes. If these change, every digest changes, and with them every identity a consumer stored.
    #[test]
    fn a_def_is_hashed_the_way_pylspt_wrote_it() {
        let marked = Hashed {
            def: StableDef::Marked("abc".into()),
        };
        assert_eq!(
            serde_json::to_string(&marked).unwrap(),
            r#"{"def":{"Node":"abc"}}"#
        );
        let path = Hashed {
            def: StableDef::Path {
                module: "m".into(),
                qname: "f".into(),
                nth: 2,
            },
        };
        assert_eq!(
            serde_json::to_string(&path).unwrap(),
            r#"{"def":{"Path":{"module":"m","qname":"f","nth":2}}}"#
        );
    }
}
