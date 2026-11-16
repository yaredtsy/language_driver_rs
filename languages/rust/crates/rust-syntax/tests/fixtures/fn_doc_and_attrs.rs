/// One line.
fn doc_only() {}

#[inline]
fn attr_only() {}

/// first
#[inline]
/// second
#[must_use]
fn interleaved() {}

#[inline]
/// doc after the attribute
fn attr_then_doc() {}

/** block doc */
fn block_doc() {}

#[doc = "an attribute, not a doc comment"]
fn doc_attribute() {}
