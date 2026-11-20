//! A foreign function reached by a call (policy row `foreign`) and a union built (policy row `construct`).
use crate::shapes::{self, Bits};

/// The foreign function has no body: a leaf.
pub fn calls_foreign() -> u8 {
    shapes::foreign(1)
}

/// A union is built like a struct (D45).
pub fn builds_union() -> Bits {
    Bits { whole: 1 }
}
