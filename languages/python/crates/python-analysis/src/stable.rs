//! Identities that mean the same thing in another request, another process, or
//! after an edit above the function. Plain data: no `'db`.

pub use analyzer_core::graph::StableDef;

/// A type, by structure. Never a display string.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, serde::Serialize)]
pub enum StableType {
    Instance {
        class: StableDef,
        args: Vec<StableType>,
    },
    Callable {
        params: Option<Vec<StableType>>, // None: `...`
        ret: Box<StableType>,
    },
}
