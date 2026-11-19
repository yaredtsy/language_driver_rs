//! D45 (`tests/wire.rs`): a marked struct, a marked enum and a marked union, each an entry of its own and each built by a function. On the wire they
//! all take the style's class prefix, and each keeps its real kind.
#![allow(dead_code)]

/// ID: kinds-struct
pub struct S {
    pub a: u8,
}

/// ID: kinds-enum
pub enum E {
    A(u8),
    B,
}

/// ID: kinds-union
pub union U {
    pub whole: u32,
    pub half: u16,
}

pub fn build_struct() -> S {
    S { a: 1 }
}

pub fn build_enum() -> E {
    E::A(1)
}

pub fn build_union() -> U {
    U { whole: 1 }
}
