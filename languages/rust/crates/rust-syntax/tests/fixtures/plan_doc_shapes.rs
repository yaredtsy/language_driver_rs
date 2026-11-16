// RS2 §4: one unmarked def per row, so that planning a marker on each has a shape to meet.
/// Parses an order.
///
fn row1_last_line_empty() {}

/// Parses an order.
fn row2_last_line_text() {}

/// a
#[inline]
/// b
#[must_use]
fn row4_interleaved() {}

#[doc = "from an attribute"]
fn row5_doc_attribute() {}

#[inline]
fn row6_attribute_only() {}

fn row6_no_docs() {}

struct S;

impl S {
    /// indented docs
    fn indented_method() {}

    fn indented_no_docs() {}
}

mod m {
    #[doc = "x"]
    #[doc = "y"]
    fn two_doc_attributes() {}
}

// a plain comment above
fn plain_comment_above() {}

/// a
// a plain comment between the docs and the def
fn docs_then_plain_comment() {}

/// a

/// b after a blank line
fn docs_across_a_blank_line() {}
