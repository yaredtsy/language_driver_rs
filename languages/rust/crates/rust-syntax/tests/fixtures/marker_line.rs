// §3 which line: the first doc line that reads `<label>: <text>`, wherever it stands among the item's doc lines.
/// ID: first
fn only_line() {}

/// Parses an order.
///
/// ID: last
fn last_line() {}

/// Parses an order.
/// ID: middle
/// More about it.
fn middle_line() {}

/// Parses an order.
fn no_marker() {}

fn no_docs() {}
