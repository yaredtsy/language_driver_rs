// §3, the accepted cost of reading after any blanks: a line that is only an example in the docs is read as a marker, as TypeScript reads one
// inside a fenced block. Planning (RS2 §4) is what stays strict.

/// Shows the syntax of a marker:
///
/// ```text
/// ID: from_a_fenced_example
/// ```
fn fenced_example() {}

/// Shows it as an indented code block:
///
///     ID: from_an_indented_line
fn indented_line() {}

/// The real marker comes after the example, and the example still wins (first line first).
///
/// ```text
/// ID: example_wins
/// ```
///
/// ID: real_but_second
fn example_wins() {}
