use crate::{Csv, Json, Writer};

/// A `dyn` in a local: the trait object's type is written once and the value is one of two types, so the call is the trait's declaration (`dyn`).
pub fn dyn_in_a_local(json: bool) -> u8 {
    let w: Box<dyn Writer> = if json { Box::new(Json) } else { Box::new(Csv) };
    w.write()
}
