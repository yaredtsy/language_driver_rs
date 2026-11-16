// §3 (S-RS4): the engine's node holds every outer doc line above the item, so a blank line or a plain comment between them detaches nothing.
/// a

/// ID: after_a_blank_line
fn after_blank() {}

/// ID: before_a_blank_line

fn before_blank() {}

/// a
// plain

/// ID: past_a_plain_comment_and_a_blank_line
fn past_plain() {}

/// a
/// ID: before_the_attribute
#[inline]
fn before_attribute() {}

/// a
#[inline]
/// ID: after_the_attribute
fn after_attribute() {}

// plain, then the doc
/// ID: below_a_plain_comment
fn below_plain() {}

/// ID: above_a_plain_comment
// plain
fn above_plain() {}
