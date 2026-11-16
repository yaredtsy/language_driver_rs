// §3: a `/** */` block is read line by line, after its leading `*` (if it has one) and any blanks; the first line after the `/**`.
/** ID: one_line */
fn one_line() {}

/**ID: no_spaces*/
fn no_spaces() {}

/**
 * Parses an order.
 * ID: starred
 */
fn starred() {}

/**
    * ID: deeper_indent
    */
fn deeper_indent() {}

/**
 ID: no_star
 */
fn no_star() {}

/** Parses an order.
 * ID: after_text_on_the_first_line */
fn closed_on_the_marker_line() {}

/**
 *  ID: two_spaces
 */
fn two_spaces() {}

/** ID: */
fn empty_text() {}

/** A summary. */
fn no_marker() {}
