// src/interp/canon.rs
use crate::interp::key::{ContextId, Fp, FpId};
use crate::interp::solver::Solver;
use crate::stable::{StableDef, StableType};
use ty_python_core::ProgramFile;

/// A key written out with nothing request-local in it.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
enum Canon {
    Unknown,
    Data(StableType),
    Function(#[serde(serialize_with = "crate::stable::hashed_def")] StableDef),
    Class(#[serde(serialize_with = "crate::stable::hashed_def")] StableDef),
    Module(String),
    Union(Vec<Canon>), // sorted by Canon, deduplicated
    Closure {
        #[serde(serialize_with = "crate::stable::hashed_def")]
        func: StableDef,
        free: Vec<(String, Canon)>,
        defaults: Vec<(String, Canon)>,
    },
    Back(u8),
    Instance {
        #[serde(serialize_with = "crate::stable::hashed_def")]
        class: StableDef,
        attrs: Vec<(String, Canon)>,
    },
    Opaque(#[serde(serialize_with = "crate::stable::hashed_def")] StableDef),
    Bound {
        #[serde(serialize_with = "crate::stable::hashed_def")]
        func: StableDef,
        recv: Box<Canon>,
    },
}

#[derive(serde::Serialize)]
struct CanonKey {
    #[serde(serialize_with = "crate::stable::hashed_def")]
    func: StableDef,
    args: Vec<Canon>,
    free: Vec<(String, Canon)>,
    defaults: Vec<(String, Canon)>,
    /// Which slots are one object (07 §3). Left out when it is the identity, so a key with no aliasing has the digest it had before this
    /// field existed: every id in every wire, and the golden, stay as they were.
    #[serde(skip_serializing_if = "is_identity")]
    alias: Vec<u8>,
}

fn is_identity(alias: &[u8]) -> bool {
    alias.iter().enumerate().all(|(i, &j)| i == usize::from(j))
}

impl<'db> Solver<'db> {
    /// A context's identity outside this request: its function, and a 128-bit digest of its key.
    pub(crate) fn stable_id(&self, ctx: ContextId) -> analyzer_core::graph::ContextId {
        let k = &self.contexts[ctx];
        let file = k.func.program_file(self.sem.db); // types are read in the key's file, as `fp_one` did
        let key = CanonKey {
            func: self.sem.stable_def(k.func),
            args: k.args.iter().map(|&f| self.canon(f, file)).collect(),
            free: self.canon_named(&k.free, file),
            defaults: self.canon_named(&k.defaults, file),
            alias: k.alias.to_vec(),
        };
        // Bytes we wrote, never `#[derive(Hash)]`: std makes no promise to keep it stable.
        let bytes = serde_json::to_vec(&key).expect("a canonical key always serializes");
        analyzer_core::graph::ContextId {
            func: key.func,
            digest: xxhash_rust::xxh3::xxh3_128(&bytes).to_le_bytes(),
        }
    }

    fn canon_named(
        &self,
        named: &[(crate::sem::Name, FpId)],
        file: ProgramFile<'db>,
    ) -> Vec<(String, Canon)> {
        named
            .iter()
            .map(|(n, f)| (n.to_string(), self.canon(*f, file)))
            .collect()
    }

    fn canon(&self, fp: FpId, file: ProgramFile<'db>) -> Canon {
        match &self.fps[fp] {
            Fp::Unknown => Canon::Unknown,
            Fp::Data(t) => Canon::Data(
                self.sem
                    .stable_type(*t, file)
                    .expect("only encodable types reach a key"),
            ),
            Fp::Function(d) => Canon::Function(self.sem.stable_def(*d)),
            Fp::Class(d) => Canon::Class(self.sem.stable_def(*d)),
            Fp::Module(t) => Canon::Module(self.sem.module_name(*t)),
            // `fingerprint` sorts members by `FpId`, the interning order of this request
            Fp::Union(ids) => {
                let mut v: Vec<Canon> = ids.iter().map(|&i| self.canon(i, file)).collect();
                v.sort();
                v.dedup();
                Canon::Union(v)
            }
            Fp::Closure {
                func,
                free,
                defaults,
            } => Canon::Closure {
                func: self.sem.stable_def(*func),
                free: self.canon_named(free, file),
                defaults: self.canon_named(defaults, file),
            },
            Fp::Back(k) => Canon::Back(*k),
            Fp::Instance { class, attrs } => Canon::Instance {
                class: self.sem.stable_def(*class),
                attrs: self.canon_named(attrs, file),
            },
            Fp::Opaque(class) => Canon::Opaque(self.sem.stable_def(*class)),
            Fp::Bound { func, recv } => Canon::Bound {
                func: self.sem.stable_def(*func),
                recv: Box::new(self.canon(*recv, file)),
            },
        }
    }
}
