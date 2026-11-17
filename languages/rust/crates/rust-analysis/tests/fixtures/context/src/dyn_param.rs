use crate::{Csv, Json, Writer};

/// A `dyn` parameter: `dyn_parameter` is one node, called with a `Json` and with a `Csv`; the call inside is the trait's declaration (`dyn`).
pub fn dyn_parameter(w: &dyn Writer) -> u8 {
    w.write()
}

pub fn call_dyn() -> u8 {
    dyn_parameter(&Json) + dyn_parameter(&Csv)
}
