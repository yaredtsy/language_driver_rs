//! Aliases (policy row `aliases`): a name that is not the def's own. Every edge names the real def.
use crate::calls::helper as h;
use crate::shapes::Rec;

pub use crate::calls::helper as shown;

type A = Rec;

/// `use … as`: the call is `h()`, the def is `calls::helper`.
pub fn by_use_as() -> u8 {
    h()
}

/// `pub use … as`, called through its new path.
pub fn by_pub_use() -> u8 {
    crate::aliases::shown()
}

/// A type alias in a path: the construction is of `Rec`.
pub fn by_type_alias() -> A {
    A::mk()
}
