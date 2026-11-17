use crate::{Csv, Json, Writer};

/// A generic called from two callers, with a different type each: layer 1 has one node, `emit`, whose `w.write()` is the trait's declaration (`type_param`);
/// RC1 gives each caller its own `emit`, with `Json::write` and `Csv::write` under them.
pub fn emit<W: Writer>(w: &W) -> u8 {
    w.write()
}

pub fn from_json() -> u8 {
    emit(&Json)
}

pub fn from_csv() -> u8 {
    emit(&Csv)
}

pub fn both() -> u8 {
    from_json() + from_csv()
}
