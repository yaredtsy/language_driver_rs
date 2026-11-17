use crate::{Csv, Json, Writer};

/// A recursive generic: one node, `countdown`, which calls itself (shown, not entered) and `w.write()`, the trait's declaration (`type_param`). RC1 has to
/// end the recursion when the same function comes back under the same types.
pub fn countdown<W: Writer>(w: &W, n: u8) -> u8 {
    if n == 0 { w.write() } else { countdown(w, n - 1) }
}

pub fn start() -> u8 {
    countdown(&Json, 3) + countdown(&Csv, 3)
}
