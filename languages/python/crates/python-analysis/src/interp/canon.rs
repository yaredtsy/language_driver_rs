// src/interp/canon.rs
use crate::interp::key::{ContextId, Fp, FpId};
use crate::interp::solver::Solver;
use crate::stable::{StableDef, StableType};
use ty_python_core::ProgramFile;

/// A context's identity outside this request: its function, and a 128-bit digest of its key.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, serde::Serialize)]
pub struct StableContextId {
    pub func: StableDef,
    pub digest: [u8; 16], // 128-bit hash of the canonical key
}

/// A key written out with nothing request-local in it.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
enum Canon {
    Unknown,
    Data(StableType),
    Function(StableDef),
    Class(StableDef),
    Module(String),
    Union(Vec<Canon>), // sorted by Canon, deduplicated
    Closure {
        func: StableDef,
        free: Vec<(String, Canon)>,
        defaults: Vec<(String, Canon)>,
    },
    Back(u8),
    Instance {
        class: StableDef,
        attrs: Vec<(String, Canon)>,
    },
    Opaque(StableDef),
    Bound {
        func: StableDef,
        recv: Box<Canon>,
    },
}

#[derive(serde::Serialize)]
struct CanonKey {
    func: StableDef,
    args: Vec<Canon>,
    free: Vec<(String, Canon)>,
    defaults: Vec<(String, Canon)>,
}

impl<'db> Solver<'db> {
    pub fn stable_id(&self, ctx: ContextId) -> StableContextId {
        let k = &self.contexts[ctx];
        let file = k.func.program_file(self.sem.db); // types are read in the key's file, as `fp_one` did
        let key = CanonKey {
            func: self.sem.stable_def(k.func),
            args: k.args.iter().map(|&f| self.canon(f, file)).collect(),
            free: self.canon_named(&k.free, file),
            defaults: self.canon_named(&k.defaults, file),
        };
        // Bytes we wrote, never `#[derive(Hash)]`: std makes no promise to keep it stable.
        let bytes = serde_json::to_vec(&key).expect("a canonical key always serializes");
        StableContextId {
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
