// §3: two marker lines, the first wins (core's `plan_marker` will answer `AlreadyMarked` with it).
/// ID: first
/// ID: second
fn two_lines() {}

/// ID: line_before_block
/** ID: block_after */
fn line_then_block() {}

/** ID: block_before_line */
/// ID: line_after
fn block_then_line() {}
