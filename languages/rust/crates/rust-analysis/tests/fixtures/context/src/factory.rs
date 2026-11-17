use crate::{Csv, Json, Writer};

/// A factory that returns a trait object, so what it makes is seen by its callers only as `dyn Writer` (`dyn`); and one that returns `impl Writer`, which the
/// engine sees through to the hidden type, so the call is already the impl's method.
pub fn make(kind: u8) -> Box<dyn Writer> {
    if kind == 0 { Box::new(Json) } else { Box::new(Csv) }
}

pub fn use_factory() -> u8 {
    make(0).write()
}

pub fn make_impl() -> impl Writer {
    Json
}

pub fn use_impl_factory() -> u8 {
    make_impl().write()
}
